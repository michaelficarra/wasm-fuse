//! Merging of "name" custom sections (debug names), emitted when
//! `MergeOptions::keep_names` is set — the counterpart of wasm-merge's `-g`.
//!
//! Indices are remapped like everything else. Where two modules name the same
//! merged item — a fused import's local alias versus the definition's own
//! name — the name from the item's *owning* module wins; aliases only fill
//! gaps. Names of pruned items are dropped. The synthetic combined start
//! function is named `merged.start.combined`, as in wasm-merge.

use std::collections::BTreeMap;

use wasmparser::Name;

use crate::merge::MergeError;
use crate::parse::{Kind, ParsedModule};
use crate::remap::PRUNED;
use crate::resolve::{Layout, Resolution, Site};
use crate::types::TypeCanon;

/// A collected name map: merged index → (owned-by-its-module, name).
#[derive(Default)]
struct Names<'a> {
    entries: BTreeMap<u32, (bool, &'a str)>,
}

impl<'a> Names<'a> {
    fn insert(&mut self, index: u32, owned: bool, name: &'a str) {
        match self.entries.get(&index) {
            Some((true, _)) => {}
            Some((false, _)) if !owned => {}
            _ => {
                self.entries.insert(index, (owned, name));
            }
        }
    }

    fn encode(&self) -> wasm_encoder::NameMap {
        let mut map = wasm_encoder::NameMap::new();
        for (&index, &(_, name)) in &self.entries {
            map.append(index, name);
        }
        map
    }
}

/// Inner entries of an indirect name map: inner index → name.
type InnerNames<'a> = Vec<(u32, &'a str)>;

/// As [`Names`] but for indirect maps (locals, labels, fields): merged index
/// → inner index → name.
#[derive(Default)]
struct IndirectNames<'a> {
    entries: BTreeMap<u32, (bool, InnerNames<'a>)>,
}

impl<'a> IndirectNames<'a> {
    fn insert(&mut self, index: u32, owned: bool, inner: InnerNames<'a>) {
        match self.entries.get(&index) {
            Some((true, _)) => {}
            Some((false, _)) if !owned => {}
            _ => {
                self.entries.insert(index, (owned, inner));
            }
        }
    }

    fn encode(&self) -> wasm_encoder::IndirectNameMap {
        let mut map = wasm_encoder::IndirectNameMap::new();
        for (&index, (_, inner)) in &self.entries {
            let mut inner_map = wasm_encoder::NameMap::new();
            for &(inner_index, name) in inner {
                inner_map.append(inner_index, name);
            }
            map.append(index, &inner_map);
        }
        map
    }
}

/// Build the merged module's name section, or `None` if no input carried any
/// names.
pub(crate) fn build(
    parsed: &[ParsedModule<'_>],
    resolution: &mut Resolution<'_>,
    layout: &Layout,
    canon: &TypeCanon,
    synthetic_start: Option<u32>,
) -> Result<Option<wasm_encoder::NameSection>, MergeError> {
    let mut module_name: Option<&str> = None;
    let mut functions = Names::default();
    let mut locals = IndirectNames::default();
    let mut labels = IndirectNames::default();
    let mut types = Names::default();
    let mut tables = Names::default();
    let mut memories = Names::default();
    let mut globals = Names::default();
    let mut elements = Names::default();
    let mut datas = Names::default();
    let mut fields = IndirectNames::default();
    let mut tags = Names::default();

    for (module_idx, module) in parsed.iter().enumerate() {
        // The merged index of item `index` of `kind`, with `true` if this
        // module owns the item (defines it, or holds its surviving import
        // slot) rather than merely referencing it through a fused import.
        let item = |resolution: &mut Resolution<'_>,
                    kind: Kind,
                    index: u32|
         -> Result<Option<(u32, bool)>, MergeError> {
            if index >= module.item_count(kind) {
                return Ok(None); // bogus name entry: ignore
            }
            let owned = match resolution.resolve(kind, module_idx, index)? {
                Site::Def { module, .. } | Site::Import { module, .. } => module == module_idx,
            };
            match layout.remaps[module_idx].kind(kind)[index as usize] {
                PRUNED => Ok(None),
                merged => Ok(Some((merged, owned))),
            }
        };

        // Read a wasmparser name map leniently: names are advisory, so a
        // malformed tail is skipped rather than failing the merge.
        macro_rules! lenient {
            ($reader:expr) => {
                $reader.clone().into_iter().map_while(Result::ok)
            };
        }

        for name in &module.names {
            match name {
                Name::Module { name, .. } => {
                    if module_name.is_none() {
                        module_name = Some(name);
                    }
                }
                Name::Function(map) => {
                    for naming in lenient!(map) {
                        if let Some((merged, owned)) = item(resolution, Kind::Func, naming.index)? {
                            functions.insert(merged, owned, naming.name);
                        }
                    }
                }
                Name::Local(map) | Name::Label(map) => {
                    let into = if matches!(name, Name::Local(_)) {
                        &mut locals
                    } else {
                        &mut labels
                    };
                    for indirect in lenient!(map) {
                        if let Some((merged, owned)) = item(resolution, Kind::Func, indirect.index)?
                        {
                            let inner: Vec<_> = lenient!(&indirect.names)
                                .map(|naming| (naming.index, naming.name))
                                .collect();
                            into.insert(merged, owned, inner);
                        }
                    }
                }
                Name::Type(map) => {
                    for naming in lenient!(map) {
                        if let Some(canonical) = canon.canonical(module_idx, naming.index) {
                            types.insert(canonical, false, naming.name);
                        }
                    }
                }
                Name::Table(map) => {
                    for naming in lenient!(map) {
                        if let Some((merged, owned)) = item(resolution, Kind::Table, naming.index)?
                        {
                            tables.insert(merged, owned, naming.name);
                        }
                    }
                }
                Name::Memory(map) => {
                    for naming in lenient!(map) {
                        if let Some((merged, owned)) = item(resolution, Kind::Memory, naming.index)?
                        {
                            memories.insert(merged, owned, naming.name);
                        }
                    }
                }
                Name::Global(map) => {
                    for naming in lenient!(map) {
                        if let Some((merged, owned)) = item(resolution, Kind::Global, naming.index)?
                        {
                            globals.insert(merged, owned, naming.name);
                        }
                    }
                }
                Name::Element(map) => {
                    for naming in lenient!(map) {
                        match layout.remaps[module_idx]
                            .elements
                            .get(naming.index as usize)
                        {
                            Some(&PRUNED) | None => {}
                            Some(&merged) => elements.insert(merged, true, naming.name),
                        }
                    }
                }
                Name::Data(map) => {
                    for naming in lenient!(map) {
                        match layout.remaps[module_idx].datas.get(naming.index as usize) {
                            Some(&PRUNED) | None => {}
                            Some(&merged) => datas.insert(merged, true, naming.name),
                        }
                    }
                }
                Name::Field(map) => {
                    for indirect in lenient!(map) {
                        if let Some(canonical) = canon.canonical(module_idx, indirect.index) {
                            let inner: Vec<_> = lenient!(&indirect.names)
                                .map(|naming| (naming.index, naming.name))
                                .collect();
                            fields.insert(canonical, false, inner);
                        }
                    }
                }
                Name::Tag(map) => {
                    for naming in lenient!(map) {
                        if let Some((merged, owned)) = item(resolution, Kind::Tag, naming.index)? {
                            tags.insert(merged, owned, naming.name);
                        }
                    }
                }
                Name::Unknown { .. } => {}
            }
        }
    }

    if let Some(start) = synthetic_start {
        functions.insert(start, true, "merged.start.combined");
    }

    let mut section = wasm_encoder::NameSection::new();
    let mut any = false;
    if let Some(name) = module_name {
        section.module(name);
        any = true;
    }
    // Subsections must appear in id order: function, local, label, type,
    // table, memory, global, element, data, field, tag.
    if !functions.entries.is_empty() {
        section.functions(&functions.encode());
        any = true;
    }
    if !locals.entries.is_empty() {
        section.locals(&locals.encode());
        any = true;
    }
    if !labels.entries.is_empty() {
        section.labels(&labels.encode());
        any = true;
    }
    if !types.entries.is_empty() {
        section.types(&types.encode());
        any = true;
    }
    if !tables.entries.is_empty() {
        section.tables(&tables.encode());
        any = true;
    }
    if !memories.entries.is_empty() {
        section.memories(&memories.encode());
        any = true;
    }
    if !globals.entries.is_empty() {
        section.globals(&globals.encode());
        any = true;
    }
    if !elements.entries.is_empty() {
        section.elements(&elements.encode());
        any = true;
    }
    if !datas.entries.is_empty() {
        section.data(&datas.encode());
        any = true;
    }
    if !fields.entries.is_empty() {
        section.fields(&fields.encode());
        any = true;
    }
    if !tags.entries.is_empty() {
        section.tags(&tags.encode());
        any = true;
    }
    Ok(any.then_some(section))
}
