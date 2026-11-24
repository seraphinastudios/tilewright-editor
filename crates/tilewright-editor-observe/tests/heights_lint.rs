// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Lint's `catalog.heights_undrawable` warning, checked against the real
//! preview: a grant just inside the bound draws its worst pattern at the
//! largest zoom, and one just outside does not.
//!
//! Each test copies the MIT neutral sample to a temporary directory and
//! makes that copy isometric (a 32 x 16 diamond under its 32 x 32 ground
//! art), with a height grant and generated heights; the committed sample
//! has neither and stays unchanged.
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{self, EXIT_SUCCESS};
use tilewright_editor_observe::world::{Catalog, Observed};
use tilewright_editor_preview::{LARGEST_ZOOM, PreviewLimits};
use tilewright_editor_testkit::fixture_world;

const CODE: &str = "catalog.heights_undrawable";

/// The largest difference the sample's 32 x 32 ground art can be bent over
/// on a 32 x 16 diamond at zoom 8 (ADR 0006 measured ±460).
const LARGEST: i32 = 921;

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

fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// A copy of the neutral sample, isometric when `isometric`, granting
/// heights `grant` when given.
fn sample(isometric: bool, grant: Option<[i32; 2]>) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    if isometric {
        edit_json(&root, "profile/profile.jsonc", |profile| {
            profile["projection"] =
                json!({"kind": "isometric", "tile_width": 32, "tile_height": 16});
        });
    }
    if let Some([min, max]) = grant {
        edit_json(&root, "preview.jsonc", |catalog| {
            catalog["editing"]["heights"] = json!({"min": min, "max": max});
        });
    }
    (directory, root)
}

/// Stores `high` and `low` in the worst 3 x 3 pattern of ADR 0006 around
/// tile (2, 2), inside the catalog's view, and 0 elsewhere.
fn worst_pattern(root: &Path, low: i32, high: i32) {
    let pattern = [[1, 1, -1], [1, -1, -1], [-1, -1, 1]];
    let rows: Vec<Vec<i32>> = (0..32_usize)
        .map(|y| {
            (0..32_usize)
                .map(|x| {
                    if (1..4).contains(&x) && (1..4).contains(&y) {
                        if pattern[y - 1][x - 1] == 1 {
                            high
                        } else {
                            low
                        }
                    } else {
                        0
                    }
                })
                .collect()
        })
        .collect();
    edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}

fn lint(root: &Path) -> Value {
    let outcome = cli::run("lint", &[OsString::from("--json"), OsString::from(root)]);
    assert_eq!(outcome.code, EXIT_SUCCESS, "{}", outcome.stderr);
    serde_json::from_str(&outcome.stdout).unwrap()
}

fn findings<'a>(report: &'a Value, code: &str) -> Vec<&'a Value> {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == code)
        .collect()
}

/// Whether the preview prepares the catalog's view at `zoom`.
fn prepares(root: &Path, zoom: f32) -> Result<(), String> {
    let observed = Observed::open(root).unwrap();
    let Catalog::Loaded(loaded) = &observed.catalog else {
        panic!("the catalog does not load");
    };
    let (catalog, resources) = &**loaded;
    let mut spec = catalog.default_view(observed.world(), 1280, 800, 1);
    spec.camera.zoom = zoom;
    tilewright_editor_preview::prepare(
        observed.world(),
        catalog,
        resources,
        &spec,
        PreviewLimits::default(),
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

#[test]
fn a_grant_just_inside_the_bound_has_no_warning_and_its_worst_pattern_draws() {
    let (_plain_directory, plain) = sample(true, None);
    let without = lint(&plain);
    // From -460 to 461: 921 apart.
    let grant = [-460, LARGEST - 460];
    let (_directory, root) = sample(true, Some(grant));
    let report = lint(&root);
    assert!(findings(&report, CODE).is_empty());
    assert_eq!(report["counts"], without["counts"]);
    worst_pattern(&root, grant[0], grant[1]);
    prepares(&root, LARGEST_ZOOM).unwrap();
}

#[test]
fn a_grant_just_outside_the_bound_warns_and_its_worst_pattern_does_not_draw() {
    let (_plain_directory, plain) = sample(true, None);
    let without = lint(&plain);
    // From -460 to 462: 922 apart.
    let grant = [-460, LARGEST - 460 + 1];
    let (_directory, root) = sample(true, Some(grant));
    let report = lint(&root);
    let found = findings(&report, CODE);
    assert_eq!(found.len(), 1, "{report}");
    let finding = found[0];
    assert_eq!(finding["severity"], "warning");
    assert_eq!(finding["location"]["path"], "preview.jsonc");
    // Every ground resource of the sample is 32 x 32, in id order.
    let mut ground: Vec<String> = ["grass", "sand"]
        .into_iter()
        .map(String::from)
        .chain((1..=15).map(|n| format!("grass_edge_{n}")))
        .collect();
    ground.sort();
    assert_eq!(finding["location"]["resources"], json!(ground));
    assert!(finding.get("message_clipped").is_none());
    let message = finding["message"].as_str().unwrap();
    for part in [
        "Heights from -460 to 462 (and 0) can lie 922 apart",
        "at zoom 8",
        "32 x 32 ground art on a 32 x 16 diamond over more than 921.",
        "at most 921 wide with 0 in it, such as -460 to 461,",
    ] {
        assert!(message.contains(part), "{part}: {message}");
    }
    // A warning: lint still exits 0, and only the warning count changes.
    assert_eq!(report["counts"]["error"], without["counts"]["error"]);
    assert_eq!(report["counts"]["info"], without["counts"]["info"]);
    assert_eq!(
        report["counts"]["warning"].as_u64().unwrap(),
        without["counts"]["warning"].as_u64().unwrap() + 1
    );
    worst_pattern(&root, grant[0], grant[1]);
    let refused = prepares(&root, LARGEST_ZOOM).unwrap_err();
    assert_eq!(refused, "invalid world view");
    // The same heights draw at the opening zoom: the warning is about the
    // largest zoom.
    prepares(&root, 1.0).unwrap();
}

#[test]
fn zero_counts_as_a_height_of_every_grant() {
    // From 500 to 922 is 422 apart, but tiles without a height are at 0.
    let (_directory, root) = sample(true, Some([500, 922]));
    let report = lint(&root);
    let found = findings(&report, CODE);
    assert_eq!(found.len(), 1, "{report}");
    let message = found[0]["message"].as_str().unwrap();
    assert!(message.contains("lie 922 apart"), "{message}");
    assert!(message.contains("such as 0 to 921"), "{message}");
    // A lowering grant gets an example that ends at 0.
    let (_lower_directory, lower) = sample(true, Some([-1000, -10]));
    let report = lint(&lower);
    let message = findings(&report, CODE)[0]["message"].as_str().unwrap();
    assert!(message.contains("lie 1000 apart"), "{message}");
    assert!(message.contains("such as -921 to 0"), "{message}");
    // Just inside, with 0 at one end.
    let (_inside_directory, inside) = sample(true, Some([0, LARGEST]));
    assert!(findings(&lint(&inside), CODE).is_empty());
}

#[test]
fn top_down_ground_and_catalogs_without_a_grant_have_no_warning() {
    // Top-down ground shifts but never bends.
    let (_directory, root) = sample(false, Some([-1024, 1024]));
    assert!(findings(&lint(&root), CODE).is_empty());
    let (_iso_directory, iso) = sample(true, None);
    assert!(findings(&lint(&iso), CODE).is_empty());
    // The committed neutral sample itself.
    assert!(findings(&lint(&fixture_world("prototype-neutral")), CODE).is_empty());
}
