use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use synarith::{Backend, ImageFormat, RenderOptions};
use synarith_viewer::*;

fn event(worker: &LiveWorker) -> WorkerEvent {
    let start = Instant::now();
    loop {
        if let Some(event) = worker.poll() {
            return event;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "worker timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn loaded(worker: &LiveWorker) -> DocumentInfo {
    match event(worker) {
        WorkerEvent::Loaded(doc) => doc,
        WorkerEvent::Error(e) => panic!("{e}"),
        _ => panic!("expected document"),
    }
}
fn frame(worker: &LiveWorker) -> LiveFrame {
    loop {
        match event(worker) {
            WorkerEvent::Frame(frame) => return frame,
            WorkerEvent::Cancelled => (),
            WorkerEvent::Error(e) => panic!("{e}"),
            _ => panic!("expected frame"),
        }
    }
}
fn request(generation: u64, time: f32) -> FrameRequest {
    FrameRequest {
        generation,
        revision: 1,
        options: RenderOptions {
            width: 24,
            height: 16,
            time,
            ..Default::default()
        },
    }
}

#[test]
fn playback_seek_and_cursor_anchored_camera_are_independent() {
    let mut timeline = Timeline::default();
    timeline.advance(4000.);
    assert_eq!(timeline.time, 4000.);
    timeline.playing = false;
    timeline.advance(10.);
    assert_eq!(timeline.time, 4000.);
    timeline.seek(0.);
    assert_eq!(timeline.time, 0.);
    let mut camera = Camera::new([-1., -1., 2., 2.]);
    let size = [800., 600.];
    let anchor = [0.25, 0.75];
    let old = camera.viewport(size);
    let old_precise = camera.precise_viewport(size);
    camera.zoom_at(2.0f64.ln(), anchor, size);
    camera.advance(0.01);
    let intermediate = camera.precise_viewport(size);
    for i in 0..2 {
        assert!(
            (old_precise[i] + anchor[i] * old_precise[i + 2]
                - intermediate[i]
                - anchor[i] * intermediate[i + 2])
                .abs()
                < 1e-12
        );
    }
    camera.advance(2.);
    let new = camera.viewport(size);
    for i in 0..2 {
        let before = f64::from(old[i]) + anchor[i] * f64::from(old[i + 2]);
        let after = f64::from(new[i]) + anchor[i] * f64::from(new[i + 2]);
        assert!((before - after).abs() < 1e-6);
        assert!((new[i + 2] / old[i + 2] - 0.5).abs() < 1e-6);
    }
    camera.pan([100., -40.], size);
    camera.advance(2.);
    assert_ne!(camera.viewport(size), new);
    camera.zoom_at(70., [0.5, 0.5], size);
    assert!(camera.precision_limited);
    camera.reset();
    assert_eq!(camera.viewport(size), old);
}

#[test]
fn deep_navigation_switches_precision_and_pans_below_the_old_epsilon() {
    let mut camera = Camera::new([0., 0., 1., 1.]);
    let size = [800., 800.];
    assert!(!camera.requires_precision(size));
    camera.zoom_at(1e10f64.ln(), [0.5, 0.5], size);
    camera.advance(2.);
    assert!(!camera.precision_limited);
    assert!(camera.requires_precision(size));
    let before = camera.precise_viewport(size);
    assert!((before[2] - 1e-10).abs() < 1e-19);
    camera.pan([10., 0.], size);
    camera.advance(2.);
    let after = camera.precise_viewport(size);
    assert!(after[0] < before[0]);
    assert!((after[0] - before[0] + 10. * before[2] / size[0]).abs() < 1e-16);
    camera.zoom_at(-1e10f64.ln(), [0.5, 0.5], size);
    camera.advance(2.);
    assert!(!camera.requires_precision(size));
    // The former fixed logarithmic cap was 80. Around zero, f64 can represent
    // much smaller fields; arbitrary algorithm names have no role in navigation.
    let mut origin = Camera::new([-0.5, -0.5, 1., 1.]);
    origin.zoom_at(100., [0.5, 0.5], size);
    origin.advance(2.);
    assert!(!origin.precision_limited);
    assert!(origin.magnification() > 1e40);
    assert!(origin.precise_viewport(size)[2] < 1e-40);
    assert!(origin.requires_precision(size));
}

#[test]
fn worker_plays_reloads_retains_valid_source_cancels_and_exports() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("live.syar");
    let source = "syar 1;param shade:float=0.2;output main=rgba(fract(x(p)+t),shade,0);";
    std::fs::write(&path, source).unwrap();
    let worker = LiveWorker::new();
    worker.set_backend(Backend::Cpu);
    worker.open(path.clone());
    let doc = loaded(&worker);
    worker.render(request(doc.generation, 0.), false);
    let first = frame(&worker);
    assert_eq!(first.rgba.len(), 24 * 16 * 4);
    worker.render(request(doc.generation, 0.25), false);
    let second = frame(&worker);
    assert_ne!(first.rgba, second.rgba);
    std::fs::write(&path, source.replace("0.2", "0.8")).unwrap();
    let reload = loaded(&worker);
    assert!(reload.reloaded);
    assert_ne!(doc.generation, reload.generation);
    assert_eq!(reload.parameters[0].default.data[0], 0.8);
    worker.render(request(reload.generation, 0.), false);
    let valid = frame(&worker);
    std::fs::write(&path, "syar 1;output main=missing;").unwrap();
    assert!(matches!(event(&worker), WorkerEvent::Error(_)));
    worker.render(request(reload.generation, 0.), false);
    assert_eq!(frame(&worker).rgba, valid.rgba);
    let cancelled = request(reload.generation, 1.);
    cancelled.options.cancellation.cancel();
    worker.render(cancelled, false);
    assert!(matches!(event(&worker), WorkerEvent::Cancelled));
    for (extension, format) in [
        ("png", ImageFormat::Png),
        ("jpg", ImageFormat::Jpeg),
        ("bmp", ImageFormat::Bmp),
        ("tga", ImageFormat::Tga),
        ("ppm", ImageFormat::Ppm),
    ] {
        let destination = directory.path().join(format!("export.{extension}"));
        let mut export = request(reload.generation, 0.5);
        export.options.width = 37;
        export.options.height = 19;
        export.options.viewport = Some([0.2, 0.3, 0.5, 0.5]);
        export.options.precise_viewport = Some([0.5, 0.5, 1e-10, 1e-10]);
        worker.export(destination.clone(), format, 91, export);
        let result = event(&worker);
        assert!(
            matches!(result, WorkerEvent::Exported(_)),
            "{extension} export: {result:?}; {}",
            destination.display()
        );
        assert_eq!(image::image_dimensions(&destination).unwrap(), (37, 19));
    }
    let destination = directory.path().join("export.png");
    let bytes = std::fs::read(&destination).unwrap();
    worker.export(
        destination.clone(),
        ImageFormat::Png,
        90,
        request(reload.generation, 0.),
    );
    assert!(matches!(event(&worker), WorkerEvent::Error(_)));
    assert_eq!(bytes, std::fs::read(destination).unwrap());
    worker.open_source("corrected.syar".into(), Arc::from(source));
    let fixed = loaded(&worker);
    worker.render(request(fixed.generation, 0.), false);
    assert_eq!(frame(&worker).rgba, first.rgba);
}

#[test]
fn pending_requests_are_bounded_and_obsolete_generations_are_discarded() {
    let worker = LiveWorker::new();
    worker.set_backend(Backend::Cpu);
    worker.open_source(
        "live.syar".into(),
        Arc::from("syar 1;output main=rgba(t,0,0);"),
    );
    let doc = loaded(&worker);
    for i in 0..100 {
        let mut r = request(doc.generation, i as f32 / 100.);
        r.revision = i;
        worker.render(r, false);
    }
    loop {
        let result = frame(&worker);
        if result.request.revision == 99 {
            break;
        }
    }
    worker.render(request(doc.generation + 1, 0.), false);
    assert!(matches!(event(&worker), WorkerEvent::Cancelled));
}
