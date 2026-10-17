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
    assert!(String::from_utf8_lossy(&ops.stdout).contains("bezier"));
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
