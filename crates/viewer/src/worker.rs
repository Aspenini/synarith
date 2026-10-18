use std::{
    collections::VecDeque,
    io::Read,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};
use synarith::{
    Backend, Cancellation, ImageFormat, ParameterInfo, RenderOptions, Renderer, load, rgba8,
};

/// A successfully compiled document, ready for playback.
#[derive(Clone, Debug)]
pub struct DocumentInfo {
    /// Source generation; changes after a valid reload.
    pub generation: u64,
    /// Open source file.
    pub path: PathBuf,
    /// Original text for the source panel.
    pub source: Arc<str>,
    /// Default composition rectangle.
    pub viewbox: [f32; 4],
    /// Named outputs.
    pub outputs: Vec<String>,
    /// Exposed parameters and typed defaults.
    pub parameters: Vec<ParameterInfo>,
    /// Whether this was a reload rather than opening a different document.
    pub reloaded: bool,
}
/// Frame settings, with identity for rejecting obsolete source/settings results.
#[derive(Clone, Debug)]
pub struct FrameRequest {
    /// Client settings revision.
    pub revision: u64,
    /// Expected source generation.
    pub generation: u64,
    /// Camera, dimensions, time, parameters, and quality.
    pub options: RenderOptions,
}
/// A rendered in-memory frame.
#[derive(Debug)]
pub struct LiveFrame {
    /// Settings used for this frame.
    pub request: FrameRequest,
    /// Tightly packed straight sRGB RGBA8.
    pub rgba: Vec<u8>,
    /// Render latency, including preparation when required.
    pub elapsed: Duration,
    /// Executing backend and adapter.
    pub backend: String,
}
/// Background events consumed without blocking the interface.
#[derive(Debug)]
pub enum WorkerEvent {
    /// New or reloaded valid source.
    Loaded(DocumentInfo),
    /// Completed preview.
    Frame(LiveFrame),
    /// Error; the last valid document and preview remain available.
    Error(String),
    /// Obsolete or cancelled request finished.
    Cancelled,
    /// Still export completed.
    Exported(PathBuf),
}
enum Control {
    Open(PathBuf),
    Inline(PathBuf, Arc<str>),
    Backend(Backend),
    Export(PathBuf, ImageFormat, u8, FrameRequest),
}
#[derive(Default)]
struct Mailbox {
    controls: VecDeque<Control>,
    frame: Option<FrameRequest>,
    stop: bool,
    active: Option<Cancellation>,
}
/// Persistent renderer with a single replaceable pending-frame slot.
///
/// Compilation, file watching, rendering, and export stay off the UI thread.
/// Animation requests follow completed frames, avoiding cancellation starvation.
pub struct LiveWorker {
    shared: Arc<(Mutex<Mailbox>, Condvar)>,
    events: mpsc::Receiver<WorkerEvent>,
}
impl Default for LiveWorker {
    fn default() -> Self {
        Self::new()
    }
}
impl LiveWorker {
    /// Start using automatic GPU selection with CPU fallback.
    pub fn new() -> Self {
        let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
        let (sender, events) = mpsc::channel();
        let background = shared.clone();
        thread::Builder::new()
            .name("synarith-render".into())
            .spawn(move || run(background, sender))
            .expect("start rendering worker");
        Self { shared, events }
    }
    fn control(&self, control: Control) {
        let mut q = self.shared.0.lock().unwrap();
        if let Some(active) = &q.active {
            active.cancel();
        }
        q.frame = None;
        q.controls.push_back(control);
        self.shared.1.notify_one();
    }
    /// Open and watch a source file; invalid edits retain the valid document.
    pub fn open(&self, path: PathBuf) {
        self.control(Control::Open(path));
    }
    /// Open a bundled or unsaved source without a filesystem watcher.
    pub fn open_source(&self, name: PathBuf, source: Arc<str>) {
        self.control(Control::Inline(name, source));
    }
    /// Rebuild backend resources asynchronously.
    pub fn set_backend(&self, backend: Backend) {
        self.control(Control::Backend(backend));
    }
    /// Replace pending frame work; optionally cancel the active render.
    pub fn render(&self, request: FrameRequest, cancel: bool) {
        let mut q = self.shared.0.lock().unwrap();
        if cancel && let Some(active) = &q.active {
            active.cancel();
        }
        q.frame = Some(request);
        self.shared.1.notify_one();
    }
    /// Cancel an active preview/export and clear pending preview work.
    pub fn cancel(&self) {
        let mut q = self.shared.0.lock().unwrap();
        q.frame = None;
        if let Some(active) = &q.active {
            active.cancel();
        }
    }
    /// Export with selected dimensions, format, time, and camera; protect existing files.
    pub fn export(&self, path: PathBuf, format: ImageFormat, quality: u8, request: FrameRequest) {
        self.control(Control::Export(path, format, quality, request));
    }
    /// Poll one event without blocking.
    pub fn poll(&self) -> Option<WorkerEvent> {
        self.events.try_recv().ok()
    }
}
impl Drop for LiveWorker {
    fn drop(&mut self) {
        let mut q = self.shared.0.lock().unwrap();
        q.stop = true;
        q.frame = None;
        if let Some(active) = &q.active {
            active.cancel();
        }
        self.shared.1.notify_one();
    }
}
struct DocumentState {
    path: PathBuf,
    source: Arc<str>,
    generation: u64,
    renderer: Renderer,
    notice: String,
}
fn open(
    path: PathBuf,
    source: Arc<str>,
    generation: u64,
    backend: Backend,
    reloaded: bool,
) -> Result<(DocumentState, DocumentInfo), String> {
    let program = load(&source).map_err(|e| e.to_string())?;
    let info = DocumentInfo {
        generation,
        path: path.clone(),
        source: source.clone(),
        viewbox: program.viewbox,
        outputs: program.outputs.keys().cloned().collect(),
        parameters: program.parameters.clone(),
        reloaded,
    };
    let renderer = Renderer::with_backend(program, backend)?;
    Ok((
        DocumentState {
            path,
            source,
            generation,
            renderer,
            notice: String::new(),
        },
        info,
    ))
}
fn read_source(path: &std::path::Path) -> Result<String, String> {
    let mut source = String::new();
    std::fs::File::open(path)
        .and_then(|file| {
            file.take(synarith::MAX_SOURCE_BYTES as u64 + 1)
                .read_to_string(&mut source)
        })
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if source.len() > synarith::MAX_SOURCE_BYTES {
        return Err("Source exceeds 4 MiB".into());
    }
    Ok(source)
}
fn run(shared: Arc<(Mutex<Mailbox>, Condvar)>, events: mpsc::Sender<WorkerEvent>) {
    let mut document: Option<DocumentState> = None;
    let mut watched: Option<PathBuf> = None;
    let mut observed: Option<String> = None;
    let mut generation = 0;
    let mut backend = Backend::Auto;
    let mut checked = Instant::now();
    loop {
        let control;
        let frame;
        {
            let mut q = shared.0.lock().unwrap();
            if q.controls.is_empty() && q.frame.is_none() && !q.stop {
                q = shared
                    .1
                    .wait_timeout(q, Duration::from_millis(100))
                    .unwrap()
                    .0;
            }
            if q.stop {
                break;
            }
            control = q.controls.pop_front();
            frame = if control.is_none() {
                q.frame.take()
            } else {
                None
            };
            if let Some(request) = &frame {
                q.active = Some(request.options.cancellation.clone());
            }
        }
        if let Some(control) = control {
            let result: Result<(), String> = (|| {
                match control {
                    Control::Inline(path, source) => {
                        watched = None;
                        observed = None;
                        generation += 1;
                        let (state, info) = open(path, source, generation, backend, false)?;
                        document = Some(state);
                        let _ = events.send(WorkerEvent::Loaded(info));
                    }
                    Control::Open(path) => {
                        watched = Some(path.clone());
                        let source =
                            read_source(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                        observed = Some(source.clone());
                        generation += 1;
                        let (state, info) =
                            open(path, Arc::from(source), generation, backend, false)?;
                        document = Some(state);
                        let _ = events.send(WorkerEvent::Loaded(info));
                    }
                    Control::Backend(value) => {
                        backend = value;
                        if let Some(old) = &document {
                            let (state, info) = open(
                                old.path.clone(),
                                old.source.clone(),
                                old.generation,
                                backend,
                                true,
                            )?;
                            document = Some(state);
                            let _ = events.send(WorkerEvent::Loaded(info));
                        }
                    }
                    Control::Export(path, format, quality, request) => {
                        let doc = document.as_mut().ok_or("Open a document first.")?;
                        if doc.generation != request.generation {
                            return Err(
                                "Source changed before export; retry with the current document."
                                    .into(),
                            );
                        }
                        shared.0.lock().unwrap().active =
                            Some(request.options.cancellation.clone());
                        doc.renderer.export_image(
                            &request.options,
                            &path,
                            format,
                            quality,
                            false,
                        )?;
                        let _ = events.send(WorkerEvent::Exported(path));
                    }
                }
                Ok(())
            })();
            shared.0.lock().unwrap().active = None;
            if let Err(error) = result {
                let _ = events.send(WorkerEvent::Error(error));
            }
        }
        if checked.elapsed() >= Duration::from_millis(350) {
            checked = Instant::now();
            if let Some(path) = &watched
                && let Ok(source) = read_source(path)
                && observed.as_deref() != Some(source.as_str())
            {
                observed = Some(source.clone());
                generation += 1;
                match open(path.clone(), Arc::from(source), generation, backend, true) {
                    Ok((state, info)) => {
                        document = Some(state);
                        let _ = events.send(WorkerEvent::Loaded(info));
                    }
                    Err(error) => {
                        let _ = events.send(WorkerEvent::Error(error));
                    }
                }
            }
        }
        if let Some(request) = frame {
            let result: Result<LiveFrame, String> = (|| {
                let doc = document
                    .as_mut()
                    .ok_or("Open a source document to start.")?;
                if doc.generation != request.generation {
                    return Err("render cancelled: source changed".into());
                }
                let start = Instant::now();
                let pixels = doc.renderer.render(&request.options)?;
                let warnings = doc.renderer.take_warnings();
                if !warnings.is_empty() {
                    doc.notice = warnings.join("; ");
                }
                Ok(LiveFrame {
                    request,
                    rgba: rgba8(&pixels),
                    elapsed: start.elapsed(),
                    backend: format!(
                        "{}{}",
                        doc.renderer.backend_description(),
                        if doc.notice.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", doc.notice)
                        }
                    ),
                })
            })();
            match result {
                Ok(frame) => {
                    let _ = events.send(WorkerEvent::Frame(frame));
                }
                Err(error) => {
                    let _ = events.send(if error.contains("cancelled") {
                        WorkerEvent::Cancelled
                    } else {
                        WorkerEvent::Error(error)
                    });
                }
            }
            shared.0.lock().unwrap().active = None;
        }
    }
}
