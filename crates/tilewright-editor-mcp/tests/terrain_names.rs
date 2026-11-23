// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `catalog`'s terrain palette names, for each granted value, the terrain
//! it is in the world's terrain transitions: on a copy of the neutral
//! sample whose terrains are renamed, with a variant, a value in no
//! terrain and a grant on a grid that is not the terrain layer.
mod common;

use serde_json::json;
use std::path::Path;

/// Rename the sample's terrains (sand to `dune`, grass to `lawn`), give
/// `lawn` the variant `grass_edge_15`, and grant those values, `stone`
/// (in no terrain) and, on the `edge` grid, `grass` and `grass_edge_1`.
fn renamed_terrains(root: &Path) {
    common::edit(root, "profile/transitions.jsonc", |transitions| {
        let terrains = transitions["terrains"].as_array_mut().unwrap();
        terrains[0]["id"] = json!("dune");
        terrains[1]["id"] = json!("lawn");
        terrains[1]["protos"] = json!(["grass", "grass_edge_15"]);
        terrains[1]["transitions"][0]["with"] = json!(["dune"]);
    });
    common::edit(root, "preview.jsonc", |catalog| {
        catalog["editing"]["terrain"] = json!([
            {"grid": "floor", "values": ["sand", "grass", "grass_edge_15", "stone"]},
            {"grid": "edge", "values": ["grass", "grass_edge_1"]},
        ]);
        catalog["editing"]
            .as_object_mut()
            .unwrap()
            .remove("palettes");
    });
}

#[test]
fn catalog_names_the_terrain_of_each_granted_value() {
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    renamed_terrains(&root);
    // `common::call` checks each result against the tool's output schema.
    let mut server = common::server(&root);
    let report = common::call(&mut server, "catalog", &json!({}))
        .structured()
        .clone();
    assert_eq!(
        report["terrain_palette"],
        json!([
            // `grass` is in `lawn` only on the terrain layer.
            {"grid": "edge", "value": "grass"},
            {"grid": "edge", "value": "grass_edge_1"},
            {"grid": "floor", "value": "grass", "terrain": "lawn"},
            {"grid": "floor", "value": "grass_edge_15", "terrain": "lawn", "variant": true},
            {"grid": "floor", "value": "sand", "terrain": "dune"},
            {"grid": "floor", "value": "stone"},
        ])
    );
    // `match` still keeps values by their own id, not by their terrain.
    let matched = common::call(&mut server, "catalog", &json!({"match": "lawn"}))
        .structured()
        .clone();
    assert_eq!(matched["terrain_palette"], json!([]));
    let matched = common::call(&mut server, "catalog", &json!({"match": "SAN"}))
        .structured()
        .clone();
    assert_eq!(
        matched["terrain_palette"],
        json!([{"grid": "floor", "value": "sand", "terrain": "dune"}])
    );
}

#[test]
fn a_world_without_transitions_names_no_terrain() {
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    common::edit(&root, "profile/profile.jsonc", |profile| {
        profile.as_object_mut().unwrap().remove("transitions");
    });
    let mut server = common::server(&root);
    let report = common::call(&mut server, "catalog", &json!({}))
        .structured()
        .clone();
    assert_eq!(
        report["terrain_palette"],
        json!([
            {"grid": "floor", "value": "grass"},
            {"grid": "floor", "value": "sand"},
        ])
    );
    assert!(
        report["terrain_palette"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("terrain").is_none() && row.get("variant").is_none()),
        "{report:?}"
    );
}
