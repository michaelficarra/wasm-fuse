//! Integration tests that exercise the `wasm-fuse` binary the way a user
//! would, via `std::process::Command`.

use std::process::Command;

/// The path to the compiled `wasm-fuse` binary under test.
fn wasm_fuse() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wasm-fuse"))
}

fn fixture(name: &str) -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binaryen/merge")
        .join(name)
        .display()
        .to_string()
}

#[test]
fn help_succeeds() {
    let output = wasm_fuse().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[NAME=]PATH"));
}

#[test]
fn version_matches_cargo_metadata() {
    let output = wasm_fuse().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.trim(),
        format!("wasm-fuse {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn empty_module_name_is_an_error() {
    let output = wasm_fuse().arg("=nameless.wasm").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("empty module name"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn module_names_default_to_file_stems() {
    // `lib.wat` becomes module `lib` without an explicit name, so `app`'s
    // import of "lib" fuses.
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib.wat");
    let app = dir.path().join("app.wat");
    std::fs::write(
        &lib,
        r#"(module (func (export "f") (result i32) (i32.const 1)))"#,
    )
    .unwrap();
    std::fs::write(
        &app,
        r#"(module
            (import "lib" "f" (func $f (result i32)))
            (func (export "main") (result i32) (call $f)))"#,
    )
    .unwrap();
    let output = wasm_fuse()
        .arg(&app)
        .arg(&lib)
        .args(["--text", "-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains("(import"),
        "the lib import should have fused: {text}"
    );
}

#[test]
fn output_is_binary_by_default() {
    let output = wasm_fuse()
        .arg(format!("first={}", fixture("chain.wat")))
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        output.stdout.starts_with(b"\0asm"),
        "expected a binary module on stdout"
    );
}

#[test]
fn output_file_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("merged.wasm");
    let output = wasm_fuse()
        .arg(format!("first={}", fixture("chain.wat")))
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(std::fs::read(&out).unwrap().starts_with(b"\0asm"));
}

#[test]
fn single_module_merge_is_an_identity_operation() {
    // Merging one module with no fusable imports must produce an equivalent
    // module.
    let output = wasm_fuse()
        .arg(format!("only={}", fixture("chain.wat")))
        .args(["--text", "-o", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("(export \"f\""), "unexpected output: {text}");
}

#[test]
fn duplicate_module_names_are_an_error() {
    let output = wasm_fuse()
        .arg(format!("same={}", fixture("chain.wat")))
        .arg(format!("same={}", fixture("chain.wat.second")))
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("duplicate module name \"same\""),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn imports_of_modules_outside_the_input_set_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let uses_env = dir.path().join("uses_env.wat");
    std::fs::write(
        &uses_env,
        r#"(module
            (import "env" "print" (func $print (param i32)))
            (func (export "main") (call $print (i32.const 0))))"#,
    )
    .unwrap();
    let output = wasm_fuse()
        .arg(format!("app={}", uses_env.display()))
        .args(["--text", "-o", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains(r#"(import "env" "print""#),
        "unexpected output: {text}"
    );
}

#[test]
fn binary_and_text_inputs_can_be_mixed() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("chain.wasm");
    std::fs::write(&binary, wat::parse_file(fixture("chain.wat")).unwrap()).unwrap();
    let output = wasm_fuse()
        .arg(format!("first={}", binary.display()))
        .arg(format!("second={}", fixture("chain.wat.second")))
        .args(["--text", "-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn many_modules_fuse_along_a_chain() {
    // A chain of imports across five modules: each module calls the next
    // one's export; everything fuses into direct calls.
    let dir = tempfile::tempdir().unwrap();
    let mut args = Vec::new();
    for index in 0..5 {
        let path = dir.path().join(format!("module{index}.wat"));
        let source = if index == 4 {
            "(module (func (export \"f\") (result i32) (i32.const 4)))".to_string()
        } else {
            format!(
                "(module
                    (import \"module{}\" \"f\" (func $next (result i32)))
                    (func (export \"f\") (result i32) (call $next)))",
                index + 1
            )
        };
        std::fs::write(&path, source).unwrap();
        args.push(format!("module{index}={}", path.display()));
    }
    let output = wasm_fuse()
        .args(args)
        .args(["--export-conflicts=rename", "--text", "-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains("(import"),
        "all imports should have fused: {text}"
    );
}

#[test]
fn no_validate_skips_import_export_checking() {
    // types.wat deliberately mismatches imports and exports; with
    // --no-validate the merge must succeed anyway, like wasm-merge with -n.
    let output = wasm_fuse()
        .arg(format!("env={}", fixture("types.wat")))
        .args(["--no-validate", "-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_input_file_is_a_readable_error() {
    let output = wasm_fuse()
        .args(["ghost=does-not-exist.wasm", "-o", "-"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("failed to read does-not-exist.wasm"),
        "unexpected stderr: {stderr}"
    );
}

/// Write an `app` module (exports "main"; imports lib.answer and env.print)
/// and a `lib` module (exports "answer" and "extra") into `dir`.
fn write_entry_fixtures(dir: &std::path::Path) -> (String, String) {
    let app = dir.join("app.wat");
    let lib = dir.join("lib.wat");
    std::fs::write(
        &app,
        r#"(module
            (import "lib" "answer" (func $answer (result i32)))
            (import "env" "print" (func $print (param i32)))
            (func (export "main") (call $print (call $answer))))"#,
    )
    .unwrap();
    std::fs::write(
        &lib,
        r#"(module
            (func (export "answer") (result i32) (i32.const 42))
            (func (export "extra") (result i32) (i32.const 0)))"#,
    )
    .unwrap();
    (app.display().to_string(), lib.display().to_string())
}

#[test]
fn entry_mode_exports_only_the_entry_module() {
    let dir = tempfile::tempdir().unwrap();
    let (app, lib) = write_entry_fixtures(dir.path());
    let output = wasm_fuse()
        .args([&app, &lib])
        .args(["--entry", "app", "--text", "-o", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    // The entry module's exports survive; the library's do not.
    assert!(text.contains(r#"(export "main""#), "missing main: {text}");
    assert!(
        !text.contains(r#"(export "answer""#) && !text.contains(r#"(export "extra""#),
        "library exports should not survive: {text}"
    );
    // The library satisfied the lib.answer import; env.print is outside the
    // input set and remains an import.
    assert!(
        !text.contains(r#"(import "lib""#),
        "lib import should have fused: {text}"
    );
    assert!(
        text.contains(r#"(import "env" "print""#),
        "env import should remain: {text}"
    );
}

#[test]
fn entry_mode_with_unknown_module_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let (app, lib) = write_entry_fixtures(dir.path());
    let output = wasm_fuse()
        .args([&app, &lib])
        .args(["--entry", "ghost", "-o", "-"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("entry module \"ghost\" is not among the input modules"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn entry_mode_rejects_export_conflict_policy() {
    // --export-conflicts only makes sense when unioning exports; clap should
    // reject the combination.
    let dir = tempfile::tempdir().unwrap();
    let (app, lib) = write_entry_fixtures(dir.path());
    let output = wasm_fuse()
        .args([&app, &lib])
        .args(["--entry", "app", "--export-conflicts=rename"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("cannot be used with"),
        "unexpected stderr: {stderr}"
    );
}
