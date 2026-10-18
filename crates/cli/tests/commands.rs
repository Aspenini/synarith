use std::{path::Path, process::Command};
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_syar"))
        .args(args)
        .output()
        .unwrap()
}
fn source() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/orbit.syar")
        .to_string_lossy()
        .into_owned()
}
#[test]
fn validate_and_inspect() {
    let version = cli(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("syar "));
    let file = source();
    assert!(cli(&["validate", &file]).status.success());
    let output = cli(&["inspect", &file]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("duration"));
    let ops = cli(&["inspect", "--operations"]);
    assert!(ops.status.success());
    assert!(String::from_utf8_lossy(&ops.stdout).contains("hash"));
    let operations: serde_json::Value = serde_json::from_slice(&ops.stdout).unwrap();
    assert!(
        operations
            .as_array()
            .unwrap()
            .iter()
            .all(|op| op["name"] != "mandelbrot")
    );
    let mandelbrot = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/mandelbrot.syar");
    let inspected = cli(&["inspect", mandelbrot.to_str().unwrap()]);
    assert!(inspected.status.success());
    let inspected: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(inspected["functions"][0]["name"], "mandelbrot");
}

#[test]
fn standard_library_expansion_round_trips_through_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-runs");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root).unwrap();
    let file = directory.path().join("portable.syar");
    let source = "syar 1;\nuse /* keep */ std;\noutput main=blur(fill(circle(p-vec2(0.5,0.5),0.3),srgb(\"#54b8da80\")),0.02);\n";
    std::fs::write(&file, source).unwrap();
    let file = file.to_str().unwrap();
    let before = directory.path().join("before.png");
    let after = directory.path().join("after.png");
    assert!(
        cli(&[
            "render",
            file,
            "-o",
            before.to_str().unwrap(),
            "--width",
            "12",
            "--height",
            "8",
            "--backend",
            "cpu"
        ])
        .status
        .success()
    );
    assert!(
        !cli(&["fmt", file, "--expand-std", "--check"])
            .status
            .success()
    );
    assert!(
        cli(&["fmt", file, "--expand-std", "--write"])
            .status
            .success()
    );
    let expanded = std::fs::read_to_string(file).unwrap();
    assert!(expanded.contains("/* keep */"));
    assert!(expanded.contains("fn circle("));
    assert!(
        synarith::parse(&expanded)
            .unwrap()
            .standard_library
            .is_none()
    );
    assert!(
        cli(&["fmt", file, "--expand-std", "--check"])
            .status
            .success()
    );
    assert!(cli(&["validate", file]).status.success());
    assert!(
        cli(&[
            "render",
            file,
            "-o",
            after.to_str().unwrap(),
            "--width",
            "12",
            "--height",
            "8",
            "--backend",
            "cpu"
        ])
        .status
        .success()
    );
    assert_eq!(
        std::fs::read(before).unwrap(),
        std::fs::read(after).unwrap()
    );
}

#[test]
fn custom_algorithms_validate_inspect_render_and_animate_through_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-runs");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root).unwrap();
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/algorithms.syar");
    let file = file.to_str().unwrap();
    assert!(cli(&["validate", file]).status.success());
    let inspected = cli(&["inspect", file]);
    assert!(inspected.status.success());
    let value: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(value["functions"][0]["name"], "orbit");
    let still = directory.path().join("julia.png");
    let result = cli(&[
        "render",
        file,
        "-o",
        still.to_str().unwrap(),
        "--width",
        "12",
        "--height",
        "8",
        "--backend",
        "cpu",
        "--param",
        "iterations=8",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(std::fs::read(still).unwrap().starts_with(b"\x89PNG"));
    let frames = directory.path().join("frames");
    let result = cli(&[
        "animate",
        file,
        "-o",
        frames.to_str().unwrap(),
        "--width",
        "12",
        "--height",
        "8",
        "--backend",
        "cpu",
        "--frames",
        "3",
        "--fps",
        "2",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(frames.join("frame_000002.png").is_file());
    assert!(!frames.join("frame_000003.png").exists());
}
#[test]
fn still_format_selection_and_quality_are_forwarded_to_encoders() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-runs");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root).unwrap();
    let file = source();
    for (extension, magic) in [
        ("png", &b"\x89PNG"[..]),
        ("jpg", &b"\xff\xd8\xff"[..]),
        ("bmp", &b"BM"[..]),
        ("ppm", &b"P6"[..]),
    ] {
        let path = directory
            .path()
            .join(format!("with spaces.{extension}"))
            .to_string_lossy()
            .into_owned();
        let result = cli(&[
            "render",
            &file,
            "-o",
            &path,
            "--width",
            "16",
            "--height",
            "16",
            "--backend",
            "cpu",
            "--quality",
            "93",
        ]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(std::fs::read(path).unwrap().starts_with(magic));
    }
    let path = directory
        .path()
        .join("explicit.bin")
        .to_string_lossy()
        .into_owned();
    assert!(
        cli(&[
            "render",
            &file,
            "-o",
            &path,
            "--width",
            "4",
            "--height",
            "4",
            "--backend",
            "cpu",
            "--format",
            "tga"
        ])
        .status
        .success()
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[12], bytes[13]]), 4);
    assert!(
        !cli(&["render", &file, "-o", &path, "--quality", "0", "--force"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
#[cfg(not(feature = "gpu"))]
#[test]
fn cpu_build_reports_fallback_and_rejects_required_gpu() {
    let file = source();
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-runs/cpu-fallback");
    std::fs::create_dir_all(&directory).unwrap();
    let png = directory
        .join("fallback.png")
        .to_string_lossy()
        .into_owned();
    let result = cli(&[
        "render",
        &file,
        "-o",
        &png,
        "--width",
        "2",
        "--height",
        "2",
        "--backend",
        "auto",
        "--force",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("disabled at build time"));
    let result = cli(&["render", &file, "-o", &png, "--backend", "gpu", "--force"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("disabled at build time"));
    std::fs::remove_file(png).unwrap();
}
#[test]
fn render_sequence_and_existing_file_protection() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/test-runs/cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = source();
    let png = dir.join("still.png").to_string_lossy().into_owned();
    let result = cli(&[
        "render",
        &file,
        "-o",
        &png,
        "--width",
        "16",
        "--height",
        "12",
        "--param",
        "duration=2.0",
        "--force",
        "--backend",
        "cpu",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(&std::fs::read(&png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    assert!(!cli(&["render", &file, "-o", &png]).status.success());
    let sequence = dir.join("frames").to_string_lossy().into_owned();
    let result = cli(&[
        "animate",
        &file,
        "-o",
        &sequence,
        "--frames",
        "3",
        "--fps",
        "30",
        "--width",
        "8",
        "--height",
        "8",
        "--force",
        "--backend",
        "cpu",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(Path::new(&sequence).join("frame_000002.png").exists());
    assert!(!Path::new(&sequence).join("frame_000003.png").exists());
    assert!(
        !cli(&["animate", &file, "-o", &sequence, "--fps", "0"])
            .status
            .success()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
