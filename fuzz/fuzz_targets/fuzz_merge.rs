#![no_main]

//! Merge several wasm-smith-generated modules under randomised options.
//! The merge may fail gracefully (for example when randomly-matching import
//! and export names fuse with incompatible types), but it must never panic,
//! and any successful output must validate (the merger checks this itself
//! with `validate: true`).

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use wasm_fuse::{ExportConflictPolicy, MergeOptions, Merger};
use wasmparser::WasmFeatures;

fuzz_target!(|data: &[u8]| {
    let mut unstructured = Unstructured::new(data);

    let module_count = unstructured.int_in_range(1..=3).unwrap_or(1);
    let export_conflicts = match unstructured.int_in_range(0..=2).unwrap_or(0) {
        0 => ExportConflictPolicy::Rename,
        1 => ExportConflictPolicy::Skip,
        _ => ExportConflictPolicy::Error,
    };
    // `None` keeps every module's exports and `Some(vec![])` keeps none.
    // "module1" may not exist (module_count can be 1) and the duplicated
    // list is always invalid — both must fail gracefully, never panic.
    let entry_modules = match unstructured.int_in_range(0..=4).unwrap_or(0) {
        0 => None,
        1 => Some(vec![]),
        2 => Some(vec!["module0".to_string()]),
        3 => Some(vec!["module1".to_string(), "module0".to_string()]),
        _ => Some(vec!["module0".to_string(), "module0".to_string()]),
    };
    let options = MergeOptions {
        entry_modules,
        export_conflicts,
        features: WasmFeatures::all(),
        validate: true,
        prune_unused: unstructured.arbitrary().unwrap_or(false),
        keep_names: unstructured.arbitrary().unwrap_or(false),
        emit_manifest: unstructured.arbitrary().unwrap_or(false),
        inline_single_use: unstructured.arbitrary().unwrap_or(false),
        ..MergeOptions::default()
    };

    let mut merger = Merger::new(options);
    for index in 0..module_count {
        let Ok(module) = wasm_smith::Module::arbitrary(&mut unstructured) else {
            return;
        };
        if merger
            .add_module(format!("module{index}"), &module.to_bytes())
            .is_err()
        {
            return;
        }
    }
    let _ = merger.merge_full();
});
