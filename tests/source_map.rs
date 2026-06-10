//! Source-map round-trip tests.
//!
//! binaryen's sourcemap.wat drives `-ism`/`-osm` through `wasm-as`-generated
//! maps from `;;@ file:line:col` comments, which the wat crate treats as
//! plain comments — that route is not portable. Instead these tests build an
//! input source map programmatically against the real instruction offsets of
//! the encoded input, merge through the CLI, and check that the merged map's
//! segments land exactly on the corresponding instructions of the merged
//! binary, with sources, lines, and names preserved.

use std::path::Path;
use std::process::Command;

use wasmparser::{Parser, Payload};

fn wasm_fuse() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wasm-fuse"))
}

/// Absolute byte offsets of every instruction in every function of `module`.
fn instruction_offsets(module: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    for payload in Parser::new(0).parse_all(module) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                offsets.push(reader.original_position());
                reader.read().unwrap();
            }
        }
    }
    offsets
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn vlq_encode(value: i64, out: &mut String) {
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

fn vlq_decode_segment(encoded: &str) -> Vec<i64> {
    let mut values = Vec::new();
    let mut value: u64 = 0;
    let mut shift = 0u32;
    for byte in encoded.bytes() {
        let digit = BASE64.iter().position(|&c| c == byte).unwrap() as u64;
        value |= (digit & 0x1f) << shift;
        if digit & 0x20 != 0 {
            shift += 5;
        } else {
            values.push(if value & 1 != 0 {
                -((value >> 1) as i64)
            } else {
                (value >> 1) as i64
            });
            value = 0;
            shift = 0;
        }
    }
    values
}

/// Decode a merged map's mappings into (offset, source, line) triples.
fn decode_mappings(mappings: &str) -> Vec<(u64, i64, i64)> {
    let mut decoded = Vec::new();
    let (mut offset, mut source, mut line) = (0i64, 0i64, 0i64);
    for segment in mappings.split(',').filter(|segment| !segment.is_empty()) {
        let fields = vlq_decode_segment(segment);
        assert!(fields.len() >= 4, "segment {segment:?} has too few fields");
        offset += fields[0];
        source += fields[1];
        line += fields[2];
        decoded.push((offset as u64, source, line));
    }
    decoded
}

/// A source map covering every instruction of `module`: instruction `i` maps
/// to line `i` (0-based) of the single source `source_name`.
fn map_covering(module: &[u8], source_name: &str) -> String {
    let mut mappings = String::new();
    let mut last_offset = 0i64;
    for (ordinal, &offset) in instruction_offsets(module).iter().enumerate() {
        if ordinal > 0 {
            mappings.push(',');
        }
        vlq_encode(offset as i64 - last_offset, &mut mappings);
        vlq_encode(0, &mut mappings); // source delta: always the same source
        vlq_encode(i64::from(ordinal > 0), &mut mappings); // line delta
        vlq_encode(0, &mut mappings); // column delta
        last_offset = offset as i64;
    }
    format!(r#"{{"version":3,"sources":["{source_name}"],"names":[],"mappings":"{mappings}"}}"#)
}

fn write(path: &Path, contents: impl AsRef<[u8]>) {
    std::fs::write(path, contents).unwrap();
}

#[test]
fn source_maps_translate_through_the_merge() {
    let dir = tempfile::tempdir().unwrap();

    // lib defines the function app imports; both carry source maps.
    let lib_binary = wat::parse_str(
        r#"(module (func (export "answer") (result i32) (i32.const 40) (i32.const 2) (i32.add)))"#,
    )
    .unwrap();
    let app_binary = wat::parse_str(
        r#"(module
            (import "lib" "answer" (func $answer (result i32)))
            (func (export "main") (result i32) (call $answer)))"#,
    )
    .unwrap();
    let lib = dir.path().join("lib.wasm");
    let app = dir.path().join("app.wasm");
    let lib_map = dir.path().join("lib.wasm.map");
    let app_map = dir.path().join("app.wasm.map");
    write(&lib, &lib_binary);
    write(&app, &app_binary);
    write(&lib_map, map_covering(&lib_binary, "lib.ts"));
    write(&app_map, map_covering(&app_binary, "app.ts"));

    let out = dir.path().join("merged.wasm");
    let out_map = dir.path().join("merged.wasm.map");
    let output = wasm_fuse()
        .arg(format!("app={}", app.display()))
        .arg(format!("lib={}", lib.display()))
        .arg(format!("--source-map=app={}", app_map.display()))
        .arg(format!("--source-map=lib={}", lib_map.display()))
        .arg("--output-source-map")
        .arg(&out_map)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let merged = std::fs::read(&out).unwrap();
    let merged_map: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&out_map).unwrap()).unwrap();

    // Both sources survive.
    let sources: Vec<&str> = merged_map["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| source.as_str().unwrap())
        .collect();
    assert_eq!(sources, ["app.ts", "lib.ts"]);

    // Every input instruction was covered, so every merged instruction must
    // be too (4 in app+lib bodies in total: call; const, const, add — plus
    // each function's end opcode).
    let decoded = decode_mappings(merged_map["mappings"].as_str().unwrap());
    let merged_offsets = instruction_offsets(&merged);
    assert_eq!(
        decoded.len(),
        merged_offsets.len(),
        "expected full coverage: {decoded:?} vs {merged_offsets:?}"
    );

    // Segments are sorted, land exactly on instruction offsets, and keep
    // their per-module line numbering (instruction i of a module's body maps
    // to line i).
    for (position, &(offset, source, line)) in decoded.iter().enumerate() {
        assert_eq!(offset as usize, merged_offsets[position]);
        let source_name = sources[source as usize];
        // app's body has 2 instructions (call, end); lib's has 4. Order in
        // the merged binary is app's function then lib's.
        match position {
            0 | 1 => assert_eq!((source_name, line), ("app.ts", position as i64)),
            _ => assert_eq!((source_name, line), ("lib.ts", position as i64 - 2)),
        }
    }
}

#[test]
fn source_map_url_is_embedded() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module.wat");
    write(
        &module,
        r#"(module (func (export "f") (result i32) (i32.const 1)))"#,
    );
    let out = dir.path().join("merged.wasm");
    let output = wasm_fuse()
        .arg(format!("only={}", module.display()))
        .args(["--source-map-url", "https://example.com/merged.wasm.map"])
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success());

    let merged = std::fs::read(&out).unwrap();
    let mut found = None;
    for payload in Parser::new(0).parse_all(&merged) {
        if let Payload::CustomSection(section) = payload.unwrap()
            && section.name() == "sourceMappingURL"
        {
            found = Some(section.data().to_vec());
        }
    }
    let data = found.expect("missing sourceMappingURL section");
    // The section holds a length-prefixed string.
    let url = b"https://example.com/merged.wasm.map";
    assert_eq!(data[0] as usize, url.len());
    assert_eq!(&data[1..], url);
}

#[test]
fn pruned_function_mappings_are_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let lib_binary = wat::parse_str(
        r#"(module
            (func (export "used") (result i32) (i32.const 1))
            (func (export "unused") (result i32) (i32.const 2) (i32.const 3) (i32.add)))"#,
    )
    .unwrap();
    let app_binary = wat::parse_str(
        r#"(module
            (import "lib" "used" (func $used (result i32)))
            (func (export "main") (result i32) (call $used)))"#,
    )
    .unwrap();
    let lib = dir.path().join("lib.wasm");
    let app = dir.path().join("app.wasm");
    let lib_map = dir.path().join("lib.wasm.map");
    write(&lib, &lib_binary);
    write(&app, &app_binary);
    write(&lib_map, map_covering(&lib_binary, "lib.ts"));

    let out = dir.path().join("merged.wasm");
    let out_map = dir.path().join("merged.wasm.map");
    let output = wasm_fuse()
        .arg(format!("app={}", app.display()))
        .arg(format!("lib={}", lib.display()))
        .arg(format!("--source-map=lib={}", lib_map.display()))
        .args(["--entry", "app", "--prune"])
        .arg("--output-source-map")
        .arg(&out_map)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let merged_map: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&out_map).unwrap()).unwrap();
    let decoded = decode_mappings(merged_map["mappings"].as_str().unwrap());
    // Only lib's surviving function (const, end) keeps mappings; the pruned
    // function's four instructions are gone.
    assert_eq!(decoded.len(), 2, "{decoded:?}");
    let merged = std::fs::read(&out).unwrap();
    let merged_offsets = instruction_offsets(&merged);
    for &(offset, _, _) in &decoded {
        assert!(
            merged_offsets.contains(&(offset as usize)),
            "offset {offset} is not an instruction: {merged_offsets:?}"
        );
    }
}

#[test]
fn unknown_module_in_source_map_flag_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let module = dir.path().join("module.wat");
    let map = dir.path().join("module.map");
    write(
        &module,
        r#"(module (func (export "f") (result i32) (i32.const 1)))"#,
    );
    write(
        &map,
        r#"{"version":3,"sources":[],"names":[],"mappings":""}"#,
    );
    let output = wasm_fuse()
        .arg(format!("only={}", module.display()))
        .arg(format!("--source-map=ghost={}", map.display()))
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("no module named \"ghost\""),
        "unexpected stderr: {stderr}"
    );
}
