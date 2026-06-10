# wasm-bundle

[![CI](https://github.com/OWNER/wasm-bundle/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/wasm-bundle/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/wasm-bundle.svg)](https://crates.io/crates/wasm-bundle)
[![docs.rs](https://docs.rs/wasm-bundle/badge.svg)](https://docs.rs/wasm-bundle)

Merge multiple WebAssembly core modules into a single module — a Rust counterpart to
[binaryen's `wasm-merge`](https://github.com/WebAssembly/binaryen), built on the
Bytecode Alliance [wasm-tools](https://github.com/bytecodealliance/wasm-tools) crates
and usable both as a library and as a CLI.

Each input module is given a name. Imports in any module of the form
`(import "NAME" "item" …)`, where `NAME` is the name of another input module, are
resolved at merge time to the item that module exports as `item` — the way a
JavaScript bundler replaces runtime module loading with build-time linking. Imports
referring to modules outside the input set are left as imports, and circular
references between input modules are supported.

> **Status**: under active development towards full `wasm-merge` parity; see
> [PLAN.md](PLAN.md) for the roadmap and current parity checklist.

## Installation as a CLI

```sh
cargo install wasm-bundle
```

Or from a checkout: `cargo install --path .`

### CLI usage

```sh
# Merge two modules. Imports like (import "env" "foo" ...) in app.wasm are
# resolved against the exports of env.wasm.
wasm-bundle app.wasm app env.wasm env -o merged.wasm

# Inputs may be WebAssembly text; output as text with -S
wasm-bundle first.wat first second.wat second -S -o merged.wat

# Resolve export-name conflicts by renaming (appends _1, _2, ...)
wasm-bundle a.wasm a b.wasm b --rename-export-conflicts -o out.wasm

# ... or by keeping the first export and skipping later ones
wasm-bundle a.wasm a b.wasm b --skip-export-conflicts -o out.wasm
```

Flag names mirror `wasm-merge`, and binaryen's single-dash spellings (`-rec`,
`-sec`, `-all`, `-mvp`) are accepted, so existing `wasm-merge` invocations
translate directly. See `wasm-bundle --help` for everything.

## Use as a library

```sh
cargo add wasm-bundle --no-default-features
```

(`--no-default-features` drops the CLI-only dependencies.)

```rust
use wasm_bundle::{MergeOptions, Merger};

let mut merger = Merger::new(MergeOptions::default());
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
