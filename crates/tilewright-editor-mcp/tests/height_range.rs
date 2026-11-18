// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `catalog` on a world whose catalog grants a height range
//! (`editing.heights`): reported within the tool's output schema, whatever
//! `match` says, and absent for a catalog without it, whose report is
//! otherwise the same.
mod common;

use serde_json::json;

#[test]
fn catalog_reports_the_height_range_only_when_the_catalog_grants_one() {
    let (_plain_directory, plain) = common::fixture_copy("prototype-neutral");
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    common::edit(&root, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = json!({"min": -512, "max": 7});
    });
    // `common::call` checks each result against the tool's output schema.
    let mut server = common::server(&plain);
    let before = common::call(&mut server, "catalog", &json!({}))
        .structured()
        .clone();
    assert!(before.get("heights").is_none());
    let mut server = common::server(&root);
    let mut after = common::call(&mut server, "catalog", &json!({}))
        .structured()
        .clone();
    assert_eq!(after["heights"], json!({"min": -512, "max": 7}));
    let matched = common::call(&mut server, "catalog", &json!({"match": "zzz"}))
        .structured()
        .clone();
    assert_eq!(matched["heights"], json!({"min": -512, "max": 7}));
    // Everything else is the report of the catalog without the range, but
    // for the catalog's own digest.
    after.as_object_mut().unwrap().remove("heights");
    for key in ["prototypes", "terrain_palette", "zone_palette", "palettes"] {
        assert_eq!(after[key], before[key], "{key}");
    }
}
