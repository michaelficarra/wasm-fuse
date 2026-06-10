# wasm-bundle — project plan

## Context

`wasm-bundle` is a Rust reimplementation of [binaryen's `wasm-merge`](https://github.com/WebAssembly/binaryen/blob/main/src/tools/wasm-merge.cpp):
it merges multiple WebAssembly core modules into a single module, resolving imports in one
module against exports of another at merge time (the way a JS bundler turns runtime linking
into build-time linking). Unlike `wasm-merge` it is built on Bytecode Alliance plumbing
(`wasmparser`, `wasm-encoder`, `wat`, `wasmprinter`) rather than binaryen's C++ IR, and it is
designed to be consumed **both as a library and as a CLI**. No pure-Rust equivalent exists
(wasm-tools' `compose`/WAC operate on components, not core modules).

Dependency management follows community battery packs via [`cargo-bp`](https://crates.io/crates/cargo-bp)
(`cli`, `error`, and `ci` battery packs).

## Reference semantics (from binaryen wasm-merge)

- Input is a sequence of `(file, name)` pairs. An import `(import "N" "x" …)` where `N` is one
  of the given module names is **fused** to the item exported as `x` from that module.
  Resolution is transitive (re-exported imports are chased) with an
  `infinite loop of imports` error on cycles. Forward references (earlier module importing
  from a later one) are supported.
- Imports naming modules outside the input set remain imports.
- Export name conflicts: error by default; `--rename-export-conflicts` appends `_1`, `_2`, …;
  `--skip-export-conflicts` keeps the first export and drops later ones.
- All index spaces are concatenated and remapped (functions, tables, memories, globals, tags,
  types/rec groups, element and data segments). Memories and tables are kept separate
  (no fusing), so multi-memory output is possible.
- Multiple start functions are fused into a synthetic `merged.start.combined` that calls each
  in input order.
- Fused import/export pairs are type-checked (function subtyping; global mutability and
  type/subtype; table & memory limits and index type; tags exactly) with descriptive errors.
- Globals are reordered so initialisers only read earlier globals.
- Source maps (`-ism` per input, `-osm`/`-osu` for output), `--output-manifest` (wasm-split
  manifest, implies `-g`), `-g`/`--debuginfo` names section, `-S` text output, feature flags
  (`--enable-*` / `--disable-*` / `-all` / `-mvp`), `--no-validation`.

## Architecture decisions

- **Single package, two consumption modes**: `src/lib.rs` (merge engine; errors via
  `thiserror`) plus `src/main.rs` CLI behind a default-on `cli` feature
  (`[[bin]] required-features = ["cli"]`), so library consumers can take
  `default-features = false` and avoid clap/anyhow et al.
- **Index-based remapping**: unlike binaryen's name-based IR, wasmparser/wasm-encoder are
  index-based. Each input module gets per-kind remap tables (old index → merged index); a
  custom `wasm_encoder::reencode::Reencode` implementation carries those tables so function
  bodies and const expressions are rewritten mechanically.
- **Parsing**: `wat::parse_bytes` accepts both text and binary input, matching wasm-merge.
- **Validation**: `wasmparser::Validator` with configurable `WasmFeatures`, on by default.
- **Text output**: `wasmprinter` for `-S`. Note our text formatting differs from binaryen's,
  so ported tests assert semantic properties / our own snapshots, not binaryen CHECK lines.
- **CLI**: clap (derive), flag names mirror wasm-merge so existing invocations translate
  directly.
- **Testing**: binaryen's `test/lit/merge` inputs are vendored under
  `tests/fixtures/binaryen/` (Apache-2.0, with NOTICE); integration tests drive the real
  binary via `std::process::Command` + `env!("CARGO_BIN_EXE_wasm-bundle")`; snapshot tests
  cover merged text output; unit tests cover remap/resolution logic.

## Phases

### Phase 0 — Scaffolding & CI  ✅
- [x] Toolchain check (rustc 1.96, edition 2024), install `cargo-bp`.
- [x] Battery packs `cli`/`error`/`logging`/`ci` registered; lib+bin layout with
      default-on `cli` feature (`[[bin]] required-features`). NB: `cargo bp add`
      rewrites dependency entries and drops `optional = true` — re-check after use.
- [x] wasm-tools dependencies (`wasmparser`, `wasm-encoder` (+`wasmparser` feature for
      reencode), `wat`, `wasmprinter` 0.251.x).
- [x] GitHub Actions from ci battery pack `full` template: fmt, clippy (-D warnings),
      warnings, docsrs, build matrix (ubuntu/macos/windows × stable, nightly on ubuntu,
      ±default features), cargo-hack feature powerset, MSRV, lockfile, minimal-versions,
      dependabot, scheduled audit + rust-next, release-plz scaffolding. The
      cargo-semver-checks job is deferred until first crates.io publish (TODO in ci.yml).
- [x] README (install/use/contribute), CLAUDE.md, LICENSE-MIT + LICENSE-APACHE, NOTICE.
- [x] Validated (fmt, clippy -D warnings, tests with/without default features) and
      committed.
- NB: `repository` in Cargo.toml and badge URLs use an OWNER placeholder until a GitHub
      remote exists.

### Phase 1 — MVP merge engine
Goal: merge real modules with import fusing; cover the core binaryen test scenarios.
- [ ] Library API: `Merger::new(options).add_module(name, bytes)?…merge() -> Vec<u8>`;
      `MergeOptions { export_conflicts: Error|Rename|Skip, validate, features, debug_names }`.
- [ ] Collect pass: per-module items, export maps, import resolution (transitive, cycle
      error), per-kind remap tables; unresolved imports first in each index space.
- [ ] Emit pass: all sections re-encoded with remapping (types/rec groups copied verbatim
      without dedup for now), start fusion, export conflict policies, output validation.
- [ ] Global reordering (topological sort of init-expr dependencies).
- [ ] CLI: positional pairs, `-o` (`-` = stdout), `-S`, `-rec`, `-sec`, `-n`.
- [ ] Vendor binaryen fixtures; integration tests for: chain, cycle, import_cycle (error),
      export_options, export_options_default (error), start, start.flip, start-return,
      start3, names, renamings, memory_data, table_elem, fusing, global-ordering.
- [ ] New tests beyond binaryen's: single-module identity merge, >3 modules, stdin/stdout
      behaviour, malformed input errors, duplicate module names.

### Phase 2 — Validation & feature-flag parity
- [ ] Import/export compatibility checks with binaryen-style messages (`types.wat`):
      function types, global mutability + type, table/memory limits + index type, tags.
- [ ] Full feature-flag surface: `--enable-*`/`--disable-*`, `-all`, `-mvp` mapped to
      `WasmFeatures`; multi-memory output paths.
- [ ] Tags/exception-handling merging end to end.
- [ ] `--quiet`, `--version`, help text polish.

### Phase 3 — GC, subtyping, 64-bit
- [ ] Rec-group copying with cross-module type deduplication/canonicalisation.
- [ ] Function subtyping on fuse (`func_subtyping.wat`, `func_subtyping_return.wat`).
- [ ] Global subtyping (`global_subtyping.wat`).
- [ ] memory64/table64 (`table64.wat`).

### Phase 4 — Debug info, names, source maps
- [ ] `-g`/`--debuginfo`: merge names sections (module/function/local/type/etc.) with
      remapped indices (`names.wat`).
- [ ] Source maps: `-ism` per input, `-osm`, `-osu` (`sourcemap.wat`).
- [ ] Code annotations / branch hints (`annotations.wat`, `annotations-func-only.wat`).

### Phase 5 — Full parity & hardening
- [ ] `--output-manifest` (wasm-split manifest; implies `-g`) (`manifest.wat`).
- [ ] Fuzzing: wasm-smith-generated module sets merged and validated.
- [ ] criterion benchmarks (many/large modules); performance pass.
- [ ] cargo-deny/audit in CI; release/publish preparation (trusted publishing template).
- [ ] Custom-section handling decisions documented (binaryen leaves this TODO).

## Parity checklist (binaryen `test/lit/merge` → status)

| Fixture | Covers | Status |
|---|---|---|
| chain.wat | re-export chains across 3 modules | pending |
| cycle.wat | circular imports via internalisation | pending |
| import_cycle.wat | infinite import loop → error | pending |
| export_options.wat | `-rec` / `-sec` | pending |
| export_options_default.wat | conflict → error | pending |
| fusing.wat | import/export fusing (funcs, memories, tags) | pending |
| names.wat / renamings.wat | renaming across all item kinds | pending |
| memory_data.wat | memory + data segment remapping | pending |
| table_elem.wat | table + element segment remapping | pending |
| global-ordering.wat | global initialiser reordering | pending |
| start.wat / start.flip.wat / start-return.wat / start3.wat | start fusion | pending |
| types.wat | import/export type mismatch errors | pending |
| func_subtyping.wat / func_subtyping_return.wat | function subtyping (GC) | pending |
| global_subtyping.wat | global subtyping (GC) | pending |
| table64.wat | 64-bit tables | pending |
| sourcemap.wat | source map preservation | pending |
| annotations.wat / annotations-func-only.wat | branch hints / annotations | pending |
| manifest.wat | `--output-manifest` | pending |

## Log

- 2026-06-09: research complete (wasm-merge semantics, test inventory, wasm-tools 0.251.x,
  battery packs); plan written; Phase 0 begun.
