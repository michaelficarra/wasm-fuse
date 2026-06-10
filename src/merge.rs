//! The merge engine: collects named input modules, resolves cross-module
//! imports, and emits a single combined module.

use wasmparser::WasmFeatures;

use crate::{check, emit, inline, names, parse, prune, resolve, sourcemap, types};

/// What to do when two kept modules export the same name (see
/// [`MergeOptions::export_conflicts`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportConflictPolicy {
    /// Report an error and abort the merge (wasm-merge's default).
    #[default]
    Error,
    /// Keep the earlier export and rename later ones by appending `_1`, `_2`, …
    /// (wasm-merge's `--rename-export-conflicts`).
    Rename,
    /// Keep the earlier export and drop later conflicting ones
    /// (wasm-merge's `--skip-export-conflicts`).
    Skip,
}

/// Options controlling a [`Merger`].
#[derive(Clone, Debug)]
pub struct MergeOptions {
    /// The modules whose exports are kept, in priority order: on an export
    /// name conflict the earlier-listed module's export wins, and
    /// [`export_conflicts`](MergeOptions::export_conflicts) decides the
    /// loser's fate. Modules that are not listed only serve to (partially
    /// or fully) satisfy the listed modules' imports. `None` keeps every
    /// input module's exports, in input order — the union wasm-merge
    /// produces. `Some(vec![])` keeps no exports at all: the merged module
    /// then acts only through its start functions.
    pub entry_modules: Option<Vec<String>>,
    /// What to do when two kept modules export the same name.
    pub export_conflicts: ExportConflictPolicy,
    /// WebAssembly proposals accepted in the inputs and used to validate the
    /// output.
    pub features: WasmFeatures,
    /// Check fused import/export pairs for compatibility and validate the
    /// merged output. (Like wasm-merge, inputs are never validated
    /// individually — an input may only become valid once merged.)
    pub validate: bool,
    /// Remove items that are not reachable from the kept exports or the start
    /// functions: functions, globals, tables, memories, tags, and segments
    /// nothing live references are dropped. wasm-merge does this
    /// unconditionally (its remove-unused-module-elements pass); here it is
    /// off by default — merging alone never deletes code. Combined with
    /// [`MergeOptions::entry_modules`] this tree-shakes a bundle down to
    /// what the entry modules (or, with `Some(vec![])`, just the start
    /// functions) actually use.
    pub prune_unused: bool,
    /// Merge the inputs' "name" custom sections (debug names for functions,
    /// locals, types, …) into the output, with indices remapped. Names of
    /// pruned items are dropped; where a fused import's local alias and its
    /// definition's name disagree, the definition wins. Off by default, like
    /// wasm-merge without `-g`.
    pub keep_names: bool,
    /// Embed this URL in a `sourceMappingURL` custom section of the output,
    /// pointing consumers at the merged source map (wasm-merge's
    /// `--output-source-map-url`).
    pub source_map_url: Option<String>,
    /// Inline every function that has exactly one call site and no other
    /// references (not exported, not a start function, never used by
    /// `ref.func`, tail calls, or element segments, and not recursive). The
    /// inlined function is removed from the output — by construction nothing
    /// can observe it from inside the module. Parameters become fresh locals
    /// in the caller; `return` instructions become branches. Off by default,
    /// like pruning: merging alone never transforms code.
    pub inline_single_use: bool,
    /// Produce a wasm-split manifest in [`Merged::manifest`]: for every
    /// module except the first (primary) one, its name followed by the
    /// post-merge names of its surviving defined functions. Implies
    /// [`keep_names`](MergeOptions::keep_names), like wasm-merge's
    /// `--output-manifest` implies `-g`, so wasm-split can match the names
    /// against the binary.
    pub emit_manifest: bool,
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            entry_modules: None,
            export_conflicts: ExportConflictPolicy::default(),
            features: WasmFeatures::default(),
            validate: true,
            prune_unused: false,
            keep_names: false,
            source_map_url: None,
            inline_single_use: false,
            emit_manifest: false,
        }
    }
}

/// An error produced while adding modules to a [`Merger`] or merging them.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MergeError {
    /// An input module could not be parsed (as text or binary).
    #[error("failed to parse module {name:?}: {source}")]
    Parse {
        /// The name the module was added under.
        name: String,
        /// The underlying parse error.
        #[source]
        source: wat::Error,
    },
    /// An input module was malformed or failed validation.
    #[error("invalid module {name:?}: {source}")]
    InvalidModule {
        /// The name the module was added under.
        name: String,
        /// The underlying error.
        #[source]
        source: wasmparser::BinaryReaderError,
    },
    /// Two input modules were added under the same name.
    #[error("duplicate module name {name:?}")]
    DuplicateModuleName {
        /// The name both modules were added under.
        name: String,
    },
    /// Two modules export the same name and the policy is
    /// [`ExportConflictPolicy::Error`].
    #[error(
        "export name conflict: {name} (consider ExportConflictPolicy::Rename or ExportConflictPolicy::Skip)"
    )]
    ExportConflict {
        /// The conflicting export name.
        name: String,
    },
    /// A module named in [`MergeOptions::entry_modules`] is not among the
    /// inputs.
    #[error("entry module {name:?} is not among the input modules")]
    UnknownEntryModule {
        /// The entry module name that was not found.
        name: String,
    },
    /// The same module is named twice in
    /// [`MergeOptions::entry_modules`].
    #[error("duplicate entry module {name:?}")]
    DuplicateEntryModule {
        /// The module name that was listed more than once.
        name: String,
    },
    /// A chain of imports re-exporting other imports never reaches a
    /// definition.
    #[error("infinite loop of imports on {name:?}")]
    ImportLoop {
        /// The export name at which the cycle was detected.
        name: String,
    },
    /// One or more fused imports and the exports satisfying them have
    /// incompatible types. All mismatches are collected before failing, like
    /// wasm-merge.
    #[error("import/export mismatches:{}", details.iter().map(|d| format!("\n  {d}")).collect::<String>())]
    ImportMismatches {
        /// One description per mismatching import/export pair.
        details: Vec<String>,
    },
    /// Global initialisers depend on each other in a cycle, so no valid
    /// ordering of the merged globals exists.
    #[error("global initialisers form a dependency cycle after merging")]
    GlobalDependencyCycle,
    /// A module referenced an index beyond its index space (the module is
    /// malformed; enable validation for a more precise error).
    #[error("invalid module {module:?}: {kind} index {index} is out of bounds")]
    OutOfBoundsIndex {
        /// The name of the module containing the reference.
        module: String,
        /// The kind of index space ("function", "type", …).
        kind: &'static str,
        /// The out-of-bounds index.
        index: u32,
    },
    /// An item could not be re-encoded into the merged module.
    #[error("failed to re-encode module {module:?}: {message}")]
    Reencode {
        /// The name of the module whose item was being re-encoded.
        module: String,
        /// A description of the failure.
        message: String,
    },
    /// The merged module failed validation.
    #[error("merged module failed validation: {0}")]
    Validation(#[from] wasmparser::BinaryReaderError),
    /// A source map could not be parsed or applied.
    #[error("invalid source map for module {module:?}: {message}")]
    SourceMap {
        /// The module the source map was attached to.
        module: String,
        /// A description of the problem.
        message: String,
    },
    /// [`Merger::add_source_map`] named a module that has not been added.
    #[error("no module named {name:?} has been added")]
    UnknownModule {
        /// The module name that was not found.
        name: String,
    },
}

/// The products of a merge.
#[derive(Debug)]
pub struct Merged {
    /// The merged module, as a binary.
    pub module: Vec<u8>,
    /// The merged source map (JSON), present when any input had one attached
    /// via [`Merger::add_source_map`].
    pub source_map: Option<String>,
    /// The wasm-split manifest, present when
    /// [`MergeOptions::emit_manifest`] is set.
    pub manifest: Option<String>,
}

/// Merges named WebAssembly modules into a single module.
///
/// See the [crate documentation](crate) for an example.
pub struct Merger {
    options: MergeOptions,
    inputs: Vec<InputModule>,
}

struct InputModule {
    name: String,
    binary: Vec<u8>,
    source_map: Option<sourcemap::InputSourceMap>,
}

impl Merger {
    /// Create a merger with the given options.
    pub fn new(options: MergeOptions) -> Self {
        Merger {
            options,
            inputs: Vec::new(),
        }
    }

    /// Add an input module under `name`.
    ///
    /// `bytes` may be a binary module or WebAssembly text. Imports in other
    /// modules of the form `(import "<name>" "item" …)` will be resolved
    /// against this module's exports.
    pub fn add_module(&mut self, name: impl Into<String>, bytes: &[u8]) -> Result<(), MergeError> {
        let name = name.into();
        if self.inputs.iter().any(|input| input.name == name) {
            return Err(MergeError::DuplicateModuleName { name });
        }
        // Like wasm-merge, inputs are not validated individually — only the
        // merged output is. An input can even be invalid on its own yet
        // merge into a valid module (e.g. a ref.func of an import that only
        // becomes a declared function once fused to an export).
        let binary = wat::parse_bytes(bytes)
            .map_err(|source| MergeError::Parse {
                name: name.clone(),
                source,
            })?
            .into_owned();
        self.inputs.push(InputModule {
            name,
            binary,
            source_map: None,
        });
        Ok(())
    }

    /// Attach a source map (Source Map V3 JSON, as produced alongside the
    /// module by its toolchain) to the already-added module named `module`.
    /// The merged source map is returned by [`Merger::merge_full`].
    pub fn add_source_map(&mut self, module: &str, json: &[u8]) -> Result<(), MergeError> {
        let map = sourcemap::parse(module, json)?;
        let input = self
            .inputs
            .iter_mut()
            .find(|input| input.name == module)
            .ok_or_else(|| MergeError::UnknownModule {
                name: module.to_string(),
            })?;
        input.source_map = Some(map);
        Ok(())
    }

    /// Merge all added modules into a single binary module.
    ///
    /// A convenience for [`Merger::merge_full`] when only the module itself
    /// is needed.
    pub fn merge(self) -> Result<Vec<u8>, MergeError> {
        Ok(self.merge_full()?.module)
    }

    /// Merge all added modules, returning every product of the merge.
    pub fn merge_full(self) -> Result<Merged, MergeError> {
        let parsed = self
            .inputs
            .iter()
            .map(|input| parse::parse_module(&input.name, &input.binary))
            .collect::<Result<Vec<_>, _>>()?;

        // Also validates the entry-module list (unknown or duplicate names).
        let exports = emit::surviving_exports(&parsed, &self.options)?;

        let canon = types::canonicalise(&parsed)?;
        let mut resolution = resolve::Resolution::new(&parsed);
        let liveness = if self.options.prune_unused {
            Some(prune::compute_liveness(&parsed, &mut resolution, &exports)?)
        } else {
            None
        };
        let inline_plan = if self.options.inline_single_use {
            inline::plan(&parsed, &mut resolution, &exports, liveness.as_ref())?
        } else {
            inline::InlinePlan::default()
        };
        let layout = resolve::layout(
            &parsed,
            &mut resolution,
            liveness.as_ref(),
            &canon,
            &inline_plan.sites(),
        )?;
        if self.options.validate {
            check::check_fused(&parsed, &mut resolution, &canon)?;
        }

        // The manifest lists functions by their debug names, so it forces
        // names on, like wasm-merge's --output-manifest implies -g.
        let keep_names = self.options.keep_names || self.options.emit_manifest;
        let built_names = if keep_names {
            let starts = parsed
                .iter()
                .filter(|module| module.start.is_some())
                .count();
            let synthetic_start = (starts > 1).then_some(layout.func_count);
            Some(names::build(
                &parsed,
                &mut resolution,
                &layout,
                &canon,
                synthetic_start,
            )?)
        } else {
            None
        };
        let manifest = self.options.emit_manifest.then(|| {
            let empty = std::collections::BTreeMap::new();
            let function_names = built_names
                .as_ref()
                .map_or(&empty, |names| &names.functions);
            names::manifest(&parsed, &layout, function_names)
        });
        let name_section = built_names.and_then(|names| names.section);

        let track_offsets = self.inputs.iter().any(|input| input.source_map.is_some());
        let config = emit::EmitConfig {
            exports: &exports,
            live: liveness.as_ref(),
            canon: &canon,
            name_section: name_section.as_ref(),
            track_offsets,
            source_map_url: self.options.source_map_url.as_deref(),
            plan: &inline_plan,
        };
        let (output, code_offsets) = emit::emit(&parsed, &layout, &mut resolution, &config)?;

        let source_map = if track_offsets {
            let maps: Vec<_> = self
                .inputs
                .iter()
                .map(|input| input.source_map.as_ref())
                .collect();
            Some(sourcemap::build(&maps, &code_offsets, &output)?)
        } else {
            None
        };

        if self.options.validate {
            wasmparser::Validator::new_with_features(self.options.features)
                .validate_all(&output)?;
        }
        Ok(Merged {
            module: output,
            source_map,
            manifest,
        })
    }
}
