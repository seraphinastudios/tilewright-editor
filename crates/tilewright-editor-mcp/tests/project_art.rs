// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The server on a world whose catalog contains project art: every report
//! states it within its output schema, `render` is served with the mark,
//! and the mark's rows count against the image caps.
//!
//! No project art is used: the MIT neutral sample is labelled `project` in
//! a temporary copy's catalog only.
mod common;

use serde_json::{Value, json};
use std::path::Path;

fn labelled(project: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
    let (directory, root) = common::fixture_copy("prototype-neutral");
    tilewright_editor_testkit::label_project_art(&root, project);
    (directory, root)
}

fn structured(root: &Path, tool: &str, arguments: &Value) -> Value {
    let mut server = common::server(root);
    common::call(&mut server, tool, arguments)
        .structured
        .unwrap_or_else(|| panic!("{tool} failed"))
}

const CALLS: [(&str, &str); 5] = [
    ("overview", "{}"),
    ("tile", r#"{"plane": "main", "layer": 0, "x": 3, "y": 2}"#),
    ("catalog", "{}"),
    ("lint", "{}"),
    ("render", "{}"),
];

#[test]
fn every_report_states_project_art_within_its_schema() {
    let (_plain_directory, plain) = labelled(&[]);
    let (_directory, root) = labelled(&["grass", "bush"]);
    for (tool, arguments) in CALLS {
        let arguments: Value = serde_json::from_str(arguments).unwrap();
        // `common::call` checks each result against the tool's output schema.
        let before = structured(&plain, tool, &arguments);
        let after = structured(&root, tool, &arguments);
        assert!(before["catalog"].get("project_art").is_none(), "{tool}");
        assert_eq!(
            after["catalog"]["project_art"],
            json!({"resources": 2}),
            "{tool}"
        );
        if tool == "lint" {
            assert_eq!(after["findings"][0]["code"], "catalog.project_art");
            assert_eq!(
                after["counts"]["info"],
                before["counts"]["info"].as_u64().unwrap() + 1
            );
        }
        if tool == "render" {
            let rows = tilewright_editor_preview::mark::rows(
                u32::try_from(after["image"]["width"].as_u64().unwrap()).unwrap(),
            );
            assert_eq!(after["image"]["mark_rows"], rows);
            assert_eq!(
                after["image"]["height"],
                before["image"]["height"].as_u64().unwrap() + u64::from(rows)
            );
            assert!(before["image"].get("mark_rows").is_none());
        }
    }
}

#[test]
fn the_description_and_instructions_say_who_is_responsible() {
    let (_directory, root) = labelled(&[]);
    let mut server = common::server(&root);
    let replies = common::session(
        &mut server,
        &[
            common::request(1, "server/discover", json!({})),
            common::request(2, "tools/list", json!({})),
        ],
    );
    let instructions = replies[0].1["result"]["instructions"].as_str().unwrap();
    let render = replies[1].1["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "render")
        .unwrap()["description"]
        .as_str()
        .unwrap();
    for text in [instructions, render] {
        assert!(text.contains("project art"), "{text}");
        assert!(text.contains("the user is responsible"), "{text}");
        assert!(text.contains("model provider"), "{text}");
    }
}

#[test]
fn the_marks_rows_count_against_the_image_caps() {
    let side = tilewright_editor_mcp::MAXIMUM_IMAGE_SIDE;
    let tall = |art: u32, project: bool| {
        let (directory, root) = labelled(&[]);
        tilewright_editor_testkit::replace_resource(&root, "marker", 24, art, [9, 9, 9, 255]);
        if project {
            tilewright_editor_testkit::label_project_art(&root, &["grass"]);
        }
        (directory, root)
    };
    let call = |root: &Path| {
        let mut server = common::server(root);
        common::call(&mut server, "render", &json!({}))
    };
    let height = |root: &Path| {
        call(root).structured.unwrap()["image"]["height"]
            .as_u64()
            .unwrap()
    };
    // 2 image pixels per pixel of art: find the art whose unmarked image
    // ends just below the side cap.
    let base = height(&tall(200, false).1);
    let art = 200 + (u64::from(side) - 2 - base) / 2;
    let art = u32::try_from(art).unwrap();
    let unmarked = height(&tall(art, false).1);
    assert!(
        (u64::from(side) - 4..=u64::from(side)).contains(&unmarked),
        "{unmarked}"
    );
    let refused = call(&tall(art, true).1);
    assert!(refused.structured.is_none());
    let message = refused.reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        message.starts_with("render failed: output.budget: "),
        "{message}"
    );
    let fits = call(&tall(art - 6, true).1);
    let report = fits.structured.unwrap();
    assert_eq!(report["image"]["mark_rows"], 12);
    assert!(report["image"]["height"].as_u64().unwrap() <= u64::from(side));
}
