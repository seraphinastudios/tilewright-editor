// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `lint` on a world whose height grant is steeper than the preview can
//! bend its art over (`catalog.heights_undrawable`): reported within the
//! tool's output schema, with the limiting art's ids in its location.
mod common;

use serde_json::json;

#[test]
fn lint_reports_an_undrawable_height_grant_within_its_schema() {
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    common::edit(&root, "profile/profile.jsonc", |profile| {
        profile["projection"] = json!({"kind": "isometric", "tile_width": 32, "tile_height": 16});
    });
    common::edit(&root, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = json!({"min": -460, "max": 462});
    });
    // `common::call` checks each result against the tool's output schema.
    let mut server = common::server(&root);
    let report = common::call(&mut server, "lint", &json!({}))
        .structured()
        .clone();
    let found: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == "catalog.heights_undrawable")
        .collect();
    assert_eq!(found.len(), 1, "{report}");
    assert_eq!(found[0]["severity"], "warning");
    let resources = found[0]["location"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 17);
    assert!(resources.contains(&json!("grass")));
}
