//! End-to-end CLI tests: run the binary against generated fixtures and
//! verify the acceptance flow (preset batch export, EXIF orientation).

use std::process::Command as StdCommand;

fn cli_exe() -> std::path::PathBuf {
    // cargo test builds the bin alongside the test binary.
    let mut p = std::env::current_exe().expect("test exe");
    p.pop(); // deps/
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("photoforge-cli.exe")
        .with_extension(std::env::consts::EXE_EXTENSION)
}

fn write_preset(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("film.json");
    std::fs::write(&p, body).expect("write preset");
    p
}

use std::path::{Path, PathBuf};

#[test]
fn cli_batch_export_with_preset() {
    if !cli_exe().exists() {
        panic!("photoforge-cli binary not found — build it first");
    }
    let dir = std::env::temp_dir().join(format!("pfg_cli_{}", std::process::id()));
    let photos = dir.join("photos");
    let out = dir.join("out");
    std::fs::create_dir_all(&photos).ok();

    // Two fixture images.
    photoforge_fixtures::write_png(&photos.join("a.png"), &photoforge_fixtures::test_card())
        .expect("write a");
    photoforge_fixtures::write_png(
        &photos.join("b.png"),
        &photoforge_fixtures::gradient_h(320, 200),
    )
    .expect("write b");

    let preset = write_preset(
        &dir,
        r#"{
        "version": 1,
        "name": "punch",
        "adjustments": [
            { "op": { "type": "white_balance", "temp": 0.1, "tint": 0.0 }, "enabled": true },
            { "op": { "type": "exposure", "stops": 0.5 }, "enabled": true }
        ]
    }"#,
    );

    let out_status = StdCommand::new(cli_exe())
        .args([
            photos.to_str().expect("dir str"),
            "--preset",
            preset.to_str().expect("preset str"),
            "-o",
            out.to_str().expect("out str"),
            "--format",
            "jpeg",
            "--quality",
            "90",
        ])
        .output()
        .expect("run cli");
    assert!(
        out_status.status.success(),
        "cli failed: {}",
        String::from_utf8_lossy(&out_status.stderr)
    );
    let a = out.join("a.jpg");
    let b = out.join("b.jpg");
    assert!(a.exists() && b.exists(), "outputs written");

    // Decoded outputs carry the preset's exposure (+0.5 EV): brighter.
    let orig = photoforge_io::decode_file(&photos.join("b.png")).expect("decode orig");
    let exported = photoforge_io::decode_file(&b).expect("decode export");
    let orig_mid = orig.buffer.pixel(160, 100)[0];
    let exp_mid = exported.buffer.pixel(160, 100)[0];
    assert!(
        exp_mid > orig_mid,
        "export must be brighter: {orig_mid} -> {exp_mid}"
    );
    assert_eq!(
        (exported.buffer.width(), exported.buffer.height()),
        (320, 200),
        "no resize by default"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cli_sidecar_and_resize_and_skip() {
    if !cli_exe().exists() {
        panic!("photoforge-cli binary not found");
    }
    let dir = std::env::temp_dir().join(format!("pfg_cli2_{}", std::process::id()));
    let photos = dir.join("photos");
    let out = dir.join("out");
    std::fs::create_dir_all(&photos).ok();
    photoforge_fixtures::write_png(
        &photos.join("c.png"),
        &photoforge_fixtures::radial(800, 600),
    )
    .expect("write c");

    // A sidecar with a heavy crop: output must be the cropped size.
    let mut state = photoforge_pipeline::EditState::with_global_stack();
    state.geometry.crop = Some(photoforge_pipeline::CropRect {
        x: 100,
        y: 50,
        w: 300,
        h: 200,
    });
    photoforge_presets::save_sidecar(&photos.join("c.png"), &state).expect("sidecar");

    let out_status = StdCommand::new(cli_exe())
        .args([
            photos.to_str().expect("dir str"),
            "-o",
            out.to_str().expect("out str"),
            "--resize",
            "150",
            "--format",
            "png",
            "--sidecar-out",
        ])
        .output()
        .expect("run cli");
    assert!(
        out_status.status.success(),
        "{}",
        String::from_utf8_lossy(&out_status.stderr)
    );
    let c = out.join("c.png");
    let decoded = photoforge_io::decode_file(&c).expect("decode");
    // Crop 300x200 → resize longest edge to 150 → 150x100.
    assert_eq!(
        (decoded.buffer.width(), decoded.buffer.height()),
        (150, 100)
    );
    // Sidecar-out also written next to the output.
    assert!(out.join("c.png.pfg").exists());

    // skip_existing: rerun must not touch the file.
    let before = std::fs::metadata(&c).expect("meta").modified().ok();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let status = StdCommand::new(cli_exe())
        .args([
            photos.to_str().expect("dir str"),
            "-o",
            out.to_str().expect("out str"),
            "--resize",
            "150",
            "--format",
            "png",
            "--skip-existing",
        ])
        .output()
        .expect("run cli");
    assert!(status.status.success());
    let after = std::fs::metadata(&c).expect("meta").modified().ok();
    assert_eq!(before, after, "skip-existing must not rewrite");

    std::fs::remove_dir_all(&dir).ok();
}
