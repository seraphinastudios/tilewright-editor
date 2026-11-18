// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The sandbox server's observe tools on a world whose catalog contains
//! project art: the same members and the marked `render`. No project art is
//! used: the MIT neutral sample is labelled `project` in the sandbox's
//! temporary copy only. Linux only, as the sandbox server tests are.
#![cfg(target_os = "linux")]

mod common;

use common::Layout;
use serde_json::{Value, json};

fn label(world: &std::path::Path) {
    let path = world.join("preview.jsonc");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        tilewright_editor_testkit::project_art_catalog(&text, &["grass"]),
    )
    .unwrap();
}

fn render(layout: &Layout) -> (Value, String) {
    let mut client = layout.open(None);
    let called = client.call("render", &json!({}));
    let report = called.ok().clone();
    // `fake_encode` writes the image's size as its bytes.
    let image = called.result()["content"][1]["data"]
        .as_str()
        .unwrap()
        .to_owned();
    (report, image)
}

#[test]
fn a_sandbox_render_with_project_art_is_marked() {
    let plain = Layout::new(|_| {});
    let labelled = Layout::new(label);
    let (before, before_image) = render(&plain);
    let (after, after_image) = render(&labelled);
    let width = after["image"]["width"].as_u64().unwrap();
    let rows = tilewright_editor_preview::mark::rows(u32::try_from(width).unwrap());
    assert_eq!(after["catalog"]["project_art"], json!({"resources": 1}));
    assert_eq!(after["image"]["mark_rows"], rows);
    assert_eq!(
        after["image"]["height"],
        before["image"]["height"].as_u64().unwrap() + u64::from(rows)
    );
    assert!(before["catalog"].get("project_art").is_none());
    assert!(before["image"].get("mark_rows").is_none());
    assert_ne!(before_image, after_image);
    // The lint report says so too.
    let mut client = labelled.open(None);
    let lint = client.simple("lint");
    assert_eq!(lint.ok()["findings"][0]["code"], "catalog.project_art");
}
