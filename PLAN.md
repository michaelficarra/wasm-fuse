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

### Phase 2 — Remaining semantics parity  ✅
- [x] Import/export compatibility checks with binaryen-style messages (`types.wat`) —
      landed early, in phase 1.
- [x] Tags/exception-handling merging (covered by fusing/renamings fixtures).
- [x] Pruning (`src/prune.rs`): the equivalent of binaryen's unconditional
      remove-unused-module-elements pass, adopted as **opt-in** (`--prune` /
      `MergeOptions::prune_unused`) — merging alone never deletes code. Reachability
      over resolved sites from the surviving exports + all start functions; reference
      collection reuses the Reencode hooks (a recording no-op remapper), so operator
      coverage is maintained upstream. Conservative choices documented in prune.rs:
      declarative segments and their functions are roots; active segments targeting
      imported tables/memories are roots; types are never pruned; segment traps are
      assumed absent (as binaryen does). Combined with `--entry` this tree-shakes a
      bundle down to what the entry module uses.
- ~~Feature-flag surface~~ — dropped deliberately: the CLI has no wasm-feature
      toggles (always all proposals); `WasmFeatures` stays a library option.

### Phase 3 — GC, subtyping, 64-bit  ✅
- [x] Cross-module type canonicalisation (`src/types.rs`): rec groups are deduplicated
      isorecursively — each group is keyed by its byte encoding with external type refs
      canonicalised and in-group refs made group-relative; identical keys share canonical
      indices, so the merged type section carries one copy of each distinct group. The
      synthetic combined start reuses an existing plain `(func)` type when one exists.
      Types referenced by pruned items are still kept (future refinement).
- [x] Function subtyping on fuse: by *declared* supertype chains over canonical indices
      (function subtyping is nominal, not structural) — `func_subtyping.wat`,
      `func_subtyping_return.wat`, plus a new cross-module rec-group chain test.
      `FuncExact` imports require exact canonical equality.
- [x] Global/table/tag checks handle concrete heap types precisely (canonical identity,
      chain subtyping for immutable globals, exact-ref rules from custom-descriptors);
      mismatches that previously surfaced as opaque validator errors are now reported as
      merge-time "type mismatch when importing …" diagnostics.
- [x] memory64/table64 (`table64.wat`) — covered since phase 1.

### Phase 4 — Debug info, names, source maps  ✅
- [x] `-g`/`--keep-names` (`src/names.rs`): merges the inputs' name sections — module,
      function, local, label, type (canonical indices), table, memory, global, element,
      data, field, tag names — with indices remapped, pruned items' names dropped, and
      the definition's name beating a fused import's alias. The synthetic combined start
      is named `merged.start.combined` like wasm-merge. Malformed name subsections are
      skipped (advisory data), never a merge failure. `names.wat` ported with and
      without the flag.
- [x] Branch hints (`annotations.wat`): the `metadata.code.branch_hint` section is
      preserved unconditionally (like wasm-merge), with function indices remapped and
      instruction offsets translated through per-instruction offset recording in
      `Remapper::parse_function_body` (remapped indices can change instruction widths).
      Hints of pruned functions are dropped; hints pointing at no instruction are
      dropped (advisory). NB: the wat crate attaches folded-form hints to the first
      instruction of the unfolded condition and drops binaryen-proprietary annotations
      like `(@binaryen.js.called)` at parse time — `annotations-func-only.wat` tests
      only the latter and is not portable (permanently-ignored test documents this).
- [x] Source maps (`src/sourcemap.rs`): per-input maps attach by module name
      (`--source-map NAME=PATH` / `Merger::add_source_map`), segments translate
      through the per-instruction offset log plus a post-pass locating each function
      body in the merged binary, and `Merger::merge_full` returns the merged map
      (`--output-source-map`); `--source-map-url` / `MergeOptions::source_map_url`
      embeds a `sourceMappingURL` section. Sources/names deduplicate by string;
      sourceRoot is folded into sources; segments of pruned code are dropped.
      Uses serde/serde_json plus a hand-rolled VLQ codec. binaryen's sourcemap.wat
      drives maps via wasm-as from `;;@` comments the wat crate cannot see — covered
      instead by round-trip tests in `tests/source_map.rs` (documented ignored test).

### Phase 5 — Full parity & hardening
- [x] `--output-manifest` / `MergeOptions::emit_manifest` + `Merged::manifest`
      (`manifest.wat` ported; content matches binaryen byte for byte; implies names,
      like wasm-merge's flag implies `-g`; unnamed functions listed by merged index).
      **This closed the last wasm-merge capability gap.**
- [x] Fuzzing (`fuzz/`, via the ci battery pack fuzzing template): wasm-smith modules
      merged under randomised options; PR smoke runs + nightly runs in CI; 214k local
      runs clean. Future refinement: steer wasm-smith to generate cross-module imports
      that actually fuse more often.
- [x] criterion benchmarks (`benches/merge.rs`): import-chain fusing at 2/8/32 modules
      and wide-module re-encode throughput at 100/1000 functions. The Bencher CI
      workflow stays dormant until a BENCHER_PROJECT repo variable is configured.
- [x] cargo-deny/audit in CI (deny.toml + scheduled audit workflow, from phase 0).
- [ ] Publish preparation: restore the cargo-semver-checks CI job once a baseline is on
      crates.io (TODO note in ci.yml); release-plz scaffolding already present.
- [x] Custom-section handling documented: name sections merged (`--keep-names`),
      branch hints always preserved, `sourceMappingURL` regenerated via
      `--source-map-url`; all other custom sections are dropped, as binaryen does
      (its own TODO).

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
| func_subtyping.wat / func_subtyping_return.wat | function subtyping (GC) | ✅ ported (precise merge-time checks via canonical supertype chains) |
| global_subtyping.wat | global subtyping (GC) | ✅ ported (ditto) |
| table64.wat | 64-bit tables | ✅ ported |
| names.wat with -g | name-section merging | ✅ ported (`names_kept`, plus alias/synthetic-start/prune behaviour tests) |
| sourcemap.wat | source map preservation | ✅ capability covered by tests/source_map.rs round-trips (fixture itself not portable: `;;@` comments are invisible to the wat crate) |
| annotations.wat | branch hints | ✅ ported (binaryen-proprietary `@binaryen.js.called` cannot round-trip through the wat crate) |
| annotations-func-only.wat | binaryen-proprietary annotations only | ✖ not portable (documented ignored test) |
| manifest.wat | `--output-manifest` | ✅ ported (manifest content matches binaryen byte for byte) |

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
- 2026-06-09: crate renamed to `wasm-fuse` (wasm-bundle was taken on crates.io). Fixed
  the minimal-versions CI job by declaring the dependency minimums we actually test
  (and a documented lazy_static raise for sharded-slab's dishonest minimum), verified
  locally with the job's own commands. (The GitHub repository was later renamed to
  match; Cargo.toml `repository` and the README badge now point at
  michaelficarra/wasm-fuse.)
- 2026-06-09: Phase 2 complete — opt-in pruning landed (--prune /
  MergeOptions::prune_unused) with five behaviour tests (unused library code, unused
  imports, start-function liveness, dead table+segment removal, call_indirect keep-
  alive cascade). Refinement ideas left for later: filter declarative segment items to
  live functions; prune unused types alongside phase 3 canonicalisation. Next: phase 3.
- 2026-06-09: Phase 3 complete — isorecursive type canonicalisation dedupes rec groups
  across modules (every snapshot with duplicated types shrank; reviewed); import/export
  checks now decide concrete heap types via canonical indices and declared supertype
  chains, including exact-ref rules. Three new tests: dedup count, merge-time concrete
  mismatch diagnostics, cross-module subtype-chain satisfaction. Types unused after
  pruning are still emitted — noted as a refinement. Next: phase 4 (names section,
  source maps, annotations).
- 2026-06-09: Phase 4 complete across three commits — name-section merging
  (-g/--keep-names), branch-hint preservation (with the per-instruction offset
  infrastructure), and source maps (--source-map NAME=PATH, --output-source-map,
  --source-map-url; library: add_source_map + merge_full + source_map_url option).
  Remaining wasm-merge capability gap: --output-manifest only (phase 5).
