// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! What the read-only commands say and draw while a world's catalog
//! contains project art: resources with the license `project`, in catalog
//! format 2.
//!
//! No project art is used here. Each test copies the MIT neutral sample to
//! a temporary directory and labels some of its resources `project` in that
//! copy's catalog only: the pixels stay the MIT sample's, and nothing
//! labelled `project` is ever committed.
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{self, EXIT_SUCCESS};
use tilewright_editor_observe::render::{self, Request};
use tilewright_editor_preview::{ReferenceFrame, mark};
use tilewright_editor_testkit::fixture_world;

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the neutral sample whose catalog is format `format` with the
/// resources named in `project` labelled `project`.
fn labelled(format: &str, project: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    if format.ends_with("/2") {
        label_project_art(&root, project);
    } else {
        assert!(project.is_empty());
    }
    (directory, root)
}

/// The neutral sample's own catalog, format 1, in a copy.
fn original() -> (tempfile::TempDir, PathBuf) {
    labelled("tilewright.editor-preview/1", &[])
}

fn json_of(command: &str, root: &Path, text: bool) -> String {
    let mut arguments = vec![OsString::from(root)];
    if command == "tile" {
        // The marker's tile.
        arguments.extend(["main", "0", "3", "2"].map(OsString::from));
    }
    if !text {
        arguments.insert(0, "--json".into());
    }
    let outcome = cli::run(command, &arguments);
    assert_eq!(outcome.code, EXIT_SUCCESS, "{command}: {}", outcome.stderr);
    outcome.stdout
}

fn parsed(command: &str, root: &Path) -> Value {
    serde_json::from_str(&json_of(command, root, false)).unwrap()
}

/// Draws the whole view of `root` at `scale`: the pixels and the report.
fn draw(root: &Path, scale: u32) -> (ReferenceFrame, Value) {
    let request = Request {
        root: root.into(),
        out: "unused.png".into(),
        region: None,
        scale,
        json: true,
    };
    let observed = render::open(&request).unwrap_or_else(|outcome| panic!("{outcome:?}"));
    let rendered =
        render::render(&observed, &request).unwrap_or_else(|outcome| panic!("{outcome:?}"));
    let frame = rendered.frame.clone();
    let outcome = rendered.finish(1, &[7; 32]);
    (frame, serde_json::from_str(&outcome.stdout).unwrap())
}

const COMMANDS: [&str; 4] = ["overview", "tile", "catalog", "lint"];

#[test]
fn a_format_2_catalog_without_project_art_reports_and_draws_as_format_1() {
    let (_one_directory, one) = original();
    let (_two_directory, two) = labelled("tilewright.editor-preview/2", &[]);
    for command in COMMANDS {
        for text in [false, true] {
            assert_eq!(
                json_of(command, &one, text),
                json_of(command, &two, text),
                "{command}"
            );
        }
    }
    for scale in [1, 2] {
        assert_eq!(draw(&one, scale), draw(&two, scale));
    }
    assert!(!json_of("overview", &one, false).contains("project_art"));
}

#[test]
fn every_report_says_how_many_resources_are_project_art() {
    let (_original_directory, plain) = original();
    for project in [&["grass"][..], &["grass", "marker", "stone"]] {
        let (_directory, root) = labelled("tilewright.editor-preview/2", project);
        let expected = json!({"resources": project.len()});
        for command in COMMANDS {
            let report = parsed(command, &root);
            assert_eq!(report["catalog"]["project_art"], expected, "{command}");
            // Nothing else changes: the same report less the member.
            let mut without = report.clone();
            without["catalog"]
                .as_object_mut()
                .unwrap()
                .remove("project_art");
            let mut before = parsed(command, &plain);
            if command == "lint" {
                // The lint report also gains one info finding, first.
                let findings = without["findings"].as_array_mut().unwrap();
                let finding = findings.remove(0);
                assert_eq!(finding["code"], "catalog.project_art");
                assert_eq!(finding["severity"], "info");
                assert_eq!(finding["location"], json!({"path": "preview.jsonc"}));
                let message = finding["message"].as_str().unwrap();
                assert!(
                    message.starts_with(&format!(
                        "The editor catalog has {} resource(s) with the license project",
                        project.len()
                    )),
                    "{message}"
                );
                assert!(message.contains("visible mark"), "{message}");
                assert_eq!(finding["detail"], Value::Null);
                let info = before["counts"]["info"].as_u64().unwrap();
                assert_eq!(report["counts"]["info"], info + 1);
                before["counts"]["info"] = json!(info + 1);
            }
            assert_eq!(without, before, "{command}");
            // The text form says it too.
            let text = json_of(command, &root, true);
            assert!(text.contains("project_art"), "{command}: {text}");
        }
        let (_, report) = draw(&root, 1);
        assert_eq!(report["catalog"]["project_art"], expected);
    }
}

#[test]
fn a_render_with_project_art_is_the_same_image_with_the_mark_below_it() {
    let (_plain_directory, plain) = original();
    let (_directory, root) = labelled("tilewright.editor-preview/2", &["grass"]);
    for scale in [1, 2, 3] {
        let (unmarked, before) = draw(&plain, scale);
        let (frame, report) = draw(&root, scale);
        let rows = mark::rows(unmarked.width);
        assert!(rows >= 12);
        assert_eq!(frame.width, unmarked.width);
        assert_eq!(frame.height, unmarked.height + rows);
        // The map part is the unmarked image, pixel for pixel, and the
        // strip is exactly the mark.
        assert_eq!(
            frame.rgba8[..unmarked.rgba8.len()],
            unmarked.rgba8[..],
            "{scale}"
        );
        assert_eq!(Some(&frame), mark::marked(&unmarked).as_ref());
        assert_ne!(
            frame.rgba8[unmarked.rgba8.len()..],
            vec![frame.rgba8[unmarked.rgba8.len()]; frame.rgba8.len() - unmarked.rgba8.len()][..]
        );
        // The report's image is the marked one, and says which rows are
        // the mark; everything else is as before.
        assert_eq!(report["image"]["height"], frame.height);
        assert_eq!(report["image"]["width"], frame.width);
        assert_eq!(report["image"]["mark_rows"], rows);
        let mut expected = before.clone();
        expected["image"]["height"] = json!(frame.height);
        expected["image"]["mark_rows"] = json!(rows);
        expected["catalog"]["project_art"] = json!({"resources": 1});
        assert_eq!(report, expected);
        assert!(before["image"].get("mark_rows").is_none());
    }
}

#[test]
fn the_marked_image_keeps_within_the_image_bounds() {
    let (_directory, root) = labelled("tilewright.editor-preview/2", &["grass"]);
    let (_plain_directory, plain) = original();
    let request = |root: &Path, scale: u32| Request {
        root: root.into(),
        out: "unused.png".into(),
        region: None,
        scale,
        json: true,
    };
    // The largest scale whose unmarked image fits; the marked one must fit
    // too, or be refused as over budget, never drawn past the bounds.
    let mut checked = 0;
    for scale in 1..=tilewright_editor_observe::limits::MAXIMUM_RENDER_SCALE {
        let observed = render::open(&request(&plain, scale)).unwrap();
        let Ok(unmarked) = render::frame_view(&observed, None, scale) else {
            continue;
        };
        let [width, height] = unmarked.size();
        assert_eq!(unmarked.mark_rows(), None);
        let observed = render::open(&request(&root, scale)).unwrap();
        let marked = render::frame_view(&observed, None, scale);
        let total = height + mark::rows(width);
        let fits = total <= tilewright_editor_observe::limits::MAXIMUM_RENDER_SIDE
            && u64::from(width) * u64::from(total)
                <= tilewright_editor_observe::limits::MAXIMUM_RENDER_PIXELS;
        match marked {
            Ok(framed) => {
                assert!(fits, "{scale}");
                assert_eq!(framed.size(), [width, total]);
                assert_eq!(framed.mark_rows(), Some(mark::rows(width)));
                checked += 1;
            }
            Err(refusal) => {
                assert!(!fits, "{scale}");
                assert_eq!(refusal.code, "output.budget");
            }
        }
    }
    assert!(checked > 0);
}

/// A copy of the neutral sample with its marker drawn `height` pixels tall,
/// in format 2 with grass labelled `project` when `project` is set.
fn tall(height: u32, project: bool) -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = labelled("tilewright.editor-preview/2", &[]);
    tall_marker(&root, height);
    if project {
        label_project_art(&root, &["grass"]);
    }
    (directory, root)
}

fn framed_size(root: &Path, scale: u32) -> Result<([u32; 2], Option<u32>), &'static str> {
    let request = Request {
        root: root.into(),
        out: "unused.png".into(),
        region: None,
        scale,
        json: true,
    };
    let observed = render::open(&request).unwrap_or_else(|outcome| panic!("{outcome:?}"));
    render::frame_view(&observed, None, scale)
        .map(|framed| (framed.size(), framed.mark_rows()))
        .map_err(|refusal| refusal.code)
}

#[test]
fn a_marked_image_past_the_side_bound_is_refused_and_one_within_it_is_not() {
    use tilewright_editor_observe::limits::MAXIMUM_RENDER_SIDE;
    // A taller marker makes a taller image, 2 pixels for each pixel of art
    // (the frame is centred). Find the art height whose unmarked image
    // ends just below the bound.
    let height_for = |art: u32| framed_size(&tall(art, false).1, 1).unwrap().0[1];
    let base = height_for(1_000);
    let art = 1_000 + (MAXIMUM_RENDER_SIDE - 2 - base) / 2;
    let ([width, unmarked], None) = framed_size(&tall(art, false).1, 1).unwrap() else {
        panic!("unmarked");
    };
    assert!(
        (MAXIMUM_RENDER_SIDE - 4..=MAXIMUM_RENDER_SIDE).contains(&unmarked),
        "{unmarked}"
    );
    assert_eq!(mark::rows(width), 12);
    // Unmarked it fits; marked, 12 rows more, it does not.
    assert_eq!(framed_size(&tall(art, true).1, 1), Err("output.budget"));
    // With room for the mark it is drawn, the mark included in the size.
    let art = art - 6;
    let ([width, unmarked], None) = framed_size(&tall(art, false).1, 1).unwrap() else {
        panic!("unmarked");
    };
    assert!(unmarked + 12 <= MAXIMUM_RENDER_SIDE, "{unmarked}");
    assert_eq!(
        framed_size(&tall(art, true).1, 1),
        Ok(([width, unmarked + 12], Some(12)))
    );
}

/// Labels resources `ids` of the world copy at `root` as project art (see
/// `tilewright_editor_testkit::project_art_catalog`).
fn label_project_art(root: &std::path::Path, ids: &[&str]) {
    let path = root.join("preview.jsonc");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        tilewright_editor_testkit::project_art_catalog(&text, ids),
    )
    .unwrap();
}

/// Replaces the neutral sample's `marker` resource in the copy at `root`
/// with a generated 24 × `height` image.
fn tall_marker(root: &std::path::Path, height: u32) {
    let path = root.join("preview.jsonc");
    let text = std::fs::read_to_string(&path).unwrap();
    let (bytes, catalog) =
        tilewright_editor_testkit::generated_resource(&text, "marker", 24, height, [9, 9, 9, 255]);
    std::fs::write(root.join("preview-assets/marker.atx1"), bytes).unwrap();
    std::fs::write(path, catalog).unwrap();
}
