//! Compatibility checks between fused imports and the exports that satisfy
//! them, mirroring wasm-merge's checks: a mismatch that would trap at
//! instantiation time should fail the merge instead.
//!
//! Comparisons are structural. References to concrete (module-declared) heap
//! types are not yet compared — that requires cross-module type
//! canonicalisation (PLAN.md phase 3); incompatibilities involving them are
//! still caught by output validation, just with a less precise error.

use wasmparser::{
    AbstractHeapType, CompositeInnerType, FuncType, GlobalType, HeapType, Import, MemoryType,
    RefType, SubType, TableType, TagType, TypeRef, ValType,
};

use crate::merge::MergeError;
use crate::parse::{Kind, ParsedModule};
use crate::resolve::{Resolution, Site};

/// Check every fused import against the item that satisfies it. All
/// mismatches are collected and reported together, like wasm-merge does.
pub(crate) fn check_fused(
    parsed: &[ParsedModule<'_>],
    resolution: &mut Resolution<'_>,
) -> Result<(), MergeError> {
    // Flattened type lists (rec groups expanded) per module, for looking up
    // function types by index.
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
                if let Some(detail) = check_one(parsed, &types_flat, module_idx, import, site) {
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

/// The entity description of the item at a site.
enum Provided<'a> {
    Func(&'a FuncType),
    Table(TableType),
    Memory(MemoryType),
    Global(GlobalType),
    Tag(&'a FuncType),
}

/// Check one fused import against the item satisfying it, returning a
/// description of the mismatch if they are incompatible.
fn check_one(
    parsed: &[ParsedModule<'_>],
    types_flat: &[Vec<&SubType>],
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

    let func_type_of = |module: usize, type_index: u32| -> Option<&FuncType> {
        match &types_flat[module]
            .get(type_index as usize)?
            .composite_type
            .inner
        {
            CompositeInnerType::Func(func_type) => Some(func_type),
            _ => None,
        }
    };

    // Describe the item the import was fused to. Anything that cannot be
    // described without cross-module type canonicalisation is skipped
    // (PLAN.md phase 3); output validation still catches real problems there.
    let provided = match site {
        Site::Import { module, slot } => {
            let target = parsed[module].imports[Kind::of_import(import.ty)][slot as usize];
            match target.ty {
                TypeRef::Func(type_index) | TypeRef::FuncExact(type_index) => {
                    Provided::Func(func_type_of(module, type_index)?)
                }
                TypeRef::Table(ty) => Provided::Table(ty),
                TypeRef::Memory(ty) => Provided::Memory(ty),
                TypeRef::Global(ty) => Provided::Global(ty),
                TypeRef::Tag(ty) => Provided::Tag(func_type_of(module, ty.func_type_idx)?),
            }
        }
        Site::Def { module, def_index } => {
            let target = &parsed[module];
            match Kind::of_import(import.ty) {
                Kind::Func => {
                    Provided::Func(func_type_of(module, target.func_types[def_index as usize])?)
                }
                Kind::Table => Provided::Table(target.tables[def_index as usize].ty),
                Kind::Memory => Provided::Memory(target.memories[def_index as usize]),
                Kind::Global => Provided::Global(target.globals[def_index as usize].ty),
                Kind::Tag => {
                    let tag: TagType = target.tags[def_index as usize];
                    Provided::Tag(func_type_of(module, tag.func_type_idx)?)
                }
            }
        }
    };

    match (import.ty, provided) {
        (
            TypeRef::Func(expected_index) | TypeRef::FuncExact(expected_index),
            Provided::Func(provided_type),
        ) => {
            // The export must be usable wherever the import is: a subtype.
            let expected_type = func_type_of(importer, expected_index)?;
            if let Some(false) = func_subtype(provided_type, expected_type) {
                return mismatch(format!(
                    "the export's type is not a subtype of the import's type \
                     (import expects {expected_type:?}, export has {provided_type:?})"
                ));
            }
        }
        (TypeRef::Global(expected), Provided::Global(provided)) => {
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
                val_types_equal(expected.content_type, provided.content_type)
            } else {
                val_subtype(provided.content_type, expected.content_type)
            };
            if let Some(false) = compatible {
                return mismatch(format!(
                    "export type {:?} is incompatible with import type {:?}",
                    provided.content_type, expected.content_type,
                ));
            }
        }
        (TypeRef::Table(expected), Provided::Table(provided)) => {
            if expected.table64 != provided.table64 {
                return mismatch("table index types (table64) differ".to_string());
            }
            if let Some(false) = val_types_equal(
                ValType::Ref(expected.element_type),
                ValType::Ref(provided.element_type),
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
        (TypeRef::Tag(expected), Provided::Tag(provided_type)) => {
            // Tag types are invariant.
            let expected_type = func_type_of(importer, expected.func_type_idx)?;
            if let Some(false) = func_types_equal(expected_type, provided_type) {
                return mismatch(format!(
                    "the import expects type {expected_type:?} but the export has type {provided_type:?}"
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

// The comparisons below return `Some(verdict)` when the types can be compared
// without cross-module type canonicalisation, and `None` when a concrete
// (module-declared) heap type is involved — those comparisons are deferred to
// output validation until phase 3 (see PLAN.md).

/// Is `sub` a function subtype of `sup` (contravariant parameters, covariant
/// results)?
fn func_subtype(sub: &FuncType, sup: &FuncType) -> Option<bool> {
    if sub.params().len() != sup.params().len() || sub.results().len() != sup.results().len() {
        return Some(false);
    }
    let mut comparable = true;
    let parameters = sup.params().iter().zip(sub.params());
    let results = sub.results().iter().zip(sup.results());
    for (&narrower, &wider) in parameters.chain(results) {
        match val_subtype(narrower, wider) {
            Some(true) => {}
            Some(false) => return Some(false),
            None => comparable = false,
        }
    }
    comparable.then_some(true)
}

/// Are the two function types equal?
fn func_types_equal(expected: &FuncType, provided: &FuncType) -> Option<bool> {
    if expected.params().len() != provided.params().len()
        || expected.results().len() != provided.results().len()
    {
        return Some(false);
    }
    let mut comparable = true;
    for (&e, &p) in expected
        .params()
        .iter()
        .zip(provided.params())
        .chain(expected.results().iter().zip(provided.results()))
    {
        match val_types_equal(e, p) {
            Some(true) => {}
            Some(false) => return Some(false),
            None => comparable = false,
        }
    }
    comparable.then_some(true)
}

/// Is `sub` a value subtype of `sup`?
fn val_subtype(sub: ValType, sup: ValType) -> Option<bool> {
    match (sub, sup) {
        (ValType::Ref(sub), ValType::Ref(sup)) => ref_subtype(sub, sup),
        _ => Some(sub == sup),
    }
}

/// Are the two value types equal?
fn val_types_equal(expected: ValType, provided: ValType) -> Option<bool> {
    let concrete = |ty: ValType| matches!(ty, ValType::Ref(r) if matches!(r.heap_type(), HeapType::Concrete(_)));
    if concrete(expected) || concrete(provided) {
        return None;
    }
    Some(expected == provided)
}

/// Is `sub` a reference subtype of `sup`? Only abstract heap types can be
/// decided here.
fn ref_subtype(sub: RefType, sup: RefType) -> Option<bool> {
    let (
        HeapType::Abstract {
            shared: sub_shared,
            ty: sub_heap,
        },
        HeapType::Abstract {
            shared: sup_shared,
            ty: sup_heap,
        },
    ) = (sub.heap_type(), sup.heap_type())
    else {
        return None;
    };
    if sub.is_nullable() && !sup.is_nullable() {
        return Some(false);
    }
    Some(sub_shared == sup_shared && abstract_subtype(sub_heap, sup_heap))
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
