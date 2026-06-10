//! Emission of the merged module: every section is rebuilt from the input
//! modules' items, re-encoded with merged indices.

use std::collections::{BTreeMap, HashSet};

use wasm_encoder::reencode::Reencode;
use wasm_encoder::{
    CodeSection, DataCountSection, DataSection, ElementSection, ExportSection, Function,
    FunctionSection, GlobalSection, ImportSection, Instruction, MemorySection, Module,
    StartSection, TableSection, TagSection, TypeSection,
};

use crate::inline::InlinePlan;
use crate::merge::{ExportConflictPolicy, ExportSelection, MergeError, MergeOptions};
use crate::parse::{Kind, ParsedModule};
use crate::prune::Liveness;
use crate::remap::{Remapper, flatten_error};
use crate::resolve::{Layout, Resolution, Site};
use crate::types::TypeCanon;

/// An export that survives export selection, with conflicts already resolved.
pub(crate) struct SurvivingExport {
    pub(crate) module: usize,
    pub(crate) name: String,
    pub(crate) kind: wasmparser::ExternalKind,
    /// The exported item's module-local index.
    pub(crate) index: u32,
}

/// Apply the export selection: which exports the merged module keeps, under
/// which names.
pub(crate) fn surviving_exports(
    parsed: &[ParsedModule<'_>],
    options: &MergeOptions,
) -> Result<Vec<SurvivingExport>, MergeError> {
    let mut survivors = Vec::new();
    match &options.exports {
        // Only the entry-point module's exports survive; its imports were
        // satisfied (where possible) by the other modules during resolution.
        // Export names within one module are already unique, so no conflict
        // handling is needed.
        ExportSelection::Entry(entry) => {
            let module_idx = parsed
                .iter()
                .position(|module| module.name == *entry)
                .ok_or_else(|| MergeError::UnknownEntryModule {
                    name: entry.clone(),
                })?;
            for export in &parsed[module_idx].exports {
                survivors.push(SurvivingExport {
                    module: module_idx,
                    name: export.name.to_string(),
                    kind: export.kind,
                    index: export.index,
                });
            }
        }
        ExportSelection::Union(conflict_policy) => {
            let mut export_names: HashSet<String> = HashSet::new();
            for (module_idx, input) in parsed.iter().enumerate() {
                for export in &input.exports {
                    let name = if export_names.contains(export.name) {
                        match conflict_policy {
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
                    export_names.insert(name.clone());
                    survivors.push(SurvivingExport {
                        module: module_idx,
                        name,
                        kind: export.kind,
                        index: export.index,
                    });
                }
            }
        }
    }
    Ok(survivors)
}

/// Everything `emit` needs besides the parsed modules and the layout.
pub(crate) struct EmitConfig<'a> {
    pub(crate) exports: &'a [SurvivingExport],
    pub(crate) live: Option<&'a Liveness>,
    pub(crate) canon: &'a TypeCanon,
    pub(crate) name_section: Option<&'a wasm_encoder::NameSection>,
    /// Record per-instruction offsets for every emitted function (for source
    /// maps); branch hints record their own functions regardless.
    pub(crate) track_offsets: bool,
    /// Embed this URL in a `sourceMappingURL` custom section.
    pub(crate) source_map_url: Option<&'a str>,
    /// Which functions to inline at their single call site.
    pub(crate) plan: &'a InlinePlan,
}

pub(crate) fn emit(
    parsed: &[ParsedModule<'_>],
    layout: &Layout,
    resolution: &mut Resolution<'_>,
    config: &EmitConfig<'_>,
) -> Result<(Vec<u8>, crate::sourcemap::CodeOffsets), MergeError> {
    let EmitConfig {
        exports,
        live,
        canon,
        name_section,
        track_offsets,
        source_map_url,
        plan,
    } = *config;
    let def_live = |kind: Kind, module: usize, def_index: u32| {
        live.is_none_or(|live| live.def(kind, module, def_index))
    };
    let elem_live = |module: usize, index: u32| live.is_none_or(|live| live.elem(module, index));
    let data_live = |module: usize, index: u32| live.is_none_or(|live| live.data(module, index));
    let remapper = |module_idx: usize| Remapper {
        module_name: &parsed[module_idx].name,
        tables: &layout.remaps[module_idx],
        instruction_offsets: None,
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
    // The synthetic combined start reuses an input's plain (func) type if one
    // exists, otherwise a fresh one goes at the end of the type section.
    let start_type = canon.empty_func.unwrap_or(canon.count);
    let synthetic_start = (start_functions.len() > 1).then_some((start_type, layout.func_count));

    let mut module = Module::new();

    // Types: the first occurrence of each distinct rec group, in canonical
    // order (duplicates across modules collapse onto the same indices).
    let mut types = TypeSection::new();
    for &(module_idx, group_idx) in &canon.unique_groups {
        let mut remapper = remapper(module_idx);
        remapper
            .parse_recursive_type_group(types.ty(), parsed[module_idx].types[group_idx].clone())
            .map_err(in_module(module_idx))?;
    }
    if synthetic_start.is_some() && canon.empty_func.is_none() {
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
        for (def_index, &type_index) in input.func_types.iter().enumerate() {
            let site = Site::Def {
                module: module_idx,
                def_index: def_index as u32,
            };
            // Inlined functions get neither a declaration nor a body.
            if !def_live(Kind::Func, module_idx, def_index as u32) || plan.get(site).is_some() {
                continue;
            }
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
        for (def_index, table) in input.tables.iter().enumerate() {
            if !def_live(Kind::Table, module_idx, def_index as u32) {
                continue;
            }
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
        for (def_index, &memory) in input.memories.iter().enumerate() {
            if !def_live(Kind::Memory, module_idx, def_index as u32) {
                continue;
            }
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
        for (def_index, &tag) in input.tags.iter().enumerate() {
            if !def_live(Kind::Tag, module_idx, def_index as u32) {
                continue;
            }
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

    let mut export_section = ExportSection::new();
    for export in exports {
        let kind = Kind::of_export(export.kind);
        let index = layout.remaps[export.module].kind(kind)[export.index as usize];
        let export_kind = remapper(export.module)
            .export_kind(export.kind)
            .map_err(in_module(export.module))?;
        export_section.export(&export.name, export_kind, index);
    }
    if !export_section.is_empty() {
        module.section(&export_section);
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
        for (index, element) in input.elements.iter().enumerate() {
            if !elem_live(module_idx, index as u32) {
                continue;
            }
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
    let live_datas: u32 = parsed
        .iter()
        .enumerate()
        .map(|(module_idx, input)| {
            (0..input.datas.len() as u32)
                .filter(|&index| data_live(module_idx, index))
                .count() as u32
        })
        .sum();
    if parsed.iter().any(|input| input.has_data_count) {
        module.section(&DataCountSection { count: live_datas });
    }

    // Code, while translating branch hints: a hint's offset (relative to its
    // function's body) is mapped through the per-instruction offsets recorded
    // during re-encoding — remapped indices can change instruction widths.
    // With offset tracking on, the same records are collected for every
    // function, for source-map translation.
    let mut code = CodeSection::new();
    let mut merged_hints: BTreeMap<u32, Vec<wasm_encoder::BranchHint>> = BTreeMap::new();
    let mut code_offsets: crate::sourcemap::CodeOffsets = Vec::new();
    let splice_ctx = SpliceContext {
        parsed,
        layout,
        plan,
    };
    for (module_idx, input) in parsed.iter().enumerate() {
        let import_count = input.import_count(Kind::Func);
        for (def_index, body) in input.code.iter().enumerate() {
            let site = Site::Def {
                module: module_idx,
                def_index: def_index as u32,
            };
            // Inlined functions are not emitted: their bodies live inside
            // their callers.
            if !def_live(Kind::Func, module_idx, def_index as u32) || plan.get(site).is_some() {
                continue;
            }
            let local_index = import_count + def_index as u32;
            let hints = input
                .branch_hints
                .iter()
                .find(|(func, _)| *func == local_index)
                .map(|(_, hints)| hints);
            let want_offsets = track_offsets || hints.is_some();

            // (module, input offset, output offset within the new body) per
            // emitted instruction; inlined instructions carry their own
            // source module.
            let mut offsets: Vec<(usize, usize, u32)> = Vec::new();
            let instances = if plan.is_empty() {
                Vec::new()
            } else {
                let mut instances = Vec::new();
                collect_instances(&splice_ctx, resolution, module_idx, body, &mut instances)?;
                instances
            };
            if instances.is_empty() {
                let mut pairs = Vec::new();
                let mut remapper = Remapper {
                    module_name: &input.name,
                    tables: &layout.remaps[module_idx],
                    instruction_offsets: want_offsets.then_some(&mut pairs),
                };
                remapper
                    .parse_function_body(&mut code, body.clone())
                    .map_err(in_module(module_idx))?;
                offsets.extend(
                    pairs
                        .into_iter()
                        .map(|(input, output)| (module_idx, input, output)),
                );
            } else {
                splice_function(
                    &splice_ctx,
                    resolution,
                    module_idx,
                    def_index as u32,
                    &instances,
                    &mut code,
                    want_offsets.then_some(&mut offsets),
                )?;
            }

            if let Some(hints) = hints {
                let body_start = body.range().start;
                let translated: Vec<_> = hints
                    .iter()
                    .filter_map(|hint| {
                        let input_offset = body_start + hint.func_offset as usize;
                        // Entries of inlined callees are interleaved, so find
                        // this function's own instruction by module + offset.
                        let (_, _, output) = offsets
                            .iter()
                            .find(|&&(m, input, _)| m == module_idx && input == input_offset)?;
                        Some(wasm_encoder::BranchHint {
                            branch_func_offset: *output,
                            branch_hint_value: hint.taken.into(),
                        })
                    })
                    .collect();
                if !translated.is_empty() {
                    let merged_index = layout.remaps[module_idx].funcs[local_index as usize];
                    merged_hints
                        .entry(merged_index)
                        .or_default()
                        .extend(translated);
                }
            }
            if track_offsets {
                code_offsets.push(offsets);
            }
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
    // The branch-hint custom section must precede the code section.
    if !merged_hints.is_empty() {
        let mut hints = wasm_encoder::BranchHints::new();
        for (function_index, function_hints) in &merged_hints {
            hints.function_hints(*function_index, function_hints.iter().copied());
        }
        module.section(&hints);
    }
    if !code.is_empty() {
        module.section(&code);
    }

    let mut data = DataSection::new();
    for (module_idx, input) in parsed.iter().enumerate() {
        let mut remapper = remapper(module_idx);
        for (index, datum) in input.datas.iter().enumerate() {
            if !data_live(module_idx, index as u32) {
                continue;
            }
            remapper
                .parse_data(&mut data, datum.clone())
                .map_err(in_module(module_idx))?;
        }
    }
    if !data.is_empty() {
        module.section(&data);
    }

    if let Some(names) = name_section {
        module.section(names);
    }

    if let Some(url) = source_map_url {
        let mut data = Vec::new();
        wasm_encoder::Encode::encode(url, &mut data);
        module.section(&wasm_encoder::CustomSection {
            name: "sourceMappingURL".into(),
            data: data.into(),
        });
    }

    Ok((module.finish(), code_offsets))
}

/// Shared state for splicing inlined bodies into their callers.
struct SpliceContext<'a, 'p> {
    parsed: &'a [ParsedModule<'p>],
    layout: &'a Layout,
    plan: &'a InlinePlan,
}

impl SpliceContext<'_, '_> {
    fn remapper(&self, module_idx: usize) -> Remapper<'_> {
        Remapper {
            module_name: &self.parsed[module_idx].name,
            tables: &self.layout.remaps[module_idx],
            instruction_offsets: None,
        }
    }

    fn invalid(&self, module_idx: usize) -> impl Fn(wasmparser::BinaryReaderError) -> MergeError {
        let name = self.parsed[module_idx].name.clone();
        move |source| MergeError::InvalidModule {
            name: name.clone(),
            source,
        }
    }
}

/// Find, in pre-order, every inlined call inside `body` (and transitively
/// inside the bodies it inlines) — the same traversal the splicer performs.
fn collect_instances(
    ctx: &SpliceContext<'_, '_>,
    resolution: &mut Resolution<'_>,
    module_idx: usize,
    body: &wasmparser::FunctionBody<'_>,
    out: &mut Vec<Site>,
) -> Result<(), MergeError> {
    let mut reader = body
        .get_operators_reader()
        .map_err(ctx.invalid(module_idx))?;
    while !reader.eof() {
        if let wasmparser::Operator::Call { function_index } =
            reader.read().map_err(ctx.invalid(module_idx))?
        {
            let target = resolution.resolve(Kind::Func, module_idx, function_index)?;
            if let Some(inlinee) = ctx.plan.get(target) {
                out.push(target);
                let callee_body = &ctx.parsed[inlinee.module].code[inlinee.def_index as usize];
                collect_instances(ctx, resolution, inlinee.module, callee_body, out)?;
            }
        }
    }
    Ok(())
}

/// Emit one function with its inlined callees spliced in.
fn splice_function(
    ctx: &SpliceContext<'_, '_>,
    resolution: &mut Resolution<'_>,
    module_idx: usize,
    def_index: u32,
    instances: &[Site],
    code: &mut CodeSection,
    mut offsets: Option<&mut Vec<(usize, usize, u32)>>,
) -> Result<(), MergeError> {
    let body = &ctx.parsed[module_idx].code[def_index as usize];

    // Locals: the caller's own declared locals, then one run per inline
    // instance (the callee's parameters followed by its declared locals).
    // Bases index the instances in pre-order, matching the walk below.
    let mut locals: Vec<(u32, wasm_encoder::ValType)> = Vec::new();
    let push_local =
        |locals: &mut Vec<(u32, wasm_encoder::ValType)>, ty: wasm_encoder::ValType| match locals
            .last_mut()
        {
            Some((count, last)) if *last == ty => *count += 1,
            _ => locals.push((1, ty)),
        };
    let mut total = ctx.plan.param_count(module_idx, def_index);
    let mut locals_reader = body.get_locals_reader().map_err(ctx.invalid(module_idx))?;
    for _ in 0..locals_reader.get_count() {
        let (count, ty) = locals_reader.read().map_err(ctx.invalid(module_idx))?;
        let ty = ctx
            .remapper(module_idx)
            .val_type(ty)
            .map_err(|error| flatten_error(error, &ctx.parsed[module_idx].name))?;
        for _ in 0..count {
            push_local(&mut locals, ty);
        }
        total += count;
    }
    let mut bases = Vec::with_capacity(instances.len());
    for &site in instances {
        let inlinee = ctx.plan.get(site).expect("instances come from the plan");
        bases.push(total);
        for &ty in inlinee.params.iter().chain(&inlinee.locals) {
            let ty = ctx
                .remapper(inlinee.module)
                .val_type(ty)
                .map_err(|error| flatten_error(error, &ctx.parsed[inlinee.module].name))?;
            push_local(&mut locals, ty);
            total += 1;
        }
    }

    let mut function = Function::new(locals);
    let mut next_instance = 0usize;
    let mut reader = body
        .get_operators_reader()
        .map_err(ctx.invalid(module_idx))?;
    walk(
        ctx,
        resolution,
        &mut function,
        module_idx,
        &mut reader,
        0,
        false,
        &bases,
        &mut next_instance,
        &mut offsets,
    )?;
    code.function(&function);
    Ok(())
}

/// Re-encode one body into `function`, splicing inlined callees at their call
/// sites. Within an inlinee, locals shift by `local_base` and `return`
/// becomes a branch to the wrapper block (whose depth is tracked here).
#[expect(clippy::too_many_arguments)]
fn walk(
    ctx: &SpliceContext<'_, '_>,
    resolution: &mut Resolution<'_>,
    function: &mut Function,
    module_idx: usize,
    reader: &mut wasmparser::OperatorsReader<'_>,
    local_base: u32,
    inlinee: bool,
    bases: &[u32],
    next_instance: &mut usize,
    offsets: &mut Option<&mut Vec<(usize, usize, u32)>>,
) -> Result<(), MergeError> {
    use wasmparser::Operator;

    let mut depth: u32 = 0;
    while !reader.eof() {
        let input_offset = reader.original_position();
        let operator = reader.clone().read().map_err(ctx.invalid(module_idx))?;
        match operator {
            Operator::Call { function_index } => {
                let target = resolution.resolve(Kind::Func, module_idx, function_index)?;
                if let Some(inl) = ctx.plan.get(target) {
                    reader.read().map_err(ctx.invalid(module_idx))?; // consume the call
                    let base = bases[*next_instance];
                    *next_instance += 1;
                    let in_callee = |error| flatten_error(error, &ctx.parsed[inl.module].name);

                    // Arguments are on the stack with the last parameter on
                    // top: assign the fresh locals in reverse.
                    for param in (0..inl.params.len() as u32).rev() {
                        if let Some(o) = offsets.as_deref_mut() {
                            o.push((module_idx, input_offset, function.byte_len() as u32));
                        }
                        function.instruction(&Instruction::LocalSet(base + param));
                    }
                    // The call site may execute more than once, but locals
                    // are only zero-initialised at function entry: re-zero
                    // the defaultable declared locals, except where the plan
                    // proved the local is written before any read (or never
                    // read). Non-defaultable ones are written before read by
                    // validation.
                    for (index, &ty) in inl.locals.iter().enumerate() {
                        if !inl.zero_locals[index] {
                            continue;
                        }
                        let Some(zero) =
                            zero_value(&mut ctx.remapper(inl.module), ty).map_err(in_callee)?
                        else {
                            continue;
                        };
                        let local = base + inl.params.len() as u32 + index as u32;
                        if let Some(o) = offsets.as_deref_mut() {
                            o.push((module_idx, input_offset, function.byte_len() as u32));
                        }
                        function.instruction(&zero);
                        function.instruction(&Instruction::LocalSet(local));
                    }
                    // A body containing `return` is wrapped in a block; each
                    // `return` becomes a branch to it.
                    if inl.has_return {
                        let block_type = match inl.results[..] {
                            [] => wasm_encoder::BlockType::Empty,
                            [ty] => wasm_encoder::BlockType::Result(
                                ctx.remapper(inl.module).val_type(ty).map_err(in_callee)?,
                            ),
                            _ => unreachable!("excluded by inline::plan"),
                        };
                        if let Some(o) = offsets.as_deref_mut() {
                            o.push((module_idx, input_offset, function.byte_len() as u32));
                        }
                        function.instruction(&Instruction::Block(block_type));
                    }
                    let callee_body = &ctx.parsed[inl.module].code[inl.def_index as usize];
                    let mut callee_reader = callee_body
                        .get_operators_reader()
                        .map_err(ctx.invalid(inl.module))?;
                    walk(
                        ctx,
                        resolution,
                        function,
                        inl.module,
                        &mut callee_reader,
                        base,
                        true,
                        bases,
                        next_instance,
                        offsets,
                    )?;
                    if inl.has_return {
                        function.instruction(&Instruction::End);
                    }
                    continue;
                }
            }
            Operator::Return if inlinee => {
                reader.read().map_err(ctx.invalid(module_idx))?;
                if let Some(o) = offsets.as_deref_mut() {
                    o.push((module_idx, input_offset, function.byte_len() as u32));
                }
                function.instruction(&Instruction::Br(depth));
                continue;
            }
            Operator::End => {
                if depth == 0 {
                    // The body's final `end`: an inlinee's is dropped (the
                    // wrapper block supplies its own), the caller's is kept.
                    reader.read().map_err(ctx.invalid(module_idx))?;
                    if !inlinee {
                        if let Some(o) = offsets.as_deref_mut() {
                            o.push((module_idx, input_offset, function.byte_len() as u32));
                        }
                        function.instruction(&Instruction::End);
                    }
                    return Ok(());
                }
                depth -= 1;
            }
            Operator::Block { .. }
            | Operator::Loop { .. }
            | Operator::If { .. }
            | Operator::TryTable { .. }
            | Operator::Try { .. } => depth += 1,
            Operator::Delegate { .. } => depth = depth.saturating_sub(1),
            _ => {}
        }

        let instruction = ctx
            .remapper(module_idx)
            .parse_instruction(reader)
            .map_err(|error| flatten_error(error, &ctx.parsed[module_idx].name))?;
        let instruction = shift_locals(instruction, local_base);
        if let Some(o) = offsets.as_deref_mut() {
            o.push((module_idx, input_offset, function.byte_len() as u32));
        }
        function.instruction(&instruction);
    }
    Err(MergeError::Reencode {
        module: ctx.parsed[module_idx].name.clone(),
        message: "function body ended without a final end".to_string(),
    })
}

/// Shift local indices by an inline instance's base.
fn shift_locals(instruction: Instruction<'_>, base: u32) -> Instruction<'_> {
    if base == 0 {
        return instruction;
    }
    match instruction {
        Instruction::LocalGet(index) => Instruction::LocalGet(index + base),
        Instruction::LocalSet(index) => Instruction::LocalSet(index + base),
        Instruction::LocalTee(index) => Instruction::LocalTee(index + base),
        other => other,
    }
}

/// The zero value for a defaultable type, or `None` for non-defaultable ones.
fn zero_value(
    remapper: &mut Remapper<'_>,
    ty: wasmparser::ValType,
) -> Result<Option<Instruction<'static>>, wasm_encoder::reencode::Error<MergeError>> {
    use wasmparser::ValType;
    Ok(match ty {
        ValType::I32 => Some(Instruction::I32Const(0)),
        ValType::I64 => Some(Instruction::I64Const(0)),
        ValType::F32 => Some(Instruction::F32Const(0.0f32.into())),
        ValType::F64 => Some(Instruction::F64Const(0.0f64.into())),
        ValType::V128 => Some(Instruction::V128Const(0)),
        ValType::Ref(ref_type) if ref_type.is_nullable() => Some(Instruction::RefNull(
            remapper.heap_type(ref_type.heap_type())?,
        )),
        ValType::Ref(_) => None,
    })
}
