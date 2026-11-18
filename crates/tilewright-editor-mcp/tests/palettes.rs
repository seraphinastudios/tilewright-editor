// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `catalog` on a world whose catalog has scatter palettes
//! (`editing.palettes`): listed within the tool's output schema, kept by
//! name under `match`, absent for a catalog without them, and paged like
//! the other lists so that a result always fits its byte budget.
mod common;

use serde_json::{Value, json};
use std::{fs, path::Path};

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
    assert_eq!(after["palettes_cursor"], Value::Null);
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

/// Give the copy at `root` `count` more terrain prototypes, granted on
/// `floor`, with ids of `length` bytes; returns their ids.
fn many_values(root: &Path, count: usize, length: usize) -> Vec<String> {
    let ids: Vec<String> = (0..count)
        .map(|index| format!("v{index:04}{}", "x".repeat(length - 5)))
        .collect();
    for id in &ids {
        fs::write(
            root.join(format!("defs/proto/{id}.jsonc")),
            json!({"format": "tilewright.def/1", "type": "proto", "id": id, "fields": {}})
                .to_string(),
        )
        .unwrap();
    }
    common::edit(root, "preview.jsonc", |catalog| {
        for id in &ids {
            catalog["prototypes"]
                .as_array_mut()
                .unwrap()
                .push(json!({"proto": id, "resource": "grass", "anchor": "ground"}));
            catalog["editing"]["terrain"][0]["values"]
                .as_array_mut()
                .unwrap()
                .push(json!(id));
        }
    });
    ids
}

/// 32 palettes of 32 entries each, with 64-byte names: the catalog bounds.
fn largest_palettes(root: &Path, values: &[String]) {
    let entries: Vec<Value> = values[..32]
        .iter()
        .map(|id| json!({"value": id, "weight": 10_000}))
        .collect();
    common::edit(root, "preview.jsonc", |catalog| {
        catalog["editing"]["palettes"] = (0..32)
            .map(|index| {
                json!({"name": format!("p{index:02}{}", "n".repeat(61)), "grid": "floor",
                    "entries": entries})
            })
            .collect();
    });
}

#[test]
fn palettes_at_the_catalog_bounds_page_within_the_budget() {
    // 128-byte values, World's longest identifiers.
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    let values = many_values(&root, 32, 128);
    largest_palettes(&root, &values);
    let mut server = common::server(&root);
    for arguments in [json!({}), json!({"limit": 1}), json!({"limit": 500})] {
        let called = common::call(&mut server, "catalog", &arguments);
        assert!(!called.is_error(), "{arguments}: {}", called.error_text());
        let report = called.structured();
        let listed = report["palettes"].as_array().unwrap().len();
        assert!(listed >= 1, "{arguments}");
        assert_eq!(
            listed + usize::try_from(report["palettes_more"].as_u64().unwrap()).unwrap(),
            32
        );
        assert_eq!(report["palettes_cursor"].is_null(), listed == 32);
    }
    // Paging the section gives every palette whole, in name order.
    let (items, calls) = common::page_through(&mut server, "catalog", &json!({}), "palettes", 50);
    assert_eq!(items.len(), 32);
    assert!(calls > 1, "the budget splits the list");
    for (index, item) in items.iter().enumerate() {
        assert!(
            item["name"]
                .as_str()
                .unwrap()
                .starts_with(&format!("p{index:02}"))
        );
        assert_eq!(item["entries"].as_array().unwrap().len(), 32);
        assert_eq!(item["entries_more"], 0);
    }
}

#[test]
fn palettes_after_a_large_prototype_list_still_fit_at_the_default_limit() {
    // 600 prototypes with 64-byte ids fill the lists before the palettes.
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    let values = many_values(&root, 600, 64);
    largest_palettes(&root, &values);
    let mut server = common::server(&root);
    let called = common::call(&mut server, "catalog", &json!({}));
    assert!(!called.is_error(), "{}", called.error_text());
    let report = called.structured();
    let listed = report["palettes"].as_array().unwrap().len();
    assert_eq!(
        listed + usize::try_from(report["palettes_more"].as_u64().unwrap()).unwrap(),
        32
    );
    assert!(listed < 32 && report["palettes_cursor"].is_string());
    let (items, _) = common::page_through(&mut server, "catalog", &json!({}), "palettes", 50);
    assert_eq!(items.len(), 32);
}

#[test]
fn a_catalog_without_palettes_has_an_empty_palettes_section() {
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    let mut server = common::server(&root);
    let page = common::call(&mut server, "catalog", &json!({"section": "palettes"}))
        .structured()
        .clone();
    assert_eq!(page["total"], 0);
    assert_eq!(page["items"], json!([]));
    assert_eq!(page["next_cursor"], Value::Null);
}

#[test]
fn a_report_lists_all_32_palettes_when_they_fit() {
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    common::edit(&root, "preview.jsonc", |catalog| {
        catalog["editing"]["palettes"] = (0..32)
            .map(|index| {
                json!({"name": format!("p{index:02}"), "grid": "floor",
                    "entries": [{"value": "grass", "weight": 1}]})
            })
            .collect();
    });
    let mut server = common::server(&root);
    let report = common::call(&mut server, "catalog", &json!({"limit": 500}))
        .structured()
        .clone();
    assert_eq!(report["palettes"].as_array().unwrap().len(), 32);
    assert_eq!(report["palettes_more"], 0);
    assert_eq!(report["palettes_cursor"], Value::Null);
}
