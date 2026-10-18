//! Egui live mathematical playback, source watching, and camera exploration.
#![cfg_attr(windows, windows_subsystem = "windows")]
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use synarith::{Backend, ImageFormat, RenderOptions, Type, Value, parameter_value};
use synarith_viewer::{Camera, DocumentInfo, FrameRequest, LiveWorker, Timeline, WorkerEvent};

const KOCH: &str = include_str!("../../../examples/koch-snowflake.syar");
const DRAGON: &str = include_str!("../../../examples/dragon-curve.syar");
const SPACE: &str = include_str!("../../../examples/deep-space.syar");
const FORMATS: [(&str, ImageFormat); 5] = [
    ("PNG", ImageFormat::Png),
    ("JPEG", ImageFormat::Jpeg),
    ("BMP", ImageFormat::Bmp),
    ("TGA", ImageFormat::Tga),
    ("PPM", ImageFormat::Ppm),
];
const CAPS: [u32; 3] = [768, 1280, 2048];
struct Smoke {
    snapshot: Option<PathBuf>,
    export: Option<PathBuf>,
    phase: u8,
    initial_width: f32,
    started: Instant,
    outcome: Arc<Mutex<Result<(), String>>>,
}
struct Viewer {
    worker: LiveWorker,
    document: Option<DocumentInfo>,
    camera: Camera,
    timeline: Timeline,
    live: bool,
    parameters: Vec<Value>,
    overrides: std::collections::BTreeSet<String>,
    parameter_text: Vec<String>,
    output: String,
    backend: Backend,
    revision: u64,
    in_flight: bool,
    dirty: bool,
    exporting: bool,
    image: Option<egui::TextureHandle>,
    last_rgba: Option<(u32, u32, Vec<u8>)>,
    frame_time: f32,
    latency: f64,
    backend_text: String,
    message: String,
    error: bool,
    canvas: [f64; 4],
    last_tick: Instant,
    quality: usize,
    adaptive_cap: u32,
    samples: u32,
    export_width: u32,
    export_height: u32,
    jpeg_quality: u8,
    format: usize,
    source_panel: bool,
    smoke: Option<Smoke>,
}
impl Viewer {
    fn new(file: Option<PathBuf>, smoke: Option<Smoke>, paused: bool) -> Self {
        let worker = LiveWorker::new();
        if let Some(path) = file {
            worker.open(path);
        } else {
            worker.open_source("Koch snowflake.syar".into(), Arc::from(KOCH));
        }
        Self {
            worker,
            document: None,
            camera: Camera::new([0., 0., 1., 1.]),
            timeline: Timeline {
                playing: !paused,
                ..Default::default()
            },
            live: !paused,
            parameters: vec![],
            overrides: Default::default(),
            parameter_text: vec![],
            output: "main".into(),
            backend: Backend::Auto,
            revision: 1,
            in_flight: false,
            dirty: true,
            exporting: false,
            image: None,
            last_rgba: None,
            frame_time: 0.,
            latency: 0.,
            backend_text: "Preparing renderer".into(),
            message: "Opening source…".into(),
            error: false,
            canvas: [0., 0., 800., 600.],
            last_tick: Instant::now(),
            quality: 1,
            adaptive_cap: 960,
            samples: 1,
            export_width: 1920,
            export_height: 1080,
            jpeg_quality: 90,
            format: 0,
            source_panel: false,
            smoke,
        }
    }
    fn invalidate(&mut self) {
        self.revision += 1;
        self.worker.cancel();
        self.in_flight = false;
        self.dirty = true;
        self.error = false;
    }
    fn request(&self, width: u32, height: u32) -> Option<FrameRequest> {
        let doc = self.document.as_ref()?;
        Some(FrameRequest {
            revision: self.revision,
            generation: doc.generation,
            options: RenderOptions {
                width,
                height,
                time: self.timeline.time as f32,
                viewport: Some(self.camera.viewport([self.canvas[2], self.canvas[3]])),
                samples: if self.timeline.playing {
                    1
                } else {
                    self.samples
                },
                output: self.output.clone(),
                parameters: self.parameters.clone(),
                ..Default::default()
            },
        })
    }
    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        self.timeline.advance(dt);
        let moving = self.camera.advance(dt);
        self.dirty |= moving || self.timeline.playing;
        let mut changed = moving;
        while let Some(event) = self.worker.poll() {
            changed = true;
            match event {
                WorkerEvent::Loaded(doc) => {
                    if !doc.reloaded {
                        self.overrides.clear();
                    }
                    if doc.reloaded
                        && self
                            .document
                            .as_ref()
                            .is_some_and(|old| old.viewbox != doc.viewbox)
                    {
                        self.camera = Camera::new(doc.viewbox);
                    }
                    if !doc.reloaded {
                        self.camera = Camera::new(doc.viewbox);
                        self.timeline.seek(0.);
                        self.output = if doc.outputs.iter().any(|v| v == "main") {
                            "main".into()
                        } else {
                            doc.outputs[0].clone()
                        };
                        self.export_height = (1920. * f64::from(doc.viewbox[3] / doc.viewbox[2]))
                            .round()
                            .clamp(1., 16384.) as u32;
                    }
                    let old = self.document.as_ref();
                    self.parameters = doc
                        .parameters
                        .iter()
                        .map(|p| {
                            old.and_then(|old| {
                                old.parameters
                                    .iter()
                                    .position(|v| v.name == p.name && v.ty == p.ty)
                            })
                            .and_then(|i| self.parameters.get(i).copied())
                            .filter(|_| doc.reloaded && self.overrides.contains(&p.name))
                            .unwrap_or(p.default)
                        })
                        .collect();
                    if !doc.outputs.contains(&self.output) {
                        self.output = doc.outputs[0].clone();
                    }
                    self.parameter_text =
                        self.parameters.iter().copied().map(Self::value).collect();
                    self.document = Some(doc);
                    self.timeline.playing = self.live;
                    self.invalidate();
                    self.message = "Source ready · watching for changes".into();
                }
                WorkerEvent::Frame(frame) => {
                    self.in_flight = false;
                    if frame.request.revision != self.revision
                        || self
                            .document
                            .as_ref()
                            .is_none_or(|d| d.generation != frame.request.generation)
                    {
                        continue;
                    }
                    self.frame_time = frame.request.options.time;
                    self.latency = frame.elapsed.as_secs_f64();
                    self.backend_text = frame.backend;
                    let w = frame.request.options.width;
                    let h = frame.request.options.height;
                    self.last_rgba = Some((w, h, frame.rgba.clone()));
                    let pixels = egui::ColorImage::from_rgba_unmultiplied(
                        [w as usize, h as usize],
                        &frame.rgba,
                    );
                    if let Some(texture) = &mut self.image {
                        texture.set(pixels, egui::TextureOptions::LINEAR);
                    } else {
                        self.image = Some(ctx.load_texture(
                            "synarith-preview",
                            pixels,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    if self.timeline.playing {
                        if self.latency > 0.08 {
                            self.adaptive_cap = ((w.max(h) as f64 * (0.045 / self.latency).sqrt())
                                as u32)
                                .clamp(192, CAPS[self.quality]);
                        } else if self.latency < 0.025 {
                            self.adaptive_cap = (self.adaptive_cap + 32).min(CAPS[self.quality]);
                        }
                    }
                    if !self.error {
                        self.message = format!(
                            "{} × {} · {:.1} ms/frame · in-memory rendering",
                            w,
                            h,
                            self.latency * 1000.
                        );
                    }
                    self.smoke_frame(frame.request.options.viewport.unwrap(), ctx);
                }
                WorkerEvent::Error(error) => {
                    self.in_flight = false;
                    self.exporting = false;
                    self.message = error;
                    self.error = true;
                    self.timeline.playing = false;
                    self.dirty = false;
                    if self.smoke.is_some() {
                        self.finish_smoke(Err(self.message.clone()), ctx);
                    }
                }
                WorkerEvent::Cancelled => {
                    self.in_flight = false;
                }
                WorkerEvent::Exported(path) => {
                    self.exporting = false;
                    self.message = format!("Exported {}", path.display());
                    if self.smoke.as_ref().is_some_and(|s| s.phase == 3) {
                        self.finish_smoke(Ok(()), ctx);
                    }
                }
            }
        }
        if self
            .smoke
            .as_ref()
            .is_some_and(|s| s.started.elapsed() > Duration::from_secs(90))
        {
            self.finish_smoke(Err("Viewer workflow timed out".into()), ctx);
        }
        if self.dirty && !self.in_flight && !self.exporting && self.document.is_some() {
            let cap = if self.timeline.playing {
                self.adaptive_cap.min(CAPS[self.quality])
            } else {
                CAPS[self.quality]
            };
            let scale = (f64::from(cap) / self.canvas[2].max(self.canvas[3])).min(1.0);
            let w = (self.canvas[2] * scale).round().max(1.) as u32;
            let h = (self.canvas[3] * scale).round().max(1.) as u32;
            if let Some(request) = self.request(w, h) {
                self.worker.render(request, false);
                self.in_flight = true;
                self.dirty = false;
            }
        }
        if changed || self.timeline.playing {
            ctx.request_repaint();
        }
    }
    fn smoke_frame(&mut self, viewport: [f32; 4], ctx: &egui::Context) {
        let Some(smoke) = &mut self.smoke else {
            return;
        };
        match smoke.phase {
            0 => {
                smoke.initial_width = viewport[2];
                smoke.phase = 1;
                self.timeline.seek(1.0);
                self.dirty = true;
            }
            1 if self.frame_time >= 1.0 => {
                smoke.phase = 2;
                self.live = false;
                self.timeline.playing = false;
                self.camera
                    .zoom_at(2.0f64.ln(), [0.5, 0.5], [self.canvas[2], self.canvas[3]]);
                self.camera.advance(1.0);
                self.invalidate();
            }
            2 if viewport[2] < smoke.initial_width * 0.75 => {
                smoke.phase = 3;
                if let Some(path) = &smoke.snapshot
                    && let Some((w, h, data)) = &self.last_rgba
                    && let Err(e) = image::save_buffer(path, data, *w, *h, image::ColorType::Rgba8)
                {
                    self.finish_smoke(Err(e.to_string()), ctx);
                    return;
                }
                if let Some(path) = smoke.export.take() {
                    let request = self.request(640, 400).unwrap();
                    self.exporting = true;
                    self.worker.export(path, ImageFormat::Png, 90, request);
                } else {
                    self.finish_smoke(Ok(()), ctx);
                }
            }
            _ => (),
        }
    }
    fn finish_smoke(&mut self, result: Result<(), String>, ctx: &egui::Context) {
        if let Some(smoke) = self.smoke.take() {
            *smoke.outcome.lock().unwrap() = result;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn value(value: Value) -> String {
        match value.ty {
            Type::Float | Type::Int => format!("{}", value.data[0]),
            Type::Bool => {
                if value.data[0] != 0. {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            Type::Vec2 => format!("vec2({}, {})", value.data[0], value.data[1]),
            Type::Color => {
                let a = value.data[3];
                let u = if a > 0. { 1. / a } else { 0. };
                format!(
                    "rgba({:.3}, {:.3}, {:.3}, {:.3})",
                    value.data[0] * u,
                    value.data[1] * u,
                    value.data[2] * u,
                    a
                )
            }
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        self.invalidate();
        self.worker.open(path);
        self.message = "Opening source…".into();
    }
    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Synarith source", &["syar"])
            .pick_file()
        {
            self.open_path(path);
        }
        self.last_tick = Instant::now();
    }
    fn export_dialog(&mut self) {
        if self.document.is_none() || self.exporting {
            return;
        }
        let extension = FORMATS[self.format].0.to_ascii_lowercase();
        let dialog = rfd::FileDialog::new()
            .add_filter(FORMATS[self.format].0, &[extension.as_str()])
            .set_file_name(format!("synarith.{extension}"));
        if let Some(path) = dialog.save_file() {
            let mut request = self.request(self.export_width, self.export_height).unwrap();
            request.options.samples = self.samples;
            self.worker
                .export(path, FORMATS[self.format].1, self.jpeg_quality, request);
            self.exporting = true;
            self.in_flight = false;
            self.message = "Exporting current camera and time…".into();
        }
        self.last_tick = Instant::now();
    }
    fn toggle_play(&mut self) {
        self.live = !self.timeline.playing;
        self.timeline.playing = self.live;
        self.dirty = true;
    }
    fn restart(&mut self) {
        self.timeline.seek(0.);
        self.invalidate();
    }
    fn zoom(&mut self, amount: f64, anchor: [f64; 2]) {
        self.camera
            .zoom_at(amount, anchor, [self.canvas[2], self.canvas[3]]);
    }
    fn controls(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Synarith");
                ui.separator();
                if ui.button("Open source…").clicked() {
                    self.open_dialog();
                }
                for (label, source) in [("Koch", KOCH), ("Dragon", DRAGON), ("Space", SPACE)] {
                    if ui.button(label).clicked() {
                        self.invalidate();
                        self.worker
                            .open_source(format!("{label}.syar").into(), Arc::from(source));
                    }
                }
                ui.separator();
                if ui.button("Fit").clicked() {
                    self.camera.reset();
                    self.dirty = true;
                }
                if ui.button("−").clicked() {
                    self.zoom(-0.5, [0.5, 0.5]);
                }
                if ui.button("+").clicked() {
                    self.zoom(0.5, [0.5, 0.5]);
                }
                ui.label(format!("{:.2}×", self.camera.magnification()));
                ui.checkbox(&mut self.source_panel, "Source");
            });
        });
        egui::TopBottomPanel::bottom("timeline").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(if self.timeline.playing {
                        "Pause / still"
                    } else {
                        "Play continuously"
                    })
                    .clicked()
                {
                    self.toggle_play();
                }
                if ui.button("Restart").clicked() {
                    self.restart();
                }
                ui.label("Time (s)");
                if ui
                    .add(
                        egui::DragValue::new(&mut self.timeline.time)
                            .speed(0.05)
                            .range(0.0..=f64::from(f32::MAX)),
                    )
                    .changed()
                {
                    self.timeline.playing = false;
                    self.live = false;
                    self.invalidate();
                }
                ui.label("Rate");
                ui.add(
                    egui::DragValue::new(&mut self.timeline.speed)
                        .speed(0.05)
                        .range(0.01..=100.0)
                        .suffix("×"),
                );
                ui.label(format!("Displayed t = {:.3}", self.frame_time));
                if ui.button("Stop work").clicked() {
                    self.worker.cancel();
                    self.live = false;
                    self.timeline.playing = false;
                    self.exporting = false;
                    self.dirty = false;
                    self.message = "Stopped".into();
                }
            });
            if self.camera.precision_limited {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Coordinate precision limit reached (f32 evaluation)",
                );
            }
            ui.colored_label(
                if self.error {
                    egui::Color32::LIGHT_RED
                } else {
                    egui::Color32::GRAY
                },
                &self.message,
            );
            ui.small(&self.backend_text);
        });
        egui::SidePanel::left("inspector").default_width(260.).min_width(220.).max_width(420.).show(ctx,|ui| {
            egui::ScrollArea::vertical().show(ui,|ui| {
                ui.heading("Document");
                if let Some(doc)=&self.document {ui.label(doc.path.display().to_string());}
                let mut changed=false;
                egui::ComboBox::from_label("Output").selected_text(&self.output).show_ui(ui,|ui| {
                    if let Some(doc)=&self.document {for output in &doc.outputs {
                        changed|=ui.selectable_value(&mut self.output,output.clone(),output).changed();
                    }}
                });
                let old_backend=self.backend;
                egui::ComboBox::from_label("Backend").selected_text(format!("{:?}",self.backend)).show_ui(ui,|ui| {
                    for b in [Backend::Auto,Backend::Cpu,Backend::Gpu] {ui.selectable_value(&mut self.backend,b,format!("{b:?}"));}
                });
                if old_backend!=self.backend {self.invalidate(); self.worker.set_backend(self.backend);}
                ui.separator(); ui.heading("Preview");
                let old_quality=self.quality;
                egui::ComboBox::from_label("Maximum edge").selected_text(format!("{} px",CAPS[self.quality])).show_ui(ui,|ui| {
                    for (i,cap) in CAPS.iter().enumerate(){ui.selectable_value(&mut self.quality,i,format!("{cap} px"));}
                });
                if self.quality!=old_quality {self.adaptive_cap=CAPS[self.quality]; changed=true;}
                changed|=ui.add(egui::Slider::new(&mut self.samples,1..=8).text("Still samples/axis")).changed();
                ui.small("Playback adapts resolution. Pausing resamples at the selected quality.");
                ui.separator(); ui.heading("Parameters");
                if let Some(doc)=&self.document {
                    for (i,param) in doc.parameters.iter().enumerate() {
                        ui.label(format!("{} : {:?}",param.name,param.ty));
                        let response=ui.add(egui::TextEdit::singleline(&mut self.parameter_text[i]).desired_width(f32::INFINITY));
                        if response.lost_focus() || (response.has_focus() && ui.input(|i|i.key_pressed(egui::Key::Enter))) {
                            match parameter_value(param.ty,&self.parameter_text[i]) {
                                Ok(value) if value!=self.parameters[i] => {
                                    self.parameters[i]=value; self.overrides.insert(param.name.clone()); changed=true;
                                }
                                Err(e)=>{self.error=true; self.message=e;}
                                _=>(),
                            }
                        }
                    }
                }
                if ui.button("Reset parameters").clicked() {
                    self.overrides.clear();
                    if let Some(doc)=&self.document {
                        self.parameters=doc.parameters.iter().map(|p|p.default).collect();
                        self.parameter_text=self.parameters.iter().copied().map(Self::value).collect();
                        changed=true;
                    }
                }
                if changed {self.invalidate();}
                ui.separator(); ui.heading("Export still");
                ui.horizontal(|ui|{ui.label("Width");ui.add(egui::DragValue::new(&mut self.export_width).range(1..=1048576));});
                ui.horizontal(|ui|{ui.label("Height");ui.add(egui::DragValue::new(&mut self.export_height).range(1..=1048576));});
                egui::ComboBox::from_label("Format").selected_text(FORMATS[self.format].0).show_ui(ui,|ui| {
                    for (i,(name,_)) in FORMATS.iter().enumerate(){ui.selectable_value(&mut self.format,i,*name);}
                });
                if self.format==1 {ui.add(egui::Slider::new(&mut self.jpeg_quality,1..=100).text("JPEG quality"));}
                if ui.add_enabled(!self.exporting && self.document.is_some(),egui::Button::new("Render / export…")).clicked(){self.export_dialog();}
                ui.small("Uses the current camera and time. Existing files are protected; choose a new filename.");
                ui.separator(); ui.small("Wheel / pinch: zoom at cursor
Drag: pan · Double-click: fit
Space: play/pause · R: restart
0: fit · O: open · E: export");
            });
        });
        if self.source_panel {
            egui::SidePanel::right("source")
                .default_width(400.)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading("Source · read only");
                    ui.small("Edit the file in your editor; valid changes reload automatically.");
                    if let Some(doc) = &self.document {
                        if ui.button("Copy source").clicked() {
                            ui.ctx().copy_text(doc.source.to_string());
                        }
                        egui::ScrollArea::both().show(ui, |ui| {
                            for (i, line) in doc.source.lines().enumerate() {
                                ui.monospace(format!("{:4}  {line}", i + 1));
                            }
                        });
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(9, 13, 20)))
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(
                    ui.available_size().max(egui::vec2(1., 1.)),
                    egui::Sense::click_and_drag(),
                );
                let previous = [self.canvas[2], self.canvas[3]];
                self.canvas = [
                    f64::from(rect.left()),
                    f64::from(rect.top()),
                    f64::from(rect.width()),
                    f64::from(rect.height()),
                ];
                if previous != [self.canvas[2], self.canvas[3]] {
                    self.dirty = true;
                }
                if let Some(texture) = &self.image {
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                        egui::Color32::WHITE,
                    );
                } else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Preparing mathematical image…",
                        egui::FontId::proportional(20.),
                        egui::Color32::GRAY,
                    );
                }
                if response.dragged() {
                    let d = ui.input(|i| i.pointer.delta());
                    self.camera.pan(
                        [f64::from(d.x), f64::from(d.y)],
                        [self.canvas[2], self.canvas[3]],
                    );
                }
                if response.double_clicked() {
                    self.camera.reset();
                    self.dirty = true;
                }
                if response.hovered() {
                    let (wheel, pinch, pos) = ui.input(|i| {
                        (
                            i.smooth_scroll_delta.y,
                            i.zoom_delta(),
                            i.pointer.hover_pos(),
                        )
                    });
                    if let Some(pos) = pos {
                        let anchor = [
                            f64::from((pos.x - rect.left()) / rect.width()),
                            f64::from((pos.y - rect.top()) / rect.height()),
                        ];
                        // Ctrl-wheel is translated into a zoom gesture by egui.
                        let amount = if (pinch - 1.).abs() > 0.00001 {
                            f64::from(pinch).ln()
                        } else {
                            f64::from(wheel) * 0.005
                        };
                        if amount != 0. {
                            self.zoom(amount, anchor);
                        }
                    }
                }
            });
    }
}
impl eframe::App for Viewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle_play();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::R)) {
                self.restart();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                self.camera.reset();
                self.dirty = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::O)) {
                self.open_dialog();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::E)) {
                self.export_dialog();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Plus)) {
                self.zoom(0.5, [0.5, 0.5]);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
                self.zoom(-0.5, [0.5, 0.5]);
            }
        }
        if let Some(path) = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone())) {
            self.open_path(path);
        }
        self.controls(ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
fn run() -> Result<(), String> {
    let mut file = None;
    let mut smoke = false;
    let mut snapshot = None;
    let mut export = None;
    let mut paused = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--smoke-test") => {
                smoke = true;
                file = Some(PathBuf::from(
                    args.next().ok_or("--smoke-test needs a source path")?,
                ));
            }
            Some("--snapshot") => {
                snapshot = Some(PathBuf::from(
                    args.next().ok_or("--snapshot needs a PNG path")?,
                ))
            }
            Some("--export-test") => {
                export = Some(PathBuf::from(
                    args.next().ok_or("--export-test needs a PNG path")?,
                ))
            }
            Some("--paused") => paused = true,
            Some("--help") => {
                println!(
                    "syar-view [FILE.syar] [--paused]\nEgui live rendering; watches source edits. Scroll zoom, drag pan, Space play/pause, R restart, 0 fit.\nDeveloper workflow: --smoke-test FILE --snapshot CANVAS.png --export-test OUTPUT.png"
                );
                return Ok(());
            }
            Some(v) if v.starts_with('-') => return Err(format!("Unknown argument: {v}")),
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => return Err("Open one source file at a time".into()),
        }
    }
    let outcome = Arc::new(Mutex::new(Err(
        "Viewer closed before the workflow completed".into(),
    )));
    let returned = outcome.clone();
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Synarith · live mathematical images")
            .with_inner_size([1280., 860.])
            .with_min_inner_size([900., 620.]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Synarith",
        native,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let smoke = smoke.then(|| Smoke {
                snapshot,
                export,
                phase: 0,
                initial_width: 0.,
                started: Instant::now(),
                outcome,
            });
            Ok(Box::new(Viewer::new(file, smoke, paused)))
        }),
    )
    .map_err(|e| e.to_string())?;
    if smoke {
        returned.lock().unwrap().clone()
    } else {
        Ok(())
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("syar-view: {error}");
        std::process::exit(1);
    }
}
