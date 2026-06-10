//! Scenarios ported from binaryen's wasm-merge test suite
//! (`test/lit/merge/`, vendored under `tests/fixtures/binaryen/merge/`; see
//! NOTICE). Each test uses the original `RUN:` line's modules, with its flags
//! translated to wasm-fuse's own interface (the CLI deliberately does not
//! copy wasm-merge's flags): `--rename-export-conflicts` →
//! `--export-conflicts rename`, `--skip-export-conflicts` →
//! `--export-conflicts skip`, `-S` → `--text`, and `-all` → nothing, since
//! wasm-fuse always accepts every proposal.
//!
//! The original tests assert binaryen's printed output via FileCheck; our
//! text output comes from wasmprinter and is formatted (and numbered)
//! differently, so successful merges are snapshot-tested against our own
//! output instead (`tests/snapshots/`, blessed with `SNAPSHOTS=overwrite`).
//! Scenarios not yet implemented are listed at the bottom as ignored tests so
//! coverage gaps stay visible; see PLAN.md for the phase plan.

use std::path::PathBuf;
use std::process::Command;

use snapbox::{assert_data_eq, file};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binaryen/merge")
        .join(name)
}

/// Run `wasm-fuse` over `(file, name)` module pairs with `flags`, returning
/// the process output.
fn merge(modules: &[(&str, &str)], flags: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wasm-fuse"));
    for (file, name) in modules {
        command.arg(format!("{name}={}", fixture(file).display()));
    }
    command.args(flags).output().unwrap()
}

/// Merge and return the textual (`--text`) form of the merged module.
fn merge_to_text(modules: &[(&str, &str)], flags: &[&str]) -> String {
    let flags: Vec<&str> = flags
        .iter()
        .chain(&["--text", "-o", "-"])
        .copied()
        .collect();
    let output = merge(modules, &flags);
    assert!(
        output.status.success(),
        "merge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Merge expecting failure; return the diagnostics.
fn merge_failure(modules: &[(&str, &str)], flags: &[&str]) -> String {
    let output = merge(modules, flags);
    assert!(
        !output.status.success(),
        "expected the merge to fail, but it succeeded"
    );
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn chain() {
    // Chains of imports/exports: the first module exports a function, the
    // second re-exports it, the third imports it from the second.
    let text = merge_to_text(
        &[
            ("chain.wat", "first"),
            ("chain.wat.second", "second"),
            ("chain.wat.third", "third"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/chain.wat"]);
}

#[test]
fn cycle() {
    // Circular imports between the input modules are internalised.
    let text = merge_to_text(
        &[
            ("cycle.wat", "first"),
            ("cycle.wat.second", "second"),
            ("cycle.wat.third", "third"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/cycle.wat"]);
}

#[test]
fn import_cycle() {
    // An import chain that never reaches a definition must be reported, not
    // looped on forever.
    let stderr = merge_failure(
        &[
            ("import_cycle.wat", "first"),
            ("import_cycle.wat.second", "second"),
        ],
        &[],
    );
    // binaryen names the first module's internal $f here; we name the import
    // field at which the cycle was first chased ("g", imported by $f).
    assert!(
        stderr.contains("infinite loop of imports on \"g\""),
        "unexpected diagnostics: {stderr}"
    );
}

#[test]
fn export_options_rename() {
    let text = merge_to_text(
        &[
            ("export_options.wat", "first"),
            ("export_options.wat.second", "second"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/export_options.rename.wat"]);
}

#[test]
fn export_options_skip() {
    let text = merge_to_text(
        &[
            ("export_options.wat", "first"),
            ("export_options.wat.second", "second"),
        ],
        &["--export-conflicts=skip"],
    );
    assert_data_eq!(text, file!["snapshots/export_options.skip.wat"]);
}

#[test]
fn export_options_default_is_an_error() {
    let stderr = merge_failure(
        &[
            ("export_options_default.wat", "first"),
            ("export_options_default.wat.second", "second"),
        ],
        &[],
    );
    assert!(
        stderr.contains("export name conflict: func"),
        "unexpected diagnostics: {stderr}"
    );
}

#[test]
fn fusing() {
    // Imports fuse to exports across modules: functions, memories, and tags.
    let text = merge_to_text(
        &[("fusing.wat", "first"), ("fusing.wat.second", "second")],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/fusing.wat"]);
}

#[test]
fn global_ordering() {
    // A fused global import may refer to a global appended from a later
    // module; defined globals must be reordered so initialisers only read
    // earlier globals.
    let text = merge_to_text(
        &[
            ("global-ordering.wat", "first"),
            ("global-ordering.wat.second", "second"),
        ],
        &[],
    );
    assert_data_eq!(text, file!["snapshots/global-ordering.wat"]);
}

#[test]
fn memory_data() {
    let text = merge_to_text(
        &[
            ("memory_data.wat", "first"),
            ("memory_data.wat.second", "second"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/memory_data.wat"]);
}

#[test]
fn names() {
    // The binaryen test runs with -g to check name-section handling, which we
    // don't support yet (PLAN.md phase 4); the merge of its modules — with
    // GC types, duplicate names, unnamed items — must still work.
    let text = merge_to_text(
        &[("names.wat", "first"), ("names.wat.second", "second")],
        &[],
    );
    assert_data_eq!(text, file!["snapshots/names.wat"]);
}

#[test]
fn renamings() {
    // Conflicting internal names across every item kind: globals, memories,
    // data segments, tables, element segments, tags, functions.
    let text = merge_to_text(
        &[
            ("renamings.wat", "first"),
            ("renamings.wat.second", "second"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/renamings.wat"]);
}

#[test]
fn start() {
    // First module has a function *named* start (not an actual start);
    // second has a real start function.
    let text = merge_to_text(
        &[("start.wat", "first"), ("start.wat.second", "second")],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/start.wat"]);
}

#[test]
fn start_flip() {
    // As `start`, with the modules' roles flipped.
    let text = merge_to_text(
        &[
            ("start.flip.wat", "first"),
            ("start.flip.wat.second", "second"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/start.flip.wat"]);
}

#[test]
fn start_return() {
    // Start functions containing early returns must be called in sequence
    // from a fused start, not concatenated.
    let text = merge_to_text(
        &[
            ("start-return.wat", "first"),
            ("start-return.wat.second", "second"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/start-return.wat"]);
}

#[test]
fn start3() {
    // Three modules, two of which have start functions.
    let text = merge_to_text(
        &[
            ("start3.wat", "first"),
            ("start3.wat.second", "second"),
            ("start3.wat.third", "third"),
        ],
        &[],
    );
    assert_data_eq!(text, file!["snapshots/start3.wat"]);
}

#[test]
fn table_elem() {
    // Element segments must refer to the right tables after renumbering.
    let text = merge_to_text(
        &[
            ("table_elem.wat", "first"),
            ("table_elem.wat.second", "second"),
        ],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/table_elem.wat"]);
}

#[test]
fn table64() {
    let text = merge_to_text(
        &[("table64.wat", "first"), ("table64.wat.second", "second")],
        &["--export-conflicts=rename"],
    );
    assert_data_eq!(text, file!["snapshots/table64.wat"]);
}

#[test]
fn func_subtyping() {
    // A function exported with a subtype satisfies an import of its
    // supertype.
    let text = merge_to_text(
        &[
            ("func_subtyping.wat", "primary"),
            ("func_subtyping.wat.second", "secondary"),
        ],
        &["--export-conflicts=skip"],
    );
    assert_data_eq!(text, file!["snapshots/func_subtyping.wat"]);
}

#[test]
fn func_subtyping_return() {
    let text = merge_to_text(
        &[
            ("func_subtyping_return.wat", "primary"),
            ("func_subtyping_return.wat.second", "secondary"),
        ],
        &["--export-conflicts=skip"],
    );
    assert_data_eq!(text, file!["snapshots/func_subtyping_return.wat"]);
}

#[test]
fn global_subtyping() {
    let text = merge_to_text(
        &[
            ("global_subtyping.wat", "primary"),
            ("global_subtyping.wat.second", "secondary"),
        ],
        &["--export-conflicts=skip"],
    );
    assert_data_eq!(text, file!["snapshots/global_subtyping.wat"]);
}

#[test]
fn types_mismatches_are_all_reported() {
    // A single module named env importing from itself, with deliberately
    // mismatched function/table/memory/global/tag imports. All mismatches
    // are reported at once.
    let stderr = merge_failure(&[("types.wat", "env")], &[]);
    for expected in [
        "import/export mismatches",
        "type mismatch when importing function f1 from module env",
        "type mismatch when importing function f3 from module env",
        "type mismatch when importing table t1 from module env",
        "type mismatch when importing memory m1 from module env",
        "type mismatch when importing global g1 from module env",
        "type mismatch when importing global g2 from module env",
        "type mismatch when importing tag t from module env",
    ] {
        assert!(
            stderr.contains(expected),
            "missing {expected:?} in diagnostics: {stderr}"
        );
    }
}

// Not yet implemented — these fixtures are vendored but their features are
// pending (see PLAN.md): sourcemap.wat (phase 4: source maps), names.wat with
// -g (phase 4: names section), annotations.wat and annotations-func-only.wat
// (phase 4: code annotations), manifest.wat (phase 5: --output-manifest).

#[test]
#[ignore = "source maps are not yet supported (PLAN.md phase 4)"]
fn sourcemap() {
    unimplemented!("port sourcemap.wat once -ism/-osm/-osu are implemented");
}

#[test]
#[ignore = "code annotations are not yet supported (PLAN.md phase 4)"]
fn annotations() {
    unimplemented!("port annotations.wat once annotations are preserved");
}

#[test]
#[ignore = "--output-manifest is not yet supported (PLAN.md phase 5)"]
fn manifest() {
    unimplemented!("port manifest.wat once --output-manifest is implemented");
}
