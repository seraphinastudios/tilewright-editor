// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, SelectionKey, Session, SessionPhase};

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
fn fixture() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    (
        dir,
        Session::new(History::new(document), catalog, resources),
    )
}
#[derive(Default)]
struct Records {
    fail: bool,
    inverses: Vec<String>,
}
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        if self.fail {
            return Err("injected durable retention failure".into());
        }
        self.inverses.push(inverse.into());
        Ok(())
    }
}
fn paint(session: &Session, x: i64, y: i64) -> Action {
    Action::Paint {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
        proto: "sand".into(),
        expected: session.history().document().revision(),
    }
}
fn movement(session: &Session, x: i64) -> Action {
    Action::Move {
        uid: "marker_1".into(),
        x,
        y: 2,
        expected: session.history().document().revision(),
    }
}
#[test]
fn pending_scene_diff_and_cancel_never_write() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let bytes = fs::read(&chunk).unwrap();
    let generation = session.generation();
    let mut records = Records::default();
    session
        .dispatch(movement(&session, 4), &mut records)
        .unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    assert!(
        session
            .history()
            .pending()
            .unwrap()
            .changed()
            .next()
            .is_some()
    );
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([4, 2]));
    assert_eq!(
        session
            .history()
            .document()
            .world()
            .object("marker_1")
            .unwrap()
            .at,
        Some([3, 2])
    );
    assert!(session.generation() > generation);
    assert_eq!(fs::read(&chunk).unwrap(), bytes);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(fs::read(chunk).unwrap(), bytes);
    assert!(records.inverses.is_empty());
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([3, 2]));
}
#[test]
fn rejects_stale_frames_revisions_bounds_and_unsupported_capabilities() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let key = SelectionKey::Object {
        uid: "marker_1".into(),
    };
    assert!(
        session
            .dispatch(
                Action::Select {
                    generation: 0,
                    key: key.clone()
                },
                &mut records
            )
            .is_err()
    );
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key,
            },
            &mut records,
        )
        .unwrap();
    assert!(
        session
            .dispatch(movement(&session, i64::MAX), &mut records)
            .is_err()
    );
    assert!(
        session
            .dispatch(
                Action::Move {
                    uid: "marker_1".into(),
                    x: 4,
                    y: 2,
                    expected: [0; 32]
                },
                &mut records
            )
            .is_err()
    );
    assert!(
        session
            .dispatch(
                Action::SetField {
                    uid: "marker_1".into(),
                    field: "proto".into(),
                    value: serde_json::json!("evil"),
                    expected: session.history().document().revision()
                },
                &mut records
            )
            .is_err()
    );
    assert!(
        session
            .dispatch(paint(&session, -1, 0), &mut records)
            .is_err()
    );
    assert!(session.history().pending().is_none());
}
#[test]
fn typed_strings_escape_and_invalid_types_never_publish() {
    let (dir, mut session) = fixture();
    let mut records = Records::default();
    let original = WorldDocument::open(dir.path()).unwrap().revision();
    let label = "quote \" backslash \\ newline\n snowman ☃";
    session
        .dispatch(
            Action::SetField {
                uid: "marker_1".into(),
                field: "label".into(),
                value: serde_json::json!(label),
                expected: original,
            },
            &mut records,
        )
        .unwrap();
    assert!(
        session
            .world()
            .object("marker_1")
            .unwrap()
            .fields
            .iter()
            .any(|(name, value)| name == "label"
                && *value == tilewright_source::FieldValue::String(label.into()))
    );
    assert_eq!(
        WorldDocument::open(dir.path()).unwrap().revision(),
        original
    );
    assert!(
        session
            .dispatch(
                Action::SetField {
                    uid: "marker_1".into(),
                    field: "label".into(),
                    value: serde_json::json!(42),
                    expected: original
                },
                &mut records
            )
            .is_err()
    );
    assert!(session.history().pending().is_none());
}
#[test]
fn changed_catalog_and_source_refuse_before_retention() {
    for catalog in [true, false] {
        let (dir, mut session) = fixture();
        let mut records = Records::default();
        session
            .dispatch(paint(&session, 6, 4), &mut records)
            .unwrap();
        let path = if catalog {
            dir.path().join("preview.jsonc")
        } else {
            dir.path().join("planes/main/layers/0/0_0.jsonc")
        };
        let changed = format!(
            "// changed externally\n{}",
            fs::read_to_string(&path).unwrap()
        );
        fs::write(&path, &changed).unwrap();
        assert!(session.dispatch(Action::Accept, &mut records).is_err());
        assert!(records.inverses.is_empty());
        assert_eq!(fs::read_to_string(path).unwrap(), changed);
    }
}
#[test]
fn retention_failure_keeps_reviewable_pending_and_source() {
    let (dir, mut session) = fixture();
    let revision = session.history().document().revision();
    let mut records = Records {
        fail: true,
        ..Records::default()
    };
    session
        .dispatch(paint(&session, 6, 4), &mut records)
        .unwrap();
    assert!(session.dispatch(Action::Accept, &mut records).is_err());
    assert_eq!(session.status().phase, SessionPhase::Pending);
    assert!(session.status().message.contains("retention failure"));
    assert!(session.history().pending().is_some());
    assert_eq!(
        WorldDocument::open(dir.path()).unwrap().revision(),
        revision
    );
    assert!(records.inverses.is_empty());
}

#[cfg(target_os = "linux")]
#[test]
fn accept_undo_redo_reopen_preserve_exact_world_bytes_and_scene() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let mut records = Records::default();
    session
        .dispatch(movement(&session, 4), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    let moved = fs::read(&chunk).unwrap();
    assert_ne!(moved, original);
    assert_eq!(session.history().depths(), (1, 0));
    assert_eq!(session.status().phase, SessionPhase::Ready);
    assert_eq!(session.status().applied, Some(true));
    assert_eq!(session.status().durable, Some(true));
    session.dispatch(Action::Undo, &mut records).unwrap();
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([3, 2]));
    assert_eq!(fs::read(&chunk).unwrap(), moved);
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), moved);
    session.dispatch(Action::Reopen, &mut records).unwrap();
    assert_eq!(session.world().object("marker_1").unwrap().at, Some([4, 2]));
    assert_eq!(records.inverses.len(), 3);
}

struct ConcurrentEdit {
    chunk: std::path::PathBuf,
    inverse: Option<String>,
}
impl RecoveryStore for ConcurrentEdit {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.inverse = Some(inverse.into());
        let bytes = fs::read_to_string(&self.chunk).unwrap();
        fs::write(
            &self.chunk,
            format!("// concurrent writer after retention\n{bytes}"),
        )
        .unwrap();
        Ok(())
    }
}
#[test]
fn publication_failure_exposes_outcome_and_blocks_until_explicit_inspection() {
    let (dir, mut session) = fixture();
    let mut recovery = ConcurrentEdit {
        chunk: dir.path().join("planes/main/layers/0/0_0.jsonc"),
        inverse: None,
    };
    session
        .dispatch(movement(&session, 4), &mut recovery)
        .unwrap();
    let exact_inverse = session.history().pending().unwrap().inverse().to_owned();
    assert!(session.dispatch(Action::Accept, &mut recovery).is_err());
    assert_eq!(recovery.inverse, Some(exact_inverse));
    assert_eq!(session.status().phase, SessionPhase::Blocked);
    assert!(session.status().applied.is_some());
    assert!(session.status().durable.is_some());
    assert!(session.status().cleaned.is_some());
    assert!(session.status().to_string().contains("applied="));
    assert!(session.dispatch(Action::Undo, &mut recovery).is_err());
    assert!(session.dispatch(Action::Redo, &mut recovery).is_err());
    assert!(session.dispatch(Action::Reopen, &mut recovery).is_err());
    assert_eq!(session.status().phase, SessionPhase::Blocked);
    assert!(session.history().pending().is_some());
}

#[test]
fn semantic_field_encoding_rejects_large_or_deep_values_before_planning() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let mut deep = serde_json::json!(true);
    for _ in 0..10 {
        deep = serde_json::json!([deep]);
    }
    for value in [deep, serde_json::json!("x".repeat(65536))] {
        assert!(
            session
                .dispatch(
                    Action::SetField {
                        uid: "marker_1".into(),
                        field: "label".into(),
                        value,
                        expected: session.history().document().revision()
                    },
                    &mut records
                )
                .is_err()
        );
        assert!(session.history().pending().is_none());
    }
}

// The prototype deliberately uses checked publication on every platform. Windows
// cannot provide the released toolkit's checked-access guarantee and must refuse.
#[cfg(windows)]
#[test]
fn windows_checked_publication_refuses_without_false_success_or_source_changes() {
    let (dir, mut session) = fixture();
    let chunk = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let original_revision = session.history().document().revision();
    let mut records = Records::default();
    session
        .dispatch(movement(&session, 4), &mut records)
        .unwrap();
    let inverse = session.history().pending().unwrap().inverse().to_owned();

    let error = session.dispatch(Action::Accept, &mut records).unwrap_err();
    assert!(error.to_string().contains("tx.io"), "{error}");
    assert!(error.to_string().contains("unchecked access"), "{error}");
    assert_eq!(session.status().phase, SessionPhase::Blocked);
    assert_eq!(session.status().applied, Some(false));
    assert_eq!(session.status().durable, Some(true));
    assert_eq!(session.status().cleaned, Some(true));
    assert!(
        !session
            .status()
            .message
            .contains("Published World workspace")
    );
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert_eq!(
        session.history().document().reload().unwrap().revision(),
        original_revision
    );
    assert_eq!(session.history().document().revision(), original_revision);
    assert_eq!(session.history().depths(), (0, 0));
    assert!(session.history().needs_reload());
    assert_eq!(session.history().pending().unwrap().inverse(), inverse);
    // History retains recovery before asking the toolkit to publish. Refusal
    // preserves that exact record; it never acknowledges a successful edit.
    assert_eq!(records.inverses, [inverse]);
    for action in [Action::Accept, Action::Undo, Action::Redo, Action::Reopen] {
        assert!(session.dispatch(action, &mut records).is_err());
        assert_eq!(session.status().phase, SessionPhase::Blocked);
    }
    assert_eq!(records.inverses.len(), 1);
    assert_eq!(fs::read(chunk).unwrap(), original);
}
