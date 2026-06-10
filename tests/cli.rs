//! Integration tests that exercise the `wasm-bundle` binary the way a user
//! would, via `std::process::Command`.

use std::process::Command;

/// The path to the compiled `wasm-bundle` binary under test.
fn wasm_bundle() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wasm-bundle"))
}

#[test]
fn help_succeeds() {
    let output = wasm_bundle().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("INFILE NAME"));
}

#[test]
fn version_matches_cargo_metadata() {
    let output = wasm_bundle().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout.trim(),
        format!("wasm-bundle {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn odd_number_of_input_arguments_is_an_error() {
    let output = wasm_bundle().args(["only-one.wasm"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("alternating INFILE NAME pairs"),
        "unexpected stderr: {stderr}"
    );
}

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/binaryen/merge")
        .join(name)
}

#[test]
fn output_is_binary_by_default() {
    let output = wasm_bundle()
        .arg(fixture("chain.wat"))
        .arg("first")
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
    let output = wasm_bundle()
        .arg(fixture("chain.wat"))
        .arg("first")
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
    let output = wasm_bundle()
        .arg(fixture("chain.wat"))
        .arg("only")
        .args(["-S", "-o", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("(export \"f\""), "unexpected output: {text}");
}

#[test]
fn duplicate_module_names_are_an_error() {
    let output = wasm_bundle()
        .arg(fixture("chain.wat"))
        .arg("same")
        .arg(fixture("chain.wat.second"))
        .arg("same")
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
    let output = wasm_bundle()
        .arg(&uses_env)
        .arg("app")
        .args(["-S", "-o", "-"])
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
    let output = wasm_bundle()
        .arg(&binary)
        .arg("first")
        .arg(fixture("chain.wat.second"))
        .arg("second")
        .args(["-S", "-o", "-"])
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
        args.push(path.into_os_string());
        args.push(format!("module{index}").into());
    }
    let output = wasm_bundle()
        .args(args)
        .args(["--rename-export-conflicts", "-S", "-o", "-"])
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
fn no_validation_skips_import_export_checking() {
    // types.wat deliberately mismatches imports and exports; with -n the
    // merge must succeed anyway, like wasm-merge.
    let output = wasm_bundle()
        .arg(fixture("types.wat"))
        .arg("env")
        .args(["-all", "-n", "-o", "-"])
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
    let output = wasm_bundle()
        .args(["does-not-exist.wasm", "ghost", "-o", "-"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("failed to read does-not-exist.wasm"),
        "unexpected stderr: {stderr}"
    );
}
