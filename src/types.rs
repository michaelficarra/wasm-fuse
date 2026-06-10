//! Cross-module type canonicalisation.
//!
//! WebAssembly type identity is isorecursive: a rec group is equivalent to
//! another iff they are structurally identical, comparing references to types
//! *inside* the group by relative position and references to types *outside*
//! it by the identity of the referenced type. Modules guarantee that type
//! references point to earlier types (or members of the same group), so
//! processing groups in module order lets every external reference be
//! rewritten to an already-canonical index.
//!
//! Each group's canonical key is its byte encoding with external references
//! canonicalised and internal references made group-relative; identical keys
//! mean identical types, which then share indices in the merged type section.
//! This both deduplicates the output and gives the import/export checks a
//! meaningful identity for concrete heap types (canonical indices compare
//! across modules; declared subtype chains are recorded per canonical type).

use std::collections::HashMap;

use wasm_encoder::Encode;
use wasm_encoder::reencode::{Error as ReencodeError, Reencode};
use wasmparser::{CompositeInnerType, UnpackedIndex};

use crate::merge::MergeError;
use crate::parse::ParsedModule;
use crate::remap::flatten_error;

/// Which abstract-type hierarchy a concrete (composite) type belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeapKind {
    Func,
    Struct,
    Array,
    Cont,
}

/// The canonicalised type space of the merged module.
pub(crate) struct TypeCanon {
    /// Per input module: module-local type index → canonical merged index.
    pub(crate) maps: Vec<Vec<u32>>,
    /// First occurrence of each distinct rec group, as (module, group
    /// position) — the groups the type section emits, in canonical order.
    pub(crate) unique_groups: Vec<(usize, usize)>,
    /// Total number of canonical types.
    pub(crate) count: u32,
    /// Each canonical type's declared supertype, as a canonical index.
    supertypes: Vec<Option<u32>>,
    /// Each canonical type's hierarchy and sharedness.
    kinds: Vec<(HeapKind, bool)>,
    /// The canonical index of a plain `(func)` type, if any input declared
    /// one — reused for the synthetic combined start function.
    pub(crate) empty_func: Option<u32>,
}

impl TypeCanon {
    /// The canonical index of `index` in `module`'s type space.
    pub(crate) fn canonical(&self, module: usize, index: u32) -> Option<u32> {
        self.maps.get(module)?.get(index as usize).copied()
    }

    /// Is canonical type `sub` equal to `sup` or declared (transitively) as
    /// one of its subtypes?
    pub(crate) fn is_subtype(&self, sub: u32, sup: u32) -> bool {
        let mut current = Some(sub);
        while let Some(ty) = current {
            if ty == sup {
                return true;
            }
            current = self.supertypes[ty as usize];
        }
        false
    }

    /// The hierarchy and sharedness of canonical type `ty`.
    pub(crate) fn kind(&self, ty: u32) -> (HeapKind, bool) {
        self.kinds[ty as usize]
    }
}

/// References to members of the group currently being keyed are rewritten
/// relative to this base, making the key position-independent.
const GROUP_RELATIVE_BASE: u32 = 1 << 31;

/// Rewrites type references for group keying: external references become
/// canonical indices, in-group references become group-relative.
struct KeyMapper<'a> {
    module_name: &'a str,
    /// Canonical indices of this module's types before the current group.
    canonical_prefix: &'a [u32],
    group_start: u32,
    group_size: u32,
}

impl KeyMapper<'_> {
    fn rewrite(&self, index: u32) -> Result<u32, ReencodeError<MergeError>> {
        if index >= self.group_start && index < self.group_start + self.group_size {
            return Ok(GROUP_RELATIVE_BASE + (index - self.group_start));
        }
        self.canonical_prefix
            .get(index as usize)
            .copied()
            .ok_or_else(|| {
                ReencodeError::UserError(MergeError::OutOfBoundsIndex {
                    module: self.module_name.to_string(),
                    kind: "type",
                    index,
                })
            })
    }
}

impl Reencode for KeyMapper<'_> {
    type Error = MergeError;

    fn type_index(&mut self, ty: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.rewrite(ty)
    }

    fn type_index_unpacked(&mut self, ty: UnpackedIndex) -> Result<u32, ReencodeError<MergeError>> {
        match ty {
            UnpackedIndex::Module(index) => self.rewrite(index),
            UnpackedIndex::RecGroup(relative) => Ok(GROUP_RELATIVE_BASE + relative),
            _ => Err(ReencodeError::UserError(MergeError::Reencode {
                module: self.module_name.to_string(),
                message: "unexpected canonicalised type reference in input".to_string(),
            })),
        }
    }
}

/// Canonicalise all input modules' rec groups.
pub(crate) fn canonicalise(parsed: &[ParsedModule<'_>]) -> Result<TypeCanon, MergeError> {
    let mut canon = TypeCanon {
        maps: Vec::with_capacity(parsed.len()),
        unique_groups: Vec::new(),
        count: 0,
        supertypes: Vec::new(),
        kinds: Vec::new(),
        empty_func: None,
    };
    let mut seen: HashMap<Vec<u8>, u32> = HashMap::new();

    for (module_idx, module) in parsed.iter().enumerate() {
        let mut map: Vec<u32> = Vec::with_capacity(module.type_count() as usize);
        for (group_idx, group) in module.types.iter().enumerate() {
            let start = map.len() as u32;
            let size = group.types().len() as u32;

            let mut mapper = KeyMapper {
                module_name: &module.name,
                canonical_prefix: &map,
                group_start: start,
                group_size: size,
            };
            let mut keyed = wasm_encoder::TypeSection::new();
            mapper
                .parse_recursive_type_group(keyed.ty(), group.clone())
                .map_err(|error| flatten_error(error, &module.name))?;
            let mut key = Vec::new();
            keyed.encode(&mut key);

            if let Some(&base) = seen.get(&key) {
                map.extend(base..base + size);
                continue;
            }

            let base = canon.count;
            seen.insert(key, base);
            canon.unique_groups.push((module_idx, group_idx));
            map.extend(base..base + size);
            canon.count += size;
            for sub_type in group.types() {
                let supertype = match sub_type.supertype_idx {
                    None => None,
                    Some(packed) => match packed.unpack() {
                        UnpackedIndex::Module(index) => {
                            Some(*map.get(index as usize).ok_or_else(|| {
                                MergeError::OutOfBoundsIndex {
                                    module: module.name.clone(),
                                    kind: "type",
                                    index,
                                }
                            })?)
                        }
                        UnpackedIndex::RecGroup(relative) => Some(base + relative),
                        _ => {
                            return Err(MergeError::Reencode {
                                module: module.name.clone(),
                                message: "unexpected canonicalised supertype reference in input"
                                    .to_string(),
                            });
                        }
                    },
                };
                canon.supertypes.push(supertype);
                let kind = match &sub_type.composite_type.inner {
                    CompositeInnerType::Func(_) => HeapKind::Func,
                    CompositeInnerType::Struct(_) => HeapKind::Struct,
                    CompositeInnerType::Array(_) => HeapKind::Array,
                    CompositeInnerType::Cont(_) => HeapKind::Cont,
                };
                canon.kinds.push((kind, sub_type.composite_type.shared));
            }
        }
        canon.maps.push(map);
    }

    // If some input already declared a plain `(func)` type, the synthetic
    // combined start function can reuse it instead of appending a duplicate.
    let mut empty_func_section = wasm_encoder::TypeSection::new();
    empty_func_section.ty().function([], []);
    let mut empty_func_key = Vec::new();
    empty_func_section.encode(&mut empty_func_key);
    canon.empty_func = seen.get(&empty_func_key).copied();

    Ok(canon)
}
