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
    fixture_with(|_| {})
}
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    configure(dir.path());
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

fn widen_view(root: &Path) {
    let path = root.join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    catalog["view"]["bounds"] = serde_json::json!([-34, 0, 72, 6]);
    catalog["editing"]["movable_uids"] = serde_json::json!(["marker_1", "marker_negative"]);
    fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
}
#[cfg(target_os = "linux")]
fn object_order(session: &Session, cx: i64) -> Vec<String> {
    session.world().planes()["main"].chunks[&(0, cx, 0)]
        .objects
        .iter()
        .map(|object| object.uid.clone().unwrap())
        .collect()
}
#[cfg(target_os = "linux")]
#[test]
fn same_chunk_move_preserves_interleaved_object_order_and_only_coordinate_bytes() {
    let (dir, mut session) = fixture_with(|root| {
        let path = root.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut marker = chunk["objects"][0].clone();
        marker["custom"] =
            serde_json::json!({"example.org/editor":{"unknown":[1,true,"retained"]}});
        let mut before = marker.clone();
        before["uid"] = serde_json::json!("before_same");
        let mut after_other = marker.clone();
        after_other["uid"] = serde_json::json!("after_other");
        after_other["at"] = serde_json::json!([7, 4]);
        let mut after_same = marker.clone();
        after_same["uid"] = serde_json::json!("after_same");
        let mut destination = marker.clone();
        destination["uid"] = serde_json::json!("destination");
        destination["at"] = serde_json::json!([4, 2]);
        chunk["objects"] =
            serde_json::json!([before, marker, after_other, after_same, destination]);
        fs::write(
            path,
            format!(
                "// Keep all source comments and extension data\n{}",
                serde_json::to_string_pretty(&chunk).unwrap()
            ),
        )
        .unwrap();
    });
    let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&path).unwrap();
    let order = object_order(&session, 0);
    assert_eq!(
        session.world().object_location("marker_1").unwrap().path,
        [1]
    );
    let mut recovery = Records::default();
    session
        .dispatch(movement(&session, 4), &mut recovery)
        .unwrap();
    assert_eq!(object_order(&session, 0), order);
    let preview = session.history().pending().unwrap();
    let transaction: serde_json::Value = serde_json::from_str(preview.sealed()).unwrap();
    assert_eq!(transaction["operations"][0]["op"], "test");
    assert_eq!(
        transaction["operations"][1]["pointer"],
        serde_json::json!(["objects", 1, "uid"])
    );
    assert_eq!(transaction["operations"][2]["op"], "set");
    assert!(
        transaction["base"]
            .as_array()
            .is_some_and(|base| !base.is_empty())
    );
    let before_objects =
        &session.history().document().world().planes()["main"].chunks[&(0, 0, 0)].objects;
    let mut expected_objects = before_objects.clone();
    expected_objects[1].at = Some([4, 2]);
    assert_eq!(
        session.world().planes()["main"].chunks[&(0, 0, 0)].objects,
        expected_objects
    );
    let changed = preview.changed().next().unwrap();
    let before = changed.before.as_ref().unwrap();
    let after = changed.after.as_ref().unwrap();
    let start = before.find("\"uid\": \"marker_1\"").unwrap();
    let at_start = before[..start]
        .rfind("[\n        3,\n        2\n      ]")
        .unwrap();
    let mut expected = before.clone();
    expected.replace_range(
        at_start..at_start + "[\n        3,\n        2\n      ]".len(),
        "[4,2]",
    );
    assert_eq!(after, &expected);
    assert_eq!(fs::read(&path).unwrap(), original);
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    let moved = fs::read(&path).unwrap();
    session.dispatch(Action::Undo, &mut recovery).unwrap();
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(object_order(&session, 0), order);
    session.dispatch(Action::Redo, &mut recovery).unwrap();
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    assert_eq!(fs::read(path).unwrap(), moved);
    assert_eq!(object_order(&session, 0), order);
}

#[test]
fn same_chunk_move_tests_uid_prototype_and_old_coordinates() {
    let (_dir, mut session) = fixture();
    let mut recovery = Records::default();
    session
        .dispatch(movement(&session, 4), &mut recovery)
        .unwrap();
    let sealed = session.history().pending().unwrap().sealed();
    for (index, field, wrong) in [
        (0, "value", serde_json::json!("grass")),
        (1, "value", serde_json::json!("missing_uid")),
        (2, "expect", serde_json::json!([0, 0])),
    ] {
        let mut transaction: serde_json::Value = serde_json::from_str(sealed).unwrap();
        transaction["operations"][index][field] = wrong;
        let result = tilewright_editor_commands::preview(
            session.history().document(),
            "wrong-precondition",
            &transaction.to_string(),
        );
        let Err(tilewright_editor_commands::Error::Rejected(diagnostics)) = result else {
            panic!("incorrect move precondition was accepted")
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "tx.conflict")
        );
    }
}
#[test]
fn negative_coordinates_use_floor_chunks_and_cross_chunk_moves_use_relocation() {
    let (_dir, mut session) = fixture_with(widen_view);
    let mut recovery = Records::default();
    session
        .dispatch(
            Action::Move {
                uid: "marker_negative".into(),
                x: -2,
                y: 1,
                expected: session.history().document().revision(),
            },
            &mut recovery,
        )
        .unwrap();
    assert_eq!(
        session
            .world()
            .object_location("marker_negative")
            .unwrap()
            .chunk,
        (0, -1, 0)
    );
    assert_eq!(
        session.world().object("marker_negative").unwrap().at,
        Some([30, 1])
    );
    let transaction: serde_json::Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    assert_eq!(
        transaction["operations"][2]["value"],
        serde_json::json!([30, 1])
    );
    session.dispatch(Action::Cancel, &mut recovery).unwrap();
    session
        .dispatch(movement(&session, -1), &mut recovery)
        .unwrap();
    let transaction: serde_json::Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    assert_eq!(transaction["operations"][0]["op"], "object.move");
    assert_eq!(
        session.world().object_location("marker_1").unwrap().chunk,
        (0, -1, 0)
    );
    assert_eq!(
        session.world().object("marker_1").unwrap().at,
        Some([31, 2])
    );
}

#[test]
fn same_tile_move_preserves_coordinate_whitespace_without_publication_or_recovery() {
    let (dir, mut session) = fixture();
    let mut recovery = Records::default();
    let path = dir.path().join("planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&path).unwrap();
    session
        .dispatch(movement(&session, 3), &mut recovery)
        .unwrap();
    assert_eq!(session.history().pending().unwrap().changed().count(), 0);
    session.dispatch(Action::Accept, &mut recovery).unwrap();
    assert_eq!(fs::read(path).unwrap(), original);
    assert!(recovery.inverses.is_empty());
    assert_eq!(session.history().depths(), (0, 0));
    assert_eq!(session.status().applied, Some(false));
}

#[test]
fn read_only_or_unknown_uids_cannot_prepare_coordinate_edits() {
    let (_dir, mut session) = fixture();
    let mut recovery = Records::default();
    for uid in ["marker_negative", "unknown_uid"] {
        let error = session
            .dispatch(
                Action::Move {
                    uid: uid.into(),
                    x: 4,
                    y: 2,
                    expected: session.history().document().revision(),
                },
                &mut recovery,
            )
            .unwrap_err();
        assert!(error.to_string().contains("movement capability"));
        assert!(session.history().pending().is_none());
    }
    assert!(recovery.inverses.is_empty());
}
