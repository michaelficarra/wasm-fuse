//! Merge multiple WebAssembly core modules into a single module.
//!
//! `wasm-bundle` is a Rust counterpart to binaryen's `wasm-merge`, built on the
//! Bytecode Alliance wasm-tools crates. Each input module is given a name, and
//! imports in any module of the form `(import "name" "item" …)` — where `name`
//! is the name of another input module — are resolved ("fused") at merge time
//! to the item that module exports as `item`, the way a JavaScript bundler
//! replaces runtime module loading with build-time linking. Imports referring
//! to modules outside the input set are left as imports.
//!
//! # Example
//!
//! ```no_run
//! use wasm_bundle::{MergeOptions, Merger};
//!
//! let library = wat::parse_str(r#"(module (func (export "answer") (result i32) i32.const 42))"#)?;
//! let application = wat::parse_str(r#"
//!     (module
//!         (import "library" "answer" (func $answer (result i32)))
//!         (func (export "main") (result i32) call $answer))
//! "#)?;
//!
//! let mut merger = Merger::new(MergeOptions::default());
//! merger.add_module("library", &library)?;
//! merger.add_module("application", &application)?;
//! let merged = merger.merge()?;
//! # let _ = merged;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
mod merge;

pub use merge::{ExportConflictPolicy, MergeError, MergeOptions, Merger};
