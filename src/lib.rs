//! Merge multiple WebAssembly core modules into a single module.
//!
//! `wasm-fuse` is a Rust counterpart to binaryen's `wasm-merge`, built on the
//! Bytecode Alliance wasm-tools crates. Each input module is given a name, and
//! imports in any module of the form `(import "name" "item" …)` — where `name`
//! is the name of another input module — are resolved ("fused") at merge time
//! to the item that module exports as `item`, the way a JavaScript bundler
//! replaces runtime module loading with build-time linking. Imports referring
//! to modules outside the input set are left as imports.
//!
//! The merged module's exports are chosen by [`ExportSelection`]: either the
//! union of every input's exports (with a configurable
//! [`ExportConflictPolicy`] for name clashes — what wasm-merge produces), or
//! only the exports of one designated entry-point module, with the other
//! modules serving solely to satisfy (some of) its imports.
//!
//! # Example
//!
//! ```
//! use wasm_fuse::{MergeOptions, Merger};
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
mod check;
mod emit;
mod merge;
mod names;
mod parse;
mod prune;
mod remap;
mod resolve;
mod types;

pub use merge::{ExportConflictPolicy, ExportSelection, MergeError, MergeOptions, Merger};
