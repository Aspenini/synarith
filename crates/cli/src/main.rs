//! # `syar` — the Synarith command-line application
//!
//! Validate, inspect, format, render, or animate `.syar` documents.
//! See the [CLI manual](synarith::cli) for commands and examples, or run
//! `syar --help` and a subcommand's `--help` for the complete option reference.
//!
//! Diagnostics and progress go to stderr; structured inspection and formatted
//! source go to stdout. Failed validation or rendering produces a nonzero exit
//! status. The default build enables GPU support; `--no-default-features` builds
//! a CPU-only executable with the same command interface.
use clap::{Args, Parser, Subcommand};
use std::{path::PathBuf, process::ExitCode};
use synarith::*;

#[derive(Parser)]
#[command(
    name = "syar",
    version,
    about = "Continuous procedural images, authored as text"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Parse and type-check a document.
    Validate { file: PathBuf },
    /// Normalize text without discarding comments.
    Fmt {
        file: PathBuf,
        #[arg(long)]
        check: bool,
        /// Embed the required standard-library definitions as standalone source.
        #[arg(long)]
        expand_std: bool,
        #[arg(long)]
        write: bool,
    },
    /// Inspect parameters, outputs, or the operation catalog as JSON.
    Inspect {
        file: Option<PathBuf>,
        #[arg(long)]
        operations: bool,
    },
    /// Render a still as PNG, JPEG, BMP, TGA, or PPM.
    Render {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// Override the output format (otherwise inferred from the filename).
        #[arg(long)]
        format: Option<ImageFormat>,
        /// JPEG quality, from 1 through 100.
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
        quality: u8,
        #[command(flatten)]
        render: RenderArgs,
    },
    /// Export numbered PNG frames. Time is start + frame/fps; no duplicate endpoint.
    Animate {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value_t = 120)]
        frames: u32,
        #[arg(long, default_value_t = 30.)]
        fps: f32,
        #[arg(long, default_value_t = 0.)]
        start: f32,
        #[command(flatten)]
        render: RenderArgs,
    },
}
#[derive(Args)]
struct RenderArgs {
    #[arg(long, default_value = "auto")]
    backend: Backend,
    #[arg(long, default_value_t = 1024)]
    width: u32,
    #[arg(long, default_value_t = 1024)]
    height: u32,
    #[arg(long, default_value_t = 0.)]
    time: f32,
    #[arg(long)]
    seed: Option<u32>,
    #[arg(long, default_value_t = 1)]
    samples: u32,
    #[arg(long, default_value = "main")]
    output_name: String,
    #[arg(long = "param", value_name = "NAME=VALUE")]
    parameters: Vec<String>,
    #[arg(long)]
    force: bool,
}
fn read(file: &PathBuf) -> Result<String, String> {
    std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))
}
fn options(program: &Program, args: &RenderArgs) -> Result<RenderOptions, String> {
    Ok(RenderOptions {
        width: args.width,
        height: args.height,
        time: args.time,
        seed: args.seed,
        samples: args.samples,
        output: args.output_name.clone(),
        parameters: apply_overrides(program, &args.parameters)?,
        ..Default::default()
    })
}
fn run() -> Result<(), String> {
    match Cli::parse().command {
        Command::Validate { file } => {
            let program = load(&read(&file)?).map_err(|e| e.to_string())?;
            println!(
                "valid: {} nodes, {} parameters, {} outputs, {} functions",
                program.nodes.len(),
                program.parameters.len(),
                program.outputs.len(),
                program.functions.len()
            );
        }
        Command::Fmt {
            file,
            check,
            write,
            expand_std,
        } => {
            if check && write {
                return Err("--check and --write are mutually exclusive".into());
            }
            let source = read(&file)?;
            let expanded;
            let input = if expand_std {
                expanded = expand_standard_library(&parse(&source).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                &expanded
            } else {
                &source
            };
            let formatted = format_source(input).map_err(|e| e.to_string())?;
            if check {
                if formatted != source {
                    return Err("document needs formatting".into());
                }
            } else if write {
                std::fs::write(file, formatted).map_err(|e| e.to_string())?;
            } else {
                print!("{formatted}");
            }
        }
        Command::Inspect {
            file,
            operations: catalog,
        } => {
            let value = if catalog {
                serde_json::json!(operations().iter().map(|op|serde_json::json!({"name":op.name,"category":op.category,"description":op.description,"inputs":op.inputs.iter().map(|a|serde_json::json!({"name":a.name,"type":format!("{:?}",a.ty),"default":a.default.map(|d|d.data)})).collect::<Vec<_>>(),"result":format!("{:?}",op.result),"cpu":true,"gpu":cfg!(feature="gpu")})).collect::<Vec<_>>())
            } else {
                let file = file.ok_or("provide a file or --operations")?;
                let program = load(&read(&file)?).map_err(|e| e.to_string())?;
                serde_json::json!({"version":1,"viewbox":program.viewbox,"seed":program.seed,"nodes":program.nodes.len(),"estimated_cost":program.estimated_cost,"outputs":program.outputs.keys().collect::<Vec<_>>(),"parameters":program.parameters.iter().map(|p|serde_json::json!({"name":p.name,"type":p.ty,"default":p.default})).collect::<Vec<_>>(),"functions":program.functions.iter().map(|f|serde_json::json!({"name":f.name,"parameters":f.parameters.iter().map(|(name,ty)|serde_json::json!({"name":name,"type":ty})).collect::<Vec<_>>(),"result":f.result,"locals":f.locals.len()})).collect::<Vec<_>>()})
            };
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
        }
        Command::Render {
            file,
            output,
            format,
            quality,
            render: args,
        } => {
            let program = load(&read(&file)?).map_err(|e| e.to_string())?;
            let options = options(&program, &args)?;
            let format = format
                .map(Ok)
                .unwrap_or_else(|| ImageFormat::from_path(&output))?;
            let mut renderer = Renderer::with_backend(program, args.backend)?;
            let result = renderer.export_image(&options, &output, format, quality, args.force);
            for warning in renderer.take_warnings() {
                eprintln!("warning: {warning}");
            }
            eprintln!("{}", renderer.backend_description());
            result?;
            eprintln!("wrote {}", output.display());
        }
        Command::Animate {
            file,
            output,
            frames,
            fps,
            start,
            render: args,
        } => {
            if frames == 0 || frames > 1_000_000 {
                return Err("frames must be between 1 and 1000000".into());
            }
            animation_time(start, frames - 1, fps)?;
            let program = load(&read(&file)?).map_err(|e| e.to_string())?;
            let mut options = options(&program, &args)?;
            let mut renderer = Renderer::with_backend(program, args.backend)?;
            for warning in renderer.take_warnings() {
                eprintln!("warning: {warning}");
            }
            eprintln!("{}", renderer.backend_description());
            // Preflight the entire sequence so an existing frame never produces a partial overwrite.
            if !args.force {
                for frame in 0..frames {
                    let path = output.join(format!("frame_{frame:06}.png"));
                    if path.exists() {
                        return Err(format!("{} already exists", path.display()));
                    }
                }
            }
            std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
            for frame in 0..frames {
                options.time = animation_time(start, frame, fps)?;
                let path = output.join(format!("frame_{frame:06}.png"));
                renderer.export_png(&options, &path, args.force)?;
                for warning in renderer.take_warnings() {
                    eprintln!("warning: {warning}");
                }
                eprintln!("{}/{} {}", frame + 1, frames, path.display());
            }
        }
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
