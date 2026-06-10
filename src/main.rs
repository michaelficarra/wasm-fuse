use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anstream::eprintln;
use anstyle::AnsiColor;
use clap::Parser;
use wasm_bundle::{ExportConflictPolicy, MergeOptions, Merger};
use wasmparser::WasmFeatures;

/// Merge multiple WebAssembly modules into one.
///
/// Each input module is given a name, and imports of the form
/// `(import "NAME" "item" ...)` referring to another input module are resolved
/// to that module's exports at merge time. Imports of modules outside the
/// input set are left as imports.
#[derive(Parser)]
#[command(version, about, name = "wasm-bundle")]
struct Cli {
    /// Alternating module file and module name pairs: INFILE1 NAME1 INFILE2 NAME2 ...
    ///
    /// Files may be binary (.wasm) or text (.wat).
    #[arg(value_name = "INFILE NAME", required = true)]
    inputs: Vec<String>,

    /// Output file (use `-` for stdout)
    #[arg(short, long, default_value = "-", value_name = "FILE")]
    output: PathBuf,

    /// Emit WebAssembly text instead of binary
    #[arg(short = 'S', long)]
    emit_text: bool,

    /// Rename exports to resolve conflicts between modules (appends _1, _2, ...)
    #[arg(long, conflicts_with = "skip_export_conflicts")]
    rename_export_conflicts: bool,

    /// Keep the first conflicting export and skip later ones
    #[arg(long)]
    skip_export_conflicts: bool,

    /// Skip validation of the merged output
    #[arg(short = 'n', long)]
    no_validation: bool,

    /// Enable all WebAssembly proposals
    #[arg(long, conflicts_with = "mvp_features")]
    all_features: bool,

    /// Disable all WebAssembly proposals beyond the original MVP
    #[arg(long)]
    mvp_features: bool,

    #[command(flatten)]
    color: colorchoice_clap::Color,

    /// Enable verbose logging (or set RUST_LOG)
    #[arg(short, long)]
    verbose: bool,
}

/// binaryen's tools accept single-dash long options; translate the wasm-merge
/// spellings into their clap equivalents so existing invocations keep working.
fn translate_binaryen_flags(args: impl Iterator<Item = OsString>) -> Vec<OsString> {
    args.map(|arg| match arg.to_str() {
        Some("-rec") => OsString::from("--rename-export-conflicts"),
        Some("-sec") => OsString::from("--skip-export-conflicts"),
        Some("-all") => OsString::from("--all-features"),
        Some("-mvp") => OsString::from("--mvp-features"),
        _ => arg,
    })
    .collect()
}

fn main() -> ExitCode {
    human_panic::setup_panic!();

    let cli = Cli::parse_from(translate_binaryen_flags(wild::args_os()));

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

fn run(cli: &Cli) -> anyhow::Result<()> {
    if cli.inputs.len() % 2 != 0 {
        anyhow::bail!(
            "inputs must be given as alternating INFILE NAME pairs, \
             but {} arguments were provided",
            cli.inputs.len()
        );
    }

    let features = if cli.all_features {
        WasmFeatures::all()
    } else if cli.mvp_features {
        WasmFeatures::WASM1
    } else {
        WasmFeatures::default()
    };

    let options = MergeOptions {
        export_conflicts: if cli.rename_export_conflicts {
            ExportConflictPolicy::Rename
        } else if cli.skip_export_conflicts {
            ExportConflictPolicy::Skip
        } else {
            ExportConflictPolicy::Error
        },
        features,
        validate: !cli.no_validation,
    };

    let mut merger = Merger::new(options);
    for pair in cli.inputs.chunks(2) {
        let [file, name] = pair else { unreachable!() };
        let bytes = std::fs::read(file)
            .map_err(|error| anyhow::anyhow!("failed to read {file}: {error}"))?;
        merger.add_module(name.as_str(), &bytes)?;
    }

    let merged = merger.merge()?;

    let output: Vec<u8> = if cli.emit_text {
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
