// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The toolkit version the editor reports (`version`, and the prototype's
//! frame evidence) is the one the workspace pins, so a World bump that
//! forgets the labels fails here.
use tilewright_editor_document::{TOOLKIT_COMPATIBILITY, TOOLKIT_VERSION};

/// The exact version the workspace manifest pins for `crate_name`.
fn pinned(manifest: &str, crate_name: &str) -> String {
    let prefix = format!("{crate_name} = \"=");
    let line = manifest
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("{crate_name} has no exact pin"));
    line.split('"').next().unwrap().to_owned()
}

#[test]
fn the_reported_toolkit_version_is_the_pinned_one() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"),
    )
    .unwrap();
    let source = pinned(&manifest, "tilewright-source");
    // The World crates pin each other exactly, so they move together.
    assert_eq!(pinned(&manifest, "tilewright-edit"), source);
    assert_eq!(pinned(&manifest, "tilewright-jsonc"), source);
    assert_eq!(TOOLKIT_VERSION, source);
    assert_eq!(
        TOOLKIT_COMPATIBILITY,
        format!("tilewright.world/1 tilewright-source/{source}")
    );
}
