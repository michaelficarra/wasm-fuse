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

The merged module's exports can be either the **union** of every input's exports
(what `wasm-merge` produces) or, with `--entry`, just the **entry-point module's**
exports — the other modules then only serve to satisfy (some of) its imports.

> **Status**: at full `wasm-merge` capability parity.

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

# Keep only app's exports; lib.wasm just satisfies app's imports
wasm-fuse app.wasm lib.wasm --entry app -o merged.wasm

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

# When unioning exports, pick a conflict policy: error (default),
# rename (appends _1, _2, ...), or skip (first export wins)
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
use wasm_fuse::{ExportSelection, MergeOptions, Merger};

let mut merger = Merger::new(MergeOptions {
    // or ExportSelection::Union(..) to keep every module's exports
    exports: ExportSelection::Entry("application".to_string()),
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
