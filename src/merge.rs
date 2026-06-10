//! The merge engine: collects named input modules, resolves cross-module
//! imports, and emits a single combined module.

use wasmparser::WasmFeatures;

/// What to do when two input modules export the same name.
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

/// Options controlling a [`Merger`].
#[derive(Clone, Debug)]
pub struct MergeOptions {
    /// How to handle export name conflicts between input modules.
    pub export_conflicts: ExportConflictPolicy,
    /// WebAssembly proposals accepted in the inputs and used to validate the
    /// output.
    pub features: WasmFeatures,
    /// Validate the merged output before returning it.
    pub validate: bool,
}

impl Default for MergeOptions {
    fn default() -> Self {
        MergeOptions {
            export_conflicts: ExportConflictPolicy::default(),
            features: WasmFeatures::default(),
            validate: true,
        }
    }
}

/// An error produced while adding modules to a [`Merger`] or merging them.
#[derive(Debug, thiserror::Error)]
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
    /// A chain of imports re-exporting other imports never reaches a
    /// definition.
    #[error("infinite loop of imports on {name:?}")]
    ImportLoop {
        /// The export name at which the cycle was detected.
        name: String,
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
    // read once merge() is implemented; the expectation flags its removal
    #[expect(dead_code)]
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
        let _ = &self.options;
        let _ = &self.inputs;
        todo!("implemented in Phase 1 (see PLAN.md)")
    }
}
