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
    let path = root.join("preview.jsonc");
    let mut catalog: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    catalog["format"] = json!(format);
    let mut found = 0;
    for resource in catalog["resources"].as_array_mut().unwrap() {
        assert_eq!(resource["license"], "MIT");
        if project.contains(&resource["id"].as_str().unwrap()) {
            resource["license"] = json!("project");
            found += 1;
        }
    }
    assert_eq!(found, project.len());
    std::fs::write(path, serde_json::to_string_pretty(&catalog).unwrap()).unwrap();
    (directory, root)
}

/// The neutral sample's own catalog, format 1, in a copy.
fn original() -> (tempfile::TempDir, PathBuf) {
    labelled("tilewright.editor-preview/1", &[])
}

fn json_of(command: &str, root: &Path, text: bool) -> String {
    let mut arguments = vec![OsString::from(root)];
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

const COMMANDS: [&str; 3] = ["overview", "catalog", "lint"];

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
