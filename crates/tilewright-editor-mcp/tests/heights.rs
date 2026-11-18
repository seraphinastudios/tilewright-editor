// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `overview` over MCP states a profile's movement limits within its output
//! schema, only when the profile has them, and `tile` its heights.
//!
//! The MIT neutral sample is changed in temporary copies only.
mod common;

use serde_json::{Value, json};
use std::path::Path;

fn structured(root: &Path, tool: &str, arguments: &Value) -> Value {
    let mut server = common::server(root);
    // `common::call` checks each result against the tool's output schema.
    common::call(&mut server, tool, arguments)
        .structured
        .unwrap_or_else(|| panic!("{tool} failed"))
}

#[test]
fn overview_states_movement_only_when_the_profile_has_it() {
    let (_plain_directory, plain) = common::fixture_copy("prototype-neutral");
    let before = structured(&plain, "overview", &json!({}));
    assert!(before["profile"].get("movement").is_none());
    for movement in [
        json!({"max_step": 4, "fall_unit": 8}),
        json!({"max_step": 0, "fall_unit": 1}),
        json!({"max_step": 4_294_967_295_u64, "fall_unit": 4_294_967_295_u64}),
    ] {
        let (_directory, root) = common::fixture_copy("prototype-neutral");
        common::edit(&root, "profile/profile.jsonc", |profile| {
            profile["movement"] = movement.clone();
        });
        let after = structured(&root, "overview", &json!({}));
        assert_eq!(after["profile"]["movement"], movement);
        let mut without = after.clone();
        without["profile"]
            .as_object_mut()
            .unwrap()
            .remove("movement");
        without["revision"] = before["revision"].clone();
        assert_eq!(without, before);
    }
}

#[test]
fn the_overview_schema_lists_movement_as_optional_and_closed() {
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    let mut server = common::server(&root);
    let replies = common::session(&mut server, &[common::request(1, "tools/list", json!({}))]);
    let overview = replies[0].1["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "overview")
        .unwrap()["outputSchema"]
        .clone();
    // The first branch is the report; the others are refusals.
    let profile = &overview["oneOf"][0]["properties"]["profile"];
    let required = profile["required"].as_array().unwrap();
    assert!(!required.contains(&json!("movement")));
    let movement = &profile["properties"]["movement"];
    assert_eq!(movement["additionalProperties"], false);
    assert_eq!(movement["required"], json!(["max_step", "fall_unit"]));
}

#[test]
fn tile_states_generated_heights() {
    let (_directory, root) = common::fixture_copy("prototype-neutral");
    common::edit(&root, "planes/main/layers/0/0_0.jsonc", |chunk| {
        let rows: Vec<Vec<i64>> = (0..32)
            .map(|y| {
                (0..32)
                    .map(|x| match (x, y) {
                        (0, 0) => i64::from(i32::MIN),
                        (1, 0) => i64::from(i32::MAX),
                        _ => x * 3 - y,
                    })
                    .collect()
            })
            .collect();
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
    for (x, y, height) in [
        (0, 0, json!(i32::MIN)),
        (1, 0, json!(i32::MAX)),
        (5, 2, json!(13)),
        (-1, 0, json!(0)),
        (40, 0, Value::Null),
    ] {
        let report = structured(
            &root,
            "tile",
            &json!({"plane": "main", "layer": 0, "x": x, "y": y}),
        );
        assert_eq!(report["height"], height, "{x},{y}");
    }
}
