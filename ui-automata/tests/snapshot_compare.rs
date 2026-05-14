use std::collections::HashMap;
use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use ui_automata::mock::mock_desktop_from_yaml;
use ui_automata::yaml::WorkflowFile;
use ui_automata::Executor;

// ── helpers ───────────────────────────────────────────────────────────────────

fn solid_png(path: &PathBuf, r: u8, g: u8, b: u8) {
    let mut img = RgbaImage::new(4, 4);
    for px in img.pixels_mut() {
        *px = Rgba([r, g, b, 255]);
    }
    img.save(path).unwrap();
}

fn run_compare(actual: &PathBuf, golden: &PathBuf, fuzz_pct: f64) -> bool {
    let actual_str = actual.to_string_lossy().replace('\\', "/");
    let golden_str = golden.to_string_lossy().replace('\\', "/");
    let desktop = mock_desktop_from_yaml("role: window\nname: App\n");
    let yaml = format!(
        r#"
name: test
defaults:
  timeout: 100ms
anchors:
  app:
    type: Root
    selector: "[name=App]"
phases:
  - name: check
    mount: [app]
    steps:
      - intent: compare snapshots
        action:
          type: NoOp
        expect:
          type: SnapshotMatches
          actual: "{actual_str}"
          golden: "{golden_str}"
          fuzz_pct: {fuzz_pct}
"#
    );
    let wf = WorkflowFile::load_from_str(&yaml, &HashMap::new()).expect("YAML parse failed");
    let mut executor = Executor::new(desktop);
    wf.run(&mut executor, None, None).is_ok()
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[test]
fn identical_images_match() {
    let dir = std::env::temp_dir();
    let actual = dir.join("snap_identical_actual.png");
    let golden = dir.join("snap_identical_golden.png");
    solid_png(&actual, 100, 150, 200);
    solid_png(&golden, 100, 150, 200);
    assert!(run_compare(&actual, &golden, 0.0));
}

#[test]
fn different_images_do_not_match() {
    let dir = std::env::temp_dir();
    let actual = dir.join("snap_diff_actual.png");
    let golden = dir.join("snap_diff_golden.png");
    solid_png(&actual, 255, 0, 0);
    solid_png(&golden, 0, 0, 255);
    assert!(!run_compare(&actual, &golden, 0.0));
}

#[test]
fn small_difference_within_fuzz_passes() {
    let dir = std::env::temp_dir();
    let actual = dir.join("snap_fuzz_actual.png");
    let golden = dir.join("snap_fuzz_golden.png");
    // channel difference of 5 out of 255 ≈ 1.96% — passes at 2% fuzz
    solid_png(&actual, 100, 100, 100);
    solid_png(&golden, 105, 105, 105);
    assert!(run_compare(&actual, &golden, 2.0));
}

#[test]
fn small_difference_outside_fuzz_fails() {
    let dir = std::env::temp_dir();
    let actual = dir.join("snap_nofuzz_actual.png");
    let golden = dir.join("snap_nofuzz_golden.png");
    // channel difference of 10 out of 255 ≈ 3.9% — fails at 1% fuzz
    solid_png(&actual, 100, 100, 100);
    solid_png(&golden, 110, 110, 110);
    assert!(!run_compare(&actual, &golden, 1.0));
}

#[test]
fn missing_actual_returns_false() {
    let dir = std::env::temp_dir();
    let actual = dir.join("snap_missing_actual_NONEXISTENT.png");
    let golden = dir.join("snap_missing_golden.png");
    solid_png(&golden, 0, 0, 0);
    assert!(!run_compare(&actual, &golden, 0.0));
}

#[test]
fn missing_golden_returns_false() {
    let dir = std::env::temp_dir();
    let actual = dir.join("snap_missing_actual2.png");
    let golden = dir.join("snap_missing_golden_NONEXISTENT.png");
    solid_png(&actual, 0, 0, 0);
    assert!(!run_compare(&actual, &golden, 0.0));
}

#[test]
fn different_dimensions_do_not_match() {
    let dir = std::env::temp_dir();
    let actual = dir.join("snap_dim_actual.png");
    let golden = dir.join("snap_dim_golden.png");

    let mut img_a = RgbaImage::new(4, 4);
    for px in img_a.pixels_mut() {
        *px = Rgba([128, 128, 128, 255]);
    }
    img_a.save(&actual).unwrap();

    let mut img_g = RgbaImage::new(8, 8);
    for px in img_g.pixels_mut() {
        *px = Rgba([128, 128, 128, 255]);
    }
    img_g.save(&golden).unwrap();

    assert!(!run_compare(&actual, &golden, 0.0));
}
