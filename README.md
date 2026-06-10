# wasm-bundle

[![CI](https://github.com/michaelficarra/wasm-bundle/actions/workflows/ci.yml/badge.svg)](https://github.com/michaelficarra/wasm-bundle/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/wasm-bundle.svg)](https://crates.io/crates/wasm-bundle)
[![docs.rs](https://docs.rs/wasm-bundle/badge.svg)](https://docs.rs/wasm-bundle)

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

> **Status**: under active development towards full `wasm-merge` parity; see
> [PLAN.md](PLAN.md) for the roadmap and current parity checklist.

## Installation as a CLI

```sh
cargo install wasm-bundle
```

Or from a checkout: `cargo install --path .`

### CLI usage

```sh
# Merge two modules. Module names default to file stems, so imports like
# (import "env" "foo" ...) in app.wasm resolve against env.wasm's exports.
wasm-bundle app.wasm env.wasm -o merged.wasm

# Name a module explicitly with NAME=PATH
wasm-bundle app.wasm libc=wasi-libc.wasm -o merged.wasm

# Inputs may be WebAssembly text; --text/-t emits text output
wasm-bundle first.wat second.wat --text -o merged.wat

# Keep only app's exports; lib.wasm just satisfies app's imports
wasm-bundle app.wasm lib.wasm --entry app -o merged.wasm

# When unioning exports, pick a conflict policy: error (default),
# rename (appends _1, _2, ...), or skip (first export wins)
wasm-bundle a.wasm b.wasm --export-conflicts rename -o out.wasm
```

All WebAssembly proposals are accepted in inputs and used when validating the
output; there are no feature flags to manage. `wasm-bundle` matches
`wasm-merge`'s capabilities but deliberately not its CLI; see
`wasm-bundle --help` for everything.

## Use as a library

```sh
cargo add wasm-bundle --no-default-features
```

(`--no-default-features` drops the CLI-only dependencies.)

```rust
use wasm_bundle::{ExportSelection, MergeOptions, Merger};

let mut merger = Merger::new(MergeOptions {
    // or ExportSelection::Union(..) to keep every module's exports
    exports: ExportSelection::Entry("application".to_string()),
    ..MergeOptions::default()
});
merger.add_module("library", &std::fs::read("library.wasm")?)?;
merger.add_module("application", &std::fs::read("application.wasm")?)?;
let merged: Vec<u8> = merger.merge()?;
```

See the [API documentation](https://docs.rs/wasm-bundle) for details.

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

[PLAN.md](PLAN.md) describes the roadmap and where help is most useful.
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
