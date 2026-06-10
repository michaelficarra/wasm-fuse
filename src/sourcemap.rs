//! Source map support, the counterpart of wasm-merge's `-ism`/`-osm`/`-osu`.
//!
//! WebAssembly source maps follow the Source Map V3 convention used by
//! emscripten, binaryen, and browsers: every mapping lives on generated line
//! zero, and the generated *column* holds the byte offset of an instruction
//! within the wasm binary. Merging therefore means translating each input
//! segment's offset — through the per-instruction offset log recorded while
//! re-encoding function bodies, plus the position each body landed at in the
//! merged binary — and re-emitting one combined map. Segments that point at
//! nothing that survived (pruned functions, non-instruction offsets) are
//! dropped; sources and names deduplicate by string.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use wasmparser::{Parser, Payload};

use crate::merge::MergeError;

/// A decoded input source map.
#[derive(Debug)]
pub(crate) struct InputSourceMap {
    sources: Vec<String>,
    /// Aligned with `sources`; empty when the map carried no content.
    sources_content: Vec<Option<String>>,
    names: Vec<String>,
    segments: Vec<Segment>,
}

#[derive(Debug)]
struct Segment {
    /// Byte offset of the mapped instruction within the input binary.
    offset: u64,
    source: u32,
    line: i64,
    column: i64,
    name: Option<u32>,
}

#[derive(Deserialize)]
struct RawMap {
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default, rename = "sourcesContent")]
    sources_content: Vec<Option<String>>,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    mappings: String,
    #[serde(default, rename = "sourceRoot")]
    source_root: Option<String>,
}

#[derive(Serialize)]
struct RawOutput<'a> {
    version: u32,
    sources: &'a [String],
    #[serde(rename = "sourcesContent", skip_serializing_if = "Option::is_none")]
    sources_content: Option<&'a [Option<String>]>,
    names: &'a [String],
    mappings: String,
}

/// Parse and decode `json` as the source map of `module`.
pub(crate) fn parse(module: &str, json: &[u8]) -> Result<InputSourceMap, MergeError> {
    let error = |message: String| MergeError::SourceMap {
        module: module.to_string(),
        message,
    };
    let raw: RawMap = serde_json::from_slice(json).map_err(|source| error(source.to_string()))?;

    // Fold sourceRoot into the sources so the merged map needs no root.
    let root = raw.source_root.unwrap_or_default();
    let sources = raw
        .sources
        .into_iter()
        .map(|source| {
            if root.is_empty() {
                source
            } else if root.ends_with('/') {
                format!("{root}{source}")
            } else {
                format!("{root}/{source}")
            }
        })
        .collect::<Vec<_>>();
    let mut sources_content = raw.sources_content;
    if !sources_content.is_empty() {
        sources_content.resize(sources.len(), None);
    }

    // Wasm source maps put everything on generated line zero; later groups
    // cannot address wasm offsets, so only the first is read.
    let first_line = raw.mappings.split(';').next().unwrap_or("");
    let mut segments = Vec::new();
    let mut offset: i64 = 0;
    let mut source: i64 = 0;
    let mut line: i64 = 0;
    let mut column: i64 = 0;
    let mut name: i64 = 0;
    for encoded in first_line.split(',').filter(|encoded| !encoded.is_empty()) {
        let fields = vlq_decode(encoded).map_err(&error)?;
        match fields.len() {
            // A bare generated column maps to no source position.
            1 => continue,
            4 | 5 => {}
            arity => return Err(error(format!("mapping segment has {arity} fields"))),
        }
        offset += fields[0];
        source += fields[1];
        line += fields[2];
        column += fields[3];
        if offset < 0 || source < 0 || source as usize >= sources.len() {
            return Err(error("mapping segment out of range".to_string()));
        }
        let name_index = if fields.len() == 5 {
            name += fields[4];
            if name < 0 || name as usize >= raw.names.len() {
                return Err(error("mapping segment name out of range".to_string()));
            }
            Some(name as u32)
        } else {
            None
        };
        segments.push(Segment {
            offset: offset as u64,
            source: source as u32,
            line,
            column,
            name: name_index,
        });
    }

    Ok(InputSourceMap {
        sources,
        sources_content,
        names: raw.names,
        segments,
    })
}

/// Per emitted function, in code-section order: for each instruction, its
/// source module, input absolute offset, and output body-relative offset.
/// Inlined instructions carry their own source module.
pub(crate) type CodeOffsets = Vec<Vec<(usize, usize, u32)>>;

/// Build the merged source map from the inputs' maps, the offset log recorded
/// during emission, and the final merged binary (parsed to locate where each
/// function body landed).
pub(crate) fn build(
    maps: &[Option<&InputSourceMap>],
    code_offsets: &CodeOffsets,
    merged: &[u8],
) -> Result<String, MergeError> {
    // Where each output function body starts. The merged binary is our own
    // (just emitted), so parse errors are internal bugs.
    let mut body_starts = Vec::new();
    for payload in Parser::new(0).parse_all(merged) {
        if let Payload::CodeSectionEntry(body) = payload.map_err(MergeError::Validation)? {
            body_starts.push(body.range().start);
        }
    }

    // (module, input absolute offset) → output absolute offset. A possible
    // synthetic start function at the end of the code section has no log
    // entry and no mappings.
    let mut translate: HashMap<(usize, usize), u64> = HashMap::new();
    for (ordinal, offsets) in code_offsets.iter().enumerate() {
        let body_start = body_starts[ordinal];
        for &(instr_module, input_offset, output_relative) in offsets {
            // First entry wins: synthetic inline prologues share the call
            // instruction's offset, and the splice start is the best target.
            translate
                .entry((instr_module, input_offset))
                .or_insert((body_start + output_relative as usize) as u64);
        }
    }

    // Merge sources and names, deduplicating by string.
    let mut sources: Vec<String> = Vec::new();
    let mut sources_content: Vec<Option<String>> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut source_index: HashMap<String, u32> = HashMap::new();
    let mut name_index: HashMap<String, u32> = HashMap::new();
    let mut output: Vec<Segment> = Vec::new();
    for (module_idx, map) in maps.iter().enumerate() {
        let Some(map) = map else { continue };
        let source_map: Vec<u32> = map
            .sources
            .iter()
            .enumerate()
            .map(|(index, path)| {
                *source_index.entry(path.clone()).or_insert_with(|| {
                    sources.push(path.clone());
                    sources_content
                        .push(map.sources_content.get(index).cloned().unwrap_or_default());
                    sources.len() as u32 - 1
                })
            })
            .collect();
        let name_map: Vec<u32> = map
            .names
            .iter()
            .map(|name| {
                *name_index.entry(name.clone()).or_insert_with(|| {
                    names.push(name.clone());
                    names.len() as u32 - 1
                })
            })
            .collect();
        for segment in &map.segments {
            let Some(&offset) = translate.get(&(module_idx, segment.offset as usize)) else {
                continue; // pruned or not an instruction: dropped
            };
            output.push(Segment {
                offset,
                source: source_map[segment.source as usize],
                line: segment.line,
                column: segment.column,
                name: segment.name.map(|name| name_map[name as usize]),
            });
        }
    }
    output.sort_by_key(|segment| segment.offset);

    // Encode as a single generated line, all fields delta-encoded.
    let mut mappings = String::new();
    let mut last_offset: i64 = 0;
    let mut last_source: i64 = 0;
    let mut last_line: i64 = 0;
    let mut last_column: i64 = 0;
    let mut last_name: i64 = 0;
    for (position, segment) in output.iter().enumerate() {
        if position > 0 {
            mappings.push(',');
        }
        vlq_encode(segment.offset as i64 - last_offset, &mut mappings);
        vlq_encode(i64::from(segment.source) - last_source, &mut mappings);
        vlq_encode(segment.line - last_line, &mut mappings);
        vlq_encode(segment.column - last_column, &mut mappings);
        last_offset = segment.offset as i64;
        last_source = i64::from(segment.source);
        last_line = segment.line;
        last_column = segment.column;
        if let Some(name) = segment.name {
            vlq_encode(i64::from(name) - last_name, &mut mappings);
            last_name = i64::from(name);
        }
    }

    let raw = RawOutput {
        version: 3,
        sources: &sources,
        sources_content: sources_content
            .iter()
            .any(Option::is_some)
            .then_some(&sources_content),
        names: &names,
        mappings,
    };
    serde_json::to_string(&raw).map_err(|source| MergeError::SourceMap {
        module: "<output>".to_string(),
        message: source.to_string(),
    })
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn vlq_encode(value: i64, out: &mut String) {
    let mut rest = if value < 0 {
        ((value.unsigned_abs()) << 1) | 1
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

fn vlq_decode(encoded: &str) -> Result<Vec<i64>, String> {
    let mut values = Vec::new();
    let mut value: u64 = 0;
    let mut shift = 0u32;
    for byte in encoded.bytes() {
        let digit = BASE64
            .iter()
            .position(|&candidate| candidate == byte)
            .ok_or_else(|| format!("invalid VLQ character {:?}", byte as char))?
            as u64;
        value |= (digit & 0x1f) << shift;
        if digit & 0x20 != 0 {
            shift += 5;
            if shift > 62 {
                return Err("VLQ value too large".to_string());
            }
        } else {
            let signed = if value & 1 != 0 {
                -((value >> 1) as i64)
            } else {
                (value >> 1) as i64
            };
            values.push(signed);
            value = 0;
            shift = 0;
        }
    }
    if shift != 0 {
        return Err("truncated VLQ value".to_string());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_round_trips() {
        for value in [0, 1, -1, 15, 16, -16, 31, 32, 1_000_000, -123_456_789] {
            let mut encoded = String::new();
            vlq_encode(value, &mut encoded);
            assert_eq!(vlq_decode(&encoded).unwrap(), vec![value], "{value}");
        }
    }

    #[test]
    fn parses_a_minimal_map() {
        // Two segments at offsets 5 and 9, both in a.ts.
        let mut mappings = String::new();
        for delta in [[5i64, 0, 1, 2], [4, 0, 1, 3]] {
            if !mappings.is_empty() {
                mappings.push(',');
            }
            for value in delta {
                vlq_encode(value, &mut mappings);
            }
        }
        let json =
            format!(r#"{{"version":3,"sources":["a.ts"],"names":[],"mappings":"{mappings}"}}"#);
        let map = parse("test", json.as_bytes()).unwrap();
        assert_eq!(map.sources, ["a.ts"]);
        assert_eq!(map.segments.len(), 2);
        assert_eq!((map.segments[0].offset, map.segments[0].line), (5, 1));
        assert_eq!((map.segments[1].offset, map.segments[1].line), (9, 2));
    }

    #[test]
    fn source_root_is_folded_into_sources() {
        let json =
            r#"{"version":3,"sourceRoot":"src","sources":["a.ts"],"names":[],"mappings":""}"#;
        let map = parse("test", json.as_bytes()).unwrap();
        assert_eq!(map.sources, ["src/a.ts"]);
    }
}
