//! Native platform widgets with a shared Rust controller and CLI-only rendering.
//! Windows uses Win32, macOS AppKit, and Linux dynamically linked system Qt6.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod native;
use native::{Field, Ui};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use synarith_viewer::{Preview, RenderJob, RenderSettings, find_cli};

struct Job {
    process: RenderJob,
    preview: bool,
}
struct Smoke {
    source: String,
    export: Option<PathBuf>,
    snapshot: Option<PathBuf>,
    phase: u8,
    finish_ticks: u8,
    started: Instant,
}
struct App {
    cli: Result<PathBuf, String>,
    directory: tempfile::TempDir,
    job: Option<Job>,
    preview: Option<Preview>,
    smoke: Option<Smoke>,
    failure: Option<String>,
    finished: bool,
}
impl App {
    fn settings(&self) -> Result<RenderSettings, String> {
        let parse_u32 = |field, name| {
            Ui::get(field)
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("{name} must be an integer."))
        };
        let seed = Ui::get(Field::Seed);
        Ok(RenderSettings {
            width: parse_u32(Field::Width, "Width")?,
            height: parse_u32(Field::Height, "Height")?,
            time: Ui::get(Field::Time)
                .trim()
                .parse()
                .map_err(|_| "Time must be a number.")?,
            samples: parse_u32(Field::Samples, "Samples")?,
            backend: Ui::get(Field::Backend).to_ascii_lowercase(),
            output: Ui::get(Field::Output),
            seed: if seed.trim().is_empty() {
                None
            } else {
                Some(
                    seed.trim()
                        .parse()
                        .map_err(|_| "Seed must be a 32-bit unsigned integer.")?,
                )
            },
            parameters: Ui::get(Field::Parameters)
                .split(';')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
            quality: Ui::get(Field::Quality)
                .trim()
                .parse()
                .map_err(|_| "Quality must be 1–100.")?,
        })
    }
    fn start(&mut self, destination: &Path, preview: bool, overwrite: bool) -> Result<(), String> {
        let cli = self.cli.as_ref().map_err(Clone::clone)?;
        let settings = self.settings()?;
        let settings = if preview {
            settings.preview()
        } else {
            settings
        };
        let format = if preview {
            "png".into()
        } else {
            Ui::get(Field::Format).to_ascii_lowercase()
        };
        let args = settings.arguments(
            Path::new(&Ui::get(Field::Source)),
            destination,
            &format,
            overwrite,
        )?;
        self.job = None; // Drop kills and reaps the previous child before restart.
        let process = RenderJob::start(cli, &args)?;
        self.job = Some(Job { process, preview });
        Ui::status(
            if preview {
                "Rendering preview…"
            } else {
                "Exporting image…"
            },
            true,
        );
        Ok(())
    }
    fn start_preview(&mut self) -> Result<(), String> {
        self.start(&self.directory.path().join("preview.png"), true, true)
    }
    fn cancel(&mut self) {
        self.job = None;
        Ui::status("Cancelled.", false);
    }
    fn export(&mut self) -> Result<(), String> {
        // Validate before opening the native save panel.
        self.settings()?.arguments(
            Path::new(&Ui::get(Field::Source)),
            Path::new("image.png"),
            "png",
            false,
        )?;
        let format = Ui::get(Field::Format).to_ascii_lowercase();
        let extension = if format == "jpeg" { "jpg" } else { &format };
        let suggested = PathBuf::from(Ui::get(Field::Source)).with_extension(extension);
        if let Some(path) = Ui::choose(true, &suggested, extension) {
            let mut path = path;
            let selected = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if selected != extension && !(format == "jpeg" && selected == "jpeg") {
                path.set_extension(extension);
            }
            if path.exists() && !Ui::confirm(&format!("Replace {}?", path.display())) {
                return Ok(());
            }
            self.start(&path, false, path.exists())?;
        }
        Ok(())
    }
    fn error(&mut self, error: String) {
        Ui::status(&error, false);
        Ui::log(&error);
        if self.smoke.is_some() {
            self.failure = Some(error);
            Ui::quit();
        }
    }
    fn poll(&mut self) -> Result<(), String> {
        if let Some(job) = &mut self.job
            && let Some(result) = job.process.poll()?
        {
            let was_preview = job.preview;
            self.job = None;
            Ui::log(&result.message);
            if !result.success {
                if let Some(smoke) = &mut self.smoke
                    && smoke.phase == 1
                    && self.preview.is_some()
                {
                    // An intentionally invalid document must preserve the last image.
                    Ui::set(Field::Source, &smoke.source);
                    smoke.phase = 2;
                    Ui::status(
                        "Preview retained after expected invalid-document error.",
                        false,
                    );
                    return Ok(());
                }
                return Err(result.message);
            }
            if was_preview {
                let image = Preview::load(&self.directory.path().join("preview.png"))?;
                Ui::image(&image);
                Ui::status(
                    &format!(
                        "Preview {} × {} · export uses requested resolution",
                        image.width, image.height
                    ),
                    false,
                );
                self.preview = Some(image);
            } else {
                Ui::status("Export complete.", false);
            }
        }
        if let Some(smoke) = &self.smoke
            && smoke.started.elapsed() > Duration::from_secs(60)
        {
            return Err("Native viewer workflow check timed out.".into());
        }
        if self.job.is_some() || self.preview.is_none() {
            return Ok(());
        }
        let Some(smoke) = &mut self.smoke else {
            return Ok(());
        };
        match smoke.phase {
            0 => {
                smoke.phase = 1;
                Ui::set(
                    Field::Source,
                    &self.directory.path().join("missing.syar").to_string_lossy(),
                );
                self.start_preview()?;
            }
            2 => {
                smoke.phase = 3;
                if let Some(path) = smoke.export.take() {
                    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("png");
                    Ui::set(
                        Field::Format,
                        if extension == "jpg" {
                            "jpeg"
                        } else {
                            extension
                        },
                    );
                    self.start(&path, false, false)?;
                }
            }
            3 => {
                smoke.finish_ticks += 1;
                if smoke.finish_ticks >= 3 {
                    if let Some(path) = &smoke.snapshot {
                        Ui::snapshot(path)?;
                    }
                    self.finished = true;
                    Ui::quit();
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn action(&mut self, action: i32) -> Result<(), String> {
        match action {
            0 => {
                if let Err(error) = &self.cli {
                    return Err(error.clone());
                }
                if !Ui::get(Field::Source).is_empty() {
                    self.start_preview()?;
                    if self.smoke.is_some() {
                        self.cancel();
                        self.start_preview()?;
                    }
                }
            }
            1 => self.start_preview()?,
            2 => self.export()?,
            3 | 7 => self.cancel(),
            4 => self.poll()?,
            5 => {
                if self.job.as_ref().is_some_and(|j| j.preview) {
                    self.cancel();
                    Ui::status("Settings changed. Render a new preview.", false);
                }
            }
            6 => {
                if let Some(path) = Ui::choose(false, Path::new(&Ui::get(Field::Source)), "syar") {
                    Ui::set(Field::Source, &path.to_string_lossy());
                    self.start_preview()?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

struct Controller {
    handling: Cell<bool>,
    app: RefCell<App>,
}
unsafe extern "C" fn callback(context: *mut c_void, action: i32) {
    // SAFETY: Ui::run keeps this stack-owned Controller alive until the event loop exits.
    let controller = unsafe { &*context.cast::<Controller>() };
    // Native modal panels may dispatch nested timer/control events. Do not borrow
    // the controller again until the original action and its panel have returned.
    if controller.handling.replace(true) {
        return;
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut app = controller.app.borrow_mut();
        if let Err(error) = app.action(action) {
            app.error(error);
        }
    }));
    if outcome.is_err() {
        controller
            .app
            .borrow_mut()
            .error("Unexpected viewer failure. Render cancelled.".into());
        controller.app.borrow_mut().cancel();
    }
    controller.handling.set(false);
}

fn run() -> Result<(), String> {
    let mut source = None;
    let mut cli = None;
    let mut smoke = false;
    let mut snapshot = None;
    let mut export = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--cli") => cli = Some(PathBuf::from(args.next().ok_or("--cli needs a path")?)),
            Some("--smoke-test") => {
                smoke = true;
                source = Some(PathBuf::from(
                    args.next().ok_or("--smoke-test needs a document")?,
                ));
            }
            Some("--snapshot") => {
                snapshot = Some(PathBuf::from(args.next().ok_or("--snapshot needs a path")?))
            }
            Some("--export-test") => {
                export = Some(PathBuf::from(
                    args.next().ok_or("--export-test needs a path")?,
                ))
            }
            Some("--help") => {
                println!(
                    "syar-view [FILE.syar] [--cli PATH]\nNative widgets; all rendering runs through syar.\nDeveloper check: --smoke-test FILE --snapshot UI.png --export-test OUTPUT.png"
                );
                return Ok(());
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("Unknown option: {value}"));
            }
            _ if source.is_none() => source = Some(PathBuf::from(arg)),
            _ => return Err("Open one document at a time.".into()),
        }
    }
    let source = source
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let directory = tempfile::Builder::new()
        .prefix(".syar-view-")
        .tempdir_in(".")
        .or_else(|_| tempfile::Builder::new().prefix(".syar-view-").tempdir())
        .map_err(|e| e.to_string())?;
    let mut controller = Controller {
        handling: Cell::new(false),
        app: RefCell::new(App {
            cli: find_cli(cli.as_deref()),
            directory,
            job: None,
            preview: None,
            smoke: smoke.then(|| Smoke {
                source: source.clone(),
                export,
                snapshot,
                phase: 0,
                finish_ticks: 0,
                started: Instant::now(),
            }),
            failure: None,
            finished: false,
        }),
    };
    // SAFETY: callback is synchronous on this thread; context outlives the loop.
    let result = unsafe {
        Ui::run(
            callback,
            (&mut controller as *mut Controller).cast(),
            &source,
        )
    };
    let app = controller.app.into_inner();
    if let Some(error) = app.failure {
        return Err(error);
    }
    if result != 0 {
        return Err("Could not initialize native widgets.".into());
    }
    if smoke && !app.finished {
        return Err("Viewer closed before workflow check completed.".into());
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("syar-view: {error}");
        std::process::exit(1);
    }
}
