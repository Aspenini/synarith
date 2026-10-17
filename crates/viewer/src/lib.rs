//! # Synarith's standalone viewer process bridge
//!
//! This crate does not link the language, renderers, or image exporters. It builds
//! argument vectors for a sibling `syar` executable, manages that child process,
//! and decodes only its small PNG previews. Controls are native Win32 widgets on
//! Windows, AppKit on macOS, and Qt6 Widgets on Linux. No browser runtime or
//! graphics rendering framework is embedded in the viewer.
//!
//! Run `syar-view [document.syar] [--cli PATH]`. File dialogs, text editing,
//! clipboard support, focus navigation, and buttons use the platform widget
//! toolkit directly. Rendering and encoding always run through the CLI.
//!
//! Linux builds require a C++17 compiler, `pkg-config`, and system Qt6 Widgets
//! development libraries (`qt6-base-dev` on Debian/Ubuntu). Qt6 is linked
//! dynamically; static Qt libraries are rejected and Qt is never bundled.
//! Windows uses the Windows SDK/MSVC build tools, and macOS uses the Xcode
//! command-line tools. Headless CLI/library builds do not build these adapters.
//! The private widget bridge is compiled into the executable, not exposed as a
//! public C library or installed header.
//!
//! Preview dimensions are proportionally capped at 768 pixels; export uses the
//! requested dimensions. PNG/BMP/TGA retain alpha; JPEG/PPM flatten onto white.
//! The UI remains responsive while the child runs; cancel/close terminates and
//! reaps it. Explicit rendering avoids launching work on every keystroke.
#![deny(missing_docs)]
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// Validated settings passed to the CLI as separate arguments, never shell text.
#[derive(Clone, Debug)]
pub struct RenderSettings {
    /// Full export width in pixels.
    pub width: u32,
    /// Full export height in pixels.
    pub height: u32,
    /// Finite frame time in seconds.
    pub time: f32,
    /// Supersampling per axis, 1–8.
    pub samples: u32,
    /// `auto`, `cpu`, or `gpu`.
    pub backend: String,
    /// Named output.
    pub output: String,
    /// Optional exact seed override.
    pub seed: Option<u32>,
    /// Complete `NAME=VALUE` override strings, each passed as its own argument.
    pub parameters: Vec<String>,
    /// JPEG quality, 1–100.
    pub quality: u8,
}
impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            time: 0.,
            samples: 1,
            backend: "auto".into(),
            output: "main".into(),
            seed: None,
            parameters: vec![],
            quality: 90,
        }
    }
}
impl RenderSettings {
    /// Generate safe `syar render` arguments, validating settings before process creation.
    ///
    /// # Errors
    /// Rejects empty paths/output names, invalid dimensions, samples, time,
    /// quality, backend, export format, or malformed parameter assignments.
    pub fn arguments(
        &self,
        source: &Path,
        destination: &Path,
        format: &str,
        overwrite: bool,
    ) -> Result<Vec<OsString>, String> {
        if source.as_os_str().is_empty() || destination.as_os_str().is_empty() {
            return Err("Choose a document and destination.".into());
        }
        if !(1..=1_048_576).contains(&self.width) || !(1..=1_048_576).contains(&self.height) {
            return Err("Dimensions must be 1–1048576.".into());
        }
        if !(1..=8).contains(&self.samples)
            || !self.time.is_finite()
            || !(1..=100).contains(&self.quality)
        {
            return Err("Check time, samples (1–8), and quality (1–100).".into());
        }
        if !["auto", "cpu", "gpu"].contains(&self.backend.as_str())
            || !["png", "jpeg", "bmp", "tga", "ppm"].contains(&format)
        {
            return Err("Unsupported backend or format.".into());
        }
        if self.output.trim().is_empty() {
            return Err("An output name is required.".into());
        }
        let mut args = vec![
            "render".into(),
            source.as_os_str().to_owned(),
            "-o".into(),
            destination.as_os_str().to_owned(),
        ];
        for (key, value) in [
            ("--width", self.width.to_string()),
            ("--height", self.height.to_string()),
            ("--time", self.time.to_string()),
            ("--samples", self.samples.to_string()),
            ("--backend", self.backend.clone()),
            ("--output-name", self.output.clone()),
            ("--format", format.into()),
            ("--quality", self.quality.to_string()),
        ] {
            args.push(key.into());
            args.push(value.into());
        }
        if let Some(seed) = self.seed {
            args.extend(["--seed".into(), seed.to_string().into()]);
        }
        for parameter in &self.parameters {
            if !parameter
                .split_once('=')
                .is_some_and(|(name, value)| !name.trim().is_empty() && !value.trim().is_empty())
            {
                return Err("Overrides must use NAME=VALUE, separated by semicolons.".into());
            }
            args.push(format!("--param={parameter}").into());
        }
        if overwrite {
            args.push("--force".into());
        }
        Ok(args)
    }
    /// Clone settings with dimensions capped proportionally at 768 pixels per axis.
    pub fn preview(&self) -> Self {
        let mut result = self.clone();
        let largest = self.width.max(self.height).max(1);
        if largest > 768 {
            result.width = ((self.width as u64 * 768) / largest as u64).max(1) as u32;
            result.height = ((self.height as u64 * 768) / largest as u64).max(1) as u32;
        }
        result
    }
}
/// Locate an explicit CLI, a sibling executable, or an executable on `PATH`.
pub fn find_cli(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(format!("CLI not found: {}", path.display()));
        }
        return path.canonicalize().map_err(|e| e.to_string());
    }
    let name = if cfg!(windows) { "syar.exe" } else { "syar" };
    if let Ok(path) = std::env::current_exe() {
        let sibling = path.with_file_name(name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err("Place syar beside syar-view, put it on PATH, or use --cli PATH.".into())
}
/// Hide helper console windows on Windows; has no effect on other platforms.
pub fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}
/// Child completion status and CLI diagnostic text.
pub struct JobResult {
    /// Whether the CLI exited successfully.
    pub success: bool,
    /// Captured stderr, bounded to 1 MiB.
    pub message: String,
}
/// Running render process; dropping it terminates and reaps unfinished work.
pub struct RenderJob {
    child: Option<Child>,
    log: tempfile::NamedTempFile,
    _staging: tempfile::TempDir,
}
impl RenderJob {
    /// Start a render without a shell, redirecting diagnostics to a private file.
    pub fn start(cli: &Path, args: &[OsString]) -> Result<Self, String> {
        let destination = args
            .windows(2)
            .find(|pair| pair[0] == "-o")
            .map(|pair| Path::new(&pair[1]))
            .ok_or("render command needs -o DESTINATION")?;
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let staging = tempfile::Builder::new()
            .prefix(".syar-export-")
            .tempdir_in(parent)
            .map_err(|e| e.to_string())?;
        let log = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
        let mut command = Command::new(cli);
        hide_console(&mut command);
        let child = command
            .args(args)
            .arg("--staging-directory")
            .arg(staging.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log.reopen().map_err(|e| e.to_string())?))
            .spawn()
            .map_err(|e| format!("Could not launch {}: {e}", cli.display()))?;
        Ok(Self {
            child: Some(child),
            log,
            _staging: staging,
        })
    }
    /// Check completion without blocking the UI thread; drain the result only once.
    pub fn poll(&mut self) -> Result<Option<JobResult>, String> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        let Some(status) = child.try_wait().map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        self.child = None;
        let mut message = String::new();
        File::open(self.log.path())
            .map_err(|e| e.to_string())?
            .take(1_048_576)
            .read_to_string(&mut message)
            .map_err(|e| e.to_string())?;
        Ok(Some(JobResult {
            success: status.success(),
            message,
        }))
    }
    /// Terminate and reap the child, if it is still running.
    pub fn cancel(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Drop for RenderJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Small straight-RGBA PNG preview produced by the CLI.
pub struct Preview {
    /// Preview pixel width.
    pub width: usize,
    /// Preview pixel height.
    pub height: usize,
    /// Tightly packed RGBA8 bytes.
    pub rgba: Vec<u8>,
}
impl Preview {
    /// Decode a bounded RGBA8 CLI preview, rejecting unexpected formats or sizes.
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut decoder = png::Decoder::new(File::open(path).map_err(|e| e.to_string())?);
        decoder.set_limits(png::Limits {
            bytes: 16 * 1024 * 1024,
        });
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let info = reader.info();
        if info.width == 0
            || info.height == 0
            || info.width > 768
            || info.height > 768
            || info.color_type != png::ColorType::Rgba
            || info.bit_depth != png::BitDepth::Eight
        {
            return Err("Unexpected CLI preview (expected RGBA8, at most 768×768).".into());
        }
        let mut rgba = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut rgba).map_err(|e| e.to_string())?;
        rgba.truncate(info.buffer_size());
        Ok(Self {
            width: info.width as usize,
            height: info.height as usize,
            rgba,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_keep_paths_and_values_as_literal_arguments() {
        let settings = RenderSettings {
            parameters: vec![
                "center=vec2(-0.75,0.1)".into(),
                "name=$(not-a-shell)".into(),
            ],
            ..Default::default()
        };
        let args = settings
            .arguments(
                Path::new("images/雲 & clouds.syar"),
                Path::new("export image.png"),
                "png",
                false,
            )
            .unwrap();
        assert_eq!(args[1], OsString::from("images/雲 & clouds.syar"));
        assert!(args.contains(&OsString::from("--param=name=$(not-a-shell)")));
        assert!(!args.contains(&OsString::from("--force")));
    }
    #[test]
    fn preview_is_bounded_and_export_settings_are_unchanged() {
        let settings = RenderSettings {
            width: 1400,
            height: 1000,
            ..Default::default()
        };
        let preview = settings.preview();
        assert_eq!((preview.width, preview.height), (768, 548));
        assert_eq!(settings.width, 1400);
        let invalid = RenderSettings {
            width: 0,
            ..Default::default()
        };
        assert!(
            invalid
                .arguments(Path::new("x.syar"), Path::new("x.png"), "png", false)
                .is_err()
        );
    }
}
