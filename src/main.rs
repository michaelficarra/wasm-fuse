use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anstream::eprintln;
use anstyle::AnsiColor;
use clap::{Parser, ValueEnum};
use wasm_fuse::{ExportConflictPolicy, ExportSelection, MergeOptions, Merger};
use wasmparser::WasmFeatures;

/// Merge multiple WebAssembly modules into one.
///
/// Each input module has a name (defaulting to its file stem), and imports of
/// the form `(import "NAME" "item" ...)` referring to another input module are
/// resolved to that module's exports at merge time. Imports of modules outside
/// the input set are left as imports.
#[derive(Parser)]
#[command(version, about, name = "wasm-fuse")]
struct Cli {
    /// Input modules, as PATH or NAME=PATH
    ///
    /// The module name defaults to the file stem (`lib.wasm` is module
    /// `lib`); it is what other modules' imports refer to. Files may be
    /// binary (.wasm) or text (.wat).
    #[arg(value_name = "[NAME=]PATH", required = true)]
    modules: Vec<String>,

    /// Output file (defaults to stdout)
    #[arg(short, long, value_name = "PATH", default_value = "-")]
    output: PathBuf,

    /// Emit WebAssembly text instead of binary
    #[arg(short, long)]
    text: bool,

    /// Export only this module's exports
    ///
    /// The other modules are only used to satisfy (some of) its imports.
    /// Without this option, the merged module exports everything every input
    /// module exports.
    #[arg(long, value_name = "NAME")]
    entry: Option<String>,

    /// How to resolve export name conflicts between modules
    #[arg(
        long,
        value_enum,
        value_name = "POLICY",
        default_value_t = ConflictPolicy::Error,
        conflicts_with = "entry"
    )]
    export_conflicts: ConflictPolicy,

    /// Remove anything unreachable from the kept exports and start functions
    ///
    /// Tree-shakes the merged module: functions, globals, tables, memories,
    /// tags, and segments that the kept exports and start functions never
    /// reach are dropped. Combine with --entry to bundle an application
    /// with only the library code it actually uses.
    #[arg(long)]
    prune: bool,

    /// Skip output validation and import/export compatibility checking
    #[arg(long)]
    no_validate: bool,

    #[command(flatten)]
    color: colorchoice_clap::Color,

    /// Enable verbose logging (or set RUST_LOG)
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ConflictPolicy {
    /// Fail the merge when two modules export the same name
    Error,
    /// Keep the first export and rename later ones (name_1, name_2, ...)
    Rename,
    /// Keep the first export and drop later conflicting ones
    Skip,
}

fn main() -> ExitCode {
    human_panic::setup_panic!();

    let cli = Cli::parse_from(wild::args_os());

    cli.color.write_global();

    if cli.verbose {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "debug".into()),
            )
            .init();
    }

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let style = AnsiColor::Red.on_default().bold();
            eprintln!("{style}error{style:#}: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Split a `NAME=PATH` argument, deriving the name from the file stem when no
/// explicit name is given.
fn parse_module_argument(argument: &str) -> anyhow::Result<(String, PathBuf)> {
    if let Some((name, path)) = argument.split_once('=') {
        anyhow::ensure!(
            !name.is_empty(),
            "empty module name in {argument:?}; use NAME=PATH"
        );
        return Ok((name.to_string(), PathBuf::from(path)));
    }
    let path = PathBuf::from(argument);
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        anyhow::bail!("cannot derive a module name from {argument:?}; use NAME=PATH");
    };
    Ok((stem.to_string(), path))
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    let options = MergeOptions {
        exports: match &cli.entry {
            Some(entry) => ExportSelection::Entry(entry.clone()),
            None => ExportSelection::Union(match cli.export_conflicts {
                ConflictPolicy::Error => ExportConflictPolicy::Error,
                ConflictPolicy::Rename => ExportConflictPolicy::Rename,
                ConflictPolicy::Skip => ExportConflictPolicy::Skip,
            }),
        },
        // The CLI deliberately has no wasm-proposal toggles: inputs are
        // accepted and the output validated with every proposal enabled.
        features: WasmFeatures::all(),
        validate: !cli.no_validate,
        prune_unused: cli.prune,
    };

    let mut merger = Merger::new(options);
    for argument in &cli.modules {
        let (name, path) = parse_module_argument(argument)?;
        let bytes = std::fs::read(&path)
            .map_err(|error| anyhow::anyhow!("failed to read {}: {error}", path.display()))?;
        merger.add_module(name, &bytes)?;
    }

    let merged = merger.merge()?;

    let output: Vec<u8> = if cli.text {
        wasmprinter::print_bytes(&merged)?.into_bytes()
    } else {
        merged
    };

    if cli.output.as_os_str() == "-" {
        std::io::stdout().write_all(&output)?;
    } else {
        std::fs::write(&cli.output, &output).map_err(|error| {
            anyhow::anyhow!("failed to write {}: {error}", cli.output.display())
        })?;
    }
    Ok(())
}

#[test]
fn verify_cli() {
    use clap::CommandFactory;
    Cli::command().debug_assert();
}

#[test]
fn module_arguments_parse() {
    let (name, path) = parse_module_argument("app=target/app.wasm").unwrap();
    assert_eq!(
        (name.as_str(), path.to_str().unwrap()),
        ("app", "target/app.wasm")
    );

    let (name, path) = parse_module_argument("target/lib.wasm").unwrap();
    assert_eq!(
        (name.as_str(), path.to_str().unwrap()),
        ("lib", "target/lib.wasm")
    );

    assert!(parse_module_argument("=nameless.wasm").is_err());
}
