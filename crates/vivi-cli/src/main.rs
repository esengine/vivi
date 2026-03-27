use clap::{Parser, Subcommand};
use miette::{IntoDiagnostic, WrapErr};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "vivi", about = "Vivi ECS language compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile a .vivi file to WASM
    Build {
        /// Input .vivi file
        input: PathBuf,

        /// Output .wasm file
        #[arg(short, long, default_value = "output.wasm")]
        output: PathBuf,
    },
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { input, output } => {
            let source = std::fs::read_to_string(&input)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to read `{}`", input.display()))?;

            let program = vivi_parser::parse(&source)
                .map_err(|e| miette::miette!("{e}"))?;

            let resolved = vivi_sema::resolve(&program, &source)
                .map_err(|e| miette::Report::new(e))?;

            let wasm_bytes = vivi_codegen::generate_wasm(&program, &resolved);

            std::fs::write(&output, &wasm_bytes)
                .into_diagnostic()
                .wrap_err_with(|| format!("failed to write `{}`", output.display()))?;

            println!(
                "Compiled {} -> {} ({} bytes, {} pages)",
                input.display(),
                output.display(),
                wasm_bytes.len(),
                resolved.layout.required_pages()
            );
            Ok(())
        }
    }
}
