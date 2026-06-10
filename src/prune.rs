//! Optional removal of unreachable items from the merged module — the
//! counterpart of the remove-unused-module-elements pass that wasm-merge runs
//! unconditionally (we make it opt-in; see `MergeOptions::prune_unused`).
//!
//! Reachability starts from the surviving exports and every start function
//! and follows all references: calls, `ref.func`, global/table/memory/tag
//! accesses, and segment uses. A live defined table keeps its active element
//! segments (and so their contents); a live memory keeps its active data
//! segments. Conservative choices, documented here deliberately:
//!
//! - Active segments targeting *imported* tables/memories are roots (their
//!   writes are observable by whoever provides the import), which also keeps
//!   those imports.
//! - Declarative element segments and their functions are roots, so a
//!   `ref.func` in live code never loses its declaration. Filtering their
//!   items to live functions only would prune more; revisit if it matters.
//! - Types are never pruned (harmless metadata; deduplication is the phase 3
//!   topic).
//! - Dropping an active segment whose out-of-bounds offset would have trapped
//!   at instantiation changes behaviour; like binaryen, pruning assumes
//!   segments don't trap.

use std::collections::{HashMap, HashSet};

use wasm_encoder::reencode::{Error as ReencodeError, Reencode};

use crate::emit::SurvivingExport;
use crate::merge::MergeError;
use crate::parse::{Kind, KindMap, ParsedModule};
use crate::remap::flatten_error;
use crate::resolve::{Resolution, Site};

/// The set of items that survive pruning.
#[derive(Debug, Default)]
pub(crate) struct Liveness {
    items: KindMap<HashSet<Site>>,
    elems: HashSet<(usize, u32)>,
    datas: HashSet<(usize, u32)>,
}

impl Liveness {
    pub(crate) fn item(&self, kind: Kind, site: Site) -> bool {
        self.items[kind].contains(&site)
    }

    pub(crate) fn def(&self, kind: Kind, module: usize, def_index: u32) -> bool {
        self.item(kind, Site::Def { module, def_index })
    }

    pub(crate) fn elem(&self, module: usize, index: u32) -> bool {
        self.elems.contains(&(module, index))
    }

    pub(crate) fn data(&self, module: usize, index: u32) -> bool {
        self.datas.contains(&(module, index))
    }
}

/// Items whose outgoing references still need scanning. Memories, tags, and
/// imports have no outgoing references, so they never enter the worklist.
#[derive(Clone, Copy)]
enum Work {
    FuncDef(usize, u32),
    GlobalDef(usize, u32),
    TableDef(usize, u32),
    Elem(usize, u32),
    Data(usize, u32),
}

pub(crate) fn compute_liveness(
    parsed: &[ParsedModule<'_>],
    resolution: &mut Resolution<'_>,
    exports: &[SurvivingExport],
) -> Result<Liveness, MergeError> {
    // Active segments, grouped by the site of the table/memory they target,
    // so a table or memory becoming live can pull its initialisers in.
    let mut elems_by_table: HashMap<Site, Vec<(usize, u32)>> = HashMap::new();
    let mut datas_by_memory: HashMap<Site, Vec<(usize, u32)>> = HashMap::new();
    for (module_idx, module) in parsed.iter().enumerate() {
        for (index, element) in module.elements.iter().enumerate() {
            if let wasmparser::ElementKind::Active { table_index, .. } = element.kind {
                let site = resolution.resolve(Kind::Table, module_idx, table_index.unwrap_or(0))?;
                elems_by_table
                    .entry(site)
                    .or_default()
                    .push((module_idx, index as u32));
            }
        }
        for (index, datum) in module.datas.iter().enumerate() {
            if let wasmparser::DataKind::Active { memory_index, .. } = datum.kind {
                let site = resolution.resolve(Kind::Memory, module_idx, memory_index)?;
                datas_by_memory
                    .entry(site)
                    .or_default()
                    .push((module_idx, index as u32));
            }
        }
    }

    let mut live = Liveness::default();
    let mut work: Vec<Work> = Vec::new();

    fn mark_elem(live: &mut Liveness, work: &mut Vec<Work>, segment: (usize, u32)) {
        if live.elems.insert(segment) {
            work.push(Work::Elem(segment.0, segment.1));
        }
    }

    fn mark_data(live: &mut Liveness, work: &mut Vec<Work>, segment: (usize, u32)) {
        if live.datas.insert(segment) {
            work.push(Work::Data(segment.0, segment.1));
        }
    }

    fn mark_item(
        live: &mut Liveness,
        work: &mut Vec<Work>,
        elems_by_table: &HashMap<Site, Vec<(usize, u32)>>,
        datas_by_memory: &HashMap<Site, Vec<(usize, u32)>>,
        kind: Kind,
        site: Site,
    ) {
        if !live.items[kind].insert(site) {
            return;
        }
        if let Site::Def { module, def_index } = site {
            match kind {
                Kind::Func => work.push(Work::FuncDef(module, def_index)),
                Kind::Global => work.push(Work::GlobalDef(module, def_index)),
                Kind::Table => work.push(Work::TableDef(module, def_index)),
                Kind::Memory | Kind::Tag => {}
            }
        }
        // A live table or memory keeps its active initialiser segments.
        if kind == Kind::Table {
            for &segment in elems_by_table.get(&site).into_iter().flatten() {
                mark_elem(live, work, segment);
            }
        }
        if kind == Kind::Memory {
            for &segment in datas_by_memory.get(&site).into_iter().flatten() {
                mark_data(live, work, segment);
            }
        }
    }

    // Roots: every surviving export...
    for export in exports {
        let kind = Kind::of_export(export.kind);
        let site = resolution.resolve(kind, export.module, export.index)?;
        mark_item(
            &mut live,
            &mut work,
            &elems_by_table,
            &datas_by_memory,
            kind,
            site,
        );
    }
    // ...every start function (they run at instantiation regardless of
    // export selection)...
    for (module_idx, module) in parsed.iter().enumerate() {
        if let Some(start) = module.start {
            let site = resolution.resolve(Kind::Func, module_idx, start)?;
            mark_item(
                &mut live,
                &mut work,
                &elems_by_table,
                &datas_by_memory,
                Kind::Func,
                site,
            );
        }
    }
    // ...declarative element segments (they license ref.func in live code)...
    for (module_idx, module) in parsed.iter().enumerate() {
        for (index, element) in module.elements.iter().enumerate() {
            if matches!(element.kind, wasmparser::ElementKind::Declared) {
                mark_elem(&mut live, &mut work, (module_idx, index as u32));
            }
        }
    }
    // ...and imported tables/memories with active segments targeting them:
    // those writes are observable by the import's provider.
    for &site in elems_by_table.keys() {
        if matches!(site, Site::Import { .. }) {
            mark_item(
                &mut live,
                &mut work,
                &elems_by_table,
                &datas_by_memory,
                Kind::Table,
                site,
            );
        }
    }
    for &site in datas_by_memory.keys() {
        if matches!(site, Site::Import { .. }) {
            mark_item(
                &mut live,
                &mut work,
                &elems_by_table,
                &datas_by_memory,
                Kind::Memory,
                site,
            );
        }
    }

    // Follow references until nothing new becomes live.
    while let Some(item) = work.pop() {
        let module_idx = match item {
            Work::FuncDef(module, _)
            | Work::GlobalDef(module, _)
            | Work::TableDef(module, _)
            | Work::Elem(module, _)
            | Work::Data(module, _) => module,
        };
        let collected = scan(&parsed[module_idx], item)?;
        for (kind, references) in [
            (Kind::Func, &collected.funcs),
            (Kind::Table, &collected.tables),
            (Kind::Memory, &collected.memories),
            (Kind::Global, &collected.globals),
            (Kind::Tag, &collected.tags),
        ] {
            for &reference in references {
                let site = resolution.resolve(kind, module_idx, reference)?;
                mark_item(
                    &mut live,
                    &mut work,
                    &elems_by_table,
                    &datas_by_memory,
                    kind,
                    site,
                );
            }
        }
        // Element and data segments are module-local (not importable).
        for &segment in &collected.elems {
            mark_elem(&mut live, &mut work, (module_idx, segment));
        }
        for &segment in &collected.datas {
            mark_data(&mut live, &mut work, (module_idx, segment));
        }
    }

    Ok(live)
}

/// Module-local indices referenced by one item.
#[derive(Default)]
struct Collected {
    funcs: Vec<u32>,
    tables: Vec<u32>,
    memories: Vec<u32>,
    globals: Vec<u32>,
    tags: Vec<u32>,
    elems: Vec<u32>,
    datas: Vec<u32>,
}

/// A [`Reencode`] implementation whose index hooks record every reference
/// (and map it to itself). Re-encoding an item into a throwaway section
/// thereby collects exactly the indices the item uses, with the operator
/// walking maintained upstream.
struct Collector<'a> {
    refs: &'a mut Collected,
}

impl Reencode for Collector<'_> {
    type Error = MergeError;

    fn function_index(&mut self, func: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.refs.funcs.push(func);
        Ok(func)
    }

    fn table_index(&mut self, table: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.refs.tables.push(table);
        Ok(table)
    }

    fn memory_index(&mut self, memory: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.refs.memories.push(memory);
        Ok(memory)
    }

    fn global_index(&mut self, global: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.refs.globals.push(global);
        Ok(global)
    }

    fn tag_index(&mut self, tag: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.refs.tags.push(tag);
        Ok(tag)
    }

    fn element_index(&mut self, element: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.refs.elems.push(element);
        Ok(element)
    }

    fn data_index(&mut self, data: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.refs.datas.push(data);
        Ok(data)
    }
}

/// Collect every module-local index referenced by `item`.
fn scan(module: &ParsedModule<'_>, item: Work) -> Result<Collected, MergeError> {
    let mut refs = Collected::default();
    let mut collector = Collector { refs: &mut refs };
    let in_module = |error| flatten_error(error, &module.name);
    match item {
        Work::FuncDef(_, def_index) => {
            let mut throwaway = wasm_encoder::CodeSection::new();
            collector
                .parse_function_body(&mut throwaway, module.code[def_index as usize].clone())
                .map_err(in_module)?;
        }
        Work::GlobalDef(_, def_index) => {
            let mut throwaway = wasm_encoder::GlobalSection::new();
            collector
                .parse_global(&mut throwaway, module.globals[def_index as usize].clone())
                .map_err(in_module)?;
        }
        Work::TableDef(_, def_index) => {
            let mut throwaway = wasm_encoder::TableSection::new();
            collector
                .parse_table(&mut throwaway, module.tables[def_index as usize].clone())
                .map_err(in_module)?;
        }
        Work::Elem(_, index) => {
            let mut throwaway = wasm_encoder::ElementSection::new();
            collector
                .parse_element(&mut throwaway, module.elements[index as usize].clone())
                .map_err(in_module)?;
        }
        Work::Data(_, index) => {
            let mut throwaway = wasm_encoder::DataSection::new();
            collector
                .parse_data(&mut throwaway, module.datas[index as usize].clone())
                .map_err(in_module)?;
        }
    }
    Ok(refs)
}
