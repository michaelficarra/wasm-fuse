# wasm-fuse

Merges multiple WebAssembly core modules into one, resolving imports in one module
against exports of another at merge time. A Rust counterpart to binaryen's
`wasm-merge`, built on the Bytecode Alliance wasm-tools crates (`wasmparser`,
`wasm-encoder`, `wat`, `wasmprinter`). Usable as a library and as a CLI.

The project is at full wasm-merge capability parity: every scenario from binaryen's
`test/lit/merge` suite is ported (or documented as unportable in an `#[ignore]`d
test). The merge semantics and per-module design notes live in the crate and module
rustdoc; start at `src/lib.rs`.

## Layout

- `src/lib.rs` / `src/merge.rs` — the merge engine (library; errors via `thiserror`).
- `src/main.rs` — the CLI (clap), gated behind the default-on `cli` feature;
  `[[bin]] required-features = ["cli"]`. Library consumers use
  `default-features = false`. Keep CLI-only dependencies optional and listed in the
  `cli` feature.
- `tests/cli.rs` — integration tests that run the real binary via
  `std::process::Command` (`env!("CARGO_BIN_EXE_wasm-fuse")`).
- `tests/fixtures/binaryen/` — test inputs vendored from binaryen `test/lit/merge`
  (Apache-2.0; see NOTICE). Do not edit vendored fixtures; add new cases as separate
  files.

## CLI design

The CLI deliberately does NOT copy wasm-merge's flags (owner's decision): parity
with wasm-merge is at the capability level only. Follow clap best practices
instead. Current shape: `[NAME=]PATH` positionals (name defaults to the file
stem), `-o/--output`, `-t/--text`, `--entry NAME` (export only that module's
exports), `--export-conflicts <error|rename|skip>` (union mode only; clap-level
conflict with `--entry`), `--no-validate`. There are intentionally NO
wasm-feature toggles: the CLI always accepts/validates with all proposals
enabled; fine-grained `WasmFeatures` is exposed only through the library's
`MergeOptions`.

## Validation (run before every commit)

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features   # library still builds/tests without the CLI
cargo doc --no-deps --all-features # docs build (CI denies rustdoc warnings)
```

CI (`.github/workflows/ci.yml`) additionally checks the feature powerset
(cargo-hack), MSRV (`rust-version` in Cargo.toml), a locked lockfile, and
minimal dependency versions — keep `Cargo.lock` committed and in sync.

CI deliberately does NOT gate on semver compatibility (owner's decision — do not
re-add a cargo-semver-checks job): breaking API changes are acceptable, and
release-plz runs cargo-semver-checks itself when preparing the release PR,
bumping the major version as needed.

Also available: `cargo +nightly fuzz run fuzz_merge` (wasm-smith modules through
the merger; CI smoke-runs it on PRs) and `cargo bench --bench merge`.

## Dependency management

Dependencies come from cargo battery packs (`cargo bp`): `cli`, `error`, `logging`,
`ci` packs; state is tracked in `battery-pack.toml`. Use `cargo bp status` /
`cargo bp sync` to check/update. Caution: `cargo bp add`/`sync` rewrites dependency
entries and drops `optional = true` markers — re-check the `cli` feature's `dep:`
entries in Cargo.toml after running it.

## Conventions

- Index remapping is the heart of the engine: every input module gets per-kind
  remap tables (old index → merged index), applied via a custom
  `wasm_encoder::reencode::Reencode` implementation. Prefer extending that
  mechanism over ad-hoc rewriting.
- Error messages for merge failures follow binaryen's wording where a counterpart
  exists (tests assert on them).
- Our text output comes from `wasmprinter` and differs from binaryen's printer;
  snapshot tests assert our own output, never binaryen CHECK lines.
- The Capabilities section in README.md is the product's feature specification:
  whenever a feature is added, removed, or its behaviour changes, update that list
  in the same change. Keep its claims exactly as strong as what the tests exercise —
  nothing aspirational.
