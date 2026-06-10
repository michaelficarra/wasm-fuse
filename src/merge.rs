//! The merge engine: collects named input modules, resolves cross-module
//! imports, and emits a single combined module.

use wasmparser::WasmFeatures;

use crate::{check, emit, parse, resolve};

/// What to do when two input modules export the same name (in
/// [`ExportSelection::Union`] mode).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportConflictPolicy {
    /// Report an error and abort the merge (wasm-merge's default).
    #[default]
    Error,
    /// Keep the first export and rename later ones by appending `_1`, `_2`, …
    /// (wasm-merge's `--rename-export-conflicts`).
    Rename,
    /// Keep the first export and drop later conflicting ones
    /// (wasm-merge's `--skip-export-conflicts`).
    Skip,
}

/// Which exports the merged module keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportSelection {
    /// Export everything every input module exports, resolving name conflicts
    /// according to the [`ExportConflictPolicy`]. This is what wasm-merge
    /// does.
    Union(ExportConflictPolicy),
    /// Export only the exports of the named entry-point module; the other
    /// modules are only used to (partially or fully) satisfy its imports.
    Entry(String),
}

impl Default for ExportSelection {
    fn default() -> Self {
        ExportSelection::Union(ExportConflictPolicy::default())
    }
}

/// Options controlling a [`Merger`].
#[derive(Clone, Debug)]
pub struct MergeOptions {
    /// Which exports the merged module keeps.
    pub exports: ExportSelection,
    /// WebAssembly proposals accepted in the inputs and used to validate the
    /// output.
    pub features: WasmFeatures,
    /// Check fused import/export pairs for compatibility and validate the
    /// merged output. (Like wasm-merge, inputs are never validated
    /// individually — an input may only become valid once merged.)
    pub validate: bool,
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            exports: ExportSelection::default(),
            features: WasmFeatures::default(),
            validate: true,
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
    /// The module named by [`ExportSelection::Entry`] is not among the
    /// inputs.
    #[error("entry module {name:?} is not among the input modules")]
    UnknownEntryModule {
        /// The entry module name that was not found.
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
        self.inputs.push(InputModule { name, binary });
        Ok(())
    }

    /// Merge all added modules into a single binary module.
    pub fn merge(self) -> Result<Vec<u8>, MergeError> {
        let parsed = self
            .inputs
            .iter()
            .map(|input| parse::parse_module(&input.name, &input.binary))
            .collect::<Result<Vec<_>, _>>()?;

        if let ExportSelection::Entry(name) = &self.options.exports {
            if !parsed.iter().any(|module| module.name == *name) {
                return Err(MergeError::UnknownEntryModule { name: name.clone() });
            }
        }

        let mut resolution = resolve::Resolution::new(&parsed);
        let layout = resolve::layout(&parsed, &mut resolution)?;
        if self.options.validate {
            check::check_fused(&parsed, &mut resolution)?;
        }

        let output = emit::emit(&parsed, &layout, &self.options)?;

        if self.options.validate {
            wasmparser::Validator::new_with_features(self.options.features)
                .validate_all(&output)?;
        }
        Ok(output)
    }
}
