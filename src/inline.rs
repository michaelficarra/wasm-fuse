//! Opt-in inlining of functions with a single call site
//! (`MergeOptions::inline_single_use`).
//!
//! A function qualifies when, across all emitted code, it is the target of
//! exactly one plain `call` and of nothing else: not exported, not a start
//! function, never referenced by `ref.func`, a tail call, or an element
//! segment, and not (mutually) recursive. Tail calls *inside* the body also
//! disqualify it (inlining would lose the constant-stack guarantee), as does
//! a body that mixes explicit `return` with multiple results (the wrapper
//! block could not be typed without synthesising a function type).
//!
//! At the call site the splicer (in emit.rs) materialises the callee's
//! parameters as fresh locals in the caller, assigned from the stack in
//! reverse order; declared defaultable locals are explicitly re-zeroed, since
//! the call site may execute more than once while locals are only
//! zero-initialised at function entry (non-defaultable locals are safe
//! as-is: validation guarantees they are written before read). A body
//! containing `return` is wrapped in a block, and each `return` becomes a
//! `br` to it. Inlined functions are dropped from the output entirely —
//! by construction nothing references them afterwards.

use std::collections::{HashMap, HashSet};

use wasm_encoder::reencode::Reencode;
use wasmparser::{CompositeInnerType, Operator, ValType};

use crate::emit::SurvivingExport;
use crate::merge::MergeError;
use crate::parse::{Kind, ParsedModule};
use crate::prune::Liveness;
use crate::resolve::{Resolution, Site};

/// Appending thousands of locals to a caller would be pathological; the
/// validator's own limit on locals per function is the natural bound.
const MAX_INLINED_LOCALS: usize = 50_000;

/// Everything the splicer needs to know about one inlined function.
pub(crate) struct InlinedFunction {
    pub(crate) module: usize,
    pub(crate) def_index: u32,
    /// Parameter types, in the callee's (module-local) terms.
    pub(crate) params: Vec<ValType>,
    /// Declared (non-parameter) local types, flattened, module-local.
    pub(crate) locals: Vec<ValType>,
    /// Aligned with `locals`: whether the splice must re-zero the local. A
    /// local that is never read, or whose first read comes after a depth-0
    /// write (which dominates everything textually after it in structured
    /// control flow), keeps whatever value it holds unobserved.
    pub(crate) zero_locals: Vec<bool>,
    /// Result types, module-local.
    pub(crate) results: Vec<ValType>,
    /// Whether the body contains an explicit `return`.
    pub(crate) has_return: bool,
}

#[derive(Default)]
pub(crate) struct InlinePlan {
    functions: HashMap<Site, InlinedFunction>,
    /// Parameter counts of every defined function, per module — the splicer
    /// needs the caller's count to place appended locals.
    param_counts: Vec<Vec<u32>>,
}

impl InlinePlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    pub(crate) fn get(&self, site: Site) -> Option<&InlinedFunction> {
        self.functions.get(&site)
    }

    pub(crate) fn sites(&self) -> HashSet<Site> {
        self.functions.keys().copied().collect()
    }

    pub(crate) fn param_count(&self, module: usize, def_index: u32) -> u32 {
        self.param_counts[module][def_index as usize]
    }
}

/// Decide which functions to inline.
pub(crate) fn plan(
    parsed: &[ParsedModule<'_>],
    resolution: &mut Resolution<'_>,
    exports: &[SurvivingExport],
    live: Option<&Liveness>,
) -> Result<InlinePlan, MergeError> {
    let def_live = |module: usize, def_index: u32| {
        live.is_none_or(|live| live.def(Kind::Func, module, def_index))
    };

    // Function types per module, flattened from rec groups.
    let func_types: Vec<Vec<Option<&wasmparser::FuncType>>> = parsed
        .iter()
        .map(|module| {
            let flat: Vec<_> = module
                .types
                .iter()
                .flat_map(|group| group.types())
                .collect();
            module
                .func_types
                .iter()
                .map(|&type_index| {
                    match flat
                        .get(type_index as usize)
                        .map(|s| &s.composite_type.inner)
                    {
                        Some(CompositeInnerType::Func(func_type)) => Some(func_type),
                        _ => None,
                    }
                })
                .collect()
        })
        .collect();
    let param_counts: Vec<Vec<u32>> = func_types
        .iter()
        .map(|module| {
            module
                .iter()
                .map(|ty| ty.map_or(0, |ty| ty.params().len() as u32))
                .collect()
        })
        .collect();

    // Count direct call sites and find everything that blocks inlining.
    let mut counts: HashMap<Site, u32> = HashMap::new();
    let mut caller: HashMap<Site, Site> = HashMap::new();
    let mut blocked: HashSet<Site> = HashSet::new();

    for (module_idx, module) in parsed.iter().enumerate() {
        for (def_index, body) in module.code.iter().enumerate() {
            if !def_live(module_idx, def_index as u32) {
                continue;
            }
            let container = Site::Def {
                module: module_idx,
                def_index: def_index as u32,
            };
            let mut operators =
                body.get_operators_reader()
                    .map_err(|source| MergeError::InvalidModule {
                        name: module.name.clone(),
                        source,
                    })?;
            while !operators.eof() {
                let operator = operators
                    .read()
                    .map_err(|source| MergeError::InvalidModule {
                        name: module.name.clone(),
                        source,
                    })?;
                match operator {
                    Operator::Call { function_index } => {
                        let target = resolution.resolve(Kind::Func, module_idx, function_index)?;
                        *counts.entry(target).or_insert(0) += 1;
                        caller.insert(target, container);
                    }
                    Operator::ReturnCall { function_index }
                    | Operator::RefFunc { function_index } => {
                        let target = resolution.resolve(Kind::Func, module_idx, function_index)?;
                        blocked.insert(target);
                    }
                    _ => {}
                }
            }
        }
        // References from outside function bodies: element segments, global
        // and table initialisers. The recording reencoder from prune.rs
        // collects every function index these mention.
        let mut refs = crate::prune::Collected::default();
        let mut collector = crate::prune::Collector { refs: &mut refs };
        let lenient = |_: wasm_encoder::reencode::Error<MergeError>| ();
        for element in &module.elements {
            let mut throwaway = wasm_encoder::ElementSection::new();
            let _ = collector
                .parse_element(&mut throwaway, element.clone())
                .map_err(lenient);
        }
        for global in &module.globals {
            let mut throwaway = wasm_encoder::GlobalSection::new();
            let _ = collector
                .parse_global(&mut throwaway, global.clone())
                .map_err(lenient);
        }
        for table in &module.tables {
            let mut throwaway = wasm_encoder::TableSection::new();
            let _ = collector
                .parse_table(&mut throwaway, table.clone())
                .map_err(lenient);
        }
        for function_index in refs.funcs {
            blocked.insert(resolution.resolve(Kind::Func, module_idx, function_index)?);
        }
        if let Some(start) = module.start {
            blocked.insert(resolution.resolve(Kind::Func, module_idx, start)?);
        }
    }
    for export in exports {
        if Kind::of_export(export.kind) == Kind::Func {
            blocked.insert(resolution.resolve(Kind::Func, export.module, export.index)?);
        }
    }

    // Select candidates.
    let mut functions: HashMap<Site, InlinedFunction> = HashMap::new();
    for (&site, &count) in &counts {
        let Site::Def { module, def_index } = site else {
            continue; // imports cannot be inlined
        };
        if count != 1 || blocked.contains(&site) || caller.get(&site) == Some(&site) {
            continue;
        }
        if !def_live(module, def_index) {
            continue;
        }
        let Some(func_type) = func_types[module][def_index as usize] else {
            continue;
        };

        // Scan the body: explicit returns are fine (the splicer rewrites
        // them), tail calls are not.
        let parsed_module = &parsed[module];
        let body = &parsed_module.code[def_index as usize];
        let invalid = |source| MergeError::InvalidModule {
            name: parsed_module.name.clone(),
            source,
        };
        let mut has_return = false;
        let mut has_tail_call = false;
        // Per local: position of the first read anywhere, and of the first
        // depth-0 write (which dominates all later instructions).
        let mut first_read: HashMap<u32, usize> = HashMap::new();
        let mut first_dominating_write: HashMap<u32, usize> = HashMap::new();
        let mut depth = 0u32;
        let mut position = 0usize;
        let mut operators = body.get_operators_reader().map_err(invalid)?;
        while !operators.eof() {
            match operators.read().map_err(invalid)? {
                Operator::Return => has_return = true,
                Operator::ReturnCall { .. }
                | Operator::ReturnCallIndirect { .. }
                | Operator::ReturnCallRef { .. } => has_tail_call = true,
                Operator::LocalGet { local_index } => {
                    first_read.entry(local_index).or_insert(position);
                }
                Operator::LocalSet { local_index } | Operator::LocalTee { local_index }
                    if depth == 0 =>
                {
                    first_dominating_write
                        .entry(local_index)
                        .or_insert(position);
                }
                Operator::Block { .. }
                | Operator::Loop { .. }
                | Operator::If { .. }
                | Operator::TryTable { .. }
                | Operator::Try { .. } => depth += 1,
                Operator::End | Operator::Delegate { .. } => depth = depth.saturating_sub(1),
                _ => {}
            }
            position += 1;
        }
        if has_tail_call || (has_return && func_type.results().len() > 1) {
            continue;
        }

        let mut locals = Vec::new();
        let mut locals_reader = body.get_locals_reader().map_err(invalid)?;
        let mut oversized = false;
        for _ in 0..locals_reader.get_count() {
            let (count, ty) = locals_reader.read().map_err(invalid)?;
            if locals.len() + count as usize > MAX_INLINED_LOCALS {
                oversized = true;
                break;
            }
            locals.extend(std::iter::repeat_n(ty, count as usize));
        }
        if oversized {
            continue;
        }

        let zero_locals = (0..locals.len() as u32)
            .map(|declared| {
                let local = func_type.params().len() as u32 + declared;
                match (first_read.get(&local), first_dominating_write.get(&local)) {
                    (None, _) => false, // never read: any stale value is unobserved
                    (Some(_), None) => true,
                    (Some(&read), Some(&write)) => read < write,
                }
            })
            .collect();
        functions.insert(
            site,
            InlinedFunction {
                module,
                def_index,
                params: func_type.params().to_vec(),
                locals,
                zero_locals,
                results: func_type.results().to_vec(),
                has_return,
            },
        );
    }

    // Mutually-recursive candidates (A's only call in B, B's only call in A)
    // would expand forever; walking each candidate's caller chain finds and
    // drops such cycles.
    let mut in_cycle: HashSet<Site> = HashSet::new();
    for &start in functions.keys() {
        let mut path = Vec::new();
        let mut current = start;
        while functions.contains_key(&current) && !in_cycle.contains(&current) {
            if let Some(position) = path.iter().position(|&site| site == current) {
                in_cycle.extend(&path[position..]);
                break;
            }
            path.push(current);
            match caller.get(&current) {
                Some(&next) => current = next,
                None => break,
            }
        }
    }
    for site in in_cycle {
        functions.remove(&site);
    }

    Ok(InlinePlan {
        functions,
        param_counts,
    })
}
