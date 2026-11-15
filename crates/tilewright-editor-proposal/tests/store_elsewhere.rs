// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Elsewhere than Linux the store refuses to open: its durable sequence
//! cannot be checked there.
#![cfg(not(target_os = "linux"))]

use tilewright_editor_proposal::ProposalStore;
use tilewright_editor_ui::RefusalCode;
use tilewright_source::SourceRoot;

#[test]
fn the_store_is_refused_off_linux() {
    let dir = tempfile::tempdir().unwrap();
    let error = ProposalStore::open(SourceRoot::new(dir.path()))
        .err()
        .unwrap();
    assert_eq!(error.code(), RefusalCode::ProposalStore);
    assert!(error.message().contains("Linux"), "{error}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
