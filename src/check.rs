//! Compatibility checks between fused imports and the exports that satisfy
//! them, mirroring wasm-merge's checks: a mismatch that would trap at
//! instantiation time should fail the merge instead.
//!
//! Type identity is canonical (see types.rs): concrete heap types from
//! different modules compare by canonical index, and subtyping between them
//! follows their declared supertype chains, so checks are precise even for
//! GC types.

use wasmparser::{
    AbstractHeapType, CompositeInnerType, GlobalType, HeapType, Import, MemoryType, RefType,
    SubType, TableType, TypeRef, ValType,
};

use crate::merge::MergeError;
use crate::parse::{Kind, ParsedModule};
use crate::resolve::{Resolution, Site};
use crate::types::{HeapKind, TypeCanon};

/// Check every fused import against the item that satisfies it. All
/// mismatches are collected and reported together, like wasm-merge does.
pub(crate) fn check_fused(
    parsed: &[ParsedModule<'_>],
    resolution: &mut Resolution<'_>,
    canon: &TypeCanon,
) -> Result<(), MergeError> {
    // Flattened type lists (rec groups expanded) per module, used only to
    // render readable diagnostics.
    let types_flat: Vec<Vec<&SubType>> = parsed
        .iter()
        .map(|module| {
            module
                .types
                .iter()
                .flat_map(|group| group.types())
                .collect()
        })
        .collect();

    let mut mismatches = Vec::new();
    for kind in Kind::ALL {
        for (module_idx, module) in parsed.iter().enumerate() {
            for (slot, import) in module.imports[kind].iter().enumerate() {
                let site = resolution.resolve(kind, module_idx, slot as u32)?;
                if (site
                    == Site::Import {
                        module: module_idx,
                        slot: slot as u32,
                    })
                {
                    continue; // stays an import; nothing to check against
                }
                if let Some(detail) =
                    check_one(parsed, &types_flat, canon, module_idx, import, site)
                {
                    mismatches.push(detail);
                }
            }
        }
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(MergeError::ImportMismatches {
            details: mismatches,
        })
    }
}

/// The entity description of the item at a site. Types are (module, index)
/// pairs into that module's type space.
enum Provided {
    Func { module: usize, type_index: u32 },
    Table(usize, TableType),
    Memory(MemoryType),
    Global(usize, GlobalType),
    Tag { module: usize, type_index: u32 },
}

/// Check one fused import against the item satisfying it, returning a
/// description of the mismatch if they are incompatible.
fn check_one(
    parsed: &[ParsedModule<'_>],
    types_flat: &[Vec<&SubType>],
    canon: &TypeCanon,
    importer: usize,
    import: &Import<'_>,
    site: Site,
) -> Option<String> {
    // Wording follows wasm-merge: "Type mismatch when importing <kind> <name>
    // from module <module>: <detail>".
    let mismatch = |detail: String| {
        Some(format!(
            "type mismatch when importing {} {} from module {}: {detail}",
            Kind::of_import(import.ty).noun(),
            import.name,
            import.module,
        ))
    };

    // Render a module's function type for diagnostics.
    let describe = |module: usize, type_index: u32| -> String {
        match types_flat[module]
            .get(type_index as usize)
            .map(|sub_type| &sub_type.composite_type.inner)
        {
            Some(CompositeInnerType::Func(func_type)) => format!("{func_type:?}"),
            _ => format!("type {type_index}"),
        }
    };

    // Describe the item the import was fused to.
    let provided = match site {
        Site::Import { module, slot } => {
            let target = parsed[module].imports[Kind::of_import(import.ty)][slot as usize];
            match target.ty {
                TypeRef::Func(type_index) | TypeRef::FuncExact(type_index) => {
                    Provided::Func { module, type_index }
                }
                TypeRef::Table(ty) => Provided::Table(module, ty),
                TypeRef::Memory(ty) => Provided::Memory(ty),
                TypeRef::Global(ty) => Provided::Global(module, ty),
                TypeRef::Tag(ty) => Provided::Tag {
                    module,
                    type_index: ty.func_type_idx,
                },
            }
        }
        Site::Def { module, def_index } => {
            let target = &parsed[module];
            match Kind::of_import(import.ty) {
                Kind::Func => Provided::Func {
                    module,
                    type_index: target.func_types[def_index as usize],
                },
                Kind::Table => Provided::Table(module, target.tables[def_index as usize].ty),
                Kind::Memory => Provided::Memory(target.memories[def_index as usize]),
                Kind::Global => Provided::Global(module, target.globals[def_index as usize].ty),
                Kind::Tag => Provided::Tag {
                    module,
                    type_index: target.tags[def_index as usize].func_type_idx,
                },
            }
        }
    };

    match (import.ty, provided) {
        (TypeRef::Func(expected_index), Provided::Func { module, type_index }) => {
            // The export's declared type must be (a declared subtype of) the
            // import's; function subtyping is by declaration, not structure.
            let expected = canon.canonical(importer, expected_index)?;
            let found = canon.canonical(module, type_index)?;
            if !canon.is_subtype(found, expected) {
                return mismatch(format!(
                    "the export's type is not a subtype of the import's type \
                     (import expects {}, export has {})",
                    describe(importer, expected_index),
                    describe(module, type_index),
                ));
            }
        }
        (TypeRef::FuncExact(expected_index), Provided::Func { module, type_index }) => {
            // An exact import is satisfied only by exactly that type.
            let expected = canon.canonical(importer, expected_index)?;
            let found = canon.canonical(module, type_index)?;
            if found != expected {
                return mismatch(format!(
                    "the import expects exactly {}, but the export has {}",
                    describe(importer, expected_index),
                    describe(module, type_index),
                ));
            }
        }
        (TypeRef::Global(expected), Provided::Global(module, provided)) => {
            if expected.mutable != provided.mutable {
                return mismatch(format!(
                    "the import expects {} global but the export is {}",
                    mutability(expected.mutable),
                    mutability(provided.mutable),
                ));
            }
            if expected.shared != provided.shared {
                return mismatch("sharedness differs".to_string());
            }
            // Mutable globals are invariant; immutable ones allow the export
            // to be a subtype.
            let compatible = if expected.mutable {
                val_types_equal(
                    canon,
                    (importer, expected.content_type),
                    (module, provided.content_type),
                )
            } else {
                val_subtype(
                    canon,
                    (module, provided.content_type),
                    (importer, expected.content_type),
                )
            };
            if !compatible {
                return mismatch(format!(
                    "export type {:?} is incompatible with import type {:?}",
                    provided.content_type, expected.content_type,
                ));
            }
        }
        (TypeRef::Table(expected), Provided::Table(module, provided)) => {
            if expected.table64 != provided.table64 {
                return mismatch("table index types (table64) differ".to_string());
            }
            if !val_types_equal(
                canon,
                (importer, ValType::Ref(expected.element_type)),
                (module, ValType::Ref(provided.element_type)),
            ) {
                return mismatch(format!(
                    "export type {:?} is different from import type {:?}",
                    provided.element_type, expected.element_type,
                ));
            }
            if let Some(detail) = check_limits(
                expected.initial,
                expected.maximum,
                provided.initial,
                provided.maximum,
            ) {
                return mismatch(detail);
            }
        }
        (TypeRef::Memory(expected), Provided::Memory(provided)) => {
            if expected.memory64 != provided.memory64 {
                return mismatch("memory index types (memory64) differ".to_string());
            }
            if expected.shared != provided.shared {
                return mismatch("sharedness differs".to_string());
            }
            if expected.page_size_log2 != provided.page_size_log2 {
                return mismatch("page sizes differ".to_string());
            }
            if let Some(detail) = check_limits(
                expected.initial,
                expected.maximum,
                provided.initial,
                provided.maximum,
            ) {
                return mismatch(detail);
            }
        }
        (TypeRef::Tag(expected), Provided::Tag { module, type_index }) => {
            // Tag types are invariant; canonical identity is exactly type
            // equivalence.
            let expected_canonical = canon.canonical(importer, expected.func_type_idx)?;
            let found = canon.canonical(module, type_index)?;
            if found != expected_canonical {
                return mismatch(format!(
                    "the import expects type {} but the export has type {}",
                    describe(importer, expected.func_type_idx),
                    describe(module, type_index),
                ));
            }
        }
        // resolve() only fuses like-kinded items, so the pairs above are
        // exhaustive in practice
        _ => {}
    }
    None
}

fn mutability(mutable: bool) -> &'static str {
    if mutable { "mutable" } else { "immutable" }
}

/// Limits subtyping: the provided item must offer at least the required
/// initial size and must not be able to exceed a required maximum. Wording
/// follows wasm-merge.
fn check_limits(
    expected_initial: u64,
    expected_maximum: Option<u64>,
    provided_initial: u64,
    provided_maximum: Option<u64>,
) -> Option<String> {
    if provided_initial < expected_initial {
        return Some(format!(
            "minimal size {provided_initial} is smaller than expected minimal size {expected_initial}"
        ));
    }
    let expected_maximum = expected_maximum?;
    match provided_maximum {
        Some(provided_maximum) if provided_maximum <= expected_maximum => None,
        Some(provided_maximum) => Some(format!(
            "maximal size {provided_maximum} is larger than expected maximal size {expected_maximum}"
        )),
        None => Some("expecting a bounded item but the export is unbounded".to_string()),
    }
}

// Value-type comparisons take (module, type) pairs so concrete heap types can
// be translated to canonical indices. References whose indices cannot be
// canonicalised (exotic forms that plain parsing never produces) are treated
// as compatible; output validation remains the backstop.

/// Is `sub` a value subtype of `sup`?
fn val_subtype(canon: &TypeCanon, sub: (usize, ValType), sup: (usize, ValType)) -> bool {
    match (sub.1, sup.1) {
        (ValType::Ref(sub_ref), ValType::Ref(sup_ref)) => {
            ref_subtype(canon, (sub.0, sub_ref), (sup.0, sup_ref))
        }
        (sub, sup) => sub == sup,
    }
}

/// Are the two value types equal?
fn val_types_equal(canon: &TypeCanon, a: (usize, ValType), b: (usize, ValType)) -> bool {
    match (a.1, b.1) {
        (ValType::Ref(a_ref), ValType::Ref(b_ref)) => {
            if a_ref.is_nullable() != b_ref.is_nullable() {
                return false;
            }
            match (a_ref.heap_type(), b_ref.heap_type()) {
                (HeapType::Concrete(a_index), HeapType::Concrete(b_index))
                | (HeapType::Exact(a_index), HeapType::Exact(b_index)) => {
                    match (
                        canonical(canon, a.0, a_index),
                        canonical(canon, b.0, b_index),
                    ) {
                        (Some(a_canonical), Some(b_canonical)) => a_canonical == b_canonical,
                        _ => true, // undecidable: defer to output validation
                    }
                }
                (HeapType::Concrete(_) | HeapType::Exact(_), _)
                | (_, HeapType::Concrete(_) | HeapType::Exact(_)) => false,
                (a_heap, b_heap) => a_heap == b_heap,
            }
        }
        (a, b) => a == b,
    }
}

/// Is `sub` a reference subtype of `sup`?
fn ref_subtype(canon: &TypeCanon, sub: (usize, RefType), sup: (usize, RefType)) -> bool {
    if sub.1.is_nullable() && !sup.1.is_nullable() {
        return false;
    }
    match (sub.1.heap_type(), sup.1.heap_type()) {
        (
            HeapType::Abstract {
                shared: sub_shared,
                ty: sub_heap,
            },
            HeapType::Abstract {
                shared: sup_shared,
                ty: sup_heap,
            },
        ) => sub_shared == sup_shared && abstract_subtype(sub_heap, sup_heap),
        // Concrete vs concrete: follow the declared supertype chain. An
        // exact type sits strictly below its plain type, so `exact $t` is a
        // subtype of anything `$t` is, while only equality (or a bottom type)
        // gets *into* an exact type.
        (
            HeapType::Concrete(sub_index) | HeapType::Exact(sub_index),
            HeapType::Concrete(sup_index),
        ) => {
            match (
                canonical(canon, sub.0, sub_index),
                canonical(canon, sup.0, sup_index),
            ) {
                (Some(sub_canonical), Some(sup_canonical)) => {
                    canon.is_subtype(sub_canonical, sup_canonical)
                }
                _ => true,
            }
        }
        (HeapType::Exact(sub_index), HeapType::Exact(sup_index)) => {
            match (
                canonical(canon, sub.0, sub_index),
                canonical(canon, sup.0, sup_index),
            ) {
                (Some(sub_canonical), Some(sup_canonical)) => sub_canonical == sup_canonical,
                _ => true,
            }
        }
        (HeapType::Concrete(_), HeapType::Exact(_)) => false,
        // A concrete (or exact) type is a subtype of the abstract types above
        // its hierarchy's top.
        (
            HeapType::Concrete(sub_index) | HeapType::Exact(sub_index),
            HeapType::Abstract { shared, ty },
        ) => match canonical(canon, sub.0, sub_index) {
            Some(sub_canonical) => {
                let (kind, sub_shared) = canon.kind(sub_canonical);
                sub_shared == shared && abstract_subtype(hierarchy_top(kind), ty)
            }
            None => true,
        },
        // Only a hierarchy's bottom type is a subtype of a concrete type.
        (
            HeapType::Abstract { shared, ty },
            HeapType::Concrete(sup_index) | HeapType::Exact(sup_index),
        ) => match canonical(canon, sup.0, sup_index) {
            Some(sup_canonical) => {
                let (kind, sup_shared) = canon.kind(sup_canonical);
                shared == sup_shared && ty == hierarchy_bottom(kind)
            }
            None => true,
        },
    }
}

fn canonical(canon: &TypeCanon, module: usize, index: wasmparser::UnpackedIndex) -> Option<u32> {
    canon.canonical(module, index.as_module_index()?)
}

/// The abstract top of a concrete type's hierarchy.
fn hierarchy_top(kind: HeapKind) -> AbstractHeapType {
    match kind {
        HeapKind::Func => AbstractHeapType::Func,
        HeapKind::Struct => AbstractHeapType::Struct,
        HeapKind::Array => AbstractHeapType::Array,
        HeapKind::Cont => AbstractHeapType::Cont,
    }
}

/// The abstract bottom of a concrete type's hierarchy.
fn hierarchy_bottom(kind: HeapKind) -> AbstractHeapType {
    match kind {
        HeapKind::Func => AbstractHeapType::NoFunc,
        HeapKind::Struct | HeapKind::Array => AbstractHeapType::None,
        HeapKind::Cont => AbstractHeapType::NoCont,
    }
}

/// The subtyping lattice between abstract heap types, per hierarchy.
fn abstract_subtype(sub: AbstractHeapType, sup: AbstractHeapType) -> bool {
    use AbstractHeapType::*;
    if sub == sup {
        return true;
    }
    match (sub, sup) {
        // the `any` hierarchy: none <: i31/struct/array <: eq <: any
        (None, Any | Eq | I31 | Struct | Array) => true,
        (I31 | Struct | Array | Eq, Any) => true,
        (I31 | Struct | Array, Eq) => true,
        // the other hierarchies are bottom <: top
        (NoFunc, Func) | (NoExtern, Extern) | (NoExn, Exn) | (NoCont, Cont) => true,
        _ => false,
    }
}
