use clap::{Parser, Subcommand};
use miette::{IntoDiagnostic, WrapErr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use vivi_interp::Interpreter;

static RNG_STATE: AtomicU32 = AtomicU32::new(12345);

fn rand_f32() -> f32 {
    let mut s = RNG_STATE.load(Ordering::Relaxed);
    s = s.wrapping_mul(1103515245).wrapping_add(12345);
    RNG_STATE.store(s, Ordering::Relaxed);
    ((s >> 16) & 0x7FFF) as f32 / 32767.0
}

#[derive(Parser)]
#[command(name = "vivi", about = "Vivi ECS language compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compile a .vivi file to WASM (or web bundle with --target web)
    Build {
        /// Input .vivi file
        input: PathBuf,

        /// Output path (.wasm file or directory for --target web)
        #[arg(short, long, default_value = "output.wasm")]
        output: PathBuf,

        /// Build target: "wasm" (default) or "web" (generates .wasm + runtime.js + index.html)
        #[arg(long, default_value = "wasm")]
        target: String,

        /// Maximum number of entities (determines memory allocation)
        #[arg(long, default_value = "1000000")]
        max_entities: u32,
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
    max_entities: u32,
) -> miette::Result<(String, vivi_parser::ast::Program, vivi_sema::ResolvedProgram, Vec<String>)> {
    let source = std::fs::read_to_string(input)
        .into_diagnostic()
        .wrap_err_with(|| format!("failed to read `{}`", input.display()))?;

    let base_dir = input.parent();
    let (program, used_modules) =
        vivi_parser::parse_file(&source, base_dir).map_err(|e| miette::miette!("{e}"))?;

    let resolved = vivi_sema::resolve_with_max(&program, &source, max_entities)
        .map_err(|e| miette::Report::new(e))?;

    Ok((source, program, resolved, used_modules))
}

fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { input, output, target, max_entities } => {
            let (source, program, resolved, used_modules) = parse_and_resolve(&input, max_entities)?;

            if target == "web" {
                // --target web: generate dist/ with .wasm + source map + runtime.js + index.html
                let (wasm_bytes, module_mappings) =
                    vivi_codegen::generate_wasm_with_sourcemap(&program, &resolved, &source);

                let out_dir = &output;
                std::fs::create_dir_all(out_dir)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("failed to create `{}`", out_dir.display()))?;

                let title = program.items.iter().find_map(|item| {
                    if let vivi_parser::ast::Item::World(w) = item { Some(w.name.clone()) } else { None }
                }).unwrap_or_else(|| {
                    input.file_stem().unwrap_or_default().to_string_lossy().to_string()
                });

                let source_filename = input.file_name().unwrap_or_default().to_string_lossy().to_string();

                // sourceMappingURL custom section is now embedded by codegen
                // Resolve mappings to absolute byte offsets and generate source map
                let resolved_mappings = vivi_codegen::resolve_mappings(&wasm_bytes, &module_mappings);
                let source_map_json = vivi_codegen::generate_source_map(
                    &source_filename,
                    &source,
                    &resolved_mappings,
                );

                let config = vivi_web::WebBuildConfig {
                    title,
                    wasm_filename: "app.wasm".to_string(),
                    ..Default::default()
                };

                let wasm_path = out_dir.join("app.wasm");
                let map_path = out_dir.join("app.wasm.map");
                let source_path = out_dir.join(&source_filename);
                let runtime_path = out_dir.join("runtime.js");
                let html_path = out_dir.join("index.html");

                std::fs::write(&wasm_path, &wasm_bytes).into_diagnostic()?;
                std::fs::write(&map_path, &source_map_json).into_diagnostic()?;
                std::fs::write(&source_path, &source).into_diagnostic()?;
                std::fs::write(&runtime_path, vivi_web::generate_runtime_js(&resolved, &config, &used_modules)).into_diagnostic()?;
                std::fs::write(&html_path, vivi_web::generate_index_html(&config)).into_diagnostic()?;

                println!("Built web target -> {}/", out_dir.display());
                println!("  app.wasm      ({} bytes)", wasm_bytes.len());
                println!("  app.wasm.map  (source map)");
                println!("  {}   (source)", source_filename);
                println!("  runtime.js    (auto-generated)");
                println!("  index.html    (auto-generated)");
                println!("\nServe with: python -m http.server 8000 -d {}", out_dir.display());
                println!("Open Chrome DevTools → Sources to debug {}", source_filename);
            } else {
                // Default: just output .wasm
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
            }
            Ok(())
        }

        Commands::Run {
            input,
            ticks,
            dump_state,
        } => {
            let (_source, program, resolved, _used_modules) = parse_and_resolve(&input, vivi_sema::layout::DEFAULT_MAX_ENTITIES)?;

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

            interp.register_extern(
                "random",
                Box::new(|_| {
                    Some(vivi_interp::Value::F32(rand_f32()))
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
