# wasm-fuse — project plan

## Context

`wasm-fuse` is a Rust reimplementation of [binaryen's `wasm-merge`](https://github.com/WebAssembly/binaryen/blob/main/src/tools/wasm-merge.cpp):
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
- **CLI**: clap (derive), deliberately NOT flag-compatible with wasm-merge (owner's
  decision; capability parity only): `[NAME=]PATH` inputs with stem-derived default
  names, `--entry`, `--export-conflicts <error|rename|skip>`, `-t/--text`,
  `--no-validate`, and intentionally no wasm-feature toggles (the CLI always runs
  with all proposals enabled; `WasmFeatures` remains a library-level option).
- **Export selection** (extension beyond wasm-merge): `ExportSelection::Union(policy)`
  unions every module's exports like wasm-merge; `ExportSelection::Entry(name)` keeps
  only the entry-point module's exports, the other modules serving solely to satisfy
  (some of) its imports.
- **Testing**: binaryen's `test/lit/merge` inputs are vendored under
  `tests/fixtures/binaryen/` (Apache-2.0, with NOTICE); integration tests drive the real
  binary via `std::process::Command` + `env!("CARGO_BIN_EXE_wasm-fuse")`; snapshot tests
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

### Phase 1 — MVP merge engine  ✅
Goal: merge real modules with import fusing; cover the core binaryen test scenarios.
- [x] Library API: `Merger::new(options).add_module(name, bytes)?…merge() -> Vec<u8>`;
      `MergeOptions { export_conflicts: Error|Rename|Skip, features, validate }`.
- [x] Collect pass (`src/parse.rs`), import resolution with transitive chasing + cycle
      error (`src/resolve.rs`), per-kind remap tables; surviving imports first in each
      index space; fused imports dropped from the output (binaryen reaches the same end
      state via its unconditional remove-unused-module-elements pass — we do NOT run
      general DCE; that divergence is deliberate for now, revisit in phase 5).
- [x] Emit pass (`src/emit.rs`) via a `Reencode` impl carrying remap tables
      (`src/remap.rs`); types/rec groups copied verbatim without dedup; start fusion
      (synthetic combined start); export conflict policies (error / rename `_N` / skip);
      output validation.
- [x] Global reordering (stable topological sort of init-expr dependencies).
- [x] Fused import/export compatibility checks (`src/check.rs`) with wasm-merge-style
      aggregated "type mismatch when importing …" errors; function subtyping and
      immutable-global subtyping over *abstract* heap types (concrete heap types are
      skipped until phase 3 type canonicalisation — output validation still catches
      those). NB: like wasm-merge, inputs are never validated individually — only the
      merged output is (an input may only become valid once merged, e.g. ref.func of an
      import fused to an exported definition).
- [x] CLI: initially mirrored wasm-merge's flags; redesigned same day to wasm-fuse's
      own best-practice interface (see Architecture decisions) at the owner's direction.
- [x] Binaryen fixtures vendored (`tests/fixtures/binaryen/merge/`, CHECK comments
      stripped, legacy-EH syntax in renamings adapted to try_table — see NOTICE);
      `tests/binaryen_suite.rs` runs 21 ported scenarios via `std::process::Command`
      with snapbox snapshots (`tests/snapshots/`, bless with `SNAPSHOTS=overwrite`).
- [x] New tests beyond binaryen's (`tests/cli.rs`): single-module identity merge,
      5-module fusing chain, binary/text mixed inputs, binary stdout/file output,
      duplicate module names, unresolved imports preserved, `-n` skipping checks,
      missing-file diagnostics.

### Phase 2 — Remaining semantics parity
- [x] Import/export compatibility checks with binaryen-style messages (`types.wat`) —
      landed early, in phase 1.
- [x] Tags/exception-handling merging (covered by fusing/renamings fixtures).
- [ ] Decide on and (if adopted) implement binaryen's remove-unused-module-elements
      equivalent (reachability DCE from exports/start/segments), possibly opt-in.
- ~~Feature-flag surface~~ — dropped deliberately: the CLI has no wasm-feature
      toggles (always all proposals); `WasmFeatures` stays a library option.

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
| chain.wat | re-export chains across 3 modules | ✅ ported |
| cycle.wat | circular imports via internalisation | ✅ ported |
| import_cycle.wat | infinite import loop → error | ✅ ported (we name the import field at the cycle, binaryen the internal item) |
| export_options.wat | `-rec` / `-sec` | ✅ ported (both variants) |
| export_options_default.wat | conflict → error | ✅ ported |
| fusing.wat | import/export fusing (funcs, memories, tags) | ✅ ported |
| names.wat / renamings.wat | renaming across all item kinds | ✅ ported (names without `-g` → phase 4; renamings' legacy EH adapted to try_table) |
| memory_data.wat | memory + data segment remapping | ✅ ported |
| table_elem.wat | table + element segment remapping | ✅ ported |
| global-ordering.wat | global initialiser reordering | ✅ ported |
| start.wat / start.flip.wat / start-return.wat / start3.wat | start fusion | ✅ ported |
| types.wat | import/export type mismatch errors | ✅ ported (all 17 mismatches reported, matching binaryen's list) |
| func_subtyping.wat / func_subtyping_return.wat | function subtyping (GC) | ✅ ported (concrete-type checks via output validation until phase 3) |
| global_subtyping.wat | global subtyping (GC) | ✅ ported (ditto) |
| table64.wat | 64-bit tables | ✅ ported |
| sourcemap.wat | source map preservation | ⏳ phase 4 (ignored test in place) |
| annotations.wat / annotations-func-only.wat | branch hints / annotations | ⏳ phase 4 (ignored test in place) |
| manifest.wat | `--output-manifest` | ⏳ phase 5 (ignored test in place) |

## Log

- 2026-06-09: research complete (wasm-merge semantics, test inventory, wasm-tools 0.251.x,
  battery packs); plan written; Phase 0 begun.
- 2026-06-09: Phase 0 complete (scaffold, CI, docs, licences). Phase 1 complete: merge
  engine (parse → resolve → check → emit over wasm-encoder's Reencode), CLI, 21 binaryen
  scenarios ported and passing + 12 CLI behaviour tests. Key findings recorded:
  wasm-merge validates output only (not inputs); it runs reorder-globals-always and full
  remove-unused-module-elements after merging (we drop fused imports but run no general
  DCE); binaryen's text parser accepts legacy EH syntax the wat crate doesn't.
- 2026-06-09: CLI redesigned away from wasm-merge mirroring at the owner's direction
  (`[NAME=]PATH` inputs, `--export-conflicts`, `-t/--text`, `--no-validate`, no feature
  toggles). Added export selection modes: union (wasm-merge behaviour) vs `--entry NAME`
  / `ExportSelection::Entry` (only the entry module's exports; other modules just
  satisfy its imports). GitHub remote added; CI running upstream.
- 2026-06-09: crate renamed to `wasm-fuse` (wasm-bundle was taken on crates.io); the
  GitHub repository is still named wasm-bundle — update Cargo.toml `repository` and the
  README CI badge if it gets renamed. Fixed the minimal-versions CI job by declaring
  the dependency minimums we actually test (and a documented lazy_static raise for
  sharded-slab's dishonest minimum), verified locally with the job's own commands.
