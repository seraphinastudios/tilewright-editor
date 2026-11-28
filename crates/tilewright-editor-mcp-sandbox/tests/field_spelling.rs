// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! An agent's `set_field` on a number stored as a whole number (`2`, not
//! `2.0`) prepares and publishes: the transaction expects the value as the
//! source spells it.
#![cfg(target_os = "linux")]

mod common;

use common::Layout;
use serde_json::json;
use std::{fs, path::Path};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

/// A number field `weight`, granted on `marker_1` and stored there as `2`.
fn whole_weight(root: &Path) {
    common::edit_json(root, "profile/schema/object.jsonc", |schema| {
        schema["fields"]["weight"] = json!({"type": "number"});
    });
    common::edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["fields"] = json!([{"uid": "marker_1", "field": "weight"}]);
    });
    let path = root.join(CHUNK);
    let mut chunk: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    chunk["objects"][0]["fields"] = json!("FIELDS");
    let text = serde_json::to_string_pretty(&chunk).unwrap().replacen(
        "\"FIELDS\"",
        "{\"label\": \"Trail marker\", \"weight\": 2}",
        1,
    );
    fs::write(path, text).unwrap();
}

#[test]
fn an_agent_changes_a_number_stored_as_a_whole_number() {
    let layout = Layout::new(whole_weight);
    let path = layout.path("world").join(CHUNK);
    let original = fs::read_to_string(&path).unwrap();
    let mut client = layout.open(None);
    client
        .edit(json!({"kind": "set_field", "uid": "marker_1", "field": "weight", "value": 1.25}))
        .ok();
    client.simple("accept").ok();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original.replacen("\"weight\": 2", "\"weight\": 1.25", 1)
    );
}
