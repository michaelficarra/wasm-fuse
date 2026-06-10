//! Behaviour of --inline: functions with a single call site are spliced into
//! their caller and removed. Every test merges with validation on, so the
//! spliced output is also checked by wasmparser's validator.

use std::process::Command;

fn wasm_fuse() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wasm-fuse"))
}

/// Merge `modules` (written to temp files) with `--inline` plus `extra_flags`
/// and return the merged text.
fn merge_inlined(modules: &[(&str, &str)], extra_flags: &[&str]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut command = wasm_fuse();
    for (name, source) in modules {
        let path = dir.path().join(format!("{name}.wat"));
        std::fs::write(&path, source).unwrap();
        command.arg(format!("{name}={}", path.display()));
    }
    let output = command
        .args(["--inline", "--text", "-o", "-"])
        .args(extra_flags)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "merge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn func_count(text: &str) -> usize {
    text.matches("\n  (func").count()
}

#[test]
fn single_use_function_is_inlined_and_removed() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $helper (result i32) (i32.const 7))
                (func (export "main") (result i32) (call $helper)))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(!text.contains("call"), "the call should be gone: {text}");
    assert!(text.contains("i32.const 7"), "{text}");
}

#[test]
fn parameters_become_fresh_locals() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $add (param i32 i32) (result i32)
                    (i32.add (local.get 0) (local.get 1)))
                (func (export "main") (result i32)
                    (call $add (i32.const 1) (i32.const 2))))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    // The arguments land in synthesised locals, assigned from the stack.
    assert!(text.contains("(local i32 i32)"), "{text}");
    assert!(text.contains("local.set"), "{text}");
    assert!(!text.contains("call"), "{text}");
}

#[test]
fn paramless_bodies_splice_without_prologue() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $side_effect (drop (i32.const 1)))
                (func (export "main") (call $side_effect)))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(
        !text.contains("local.set") && !text.contains("(local"),
        "no locals should be synthesised: {text}"
    );
}

#[test]
fn returns_become_branches_to_a_wrapper_block() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $clamped (param i32) (result i32)
                    (if (i32.lt_s (local.get 0) (i32.const 0))
                        (then (return (i32.const 0))))
                    (local.get 0))
                (func (export "main") (result i32)
                    (call $clamped (i32.const 5))))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(!text.contains("call"), "{text}");
    assert!(
        text.contains("block (result i32)"),
        "wrapper block expected: {text}"
    );
    assert!(text.contains("br "), "return should become br: {text}");
    assert!(
        !text.contains("return"),
        "no return may survive from the inlinee: {text}"
    );
}

#[test]
fn declared_locals_are_rezeroed_at_the_splice() {
    // The helper reads its declared local before writing it (relying on
    // zero-initialisation), and the call site sits in a loop: the splice must
    // re-zero, since locals are only zeroed at function entry.
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $counts (result i32)
                    (local $scratch i32)
                    (local.get $scratch))
                (func (export "main") (result i32) (local $acc i32)
                    (loop $again
                        (local.set $acc (i32.add (local.get $acc) (call $counts)))
                        (br_if $again (i32.lt_s (local.get $acc) (i32.const 3))))
                    (local.get $acc)))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(
        text.contains("i32.const 0"),
        "the declared local should be re-zeroed: {text}"
    );
}

#[test]
fn multiple_call_sites_are_not_inlined() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $twice (result i32) (i32.const 7))
                (func (export "main") (result i32)
                    (i32.add (call $twice) (call $twice))))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 2, "{text}");
}

#[test]
fn exported_functions_are_not_inlined() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $api (export "api") (result i32) (i32.const 7))
                (func (export "main") (result i32) (call $api)))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 2, "{text}");
}

#[test]
fn table_referenced_functions_are_not_inlined() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (table 1 funcref)
                (elem (i32.const 0) func $in_table)
                (func $in_table (result i32) (i32.const 7))
                (func (export "main") (result i32) (call $in_table)))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 2, "{text}");
}

#[test]
fn chains_inline_transitively() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $level2 (result i32) (i32.const 7))
                (func $level1 (result i32) (call $level2))
                (func (export "main") (result i32) (call $level1)))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(!text.contains("call"), "{text}");
}

#[test]
fn mutually_recursive_pairs_are_skipped() {
    // Each function's only call site is in the other; inlining either would
    // expand forever, so neither is touched.
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $ping (param i32) (call $pong (local.get 0)))
                (func $pong (param i32) (call $ping (local.get 0)))
                (func (export "main") (drop (i32.const 1))))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 3, "{text}");
}

#[test]
fn inlining_crosses_module_boundaries() {
    // app's call goes through a fused import: the inlined body comes from
    // the other module.
    let text = merge_inlined(
        &[
            (
                "app",
                r#"(module
                    (import "lib" "helper" (func $helper (result i32)))
                    (func (export "main") (result i32) (call $helper)))"#,
            ),
            (
                "lib",
                r#"(module
                    (func (export "helper") (result i32) (i32.const 42)))"#,
            ),
        ],
        &["--entry", "app"],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(!text.contains("call"), "{text}");
    assert!(text.contains("i32.const 42"), "{text}");
}

#[test]
fn start_functions_are_not_inlined() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $init (drop (i32.const 1)))
                (func (export "main") (drop (i32.const 2)))
                (start $init))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 2, "{text}");
    assert!(text.contains("(start"), "{text}");
}

#[test]
fn composes_with_prune_and_names() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib.wat");
    let app = dir.path().join("app.wat");
    std::fs::write(
        &lib,
        r#"(module
            (func $used_once (export "helper") (result i32) (i32.const 1))
            (func $dead (result i32) (i32.const 2)))"#,
    )
    .unwrap();
    std::fs::write(
        &app,
        r#"(module
            (import "lib" "helper" (func $helper (result i32)))
            (func $main (export "main") (result i32) (call $helper)))"#,
    )
    .unwrap();
    let output = wasm_fuse()
        .arg(format!("app={}", app.display()))
        .arg(format!("lib={}", lib.display()))
        .args([
            "--entry",
            "app",
            "--inline",
            "--prune",
            "--keep-names",
            "--text",
            "-o",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(text.contains("$main"), "{text}");
    assert!(
        !text.contains("$used_once") && !text.contains("$dead"),
        "{text}"
    );
}

fn vlq_encode(value: i64, out: &mut String) {
    const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rest = if value < 0 {
        (value.unsigned_abs() << 1) | 1
    } else {
        (value as u64) << 1
    };
    loop {
        let mut digit = (rest & 0x1f) as u8;
        rest >>= 5;
        if rest != 0 {
            digit |= 0x20;
        }
        out.push(BASE64[digit as usize] as char);
        if rest == 0 {
            break;
        }
    }
}

#[test]
fn inlined_code_keeps_its_source_mapping() {
    // Every instruction of the helper is covered by the lib source map;
    // after inlining into main, the merged map must still attribute those
    // instructions to lib.ts.
    let dir = tempfile::tempdir().unwrap();
    let lib_binary =
        wat::parse_str(r#"(module (func (export "helper") (result i32) (i32.const 42)))"#).unwrap();
    let app = dir.path().join("app.wat");
    std::fs::write(
        &app,
        r#"(module
            (import "lib" "helper" (func $helper (result i32)))
            (func (export "main") (result i32) (call $helper)))"#,
    )
    .unwrap();
    let lib = dir.path().join("lib.wasm");
    std::fs::write(&lib, &lib_binary).unwrap();

    let mut offsets = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&lib_binary) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                offsets.push(reader.original_position() as i64);
                reader.read().unwrap();
            }
        }
    }
    let mut mappings = String::new();
    let mut last = 0i64;
    for (ordinal, &offset) in offsets.iter().enumerate() {
        if ordinal > 0 {
            mappings.push(',');
        }
        vlq_encode(offset - last, &mut mappings); // generated column delta
        vlq_encode(0, &mut mappings); // source 0
        vlq_encode(i64::from(ordinal > 0), &mut mappings); // line delta
        vlq_encode(0, &mut mappings); // column
        last = offset;
    }
    let map = dir.path().join("lib.wasm.map");
    std::fs::write(
        &map,
        format!(r#"{{"version":3,"sources":["lib.ts"],"names":[],"mappings":"{mappings}"}}"#),
    )
    .unwrap();

    let out_map = dir.path().join("merged.wasm.map");
    let output = wasm_fuse()
        .arg(format!("app={}", app.display()))
        .arg(format!("lib={}", lib.display()))
        .arg(format!("--source-map=lib={}", map.display()))
        .args(["--entry", "app", "--inline"])
        .arg("--output-source-map")
        .arg(&out_map)
        .args(["-o"])
        .arg(dir.path().join("merged.wasm"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let merged_map: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&out_map).unwrap()).unwrap();
    assert_eq!(
        merged_map["sources"],
        serde_json::json!(["lib.ts"]),
        "{merged_map}"
    );
    assert!(
        !merged_map["mappings"].as_str().unwrap().is_empty(),
        "the helper instructions were inlined, not dropped: {merged_map}"
    );
}

#[test]
fn zeroing_is_omitted_when_the_local_is_written_first() {
    // $scratch is written at depth 0 before any read, which dominates the
    // rest of the body: no re-zero prologue is needed.
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $compute (result i32)
                    (local $scratch i32)
                    (local.set $scratch (i32.const 5))
                    (i32.add (local.get $scratch) (i32.const 2)))
                (func (export "main") (result i32) (call $compute)))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(
        !text.contains("i32.const 0"),
        "no zeroing expected when the write dominates the read: {text}"
    );
}

#[test]
fn zeroing_is_kept_when_the_write_may_be_skipped() {
    // The only write to $scratch sits inside an if arm, so the later read
    // can still observe the initial value: the splice must re-zero.
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $maybe (param i32) (result i32)
                    (local $scratch i32)
                    (if (local.get 0)
                        (then (local.set $scratch (i32.const 9))))
                    (local.get $scratch))
                (func (export "main") (result i32)
                    (call $maybe (i32.const 1))))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(
        text.contains("i32.const 0"),
        "a conditionally-written local must still be re-zeroed: {text}"
    );
}

#[test]
fn zeroing_is_omitted_for_locals_that_are_never_read() {
    let text = merge_inlined(
        &[(
            "only",
            r#"(module
                (func $writes_only (param i32)
                    (local $sink i64)
                    (local.set $sink (i64.extend_i32_u (local.get 0))))
                (func (export "main") (call $writes_only (i32.const 3))))"#,
        )],
        &[],
    );
    assert_eq!(func_count(&text), 1, "{text}");
    assert!(
        !text.contains("i64.const 0"),
        "a never-read local needs no zeroing: {text}"
    );
}

#[test]
fn inlining_is_off_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("only.wat");
    std::fs::write(
        &path,
        r#"(module
            (func $helper (result i32) (i32.const 7))
            (func (export "main") (result i32) (call $helper)))"#,
    )
    .unwrap();
    let output = wasm_fuse()
        .arg(format!("only={}", path.display()))
        .args(["--text", "-o", "-"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(func_count(&text), 2, "{text}");
    assert!(text.contains("call"), "{text}");
}
