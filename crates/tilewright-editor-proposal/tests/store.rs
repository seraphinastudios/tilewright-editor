// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Proposal files on Linux checked storage.
#![cfg(target_os = "linux")]

mod common;

use common::{EDITOR, RECEIPT, Records, accept, edit, revision, sandbox, world};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};
use tilewright_editor_proposal::{Proposal, ProposalStore, limits, read_proposal};
use tilewright_editor_ui::RefusalCode;
use tilewright_source::SourceRoot;

/// Puts something at a proposal's name.
type Plant<'a> = Box<dyn Fn(&Path) + 'a>;

fn directory() -> tempfile::TempDir {
    let root = std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    let dir = tempfile::Builder::new()
        .prefix("tilewright-proposals-")
        .tempdir_in(root)
        .unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn proposal(kinds: usize) -> Proposal {
    let root = world(|_| {});
    let mut session = sandbox(root.path());
    let mut records = Records::default();
    for kind in 0..kinds {
        let expected = revision(&session);
        accept(&mut session, edit(kind, expected), &mut records);
    }
    Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn a_proposal_is_written_once_privately_and_read_back() {
    let dir = directory();
    let proposal = proposal(2);
    let mut store = ProposalStore::open(SourceRoot::new(dir.path()))
        .expect("requires a checked local test filesystem");
    // The directory's lock is held while the store is.
    let error = ProposalStore::open(SourceRoot::new(dir.path()))
        .err()
        .unwrap();
    assert_eq!(error.code(), RefusalCode::ProposalStore);
    let stored = store.retain(&proposal).unwrap();
    assert_eq!(stored.name, proposal.file_name());
    assert!(!stored.existed);
    let path = dir.path().join(&stored.name);
    assert_eq!(fs::read(&path).unwrap(), proposal.bytes());
    assert_eq!(mode(&path), 0o600);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    // The same proposal again is success, and writes nothing.
    assert!(store.retain(&proposal).unwrap().existed);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    let read = read_proposal(&SourceRoot::new(dir.path()), &stored.name).unwrap();
    assert_eq!(read, proposal);
}

#[test]
fn other_bytes_a_link_or_a_stage_at_the_name_are_refused_and_kept() {
    let proposal = proposal(1);
    let name = proposal.file_name();
    let target = directory();
    fs::write(target.path().join("elsewhere"), b"kept").unwrap();
    let plants: [(&str, Plant); 4] = [
        (
            "other bytes",
            Box::new(|p| fs::write(p.join(&name), b"other").unwrap()),
        ),
        (
            "link",
            Box::new(|p| symlink(target.path().join("elsewhere"), p.join(&name)).unwrap()),
        ),
        (
            "directory",
            Box::new(|p| fs::create_dir(p.join(&name)).unwrap()),
        ),
        (
            "stage",
            Box::new(|p| fs::write(p.join(format!("{name}.stage")), b"stage").unwrap()),
        ),
    ];
    for (what, plant) in plants {
        let dir = directory();
        plant(dir.path());
        let before: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let mut store = ProposalStore::open(SourceRoot::new(dir.path())).unwrap();
        let error = store.retain(&proposal).unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalStore, "{what}: {error}");
        let after: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after, "{what}");
    }
    assert_eq!(fs::read(target.path().join("elsewhere")).unwrap(), b"kept");
}

#[test]
fn reading_is_bounded_named_and_refuses_links() {
    let dir = directory();
    let proposal = proposal(1);
    let root = SourceRoot::new(dir.path());
    let name = proposal.file_name();
    let code = |name: &str| read_proposal(&root, name).unwrap_err().code();
    assert_eq!(code(&name), RefusalCode::ProposalStore);
    for bad in [
        "proposal.json",
        "../x.json",
        &name.to_uppercase(),
        &format!("{name}.stage"),
    ] {
        assert_eq!(code(bad), RefusalCode::ProposalInvalid, "{bad}");
    }
    // Over the bound: refused before reading.
    fs::write(dir.path().join(&name), vec![b' '; limits::BYTES + 1]).unwrap();
    assert_eq!(code(&name), RefusalCode::ProposalBudget);
    // A proposal under another proposal's name.
    let other = format!("proposal-{}.json", "0".repeat(64));
    fs::write(dir.path().join(&other), proposal.bytes()).unwrap();
    assert_eq!(code(&other), RefusalCode::ProposalDigest);
    fs::remove_file(dir.path().join(&name)).unwrap();
    symlink(dir.path().join(&other), dir.path().join(&name)).unwrap();
    assert_eq!(code(&name), RefusalCode::ProposalStore);
}

#[test]
fn a_store_needs_a_private_directory() {
    for mode in [0o755, 0o750, 0o711, 0o1700] {
        let dir = directory();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode)).unwrap();
        let error = ProposalStore::open(SourceRoot::new(dir.path()))
            .err()
            .unwrap();
        assert_eq!(error.code(), RefusalCode::ProposalStore, "{mode:o}");
    }
    let dir = directory();
    let mut store = ProposalStore::open(SourceRoot::new(dir.path())).unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o750)).unwrap();
    assert!(store.retain(&proposal(1)).is_err());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
