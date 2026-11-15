// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `Session::applied_steps` reads the undo stack with the Action of each
//! step, and `Session::dispatch_placing` reproduces a placement's uid.
//! Publishing is checked on Linux only; elsewhere World refuses it.
#![cfg(target_os = "linux")]

use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::uid::{SeededUidSource, format_uid};
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session};

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
fn open(root: &Path) -> Session {
    let document = WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let mut session = Session::new(
        History::new(document),
        catalog,
        resources,
        Actor::new("agent:steps").unwrap(),
    );
    session.set_uid_source(Box::new(SeededUidSource::new(3)));
    session
}
struct Records;
impl RecoveryStore for Records {
    fn retain(&mut self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}
fn label(session: &Session, text: &str) -> Action {
    Action::SetField {
        uid: "marker_1".into(),
        field: "label".into(),
        value: json!(text),
        expected: revision(session),
    }
}
fn place(session: &Session) -> Action {
    Action::Place {
        plane: "main".into(),
        layer: 0,
        x: 2,
        y: 2,
        proto: "marker".into(),
        expected: revision(session),
    }
}
/// The actions of the applied steps, oldest first.
fn actions(session: &Session) -> Vec<Action> {
    session
        .applied_steps()
        .map(|step| step.origin.unwrap().action().clone())
        .collect()
}
fn run(session: &mut Session, action: Action) {
    session.dispatch(action, &mut Records).unwrap();
}

#[test]
fn applied_steps_follow_accept_undo_redo_and_new_edits() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    let mut session = open(dir.path());
    assert_eq!(session.applied_steps().len(), 0);
    let first = label(&session, "First");
    let before = revision(&session);
    run(&mut session, first.clone());
    let sealed = session.history().pending().unwrap().sealed().to_owned();
    // A refused edit while one is pending does not replace its Action.
    let refused = session
        .dispatch(label(&session, "Refused"), &mut Records)
        .unwrap_err();
    assert_eq!(refused.code(), RefusalCode::PendingExists);
    run(&mut session, Action::Accept);
    let step = session.applied_steps().next().unwrap();
    assert_eq!(step.origin.unwrap().action(), &first);
    assert_eq!(step.origin.unwrap().actor().as_str(), "agent:steps");
    assert_eq!(step.origin.unwrap().placed(), None);
    assert_eq!(
        (step.transaction, step.before, step.after),
        (sealed.as_str(), before, revision(&session))
    );
    // A cancelled edit and an edit that changes nothing add no step.
    let cancelled = label(&session, "Cancelled");
    run(&mut session, cancelled);
    run(&mut session, Action::Cancel);
    let location = session.world().object("marker_1").unwrap().at.unwrap();
    let still = Action::Move {
        uid: "marker_1".into(),
        x: i64::from(location[0]),
        y: i64::from(location[1]),
        expected: revision(&session),
    };
    run(&mut session, still);
    run(&mut session, Action::Accept);
    assert_eq!(actions(&session), std::slice::from_ref(&first));
    let second = place(&session);
    run(&mut session, second.clone());
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    run(&mut session, Action::Accept);
    let placed = sealed["operations"][0]["object"]["uid"].as_str().unwrap();
    let last = session.applied_steps().last().unwrap();
    assert_eq!(last.origin.unwrap().placed(), Some(placed));
    assert_eq!(actions(&session), [first.clone(), second.clone()]);
    // An undone step leaves; a redone one returns with its Action.
    run(&mut session, Action::Undo);
    assert_eq!(
        actions(&session),
        [first.clone(), second.clone()],
        "pending undo changes nothing"
    );
    run(&mut session, Action::Accept);
    assert_eq!(actions(&session), std::slice::from_ref(&first));
    run(&mut session, Action::Redo);
    run(&mut session, Action::Accept);
    assert_eq!(actions(&session), [first.clone(), second.clone()]);
    assert_eq!(
        session
            .applied_steps()
            .last()
            .unwrap()
            .origin
            .unwrap()
            .placed(),
        Some(placed)
    );
    // A new edit after an undo drops the undone step for good.
    run(&mut session, Action::Undo);
    run(&mut session, Action::Accept);
    let third = label(&session, "Third");
    run(&mut session, third.clone());
    run(&mut session, Action::Accept);
    assert_eq!(actions(&session), [first, third]);
    assert_eq!(session.history().depths(), (2, 0));
}

#[test]
fn a_placement_with_a_fixed_uid_places_exactly_it_once() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    let mut session = open(dir.path());
    let fixed = format_uid([0x24; 16]);
    let error = session
        .dispatch_placing(label(&session, "x"), &fixed, &mut Records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::InputInvalid);
    let error = session
        .dispatch_placing(place(&session), "marker_9", &mut Records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::InputInvalid);
    assert!(session.history().pending().is_none());
    session
        .dispatch_placing(place(&session), &fixed, &mut Records)
        .unwrap();
    run(&mut session, Action::Accept);
    assert!(session.world().object(&fixed).is_some());
    assert_eq!(
        session
            .applied_steps()
            .last()
            .unwrap()
            .origin
            .unwrap()
            .placed(),
        Some(fixed.as_str())
    );
    // Taken now: the one-shot source has nothing else to give.
    let error = session
        .dispatch_placing(place(&session), &fixed, &mut Records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::UidUnavailable);
    // The session's own source is back.
    let again = place(&session);
    run(&mut session, again);
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let drawn = sealed["operations"][0]["object"]["uid"].as_str().unwrap();
    assert_ne!(drawn, fixed);
}
