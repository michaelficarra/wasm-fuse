//! Import resolution and merged-module index layout.
//!
//! Every reference in an input module is classified to a *site*: either a
//! definition in some module, or an import that stays an import because it
//! refers to a module (or export) outside the input set. Imports whose module
//! name matches an input module are chased through that module's exports —
//! transitively, since an export may itself re-export another import — and
//! everything that resolves to the same site shares one slot in the merged
//! module.

use std::collections::HashMap;

use wasmparser::Operator;

use crate::merge::MergeError;
use crate::parse::{Kind, KindMap, ParsedModule};
use crate::prune::Liveness;
use crate::remap::{PRUNED, RemapTables};
use crate::types::TypeCanon;

/// Where a reference ultimately lands after chasing fused imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Site {
    /// The `def_index`th definition (not counting imports) in a module.
    Def { module: usize, def_index: u32 },
    /// The `slot`th import of its kind in a module, kept as an import in the
    /// merged output.
    Import { module: usize, slot: u32 },
}

pub(crate) struct Resolution<'a> {
    parsed: &'a [ParsedModule<'a>],
    module_by_name: HashMap<&'a str, usize>,
    sites: HashMap<(Kind, usize, u32), Site>,
}

impl<'a> Resolution<'a> {
    pub(crate) fn new(parsed: &'a [ParsedModule<'a>]) -> Self {
        let module_by_name = parsed
            .iter()
            .enumerate()
            .map(|(position, module)| (module.name.as_str(), position))
            .collect();
        Resolution {
            parsed,
            module_by_name,
            sites: HashMap::new(),
        }
    }

    /// Resolve `index` in `module`'s index space of `kind` to its site.
    pub(crate) fn resolve(
        &mut self,
        kind: Kind,
        module: usize,
        index: u32,
    ) -> Result<Site, MergeError> {
        if let Some(&site) = self.sites.get(&(kind, module, index)) {
            return Ok(site);
        }

        let mut visited = Vec::new();
        let mut current = (module, index);
        let site = loop {
            let (cur_module, cur_index) = current;
            let imports = &self.parsed[cur_module].imports[kind];
            let Some(import) = imports.get(cur_index as usize) else {
                // Past the imports: a definition in this module.
                break Site::Def {
                    module: cur_module,
                    def_index: cur_index - imports.len() as u32,
                };
            };
            // An import of a module outside the input set — or of an input
            // module that doesn't export the requested item — stays an import.
            let Some(&target_module) = self.module_by_name.get(import.module) else {
                break Site::Import {
                    module: cur_module,
                    slot: cur_index,
                };
            };
            let Some(&target_index) = self.parsed[target_module]
                .export_map
                .get(&(kind, import.name))
            else {
                break Site::Import {
                    module: cur_module,
                    slot: cur_index,
                };
            };
            if visited.contains(&current) {
                return Err(MergeError::ImportLoop {
                    name: import.name.to_string(),
                });
            }
            visited.push(current);
            current = (target_module, target_index);
        };

        self.sites.insert((kind, module, index), site);
        Ok(site)
    }
}

/// The complete index layout of the merged module.
pub(crate) struct Layout {
    /// Imports that survive into the output, as (module, slot) pairs per kind,
    /// in merged index order.
    pub(crate) canonical_imports: KindMap<Vec<(usize, u32)>>,
    /// Defined globals in their final (dependency-ordered, live-only) emission
    /// order, as (module, def_index) pairs.
    pub(crate) global_order: Vec<(usize, u32)>,
    /// Per input module, the old-index → merged-index tables for every index
    /// space. Pruned items map to [`PRUNED`](crate::remap::PRUNED); live items
    /// never reference them.
    pub(crate) remaps: Vec<RemapTables>,
    /// The total number of functions in the output (imports plus surviving
    /// definitions) — the index a synthetic combined start function gets.
    pub(crate) func_count: u32,
}

/// Compute the merged layout: assign merged indices to every surviving import
/// and every definition, and build the per-module remap tables. With `live`
/// set, items outside the live set get no index (pruning).
pub(crate) fn layout(
    parsed: &[ParsedModule<'_>],
    resolution: &mut Resolution<'_>,
    live: Option<&Liveness>,
    canon: &TypeCanon,
) -> Result<Layout, MergeError> {
    let item_live = |kind: Kind, site: Site| live.is_none_or(|live| live.item(kind, site));

    // Surviving imports come first in each index space, in module order. An
    // import survives only if it resolves to itself (anything else remaps to
    // the site it resolved to) and is reachable.
    let mut canonical_imports: KindMap<Vec<(usize, u32)>> = KindMap::default();
    let mut import_index: HashMap<(Kind, usize, u32), u32> = HashMap::new();
    for kind in Kind::ALL {
        for (module_idx, module) in parsed.iter().enumerate() {
            for slot in 0..module.import_count(kind) {
                let site = resolution.resolve(kind, module_idx, slot)?;
                if site
                    == (Site::Import {
                        module: module_idx,
                        slot,
                    })
                    && item_live(kind, site)
                {
                    import_index.insert(
                        (kind, module_idx, slot),
                        canonical_imports[kind].len() as u32,
                    );
                    canonical_imports[kind].push((module_idx, slot));
                }
            }
        }
    }

    // Surviving definitions follow, grouped by module in input order. Each
    // definition's rank counts only the live definitions before it in its
    // module.
    let mut def_base: Vec<KindMap<u32>> = Vec::with_capacity(parsed.len());
    let mut def_rank: Vec<KindMap<Vec<u32>>> = Vec::with_capacity(parsed.len());
    let mut next_def: KindMap<u32> = KindMap::default();
    for kind in Kind::ALL {
        next_def[kind] = canonical_imports[kind].len() as u32;
    }
    for (module_idx, module) in parsed.iter().enumerate() {
        let mut base = KindMap::default();
        let mut ranks = KindMap::default();
        for kind in Kind::ALL {
            base[kind] = next_def[kind];
            let mut rank = Vec::with_capacity(module.defined_count(kind) as usize);
            for def_index in 0..module.defined_count(kind) {
                if item_live(
                    kind,
                    Site::Def {
                        module: module_idx,
                        def_index,
                    },
                ) {
                    rank.push(next_def[kind] - base[kind]);
                    next_def[kind] += 1;
                } else {
                    rank.push(PRUNED);
                }
            }
            ranks[kind] = rank;
        }
        def_base.push(base);
        def_rank.push(ranks);
    }
    let func_count = next_def[Kind::Func];

    // Globals get their indices not from module order but from dependency
    // order: initialisers may only read globals defined earlier in the
    // section, and fusing can introduce forward references (an earlier
    // module's import resolved to a later module's global). Order all defined
    // globals topologically, then keep the live ones.
    let global_order: Vec<(usize, u32)> = order_globals(parsed, resolution)?
        .into_iter()
        .filter(|&(module, def_index)| item_live(Kind::Global, Site::Def { module, def_index }))
        .collect();
    let global_import_count = canonical_imports[Kind::Global].len() as u32;
    let mut global_def_position: HashMap<(usize, u32), u32> = HashMap::new();
    for (position, &(module_idx, def_index)) in global_order.iter().enumerate() {
        global_def_position.insert((module_idx, def_index), position as u32);
    }

    // Element- and data-segment spaces cannot be imported, so they remap by
    // concatenation, skipping pruned segments. Types remap to their canonical
    // indices (deduplicated across modules, never pruned).
    let mut remaps = Vec::with_capacity(parsed.len());
    let mut element_next = 0u32;
    let mut data_next = 0u32;
    for (module_idx, module) in parsed.iter().enumerate() {
        let elem_live = |index: u32| live.is_none_or(|live| live.elem(module_idx, index));
        let data_live = |index: u32| live.is_none_or(|live| live.data(module_idx, index));
        let mut tables = RemapTables {
            types: canon.maps[module_idx].clone(),
            ..RemapTables::default()
        };
        for index in 0..module.elements.len() as u32 {
            tables.elements.push(if elem_live(index) {
                element_next += 1;
                element_next - 1
            } else {
                PRUNED
            });
        }
        for index in 0..module.datas.len() as u32 {
            tables.datas.push(if data_live(index) {
                data_next += 1;
                data_next - 1
            } else {
                PRUNED
            });
        }

        for kind in Kind::ALL {
            let map = tables.kind_mut(kind);
            for index in 0..module.item_count(kind) {
                let merged = match resolution.resolve(kind, module_idx, index)? {
                    Site::Import { module, slot } => import_index
                        .get(&(kind, module, slot))
                        .copied()
                        .unwrap_or(PRUNED),
                    Site::Def { module, def_index } => {
                        if kind == Kind::Global {
                            global_def_position
                                .get(&(module, def_index))
                                .map(|&position| global_import_count + position)
                                .unwrap_or(PRUNED)
                        } else {
                            match def_rank[module][kind][def_index as usize] {
                                PRUNED => PRUNED,
                                rank => def_base[module][kind] + rank,
                            }
                        }
                    }
                };
                map.push(merged);
            }
        }

        remaps.push(tables);
    }

    Ok(Layout {
        canonical_imports,
        global_order,
        remaps,
        func_count,
    })
}

/// Order defined globals so that every global's initialiser only reads
/// globals that appear before it (imports always precede definitions, so only
/// definition-to-definition dependencies constrain the order).
fn order_globals(
    parsed: &[ParsedModule<'_>],
    resolution: &mut Resolution<'_>,
) -> Result<Vec<(usize, u32)>, MergeError> {
    // Defined globals in pre-reorder (module, def_index) order.
    let mut nodes = Vec::new();
    let mut node_position: HashMap<(usize, u32), usize> = HashMap::new();
    for (module_idx, module) in parsed.iter().enumerate() {
        for def_index in 0..module.globals.len() as u32 {
            node_position.insert((module_idx, def_index), nodes.len());
            nodes.push((module_idx, def_index));
        }
    }

    // dependencies[n] = positions of defined globals read by n's initialiser
    let mut dependencies: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (position, &(module_idx, def_index)) in nodes.iter().enumerate() {
        let global = &parsed[module_idx].globals[def_index as usize];
        let mut operators = global.init_expr.get_operators_reader();
        while !operators.eof() {
            let operator = operators
                .read()
                .map_err(|source| MergeError::InvalidModule {
                    name: parsed[module_idx].name.clone(),
                    source,
                })?;
            if let Operator::GlobalGet { global_index } = operator
                && let Site::Def { module, def_index } =
                    resolution.resolve(Kind::Global, module_idx, global_index)?
            {
                dependencies[position].push(node_position[&(module, def_index)]);
            }
        }
    }

    // Stable topological order: always emit the lowest-numbered ready node
    // next, so untangled inputs keep their original order.
    let mut remaining_deps: Vec<usize> = dependencies.iter().map(Vec::len).collect();
    let mut dependants: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (position, deps) in dependencies.iter().enumerate() {
        for &dep in deps {
            dependants[dep].push(position);
        }
    }
    let mut ready: std::collections::BinaryHeap<std::cmp::Reverse<usize>> = remaining_deps
        .iter()
        .enumerate()
        .filter(|(_, count)| **count == 0)
        .map(|(position, _)| std::cmp::Reverse(position))
        .collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(std::cmp::Reverse(position)) = ready.pop() {
        order.push(nodes[position]);
        for &dependant in &dependants[position] {
            remaining_deps[dependant] -= 1;
            if remaining_deps[dependant] == 0 {
                ready.push(std::cmp::Reverse(dependant));
            }
        }
    }
    if order.len() != nodes.len() {
        return Err(MergeError::GlobalDependencyCycle);
    }
    Ok(order)
}
