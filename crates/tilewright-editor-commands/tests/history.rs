// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use std::{fs, path::Path};
use tilewright_edit::PublishOptions;
use tilewright_editor_commands::{Error, History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_testkit::fixture_world;

const CHUNK: &str = "planes/meadow/layers/0/0_0.jsonc";
const NEW_CHUNK: &str = "planes/meadow/layers/0/1_0.jsonc";
const PAINT: &str = r#"{"format":"tilewright.transaction/1","operations":[
{"op":"terrain.set","plane":"meadow","layer":0,"grid":"ground","cells":[
{"at":[10,10],"value":"sand","expect":"water"},
{"at":[32,0],"value":"sand","expect":null}]}]}"#;

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn session() -> (tempfile::TempDir, History) {
    let directory = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("meadow"), directory.path());
    let history = History::new(WorldDocument::open(directory.path()).unwrap());
    (directory, history)
}

#[derive(Default)]
struct Records {
    entries: Vec<String>,
    fail: bool,
}
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        if self.fail {
            return Err("injected recovery storage failure".to_owned());
        }
        self.entries.push(inverse.to_owned());
        Ok(())
    }
}

// Only disposable fixtures opt into unchecked access. Production uses the
// caller's explicit policy, with no automatic platform/filesystem bypass.
fn options() -> PublishOptions {
    PublishOptions {
        unchecked_access: true,
    }
}

#[test]
fn edit_preview_commit_undo_redo_and_reopen_preserve_exact_bytes() {
    let (directory, mut history) = session();
    let original = fs::read(directory.path().join(CHUNK)).unwrap();
    let revision = history.document().revision();
    let preview = history.prepare(revision, "paint", PAINT).unwrap();
    assert_eq!(preview.changed().count(), 2);
    assert_eq!(fs::read(directory.path().join(CHUNK)).unwrap(), original);
    assert!(!directory.path().join(NEW_CHUNK).exists());
    let mut records = Records::default();
    history.commit(&mut records, &options()).unwrap();
    assert_eq!(history.depths(), (1, 0));
    let painted = fs::read(directory.path().join(CHUNK)).unwrap();
    assert_ne!(painted, original);
    assert_eq!(
        WorldDocument::open(directory.path()).unwrap().revision(),
        history.document().revision()
    );
    for _ in 0..4 {
        history.prepare_undo().unwrap();
        assert_eq!(fs::read(directory.path().join(CHUNK)).unwrap(), painted);
        history.commit(&mut records, &options()).unwrap();
        assert_eq!(history.depths(), (0, 1));
        assert_eq!(fs::read(directory.path().join(CHUNK)).unwrap(), original);
        assert!(!directory.path().join(NEW_CHUNK).exists());
        assert_eq!(history.document().revision(), revision);
        history.prepare_redo().unwrap();
        history.commit(&mut records, &options()).unwrap();
        assert_eq!(history.depths(), (1, 0));
        assert_eq!(fs::read(directory.path().join(CHUNK)).unwrap(), painted);
    }
    assert_eq!(records.entries.len(), 9);
}

#[test]
fn stale_open_revision_and_external_edit_after_preview_refuse_without_writing() {
    let (directory, mut history) = session();
    let revision = history.document().revision();
    assert!(matches!(
        history.prepare([0; 32], "paint", PAINT),
        Err(Error::StaleRevision)
    ));
    history.prepare(revision, "paint", PAINT).unwrap();
    let path = directory.path().join(CHUNK);
    let changed = format!("// external edit\n{}", fs::read_to_string(&path).unwrap());
    fs::write(&path, &changed).unwrap();
    let mut records = Records::default();
    assert!(matches!(
        history.commit(&mut records, &options()),
        Err(Error::StaleRevision)
    ));
    assert!(records.entries.is_empty());
    assert_eq!(history.depths(), (0, 0));
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
    assert!(matches!(
        history.prepare(revision, "paint", PAINT),
        Err(Error::StaleRevision)
    ));
    assert!(history.pending().is_none());
}

#[test]
fn recovery_failure_and_cancellation_preserve_source_and_history() {
    let (_directory, mut history) = session();
    let revision = history.document().revision();
    history.prepare(revision, "paint", PAINT).unwrap();
    let mut records = Records {
        fail: true,
        ..Records::default()
    };
    assert!(matches!(
        history.commit(&mut records, &options()),
        Err(Error::Recovery(_))
    ));
    assert_eq!(
        WorldDocument::open(history.document().root())
            .unwrap()
            .revision(),
        revision
    );
    assert_eq!(history.depths(), (0, 0));
    assert!(!history.needs_reload());
    assert!(history.pending().is_some());
    history.cancel();
    assert!(matches!(
        history.commit(&mut records, &options()),
        Err(Error::NothingPrepared)
    ));
}

#[test]
fn failed_new_edit_keeps_redo_and_successful_new_edit_discards_it() {
    let (_directory, mut history) = session();
    let mut records = Records::default();
    history
        .prepare(history.document().revision(), "paint", PAINT)
        .unwrap();
    history.commit(&mut records, &options()).unwrap();
    history.prepare_undo().unwrap();
    history.commit(&mut records, &options()).unwrap();
    assert!(
        history
            .prepare(history.document().revision(), "bad", "{}")
            .is_err()
    );
    assert_eq!(history.depths(), (0, 1));
    history
        .prepare(
            history.document().revision(),
            "other",
            &PAINT.replace("sand", "grass"),
        )
        .unwrap();
    history.commit(&mut records, &options()).unwrap();
    assert_eq!(history.depths(), (1, 0));
    assert!(matches!(history.prepare_redo(), Err(Error::NothingToRedo)));
}

#[test]
fn a_noop_does_not_write_recovery_or_consume_history() {
    let (_directory, mut history) = session();
    history
        .prepare(
            history.document().revision(),
            "noop",
            r#"{"format":"tilewright.transaction/1","operations":[]}"#,
        )
        .unwrap();
    let mut records = Records::default();
    history.commit(&mut records, &options()).unwrap();
    assert!(records.entries.is_empty());
    assert_eq!(history.depths(), (0, 0));
}

#[test]
fn symlink_and_traversal_operations_are_rejected_by_world() {
    let (_directory, mut history) = session();
    let traversal = r#"{"format":"tilewright.transaction/1","operations":[{"op":"document.delete","path":"../outside.jsonc"}]}"#;
    assert!(matches!(
        history.prepare(history.document().revision(), "escape", traversal),
        Err(Error::Rejected(_))
    ));
    #[cfg(unix)]
    {
        let root = history.document().root().to_path_buf();
        fs::rename(root.join(CHUNK), root.join("original.jsonc")).unwrap();
        std::os::unix::fs::symlink(root.join("original.jsonc"), root.join(CHUNK)).unwrap();
        assert!(
            history
                .prepare(history.document().revision(), "link", PAINT)
                .is_err()
        );
    }
}
