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
