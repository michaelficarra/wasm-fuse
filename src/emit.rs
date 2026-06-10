//! Emission of the merged module: every section is rebuilt from the input
//! modules' items, re-encoded with merged indices.

use std::collections::HashSet;

use wasm_encoder::reencode::Reencode;
use wasm_encoder::{
    CodeSection, DataCountSection, DataSection, ElementSection, ExportSection, Function,
    FunctionSection, GlobalSection, ImportSection, Instruction, MemorySection, Module,
    StartSection, TableSection, TagSection, TypeSection,
};

use crate::merge::{ExportConflictPolicy, MergeError, MergeOptions};
use crate::parse::{Kind, ParsedModule};
use crate::remap::{Remapper, flatten_error};
use crate::resolve::Layout;

pub(crate) fn emit(
    parsed: &[ParsedModule<'_>],
    layout: &Layout,
    options: &MergeOptions,
) -> Result<Vec<u8>, MergeError> {
    let remapper = |module_idx: usize| Remapper {
        module_name: &parsed[module_idx].name,
        tables: &layout.remaps[module_idx],
    };
    // Attribute re-encoding failures to the module whose item we're encoding.
    let in_module = |module_idx: usize| move |error| flatten_error(error, &parsed[module_idx].name);

    // Starts of all modules, in module order; several get fused into one
    // synthetic function that calls each in turn.
    let mut start_functions = Vec::new();
    for (module_idx, module) in parsed.iter().enumerate() {
        if let Some(start) = module.start {
            start_functions.push(
                remapper(module_idx)
                    .function_index(start)
                    .map_err(in_module(module_idx))?,
            );
        }
    }
    let total_types: u32 = parsed.iter().map(|module| module.type_count()).sum();
    let total_funcs: u32 = layout.canonical_imports[Kind::Func].len() as u32
        + parsed
            .iter()
            .map(|module| module.defined_count(Kind::Func))
            .sum::<u32>();
    let synthetic_start = (start_functions.len() > 1).then_some((total_types, total_funcs));

    let mut module = Module::new();

    // Types: all rec groups in module order, plus () -> () for the synthetic
    // start function if one is needed.
    let mut types = TypeSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for group in &input.types {
            remapper
                .parse_recursive_type_group(types.ty(), group.clone())
                .map_err(in_module(module_idx))?;
        }
    }
    if synthetic_start.is_some() {
        types.ty().function([], []);
    }
    if !types.is_empty() {
        module.section(&types);
    }

    // Imports that survived resolution, grouped by kind in merged-index order.
    let mut imports = ImportSection::new();
    for kind in Kind::ALL {
        for &(module_idx, slot) in &layout.canonical_imports[kind] {
            let import = parsed[module_idx].imports[kind][slot as usize];
            let entity = remapper(module_idx)
                .entity_type(import.ty)
                .map_err(in_module(module_idx))?;
            imports.import(import.module, import.name, entity);
        }
    }
    if !imports.is_empty() {
        module.section(&imports);
    }

    let mut functions = FunctionSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for &type_index in &input.func_types {
            functions.function(
                remapper
                    .type_index(type_index)
                    .map_err(in_module(module_idx))?,
            );
        }
    }
    if let Some((start_type, _)) = synthetic_start {
        functions.function(start_type);
    }
    if !functions.is_empty() {
        module.section(&functions);
    }

    let mut tables = TableSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for table in &input.tables {
            remapper
                .parse_table(&mut tables, table.clone())
                .map_err(in_module(module_idx))?;
        }
    }
    if !tables.is_empty() {
        module.section(&tables);
    }

    let mut memories = MemorySection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for &memory in &input.memories {
            memories.memory(
                remapper
                    .memory_type(memory)
                    .map_err(in_module(module_idx))?,
            );
        }
    }
    if !memories.is_empty() {
        module.section(&memories);
    }

    let mut tags = TagSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for &tag in &input.tags {
            tags.tag(remapper.tag_type(tag).map_err(in_module(module_idx))?);
        }
    }
    if !tags.is_empty() {
        module.section(&tags);
    }

    // Globals are emitted in dependency order (see resolve::order_globals).
    let mut globals = GlobalSection::new();
    for &(module_idx, def_index) in &layout.global_order {
        let global = parsed[module_idx].globals[def_index as usize].clone();
        remapper(module_idx)
            .parse_global(&mut globals, global)
            .map_err(in_module(module_idx))?;
    }
    if !globals.is_empty() {
        module.section(&globals);
    }

    let mut exports = ExportSection::new();
    let mut export_names: HashSet<String> = HashSet::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        for export in &input.exports {
            let kind = Kind::of_export(export.kind);
            let name = if export_names.contains(export.name) {
                match options.export_conflicts {
                    ExportConflictPolicy::Error => {
                        return Err(MergeError::ExportConflict {
                            name: export.name.to_string(),
                        });
                    }
                    ExportConflictPolicy::Skip => continue,
                    ExportConflictPolicy::Rename => {
                        // Probe name_1, name_2, ... like binaryen's
                        // Names::getValidExportName.
                        let mut suffix = 1u32;
                        loop {
                            let candidate = format!("{}_{suffix}", export.name);
                            if !export_names.contains(&candidate) {
                                break candidate;
                            }
                            suffix += 1;
                        }
                    }
                }
            } else {
                export.name.to_string()
            };
            let index = layout.remaps[module_idx].kind(kind)[export.index as usize];
            let export_kind = remapper(module_idx)
                .export_kind(export.kind)
                .map_err(in_module(module_idx))?;
            exports.export(&name, export_kind, index);
            export_names.insert(name);
        }
    }
    if !exports.is_empty() {
        module.section(&exports);
    }

    match (synthetic_start, start_functions.as_slice()) {
        (Some((_, start_func)), _) => {
            module.section(&StartSection {
                function_index: start_func,
            });
        }
        (None, &[start_func]) => {
            module.section(&StartSection {
                function_index: start_func,
            });
        }
        (None, _) => {}
    }

    let mut elements = ElementSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for element in &input.elements {
            remapper
                .parse_element(&mut elements, element.clone())
                .map_err(in_module(module_idx))?;
        }
    }
    if !elements.is_empty() {
        module.section(&elements);
    }

    // The data-count section is required by bulk-memory instructions; emit one
    // whenever any input carried one.
    let total_datas: u32 = parsed.iter().map(|input| input.datas.len() as u32).sum();
    if parsed.iter().any(|input| input.has_data_count) {
        module.section(&DataCountSection { count: total_datas });
    }

    let mut code = CodeSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for body in &input.code {
            remapper
                .parse_function_body(&mut code, body.clone())
                .map_err(in_module(module_idx))?;
        }
    }
    if synthetic_start.is_some() {
        let mut start = Function::new([]);
        for &function_index in &start_functions {
            start.instruction(&Instruction::Call(function_index));
        }
        start.instruction(&Instruction::End);
        code.function(&start);
    }
    if !code.is_empty() {
        module.section(&code);
    }

    let mut data = DataSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for datum in &input.datas {
            remapper
                .parse_data(&mut data, datum.clone())
                .map_err(in_module(module_idx))?;
        }
    }
    if !data.is_empty() {
        module.section(&data);
    }

    Ok(module.finish())
}
