# wasm-fuse

[![CI](https://github.com/michaelficarra/wasm-fuse/actions/workflows/ci.yml/badge.svg)](https://github.com/michaelficarra/wasm-fuse/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/wasm-fuse.svg)](https://crates.io/crates/wasm-fuse)
[![docs.rs](https://docs.rs/wasm-fuse/badge.svg)](https://docs.rs/wasm-fuse)

Merge multiple WebAssembly core modules into a single module — a Rust counterpart to
[binaryen's `wasm-merge`](https://github.com/WebAssembly/binaryen), built on the
Bytecode Alliance [wasm-tools](https://github.com/bytecodealliance/wasm-tools) crates
and usable both as a library and as a CLI.

Each input module has a name (defaulting to its file stem). Imports in any module of
the form `(import "NAME" "item" …)`, where `NAME` is the name of another input
module, are resolved at merge time to the item that module exports as `item` — the
way a JavaScript bundler replaces runtime module loading with build-time linking.
Imports referring to modules outside the input set are left as imports, and circular
references between input modules are supported.

The merged module's exports are by default the **union** of every input's exports
(what `wasm-merge` produces); with `--entry` (repeatable), only the listed
**entry-point modules'** exports are kept — the other modules then only serve to
satisfy (some of) their imports. With `--no-exports`, no exports are kept at all —
for modules whose behaviour lives in their start function.

> **Status**: at full `wasm-merge` capability parity.

## Capabilities

**Merging and linking**

- Merges any number of core modules into one; every index space — functions, tables,
  memories, globals, tags, element and data segments, types — is concatenated and
  remapped.
- Fuses imports naming another input module to that module's exports: transitive
  through re-export chains, with forward and circular references supported and
  unresolvable import cycles reported.
- Imports of modules outside the input set remain imports (partial linking).
- Multiple memories and tables are kept side by side, never fused.
- Types deduplicate across modules by isorecursive equivalence.
- Multiple start functions fuse into one synthetic start (`merged.start.combined`)
  that calls each in input order; globals are reordered so initialisers only read
  earlier globals.
- Every WebAssembly proposal is accepted — GC, exception handling, multi-memory,
  memory64/table64, threads, exact references, … — with no feature flags; the
  library exposes fine-grained, dependency-independent `WasmFeatures` control.
- Output is deterministic.

**Export selection**

- By default every input's exports are kept, with duplicate names resolved by
  policy — error (default), rename (`_1`, `_2`, …), or skip (first wins).
- `--entry` (repeatable) keeps only the listed modules' exports; the list is a
  priority order — on a name clash the earlier-listed module's export wins and
  the policy handles the rest. The other modules merely satisfy imports;
  unknown or duplicate entry names are errors.
- `--no-exports` keeps no exports at all: the merged module acts through its
  start functions; with `--prune` everything they do not reach is dropped.

**Checking and validation**

- Fused import/export pairs are checked at merge time, all mismatches reported
  together: function subtype chains (exact references require equality), global
  mutability and content subtyping, table/memory limits, index types, sharedness,
  page sizes, invariant tags — precise for concrete GC types via canonical identity.
- The merged output is validated; inputs are not (one may only become valid once
  merged). `--no-validate` skips both.

**Optimisation (opt-in)**

- `--inline` splices every function with exactly one call site into its caller and
  removes it — when nothing else references it (exports, start, `ref.func`, tail
  calls, element segments) and it is not recursive. Parameters become fresh locals
  assigned from the stack; declared defaultable locals are re-zeroed at the splice
  unless provably written before read (or never read); `return` instructions become
  branches to a wrapper block. Chains inline transitively; inlining works across
  module boundaries through fused imports, and inlined code keeps its source-map
  attribution.
- `--prune` drops everything unreachable from the kept exports and start functions,
  across all item kinds and segments, with liveness flowing through fused imports.
  Declarative element segments and active segments targeting imported tables or
  memories are kept. With `--entry`, tree-shakes a bundle down to what the entry
  modules use.

**Debug information**

- `-g`/`--keep-names` merges every name subsection — module, function, local, label,
  type, table, memory, global, element, data, field, tag — onto merged indices; a
  definition's name beats a fused import's alias; pruned items lose their names.
- Branch hints are always preserved, with function indices remapped and instruction
  offsets translated.
- Source maps: per-input maps merge into one, with mappings translated to merged
  byte offsets, sources and names deduplicated, `sourceRoot` folded in, and an
  optional embedded `sourceMappingURL`.
- `--output-manifest` writes a wasm-split manifest (function names per non-primary
  module); implies keeping names.
- All other custom sections are dropped, as `wasm-merge` does.

**Interface**

- A library — `Merger`/`MergeOptions`, typed errors, `merge_full` returning module,
  source map, and manifest, no CLI dependencies under `default-features = false` —
  and a CLI over the same engine.
- Binary or text inputs, freely mixed; module names default to file stems, with
  `NAME=PATH` to override; output to file or stdout, binary or text.

## Installation as a CLI

```sh
cargo install wasm-fuse
```

Or from a checkout: `cargo install --path .`

### CLI usage

```sh
# Merge two modules. Module names default to file stems, so imports like
# (import "env" "foo" ...) in app.wasm resolve against env.wasm's exports.
wasm-fuse app.wasm env.wasm -o merged.wasm

# Name a module explicitly with NAME=PATH
wasm-fuse app.wasm libc=wasi-libc.wasm -o merged.wasm

# Inputs may be WebAssembly text; --text/-t emits text output
wasm-fuse first.wat second.wat --text -o merged.wat

# Keep only app's exports; lib.wasm just satisfies app's imports.
# --entry is repeatable: earlier-listed modules win export name conflicts
wasm-fuse app.wasm lib.wasm --entry app -o merged.wasm
wasm-fuse app.wasm admin.wasm lib.wasm --entry app --entry admin -o merged.wasm

# Keep no exports: the module acts through its start function; with
# --prune everything the start functions do not reach is dropped
wasm-fuse init.wasm lib.wasm --no-exports --prune -o merged.wasm

# Tree-shake: also drop everything app's exports and the start functions
# never reach (wasm-merge does this unconditionally; here it is opt-in)
wasm-fuse app.wasm lib.wasm --entry app --prune -o merged.wasm

# Keep debug names (the "name" custom section), remapped onto the merged
# indices — wasm-merge's -g
wasm-fuse app.wasm lib.wasm -g -o merged.wasm

# Write a wasm-split manifest (function names per non-primary module),
# for splitting the merged module again later; implies --keep-names
wasm-fuse app.wasm lib.wasm --output-manifest merged.manifest -o merged.wasm

# Merge source maps alongside the modules; optionally embed the map URL
wasm-fuse app.wasm lib.wasm \
    --source-map app=app.wasm.map --source-map lib=lib.wasm.map \
    --output-source-map merged.wasm.map \
    --source-map-url merged.wasm.map \
    -o merged.wasm

# Pick a conflict policy for duplicate export names: error (default),
# rename (appends _1, _2, ...), or skip (earlier export wins)
wasm-fuse a.wasm b.wasm --export-conflicts rename -o out.wasm
```

All WebAssembly proposals are accepted in inputs and used when validating the
output; there are no feature flags to manage. `wasm-fuse` matches
`wasm-merge`'s capabilities but deliberately not its CLI; see
`wasm-fuse --help` for everything.

## Use as a library

```sh
cargo add wasm-fuse --no-default-features
```

(`--no-default-features` drops the CLI-only dependencies.)

```rust
use wasm_fuse::{MergeOptions, Merger};

let mut merger = Merger::new(MergeOptions {
    // keep only application's exports; `None` (the default) keeps every
    // module's exports, and `Some(vec![])` keeps none
    entry_modules: Some(vec!["application".to_string()]),
    ..MergeOptions::default()
});
merger.add_module("library", &std::fs::read("library.wasm")?)?;
merger.add_module("application", &std::fs::read("application.wasm")?)?;
let merged: Vec<u8> = merger.merge()?;
```

See the [API documentation](https://docs.rs/wasm-fuse) for details.

## Contributing

Contributions are welcome. To get started:

1. Install a recent stable Rust toolchain (MSRV is declared in `Cargo.toml`).
2. `cargo test --all-features` runs the full suite, including CLI integration
   tests and fixtures ported from binaryen's `wasm-merge` test suite
   (`tests/fixtures/binaryen/`, vendored under Apache-2.0).
3. Before sending a change, run the same checks CI runs:

   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --all-features
   cargo doc --no-deps --all-features
   ```

Dependencies are organised with [cargo battery packs](https://crates.io/crates/cargo-bp);
see `battery-pack.toml`.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Test fixtures under `tests/fixtures/binaryen/` are derived from the
[binaryen](https://github.com/WebAssembly/binaryen) project (Apache-2.0); see
[NOTICE](NOTICE).
