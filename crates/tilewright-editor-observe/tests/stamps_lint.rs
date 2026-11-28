// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Lint and the catalog's stamps (`editing.stamps`): a malformed section
//! refuses the catalog, which lint reports as `catalog.invalid` with the
//! loader's words, and a stamp that can never fit the view whole is a
//! `catalog.stamp_unplaceable` warning. Each test edits a temporary copy of
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
fn a_stamp_that_never_fits_the_view_whole_is_a_warning_and_one_that_fits_turned_is_not() {
    let (_directory, root) = sample(json!([
        // 8 x 1 fits the 8 x 6 view as it is.
        stamp("fence", &[[0, 0], [7, 0]]),
        // 6 x 8 fits once turned (8 x 6); a mirror keeps the span.
        stamp("tall", &[[0, -4], [5, 3]]),
        // 9 x 1, turned 1 x 9: too wide and too tall.
        stamp("long", &[[-4, 0], [4, 0]]),
        // 7 x 7 either way.
        stamp("square", &[[0, 0], [6, 6], [3, 3]]),
        // One object always fits.
        stamp("post", &[[4000, -4000]]),
    ]));
    let (code, report) = lint(&root);
    assert_eq!(code, EXIT_SUCCESS, "only errors change the exit code");
    let found = findings(&report, CODE);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0]["severity"], "warning");
    assert_eq!(
        found[0]["location"],
        json!({"path": "preview.jsonc", "stamp": "long"})
    );
    assert_eq!(
        found[0]["message"],
        "The stamp spans 9 x 1 tiles, more than the view's 8 x 6 in either orientation, so it can never be placed whole: every object must land in the view."
    );
    assert_eq!(found[1]["location"]["stamp"], "square");
    assert!(
        found[1]["message"]
            .as_str()
            .unwrap()
            .starts_with("The stamp spans 7 x 7 tiles")
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
