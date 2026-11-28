// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Lint and the catalog's stamps (`editing.stamps`): a malformed section
//! refuses the catalog, which lint reports as `catalog.invalid` with the
//! loader's words, and a stamp no tile of the view can anchor with every
//! object inside is a `catalog.stamp_unplaceable` warning. Each test edits a temporary copy of
//! the MIT neutral sample (view 8 x 6); the committed sample has no stamps
//! and stays unchanged.
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{self, EXIT_SUCCESS};
use tilewright_editor_testkit::fixture_world;

const CODE: &str = "catalog.stamp_unplaceable";

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

/// A copy of the neutral sample whose catalog has `stamps`.
fn sample(stamps: Value) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    let path = root.join("preview.jsonc");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    catalog["editing"]["stamps"] = stamps;
    std::fs::write(&path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    (directory, root)
}

fn lint(root: &Path) -> (u8, Value) {
    let outcome = cli::run("lint", &[OsString::from("--json"), OsString::from(root)]);
    (outcome.code, serde_json::from_str(&outcome.stdout).unwrap())
}

fn findings<'a>(report: &'a Value, code: &str) -> Vec<&'a Value> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == code)
        .collect()
}

/// A stamp of crates on the tiles `offsets`.
fn stamp(name: &str, offsets: &[[i64; 2]]) -> Value {
    json!({
        "name": name,
        "objects": offsets
            .iter()
            .map(|offset| json!({"proto": "crate", "offset": offset}))
            .collect::<Vec<_>>(),
    })
}

#[test]
fn a_stamp_no_tile_of_the_view_can_anchor_whole_is_a_warning() {
    let (_directory, root) = sample(json!([
        // 8 x 1 fits the 8 x 6 view as it is, anchored at (0, y).
        stamp("fence", &[[0, 0], [7, 0]]),
        // Exactly the view, from either end.
        stamp("exact", &[[0, 0], [7, 5]]),
        stamp("backward", &[[-7, -5], [0, 0]]),
        // 6 x 8 fits once turned (8 x 6); a mirror keeps the condition.
        stamp("tall", &[[0, -4], [5, 3]]),
        // Six rows south: turned, six columns west, which fits from x = 6.
        stamp("south", &[[0, 6]]),
        // 9 x 1, turned 1 x 9: too wide and too tall.
        stamp("long", &[[-4, 0], [4, 0]]),
        // 7 x 7 either way.
        stamp("square", &[[0, 0], [6, 6], [3, 3]]),
        // One object eight tiles away: no tile of an 8 x 6 view reaches it.
        stamp("east", &[[8, 0]]),
        stamp("down", &[[0, 8]]),
        stamp("far", &[[4000, -4000]]),
        // Two objects that fit in span but not in reach.
        stamp("reach", &[[1, 0], [8, 0]]),
    ]));
    let (code, report) = lint(&root);
    assert_eq!(code, EXIT_SUCCESS, "only errors change the exit code");
    let found = findings(&report, CODE);
    let names: Vec<&str> = found
        .iter()
        .map(|finding| finding["location"]["stamp"].as_str().unwrap())
        .collect();
    // In name order, the catalog's.
    assert_eq!(names, ["down", "east", "far", "long", "reach", "square"]);
    assert!(found.iter().all(|finding| finding["severity"] == "warning"));
    let long = found[3];
    assert_eq!(
        long["location"],
        json!({"path": "preview.jsonc", "stamp": "long"})
    );
    assert_eq!(
        long["message"],
        "No tile of the 8 x 6 view can anchor the stamp with every object inside the view, in any turn: its offsets run from -4 to 4 across and from 0 to 0 down, so it can never be placed whole."
    );
    assert!(
        found[0]["message"]
            .as_str()
            .unwrap()
            .contains("from 0 to 0 across and from 8 to 8 down")
    );
    // Nothing else is reported about the stamps: the catalog loads.
    assert!(findings(&report, "catalog.invalid").is_empty());
}

#[test]
fn a_malformed_stamps_section_is_catalog_invalid_with_the_loaders_words() {
    for (stamps, detail) in [
        (
            json!([stamp("camp", &[[0, 0]]), stamp("camp", &[[1, 0]])]),
            "stamp camp: duplicate stamp name",
        ),
        (
            json!([{"name": "camp", "objects": [{"proto": "grass", "offset": [0, 0]}]}]),
            "stamp camp object 1: stamp prototype is not granted by editing.objects",
        ),
        (
            json!([stamp("camp", &[[0, 0], [4096, 0]])]),
            "stamp camp object 2: stamp offset must be two integers from -4095 to 4095",
        ),
        (
            json!([{"name": "camp", "objects": []}]),
            "stamp camp: a stamp holds 1 to 64 objects",
        ),
    ] {
        let (_directory, root) = sample(stamps);
        let (code, report) = lint(&root);
        assert_ne!(code, EXIT_SUCCESS);
        let invalid = findings(&report, "catalog.invalid");
        assert_eq!(invalid.len(), 1, "{report}");
        assert_eq!(invalid[0]["severity"], "error");
        assert_eq!(invalid[0]["detail"], detail, "{report}");
        assert!(findings(&report, CODE).is_empty());
    }
}
