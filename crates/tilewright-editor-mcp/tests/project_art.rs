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
    label_project_art(&root, project);
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
        tall_marker(&root, art);
        if project {
            label_project_art(&root, &["grass"]);
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

/// An encoder that keeps the pixels: width and height (little-endian u32)
/// then the RGBA8 bytes, so a test can compare the image itself.
#[allow(clippy::unnecessary_wraps)]
fn raw_encode(frame: &tilewright_editor_preview::ReferenceFrame) -> Option<(Vec<u8>, [u8; 32])> {
    let mut bytes = frame.width.to_le_bytes().to_vec();
    bytes.extend(frame.height.to_le_bytes());
    bytes.extend(&frame.rgba8);
    Some((bytes, [7; 32]))
}

fn raw_image(root: &Path, region: [i64; 4]) -> (tilewright_editor_preview::ReferenceFrame, Value) {
    let mut server = tilewright_editor_mcp::Server::open(root, "0.0.0-test", raw_encode);
    let called = common::call(&mut server, "render", &json!({"region": region}));
    let image = &called.reply["result"]["content"][1];
    assert_eq!(image["type"], "image");
    let text = image["data"].as_str().unwrap();
    let bytes = decode_base64(text);
    let width = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    (
        tilewright_editor_preview::ReferenceFrame {
            width,
            height,
            rgba8: bytes[8..].to_vec(),
        },
        called.structured.unwrap(),
    )
}

fn decode_base64(text: &str) -> Vec<u8> {
    let value = |byte: u8| -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {byte}"),
        }
    };
    let mut bytes = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let data: Vec<u8> = chunk.iter().copied().filter(|byte| *byte != b'=').collect();
        let group = data.iter().enumerate().fold(0_u32, |group, (index, byte)| {
            group | value(*byte) << (18 - 6 * index)
        });
        for index in 0..data.len().saturating_sub(1) {
            bytes.push(u8::try_from((group >> (16 - 8 * index)) & 0xff).unwrap());
        }
    }
    bytes
}

#[test]
fn the_render_image_is_the_marked_image() {
    let (_plain_directory, plain) = labelled(&[]);
    let (_directory, root) = labelled(&["grass"]);
    // A region as wide as the mark needs, and a one-tile one that is
    // widened for it.
    for (region, columns) in [([0, 0, 4, 3], false), ([3, 2, 1, 1], true)] {
        let (unmarked, before) = raw_image(&plain, region);
        let (image, report) = raw_image(&root, region);
        assert_eq!(
            Some(&image),
            tilewright_editor_preview::mark::marked(&unmarked).as_ref(),
            "{region:?}"
        );
        assert_ne!(image, unmarked);
        assert_eq!(report["image"]["width"], image.width);
        assert_eq!(report["image"]["height"], image.height);
        assert_eq!(report["image"].get("mark_columns").is_some(), columns);
        assert!(before["image"].get("mark_rows").is_none());
    }
}
