// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `catalog` on a world whose catalog has scatter palettes
//! (`editing.palettes`): listed within the tool's output schema, kept by
//! name under `match`, and absent for a catalog without them.
mod common;

use serde_json::json;

#[test]
fn catalog_lists_scatter_palettes_only_when_the_catalog_has_them() {
    let (_plain_directory, plain) = common::fixture_copy("prototype-neutral");
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    common::edit(&root, "preview.jsonc", |catalog| {
        catalog["editing"]["palettes"] = json!([
            {"name": "speckle", "grid": "floor", "entries": [
                {"value": "sand", "weight": 1}, {"value": "grass", "weight": 3}]},
            {"name": "dunes", "grid": "floor", "entries": [{"value": "sand", "weight": 2}]},
        ]);
    });
    // `common::call` checks each result against the tool's output schema.
    let mut server = common::server(&plain);
    let before = common::call(&mut server, "catalog", &json!({}))
        .structured()
        .clone();
    assert!(before.get("palettes").is_none());
    assert!(before.get("palettes_more").is_none());
    let mut server = common::server(&root);
    let after = common::call(&mut server, "catalog", &json!({}))
        .structured()
        .clone();
    assert_eq!(
        after["palettes"],
        json!([
            {"name": "dunes", "grid": "floor",
                "entries": [{"value": "sand", "weight": 2}], "entries_more": 0},
            {"name": "speckle", "grid": "floor",
                "entries": [{"value": "grass", "weight": 3}, {"value": "sand", "weight": 1}],
                "entries_more": 0},
        ])
    );
    assert_eq!(after["palettes_more"], 0);
    // Everything else is the report of the catalog without palettes.
    for key in ["prototypes", "terrain_palette", "zone_palette"] {
        assert_eq!(after[key], before[key], "{key}");
    }
    let matched = common::call(&mut server, "catalog", &json!({"match": "SPECK"}))
        .structured()
        .clone();
    assert_eq!(matched["palettes"][0]["name"], "speckle");
    assert_eq!(matched["palettes"].as_array().unwrap().len(), 1);
}
