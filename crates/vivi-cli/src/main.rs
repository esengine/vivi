use clap::{Parser, Subcommand};
use miette::{IntoDiagnostic, WrapErr};
use std::path::PathBuf;
use vivi_interp::Interpreter;

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

    /// Run a .vivi file with the interpreter
    Run {
        /// Input .vivi file
        input: PathBuf,

        /// Number of ticks to execute
        #[arg(short, long, default_value = "1")]
        ticks: u32,

        /// Dump entity state after each tick
        #[arg(long)]
        dump_state: bool,
    },
}

fn parse_and_resolve(
    input: &PathBuf,
) -> miette::Result<(String, vivi_parser::ast::Program, vivi_sema::ResolvedProgram)> {
    let source = std::fs::read_to_string(input)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read `{}`", input.display()))?;

    let program =
        vivi_parser::parse(&source).map_err(|e| miette::miette!("{e}"))?;

    let resolved = vivi_sema::resolve(&program, &source)
        .map_err(|e| miette::Report::new(e))?;

    Ok((source, program, resolved))
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { input, output } => {
            let (_source, program, resolved) = parse_and_resolve(&input)?;

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

        Commands::Run {
            input,
            ticks,
            dump_state,
        } => {
            let (_source, program, resolved) = parse_and_resolve(&input)?;

            let mut interp = Interpreter::new(&program, &resolved);

            // Register built-in extern handlers
            interp.register_extern(
                "log_f32",
                Box::new(|args| {
                    if let Some(v) = args.first() {
                        println!("[log] {:.4}", v.as_f32());
                    }
                    None
                }),
            );
            interp.register_extern(
                "clear_screen",
                Box::new(|_| {
                    println!("--- clear ---");
                    None
                }),
            );
            interp.register_extern(
                "set_color",
                Box::new(|args| {
                    println!(
                        "[color] rgb({}, {}, {})",
                        args[0].as_i32(),
                        args[1].as_i32(),
                        args[2].as_i32()
                    );
                    None
                }),
            );
            interp.register_extern(
                "draw_rect",
                Box::new(|args| {
                    println!(
                        "[draw] rect({:.1}, {:.1}, {:.1}, {:.1})",
                        args[0].as_f32(),
                        args[1].as_f32(),
                        args[2].as_f32(),
                        args[3].as_f32()
                    );
                    None
                }),
            );

            interp.init();
            println!("Initialized {} entities", resolved.entities.len());

            if dump_state {
                println!("\n--- After init ---");
                print!("{}", interp.dump_state());
            }

            for tick_num in 0..ticks {
                interp.tick();
                if dump_state {
                    println!("--- After tick {} ---", tick_num + 1);
                    print!("{}", interp.dump_state());
                }
            }

            if !dump_state {
                println!("Ran {} tick(s)", ticks);
                print!("{}", interp.dump_state());
            }

            Ok(())
        }
    }
}
