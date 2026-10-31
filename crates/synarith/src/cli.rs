//! # The `syar` command-line application
//!
//! Build with `cargo build -p synarith-cli --release`. The default build includes GPU support; add
//! `--no-default-features` for CPU-only builds. Run `syar --help` or a subcommand's `--help` for
//! the complete flag descriptions.
//!
//!
//! | Command | Behavior |
//! | --- | --- |
//! | `validate FILE` | Parse, type-check, validate all groups/functions and loop budgets, report graph size |
//! | `fmt FILE` | Print conservative formatting; `--write` edits, `--check` checks; `--expand-std` embeds source-library helpers |
//! | `inspect FILE` | Print parameter defaults, outputs, seed, view box, and graph details as JSON |
//! | `inspect --operations` | Print the native core operation registry as JSON |
//! | `render FILE -o FILE.png` | Render a still |
//! | `animate FILE -o DIRECTORY` | Render numbered PNG frames |
//! | `material FILE -o DIRECTORY` | Write every material channel as a PNG, plus `material.json` |
//!
//! `material` renders a document's `material` block at `--size` pixels square (default 1024) and
//! accepts `--backend`, `--time`, `--seed`, `--samples`, `--param`, and `--force`. Albedo and
//! emissive are sRGB; roughness, metallic, occlusion, and the normal (+Y up, OpenGL) are linear
//! 8-bit; height is 16-bit greyscale, and `material.json` records its range in document units.
//! Data outputs rendered with `render` (numbers, vectors) are likewise stored linear.
//!
//! Render/animate flags: `--backend auto|cpu|gpu`, `--width`, `--height`, `--time`, `--seed`,
//! `--samples`, `--output-name`, repeatable `--param NAME=VALUE`, and `--force`. Animate
//! additionally accepts `--frames`, `--fps`, and `--start`. Its frame time is controlled by
//! start/fps rather than `--time`.
//!
//! Still exports infer PNG, JPEG, BMP, TGA, or PPM from the filename extension.
//! `render --format FORMAT` overrides inference; `--quality 1..100` controls JPEG
//! quality (default 90). PNG streams strips. Other formats allocate a full image
//! within the 1 GiB float-pixel limit; JPEG/TGA dimensions are at most 65535 per
//! axis. PNG/BMP/TGA retain alpha; JPEG/PPM composite over white in linear light.
//! Animation continues to write PNG sequences.
//!
//! `syar-view [FILE.syar] [--paused] [--sphere]` launches the egui live viewer.
//! Material documents open as a lit preview, on a surface or (`--sphere`) a ball,
//! with an adjustable or orbiting light and a tile-span slider. It renders
//! directly through the Rust library, watches source changes, plays time expressions
//! continuously, and supports smooth mathematical zoom/pan and still exports.
//! Scroll/pinch to zoom, drag to pan, Space to play/pause, R to restart, 0 to fit.
//! Deep views automatically use CPU f64 arithmetic in Auto mode; GPU mode stays f32.
//! Compiled constants and parameter values retain their original f32 representation.
//!
//! Examples:
//!
//! ```sh
//! syar render examples/clouds.syar -o clouds.png --param scale=12.0 --param 'shadow=#061c39' --time 2.5 --backend gpu
//! syar animate examples/orbit.syar -o orbit --start 0 --frames 120 --fps 30
//! syar material examples/brick-material.syar -o brick --size 2048
//! ```
//!
//! Diagnostics and progress go to stderr; inspection/formatting data go to stdout. Invalid input or
//! failed rendering returns a nonzero exit status. Animation preflights existing filenames.
//! Completed frames remain if a later frame fails; an individual PNG is persisted only on
//! successful completion.
