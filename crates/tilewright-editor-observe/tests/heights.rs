// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! What `overview` and `tile` say about heights and movement limits.
//!
//! Each test copies the MIT neutral sample to a temporary directory and
//! gives that copy generated heights or profile members; the committed
//! sample has neither and stays unchanged.
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{self, EXIT_SUCCESS};
use tilewright_editor_testkit::fixture_world;

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the neutral sample, changed by `change`.
fn sample(change: impl FnOnce(&Path)) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    change(&root);
    (directory, root)
}

fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn profile(root: &Path, change: impl FnOnce(&mut Value)) {
    edit_json(root, "profile/profile.jsonc", change);
}

/// Chunk `0_0`'s height grid, `height(x, y)` for each of its tiles.
fn heights(root: &Path, height: impl Fn(i64, i64) -> i64) {
    let rows: Vec<Vec<i64>> = (0..32)
        .map(|y| (0..32).map(|x| height(x, y)).collect())
        .collect();
    edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}

fn output(command: &str, root: &Path, extra: &[&str], json: bool) -> String {
    let mut arguments = vec![OsString::from(root)];
    arguments.extend(extra.iter().map(OsString::from));
    if json {
        arguments.insert(0, "--json".into());
    }
    let outcome = cli::run(command, &arguments);
    assert_eq!(outcome.code, EXIT_SUCCESS, "{command}: {}", outcome.stderr);
    outcome.stdout
}

fn parsed(command: &str, root: &Path, extra: &[&str]) -> Value {
    serde_json::from_str(&output(command, root, extra, true)).unwrap()
}

#[test]
fn overview_reports_movement_only_when_the_profile_has_it() {
    let (_plain_directory, plain) = sample(|_| {});
    let before = parsed("overview", &plain, &[]);
    assert!(before["profile"].get("movement").is_none());
    assert!(!output("overview", &plain, &[], false).contains("movement"));

    let (_directory, root) = sample(|root| {
        profile(root, |profile| {
            profile["movement"] = json!({"max_step": 4, "fall_unit": 8});
        });
    });
    let after = parsed("overview", &root, &[]);
    assert_eq!(
        after["profile"]["movement"],
        json!({"max_step": 4, "fall_unit": 8})
    );
    // Nothing else changes, and it comes right after layer_height.
    let mut without = after.clone();
    without["profile"]
        .as_object_mut()
        .unwrap()
        .remove("movement");
    without["revision"] = before["revision"].clone();
    assert_eq!(without, before);
    let keys: Vec<&String> = after["profile"].as_object().unwrap().keys().collect();
    let at = keys.iter().position(|key| *key == "movement").unwrap();
    assert_eq!(keys[at - 1], "layer_height");
    let text = output("overview", &root, &[], false);
    assert!(text.contains("movement"), "{text}");
    assert!(
        text.contains("max_step") && text.contains("fall_unit"),
        "{text}"
    );
}

#[test]
fn movement_is_reported_as_stored_at_its_bounds() {
    let (_directory, root) = sample(|root| {
        profile(root, |profile| {
            profile["movement"] = json!({"max_step": 0, "fall_unit": 4_294_967_295_u64});
            profile["layer_height"] = json!(65_535);
        });
    });
    let report = parsed("overview", &root, &[]);
    assert_eq!(
        report["profile"]["movement"],
        json!({"max_step": 0, "fall_unit": 4_294_967_295_u64})
    );
    assert_eq!(report["profile"]["layer_height"], 65_535);
}

#[test]
fn tile_reports_generated_and_extreme_heights() {
    let (_directory, root) = sample(|root| {
        heights(root, |x, y| match (x, y) {
            (0, 0) => i64::from(i32::MIN),
            (1, 0) => i64::from(i32::MAX),
            _ => x * 3 - y,
        });
    });
    for (x, y, height) in [
        ("0", "0", json!(i32::MIN)),
        ("1", "0", json!(i32::MAX)),
        ("5", "2", json!(13)),
        ("0", "4", json!(-4)),
    ] {
        let report = parsed("tile", &root, &["main", "0", x, y]);
        assert_eq!(report["height"], height, "{x},{y}");
    }
    // A chunk without a height grid (the sample's -1_0) holds 0.
    let report = parsed("tile", &root, &["main", "0", "-1", "0"]);
    assert_eq!(report["height"], 0);
    // A tile of a chunk that does not exist has no height.
    let report = parsed("tile", &root, &["main", "0", "40", "0"]);
    assert_eq!(report["height"], Value::Null);
}
