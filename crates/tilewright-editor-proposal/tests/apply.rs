// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A proposal recorded in one copy of a world and applied, step by step,
//! to another: publishing is checked on Linux only; elsewhere World
//! refuses it.
#![cfg(target_os = "linux")]

mod common;

use common::{
    AGENT, EDITOR, KINDS, PERSON, RECEIPT, Records, accept, body, document, edit, fix_digests,
    open, revision, sandbox, world,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{collections::BTreeMap, fs, path::Path};
use tilewright_editor_proposal::{
    Notice, Progress, Proposal, ProposalStore, Settled, read_proposal,
};
use tilewright_editor_ui::{Action, RefusalCode, Session};
use tilewright_source::SourceRoot;

/// A change to a proposal body.
type Change = Box<dyn Fn(&mut Value)>;

/// Every edit kind in order, with an undone-and-redone step and a final
/// undone one, accepted in a sandbox.
fn recorded(root: &Path) -> (Session, Proposal) {
    let mut session = sandbox(root);
    let mut records = Records::default();
    for kind in 0..KINDS {
        let expected = revision(&session);
        accept(&mut session, edit(kind, expected), &mut records);
        if kind == 2 {
            // Undo the paint: the step leaves the list; redo it: it returns
            // with its own Action.
            accept(&mut session, Action::Undo, &mut records);
            assert_eq!(session.applied_steps().len(), 2);
            accept(&mut session, Action::Redo, &mut records);
            assert_eq!(session.applied_steps().len(), 3);
        }
    }
    // A last edit, undone: in the redo stack, not in the proposal.
    let expected = revision(&session);
    let undone = Action::SetField {
        uid: "marker_1".into(),
        field: "label".into(),
        value: json!("Undone"),
        expected,
    };
    accept(&mut session, undone, &mut records);
    accept(&mut session, Action::Undo, &mut records);
    assert_eq!(session.history().depths(), (KINDS, 1));
    let proposal =
        Proposal::from_session(&session, EDITOR, RECEIPT, Some("client\u{1b}[31m")).unwrap();
    (session, proposal)
}

/// Every file of a tree, by relative path.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                found.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    found
}

/// What a refusal must leave as it was: nothing pending, the same
/// revision, the same applied steps and the same files.
struct Untouched {
    revision: [u8; 32],
    applied: usize,
    files: BTreeMap<String, Vec<u8>>,
}
impl Untouched {
    fn of(person: &Session, root: &Path) -> Self {
        assert!(person.history().pending().is_none());
        Self {
            revision: revision(person),
            applied: person.applied_steps().len(),
            files: files(root),
        }
    }
    fn check(&self, person: &Session, root: &Path, case: &str) {
        assert!(person.history().pending().is_none(), "{case}: pending");
        assert_eq!(revision(person), self.revision, "{case}: revision");
        assert_eq!(person.applied_steps().len(), self.applied, "{case}: steps");
        assert!(files(root) == self.files, "{case}: files changed");
    }
}

/// A private directory for proposal files, as the provisioner makes one.
fn proposals_directory() -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("tilewright-proposals-")
        .tempdir_in(
            std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
                .map_or_else(std::env::temp_dir, std::path::PathBuf::from),
        )
        .unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

/// Prepare and accept every remaining step.
fn apply_all(proposal: &Proposal, progress: &mut Progress, person: &mut Session) {
    let mut records = Records::default();
    while !progress.finished(proposal) {
        let step = progress
            .prepare_step(proposal, person, &mut records)
            .unwrap_or_else(|error| panic!("{} {error}", error.code()));
        assert_eq!(progress.prepared(), Some(step));
        person.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(
            progress.settle(proposal, person).unwrap(),
            Settled::Applied { step }
        );
    }
}

#[test]
fn a_stored_proposal_applies_step_by_step_to_an_identical_copy_under_another_actor() {
    let agent_world = world(|_| {});
    let person_world = world(|_| {});
    assert_eq!(files(agent_world.path()), files(person_world.path()));
    let (session, written) = recorded(agent_world.path());
    // The sandbox stores it; the person's side reads it from the
    // directory with handles of its own, as another process would.
    let directory = proposals_directory();
    let stored = ProposalStore::open(SourceRoot::new(directory.path()))
        .unwrap()
        .retain(&written)
        .unwrap();
    drop(session);
    let proposal = read_proposal(&SourceRoot::new(directory.path()), &stored.name).unwrap();
    assert_eq!(proposal, written);
    let session = open(agent_world.path(), AGENT);
    // The steps are the undo stack (pinned in build.rs): every kind once.
    let steps = proposal.steps();
    assert_eq!(steps.len(), KINDS);
    assert_eq!(proposal.result(), revision(&session));
    assert_eq!(proposal.actor().as_str(), AGENT);

    let mut person = open(person_world.path(), PERSON);
    assert_eq!(revision(&person), proposal.base());
    assert_ne!(PERSON, AGENT);
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(notices.is_empty());
    assert_ne!(files(person_world.path()), files(agent_world.path()));
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(revision(&person), proposal.result());
    assert_eq!(files(person_world.path()), files(agent_world.path()));
    // Each applied step is a normal undo entry with the person's actor.
    assert_eq!(person.applied_steps().len(), KINDS);
    for applied in person.applied_steps() {
        assert_eq!(applied.origin.unwrap().actor().as_str(), PERSON);
        assert!(applied.transaction.contains("\"actor\":\"human:tester\""));
    }
    // The fixed uid was for one dispatch: the person's own uid source is
    // back, and a new placement draws a new uid.
    let placed = steps[6].placed().unwrap().to_owned();
    let mut records = Records::default();
    let expected = revision(&person);
    accept(&mut person, edit(6, expected), &mut records);
    let last = person
        .applied_steps()
        .last()
        .unwrap()
        .origin
        .unwrap()
        .placed()
        .unwrap()
        .to_owned();
    assert_ne!(last, placed);
    // Applying it again is refused: the world is past its base.
    let untouched = Untouched::of(&person, person_world.path());
    let error = Progress::begin(&proposal, &person).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
    untouched.check(&person, person_world.path(), "applied twice");
}

/// A proposal of the first `count` kinds and its person's copy.
fn short(
    count: usize,
    person: impl FnOnce(&Path),
) -> (tempfile::TempDir, Proposal, tempfile::TempDir) {
    let agent_world = world(|_| {});
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    for kind in 0..count {
        let expected = revision(&session);
        accept(&mut session, edit(kind, expected), &mut records);
    }
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    (agent_world, proposal, world(person))
}

/// The proposal with its body changed by `change` and every digest fixed
/// up, as a writer who can recompute digests would.
fn forged(proposal: &Proposal, change: impl FnOnce(&mut Value)) -> Proposal {
    let mut body = body(proposal.bytes());
    change(&mut body);
    fix_digests(&mut body);
    Proposal::decode(&document(&body)).unwrap_or_else(|error| panic!("{} {error}", error.code()))
}

#[test]
fn an_edited_action_or_transaction_diverges_and_leaves_nothing_pending() {
    let (_agent, proposal, _person) = short(3, |_| {});
    let cases: [(&str, Change); 4] = [
        (
            "action",
            Box::new(|body| body["steps"][1]["action"]["value"] = json!("Other")),
        ),
        (
            "transaction operation",
            Box::new(|body| {
                let text = body["steps"][1]["transaction"].as_str().unwrap();
                let edited = text.replacen("Renamed", "Renamad", 1);
                assert_ne!(edited, text);
                body["steps"][1]["transaction"] = Value::from(edited);
            }),
        ),
        (
            "transaction title",
            Box::new(|body| {
                let text = body["steps"][1]["transaction"].as_str().unwrap();
                let edited = text.replacen("Editor semantic action", "Editor semantic actions", 1);
                assert_ne!(edited, text);
                body["steps"][1]["transaction"] = Value::from(edited);
            }),
        ),
        (
            "transaction base digest",
            Box::new(|body| {
                let text = body["steps"][1]["transaction"].as_str().unwrap();
                let at = text.find("\"sha256\": \"").unwrap() + 11;
                let digit = if &text[at..=at] == "0" { "1" } else { "0" };
                let edited = format!("{}{digit}{}", &text[..at], &text[at + 1..]);
                body["steps"][1]["transaction"] = Value::from(edited);
            }),
        ),
    ];
    for (name, change) in cases {
        let forged = forged(&proposal, change);
        let root = world(|_| {});
        let mut person = open(root.path(), PERSON);
        let mut records = Records::default();
        let (mut progress, _) = Progress::begin(&forged, &person).unwrap();
        assert_eq!(
            progress
                .prepare_step(&forged, &mut person, &mut records)
                .unwrap(),
            0
        );
        person.dispatch(Action::Accept, &mut records).unwrap();
        let untouched = Untouched::of(&person, root.path());
        let error = progress
            .prepare_step(&forged, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(
            error.code(),
            RefusalCode::ProposalStepDiverged,
            "{name}: {error}"
        );
        untouched.check(&person, root.path(), name);
        assert_eq!(error.step(), Some(1), "{name}");
        assert!(person.history().pending().is_none(), "{name}");
        assert_eq!(person.applied_steps().len(), 1, "{name}");
        assert!(
            records.0.len() == 1,
            "{name}: only the first step was retained"
        );
        // Nothing world-sourced in the message.
        assert!(!error.message().contains("Renam"), "{name}: {error}");
        // Refused, not stopped: it diverges again.
        let again = progress
            .prepare_step(&forged, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(again.code(), RefusalCode::ProposalStepDiverged);
    }
}

#[test]
fn a_moved_world_a_cancel_and_a_wrong_base_are_refused() {
    let (_agent, proposal, person_world) = short(3, |_| {});
    let mut records = Records::default();
    // The world moves between steps.
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Applied { step: 0 }
    );
    let expected = revision(&person);
    accept(&mut person, edit(5, expected), &mut records);
    let untouched = Untouched::of(&person, person_world.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
    untouched.check(&person, person_world.path(), "moved world");
    assert_eq!((error.step(), progress.next()), (Some(1), 1));
    // The person cancels a step: the proposal stops, applied steps stay.
    let root = world(|_| {});
    let mut person = open(root.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Pending { step: 1 }
    );
    person.dispatch(Action::Cancel, &mut records).unwrap();
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalClosed);
    assert!(progress.stopped());
    assert_eq!(person.applied_steps().len(), 1);
    // Another world, or a world with a change of its own, is not the base.
    let other = world(|root| {
        common::edit_json(root, "planes/main/zones.jsonc", |zones| {
            zones["zones"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": "extra"}));
        });
    });
    let person = open(other.path(), PERSON);
    let untouched = Untouched::of(&person, other.path());
    let error = Progress::begin(&proposal, &person).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
    untouched.check(&person, other.path(), "wrong base");
    // A progress belongs to one proposal.
    let (_, longer, _) = short(4, |_| {});
    let root = world(|_| {});
    let mut person = open(root.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let error = progress
        .prepare_step(&longer, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalInvalid);
}

#[test]
fn a_grant_the_persons_catalog_lacks_is_refused_with_the_engines_code() {
    // The person's catalog does not grant painting sand.
    let (_agent, proposal, person_world) = short(3, |root| {
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["terrain"][0]["values"] = json!(["grass"]);
            catalog["editing"]["palettes"] = json!([]);
        });
    });
    let mut person = open(person_world.path(), PERSON);
    let mut records = Records::default();
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert_eq!(
        notices,
        [Notice::CatalogDiffers {
            proposal: proposal.catalog(),
            session: person.catalog().revision()
        }]
    );
    for step in 0..2 {
        assert_eq!(
            progress
                .prepare_step(&proposal, &mut person, &mut records)
                .unwrap(),
            step
        );
        person.dispatch(Action::Accept, &mut records).unwrap();
    }
    let untouched = Untouched::of(&person, person_world.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::GrantMissing));
    assert_eq!(error.step(), Some(2));
    untouched.check(&person, person_world.path(), "grant missing");
    // Refused, not stopped: the step can be tried again.
    assert!(!progress.stopped());
}

#[test]
fn a_placement_whose_uid_the_world_already_has_is_refused() {
    // Both copies hold an object with a generated uid; the forged
    // placement records that uid.
    let taken = tilewright_editor_ui::uid::format_uid([0x42; 16]);
    let add = |root: &Path| {
        common::edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
            chunk["objects"]
                .as_array_mut()
                .unwrap()
                .push(json!({"uid": taken, "proto": "marker", "layer": "item", "at": [5, 5]}));
        });
    };
    let agent_world = world(add);
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let expected = revision(&session);
    accept(&mut session, edit(6, expected), &mut records);
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    let placed = proposal.steps()[0].placed().unwrap().to_owned();
    let forged = forged(&proposal, |body| {
        let text = body["steps"][0]["transaction"]
            .as_str()
            .unwrap()
            .replace(&placed, &taken);
        body["steps"][0]["transaction"] = Value::from(text);
        body["steps"][0]["placed"] = Value::from(taken.clone());
    });
    let person_world = world(add);
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&forged, &person).unwrap();
    let untouched = Untouched::of(&person, person_world.path());
    let error = progress
        .prepare_step(&forged, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::UidUnavailable));
    untouched.check(&person, person_world.path(), "uid taken");
    // The unforged proposal applies.
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(revision(&person), proposal.result());
}

#[test]
fn a_removed_or_reordered_step_or_an_unknown_transaction_member_never_reaches_a_world() {
    let (_agent, proposal, _person) = short(3, |_| {});
    let mut removed = body(proposal.bytes());
    removed["steps"].as_array_mut().unwrap().remove(1);
    let mut reordered = body(proposal.bytes());
    reordered["steps"].as_array_mut().unwrap().swap(1, 2);
    for (name, body, code) in [
        ("removed", removed, RefusalCode::ProposalChain),
        ("reordered", reordered, RefusalCode::ProposalChain),
    ] {
        let error = Proposal::decode(&document(&body)).unwrap_err();
        assert_eq!(error.code(), code, "{name}");
    }
    // A recorded transaction with a member World does not know.
    let mut extra = body(proposal.bytes());
    let text = extra["steps"][0]["transaction"]
        .as_str()
        .unwrap()
        .to_owned();
    extra["steps"][0]["transaction"] = Value::from(text.replacen('{', "{\"extra\":1,", 1));
    fix_digests(&mut extra);
    let error = Proposal::decode(&document(&extra)).unwrap_err();
    assert_eq!(
        (error.code(), error.step()),
        (RefusalCode::ProposalInvalid, Some(0))
    );
}

#[test]
fn a_session_that_cannot_prepare_a_finished_or_stopped_proposal_is_refused() {
    let (_agent, proposal, person_world) = short(2, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    // A change of the person's own is pending: neither beginning nor a
    // step touches it.
    let expected = revision(&person);
    person.dispatch(edit(5, expected), &mut records).unwrap();
    let pending = person.history().pending().unwrap().sealed().to_owned();
    assert_eq!(
        Progress::begin(&proposal, &person).unwrap_err().code(),
        RefusalCode::PendingExists
    );
    person.dispatch(Action::Cancel, &mut records).unwrap();
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    person.dispatch(edit(5, expected), &mut records).unwrap();
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
    assert_eq!(person.history().pending().unwrap().sealed(), pending);
    person.dispatch(Action::Cancel, &mut records).unwrap();
    // Finished: every further step is closed.
    apply_all(&proposal, &mut progress, &mut person);
    assert!(progress.finished(&proposal));
    let untouched = Untouched::of(&person, person_world.path());
    for _ in 0..2 {
        let error = progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalClosed);
    }
    untouched.check(&person, person_world.path(), "finished");
    // Stopped by a Cancel: closed every time after, though the world is
    // still at the cancelled step's base.
    let root = world(|_| {});
    let mut person = open(root.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(revision(&person), proposal.base());
    let untouched = Untouched::of(&person, root.path());
    for _ in 0..2 {
        let error = progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalClosed);
    }
    untouched.check(&person, root.path(), "stopped");
}

#[test]
fn reaching_the_steps_result_by_other_edits_is_not_accepting_the_step() {
    // Step 0 moves marker_1 to (4, 2). The person cancels it, then moves the
    // marker to (5, 2) and on to (4, 2): the same files, so the same
    // revision as the step's result, but not the step.
    let (_agent, proposal, person_world) = short(1, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Cancel, &mut records).unwrap();
    for x in [5, 4] {
        let expected = revision(&person);
        let action = Action::Move {
            uid: "marker_1".into(),
            x,
            y: 2,
            expected,
        };
        accept(&mut person, action, &mut records);
    }
    assert_eq!(revision(&person), proposal.steps()[0].after());
    let error = progress.settle(&proposal, &person).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
    // Refused, not stopped.
    assert!(!progress.stopped());
    assert_eq!((progress.next(), progress.prepared()), (0, Some(0)));
    // Back at the base by undoing both: the person's own move, not the
    // step, is on the redo stack, so the step was cancelled. Resume
    // continues if the person wants it after all.
    for _ in 0..2 {
        accept(&mut person, Action::Undo, &mut records);
    }
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Stopped { step: 0 }
    );
    let (mut progress, _) = Progress::resume(&proposal, &person).unwrap();
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(revision(&person), proposal.result());
}

#[test]
fn an_edit_of_the_persons_own_after_accept_is_stale_until_undone() {
    let (agent, proposal, person_world) = short(3, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    // An edit of the person's own before the outcome is read.
    let expected = revision(&person);
    accept(&mut person, edit(5, expected), &mut records);
    let untouched = Untouched::of(&person, person_world.path());
    let error = progress.settle(&proposal, &person).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
    untouched.check(&person, person_world.path(), "own edit");
    assert!(!progress.stopped());
    // Undoing it puts the step back on top: applied, and the rest follows.
    accept(&mut person, Action::Undo, &mut records);
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Applied { step: 0 }
    );
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(files(person_world.path()), files(agent.path()));
}

#[test]
fn accept_then_undo_is_not_applied_and_a_later_undo_leaves_next_ahead() {
    let (agent, proposal, person_world) = short(3, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    // Accept, then Undo, before the outcome is read: not a Cancel.
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    accept(&mut person, Action::Undo, &mut records);
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::NotApplied { step: 0 }
    );
    assert!(!progress.stopped());
    assert_eq!(progress.next(), 0);
    // It can be prepared again and applied.
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Applied { step: 0 }
    );
    // A settled step undone later: next is ahead of the world, and
    // preparing is stale until a Redo.
    accept(&mut person, Action::Undo, &mut records);
    let untouched = Untouched::of(&person, person_world.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(
        (error.code(), progress.next()),
        (RefusalCode::ProposalBaseStale, 1)
    );
    untouched.check(&person, person_world.path(), "undone after settle");
    accept(&mut person, Action::Redo, &mut records);
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(files(person_world.path()), files(agent.path()));
}

#[test]
fn a_pending_change_that_is_not_the_step_is_reported_as_the_persons_own() {
    let (_agent, proposal, person_world) = short(2, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Pending { step: 0 }
    );
    // Preparing again while the step is pending is refused, and leaves it.
    let step = person.history().pending().unwrap().sealed().to_owned();
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(
        (error.code(), error.step()),
        (RefusalCode::PendingExists, Some(0))
    );
    assert_eq!(person.history().pending().unwrap().sealed(), step);
    assert_eq!(progress.prepared(), Some(0));
    person.dispatch(Action::Cancel, &mut records).unwrap();
    let expected = revision(&person);
    person.dispatch(edit(5, expected), &mut records).unwrap();
    let own = person.history().pending().unwrap().sealed().to_owned();
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::OtherPending { step: 0 }
    );
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
    assert_eq!(person.history().pending().unwrap().sealed(), own);
    // Once the person cancels their own change too, the step reads as
    // cancelled.
    person.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Stopped { step: 0 }
    );
}

#[test]
fn a_proposal_applied_part_way_resumes_where_the_world_is() {
    let (agent, proposal, person_world) = short(KINDS, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    for _ in 0..5 {
        progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap();
        person.dispatch(Action::Accept, &mut records).unwrap();
    }
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Applied { step: 4 }
    );
    // The editor closes: the progress and the session are gone.
    drop((progress, person));
    let mut person = open(person_world.path(), PERSON);
    assert_eq!(person.applied_steps().len(), 0);
    // A change of the person's own pending: refused.
    let expected = revision(&person);
    person
        .dispatch(
            Action::SetField {
                uid: "marker_1".into(),
                field: "label".into(),
                value: json!("Own"),
                expected,
            },
            &mut records,
        )
        .unwrap();
    assert_eq!(
        Progress::resume(&proposal, &person).unwrap_err().code(),
        RefusalCode::PendingExists
    );
    person.dispatch(Action::Cancel, &mut records).unwrap();
    // Not at the base, so begin is refused; resume continues at step 5.
    assert_eq!(
        Progress::begin(&proposal, &person).unwrap_err().code(),
        RefusalCode::ProposalBaseStale
    );
    let (mut progress, _) = Progress::resume(&proposal, &person).unwrap();
    assert_eq!(progress.next(), 5);
    assert_eq!(progress.proposal(), proposal.body_sha256());
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(revision(&person), proposal.result());
    assert_eq!(files(person_world.path()), files(agent.path()));
    // At the end it resumes as finished; a foreign world is refused.
    let (progress, _) = Progress::resume(&proposal, &person).unwrap();
    assert!(progress.finished(&proposal));
    let other = world(|root| {
        common::edit_json(root, "planes/main/zones.jsonc", |zones| {
            zones["zones"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": "extra"}));
        });
    });
    let person = open(other.path(), PERSON);
    assert_eq!(
        Progress::resume(&proposal, &person).unwrap_err().code(),
        RefusalCode::ProposalBaseStale
    );
}

#[test]
fn a_chain_that_returns_to_its_base_cannot_be_resumed_there() {
    // There and back: marker_1 to (4, 2), then back to where it was.
    let agent_world = world(|_| {});
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let at = session.world().object("marker_1").unwrap().at.unwrap();
    for x in [4, i64::from(at[0])] {
        let expected = revision(&session);
        let action = Action::Move {
            uid: "marker_1".into(),
            x,
            y: i64::from(at[1]),
            expected,
        };
        accept(&mut session, action, &mut records);
    }
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    assert_eq!(proposal.base(), proposal.result());
    let person_world = world(|_| {});
    let person = open(person_world.path(), PERSON);
    let error = Progress::resume(&proposal, &person).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalResumeAmbiguous);
    assert!(!error.message().contains("marker"), "{error}");
    // Beginning, at the strict base, is still possible.
    assert!(Progress::begin(&proposal, &person).is_ok());
}

/// An edit of the person's own: a new label for `marker_1`.
fn own(person: &Session, label: &str) -> Action {
    Action::SetField {
        uid: "marker_1".into(),
        field: "label".into(),
        value: json!(label),
        expected: revision(person),
    }
}

#[test]
fn accept_then_undo_is_told_from_cancel_by_the_redo_stacks_top_entry() {
    // Accept then Undo with one entry of the person's own already on the
    // redo stack: the step is on top of it, so it is not applied.
    let (agent, proposal, person_world) = short(3, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let mine = own(&person, "Mine");
    accept(&mut person, mine, &mut records);
    accept(&mut person, Action::Undo, &mut records);
    assert_eq!(person.history().depths(), (0, 1));
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    accept(&mut person, Action::Undo, &mut records);
    assert_eq!(person.history().depths(), (0, 1));
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::NotApplied { step: 0 }
    );
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(files(person_world.path()), files(agent.path()));

    // Cancel, then an edit of the person's own accepted and undone: their
    // edit, not the step, is on the redo stack, so it was a Cancel.
    let root = world(|_| {});
    let mut person = open(root.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Cancel, &mut records).unwrap();
    let mine = own(&person, "Mine");
    accept(&mut person, mine, &mut records);
    accept(&mut person, Action::Undo, &mut records);
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Stopped { step: 0 }
    );
    assert_eq!(
        progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_err()
            .code(),
        RefusalCode::ProposalClosed
    );
    // The person changes their mind: resume continues from the base.
    let (mut progress, _) = Progress::resume(&proposal, &person).unwrap();
    assert_eq!(progress.next(), 0);
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(revision(&person), proposal.result());
}

#[test]
fn not_applied_then_redo_leaves_next_behind_until_resume() {
    let (agent, proposal, person_world) = short(3, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    accept(&mut person, Action::Undo, &mut records);
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::NotApplied { step: 0 }
    );
    accept(&mut person, Action::Redo, &mut records);
    assert_eq!(revision(&person), proposal.steps()[0].after());
    let untouched = Untouched::of(&person, person_world.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(
        (error.code(), progress.next()),
        (RefusalCode::ProposalBaseStale, 0)
    );
    untouched.check(&person, person_world.path(), "redone");
    let (mut progress, _) = Progress::resume(&proposal, &person).unwrap();
    assert_eq!(progress.next(), 1);
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(files(person_world.path()), files(agent.path()));
}

#[test]
fn a_progress_kept_across_a_reopen_needs_resume() {
    // Accepted before the reopen: the new session's undo stack is empty, so
    // the kept progress reads it as stale, for good.
    let (agent, proposal, person_world) = short(3, |_| {});
    let mut records = Records::default();
    let mut person = open(person_world.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    drop(person);
    let mut person = open(person_world.path(), PERSON);
    for _ in 0..2 {
        let error = progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
    }
    assert!(!progress.stopped());
    let (mut fresh, _) = Progress::resume(&proposal, &person).unwrap();
    assert_eq!(fresh.next(), 1);
    apply_all(&proposal, &mut fresh, &mut person);
    assert_eq!(files(person_world.path()), files(agent.path()));
    // Pending when the session was lost: the kept progress reads Stopped;
    // resume starts again from the base.
    let root = world(|_| {});
    let mut person = open(root.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    drop(person);
    let person = open(root.path(), PERSON);
    assert_eq!(
        progress.settle(&proposal, &person).unwrap(),
        Settled::Stopped { step: 0 }
    );
    let (fresh, _) = Progress::resume(&proposal, &person).unwrap();
    assert_eq!(fresh.next(), 0);
}

#[test]
fn resume_reports_a_differing_catalog_too() {
    let (_agent, proposal, person_world) = short(2, |root| {
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["terrain"][0]["values"] = json!(["grass", "sand"]);
            catalog["editing"]["anchors"]["edit"] = json!(false);
        });
    });
    let person = open(person_world.path(), PERSON);
    assert_ne!(person.catalog().revision(), proposal.catalog());
    let (_, notices) = Progress::resume(&proposal, &person).unwrap();
    assert_eq!(
        notices,
        [Notice::CatalogDiffers {
            proposal: proposal.catalog(),
            session: person.catalog().revision()
        }]
    );
}

/// A proposal of one scatter step at the fixture's base.
fn scatter_proposal() -> (tempfile::TempDir, Proposal) {
    let agent_world = world(|_| {});
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let expected = revision(&session);
    accept(&mut session, edit(17, expected), &mut records);
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    (agent_world, proposal)
}

#[test]
fn a_scatter_step_replays_only_where_the_palette_draws_the_same_values() {
    let (_agent, proposal) = scatter_proposal();
    // The same palette: the same transaction, applied.
    let same = world(|_| {});
    let mut person = open(same.path(), PERSON);
    let mut records = Records::default();
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(notices.is_empty());
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(person.applied_steps().len(), 1);

    // Every weight scaled by the same factor (grass 6, sand 2): the
    // catalog differs, but every draw is the same, so the step applies.
    let scaled = world(|root| {
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["palettes"][0]["entries"][0]["weight"] = json!(6);
            catalog["editing"]["palettes"][0]["entries"][1]["weight"] = json!(2);
        });
    });
    let mut person = open(scaled.path(), PERSON);
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(matches!(
        notices.as_slice(),
        [Notice::CatalogDiffers { .. }]
    ));
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(person.history().document().revision(), proposal.result());

    // Other weights under the same name: the catalog differs, the engine
    // draws other values here, and the step diverges with nothing pending.
    let reweighted = world(|root| {
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["palettes"][0]["entries"][1]["weight"] = json!(30);
        });
    });
    let mut person = open(reweighted.path(), PERSON);
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(matches!(
        notices.as_slice(),
        [Notice::CatalogDiffers { .. }]
    ));
    let untouched = Untouched::of(&person, reweighted.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepDiverged, "{error}");
    untouched.check(&person, reweighted.path(), "reweighted");
    assert!(person.history().pending().is_none());

    // No such palette: refused with the engine's code.
    let missing = world(|root| {
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["palettes"] = json!([]);
        });
    });
    let mut person = open(missing.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::GrantMissing));
}

#[test]
fn a_scatter_step_with_another_seed_diverges() {
    let (_agent, proposal) = scatter_proposal();
    let forged = forged(&proposal, |body| {
        assert_eq!(body["steps"][0]["action"]["seed"], 1234);
        body["steps"][0]["action"]["seed"] = json!(1235);
    });
    let root = world(|_| {});
    let mut person = open(root.path(), PERSON);
    let mut records = Records::default();
    let (mut progress, _) = Progress::begin(&forged, &person).unwrap();
    let error = progress
        .prepare_step(&forged, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepDiverged, "{error}");
    assert!(person.history().pending().is_none());
}

/// A proposal of one height adjustment at the base of the
/// fixture with generated heights.
fn heights_proposal() -> (tempfile::TempDir, Proposal) {
    let agent_world = world(generated_heights);
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let expected = revision(&session);
    accept(&mut session, edit(18, expected), &mut records);
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    (agent_world, proposal)
}

/// Heights from -3 to 6 in chunk `0_0`, so a raise by 5 reaches the end of
/// the range -8 to 8 in some cells of the view's top rows.
fn generated_heights(root: &Path) {
    let rows: Vec<Vec<i64>> = (0..32_i64)
        .map(|y| (0..32_i64).map(|x| (x * 3 + y) % 10 - 3).collect())
        .collect();
    common::edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}

#[test]
fn a_height_step_replays_only_where_the_range_gives_the_same_heights() {
    let (_agent, proposal) = heights_proposal();
    assert_eq!(
        body(proposal.bytes())["steps"][0]["action"]["kind"],
        "adjust_heights"
    );
    // The same world and range: the same transaction, applied.
    let same = world(generated_heights);
    let mut person = open(same.path(), PERSON);
    let mut records = Records::default();
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(notices.is_empty());
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(person.history().document().revision(), proposal.result());

    // A lower end that no cell reaches: the catalog differs, the heights
    // are the same, so the step applies.
    let lower = world(|root| {
        generated_heights(root);
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["heights"] = json!({"min": -64, "max": 8});
        });
    });
    let mut person = open(lower.path(), PERSON);
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(matches!(
        notices.as_slice(),
        [Notice::CatalogDiffers { .. }]
    ));
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(person.history().document().revision(), proposal.result());

    // A higher upper end: the raise was cut at 8 in some cells and is not
    // now, so the step diverges.
    let higher = world(|root| {
        generated_heights(root);
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["heights"] = json!({"min": -8, "max": 64});
        });
    });
    let mut person = open(higher.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepDiverged, "{error}");
    assert!(person.history().pending().is_none());

    // A narrower range: the cells stop at another end, so the step
    // diverges with nothing pending and the world untouched.
    let narrower = world(|root| {
        generated_heights(root);
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["heights"] = json!({"min": -8, "max": 7});
        });
    });
    let mut person = open(narrower.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let untouched = Untouched::of(&person, narrower.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepDiverged, "{error}");
    untouched.check(&person, narrower.path(), "narrower");

    // No grant: refused with the engine's code.
    let missing = world(|root| {
        generated_heights(root);
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]
                .as_object_mut()
                .unwrap()
                .remove("heights");
        });
    });
    let mut person = open(missing.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::GrantMissing));
    assert!(person.history().pending().is_none());
}

#[test]
fn a_forged_height_step_is_invalid_refused_or_diverged() {
    let (_agent, proposal) = heights_proposal();
    let root = world(generated_heights);
    let mut records = Records::default();
    // Amounts the reader refuses: 0, past the bound, not an integer.
    for amount in [
        json!(0),
        json!(1025),
        json!(-1025),
        json!(5.5),
        json!("5"),
        json!(null),
    ] {
        let mut body = body(proposal.bytes());
        body["steps"][0]["action"]["amount"] = amount.clone();
        fix_digests(&mut body);
        let error = Proposal::decode(&document(&body)).unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalInvalid, "{amount}");
    }
    // Another amount: another transaction.
    let forged_amount = forged(&proposal, |body| {
        assert_eq!(body["steps"][0]["action"]["amount"], 5);
        body["steps"][0]["action"]["amount"] = json!(4);
    });
    // Cells outside the view: the engine refuses.
    let forged_cells = forged(&proposal, |body| {
        body["steps"][0]["action"]["cells"] = json!([[0, 0], [8, 0]]);
    });
    for (forged, code, engine) in [
        (&forged_amount, RefusalCode::ProposalStepDiverged, None),
        (
            &forged_cells,
            RefusalCode::ProposalStepRefused,
            Some(RefusalCode::ScopeOutsideView),
        ),
    ] {
        let mut person = open(root.path(), PERSON);
        let (mut progress, _) = Progress::begin(forged, &person).unwrap();
        let error = progress
            .prepare_step(forged, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(error.code(), code, "{error}");
        assert_eq!(error.engine_code(), engine);
        assert!(person.history().pending().is_none());
    }
}

/// A proposal of one flatten to -2 (the last kind) at the base of the
/// fixture with generated heights.
fn flatten_proposal() -> (tempfile::TempDir, Proposal) {
    let agent_world = world(generated_heights);
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let expected = revision(&session);
    accept(&mut session, edit(KINDS - 1, expected), &mut records);
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    (agent_world, proposal)
}

/// The person's copy of the fixture with generated heights and the range
/// `range` (none for `None`).
fn ranged(range: Option<&Value>) -> tempfile::TempDir {
    world(|root| {
        generated_heights(root);
        common::edit_json(root, "preview.jsonc", |catalog| {
            let editing = catalog["editing"].as_object_mut().unwrap();
            match range {
                Some(range) => editing.insert("heights".into(), range.clone()),
                None => editing.remove("heights"),
            };
        });
    })
}

#[test]
fn a_flatten_step_replays_while_the_range_holds_its_target() {
    let (_agent, proposal) = flatten_proposal();
    let action = &body(proposal.bytes())["steps"][0]["action"];
    assert_eq!(action["kind"], "flatten_heights");
    assert_eq!(action["target"], -2);
    let mut records = Records::default();
    // The same range, and other ranges that still hold -2 (the catalog
    // differs, the transaction is equal): applied.
    for range in [
        json!({"min": -8, "max": 8}),
        json!({"min": -2, "max": 0}),
        json!({"min": -1024, "max": 1024}),
    ] {
        let copy = ranged(Some(&range));
        let mut person = open(copy.path(), PERSON);
        let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
        progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_or_else(|error| panic!("{range}: {error}"));
        person.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(person.history().document().revision(), proposal.result());
    }
    // A range that no longer holds -2, or none: refused with the engine's
    // grant.missing, nothing pending, the world untouched.
    for range in [
        Some(json!({"min": -1, "max": 8})),
        Some(json!({"min": -8, "max": -3})),
        None,
    ] {
        let copy = ranged(range.as_ref());
        let mut person = open(copy.path(), PERSON);
        let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
        let untouched = Untouched::of(&person, copy.path());
        let error = progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalStepRefused, "{range:?}");
        assert_eq!(error.engine_code(), Some(RefusalCode::GrantMissing));
        untouched.check(&person, copy.path(), "out of range");
    }
}

#[test]
fn a_forged_flatten_step_is_invalid_refused_or_diverged() {
    let (_agent, proposal) = flatten_proposal();
    let root = world(generated_heights);
    let mut records = Records::default();
    // Targets the reader refuses: past the bound, not an integer.
    for target in [
        json!(1025),
        json!(-1025),
        json!(-2.5),
        json!(-2.0),
        json!("-2"),
        json!(null),
    ] {
        let mut body = body(proposal.bytes());
        body["steps"][0]["action"]["target"] = target.clone();
        fix_digests(&mut body);
        let error = Proposal::decode(&document(&body)).unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalInvalid, "{target}");
    }
    // Another target in the range: another transaction.
    let forged_target = forged(&proposal, |body| {
        body["steps"][0]["action"]["target"] = json!(3);
    });
    // A target in the hard bound but outside the range: the engine refuses.
    let forged_range = forged(&proposal, |body| {
        body["steps"][0]["action"]["target"] = json!(1000);
    });
    // Cells outside the view: the engine refuses.
    let forged_cells = forged(&proposal, |body| {
        body["steps"][0]["action"]["cells"] = json!([[0, 0], [8, 0]]);
    });
    for (forged, code, engine) in [
        (&forged_target, RefusalCode::ProposalStepDiverged, None),
        (
            &forged_range,
            RefusalCode::ProposalStepRefused,
            Some(RefusalCode::GrantMissing),
        ),
        (
            &forged_cells,
            RefusalCode::ProposalStepRefused,
            Some(RefusalCode::ScopeOutsideView),
        ),
    ] {
        let mut person = open(root.path(), PERSON);
        let (mut progress, _) = Progress::begin(forged, &person).unwrap();
        let error = progress
            .prepare_step(forged, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(error.code(), code, "{error}");
        assert_eq!(error.engine_code(), engine);
        assert!(person.history().pending().is_none());
    }
}

/// A proposal of one smooth (kind 19) of rows 1 to 3 at the base of the
/// fixture with generated heights, in the view 0..8 × 0..6.
fn smooth_proposal() -> (tempfile::TempDir, Proposal) {
    let agent_world = world(generated_heights);
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let expected = revision(&session);
    accept(&mut session, edit(19, expected), &mut records);
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    (agent_world, proposal)
}

/// The person's copy of the fixture with generated heights, the range -8
/// to 8 or none, and the view `bounds`.
fn viewed(bounds: [i64; 4], granted: bool) -> tempfile::TempDir {
    world(|root| {
        generated_heights(root);
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!(bounds);
            if !granted {
                catalog["editing"]
                    .as_object_mut()
                    .unwrap()
                    .remove("heights");
            }
        });
    })
}

#[test]
fn a_smooth_step_replays_where_the_view_reads_the_same_neighbours() {
    let (_agent, proposal) = smooth_proposal();
    let action = &body(proposal.bytes())["steps"][0]["action"];
    assert_eq!(action["kind"], "smooth_heights");
    assert_eq!(action["kernel"], 1);
    let mut records = Records::default();
    // The same view, and ones that differ only out of the stroke's reach
    // (rows 1 to 3 read rows -1 to 5; row 6 is three tiles from row 3,
    // and row -1 has no chunk): the same transaction, applied.
    for bounds in [[0, 0, 8, 6], [0, 0, 8, 7], [0, -1, 8, 8]] {
        let copy = viewed(bounds, true);
        let mut person = open(copy.path(), PERSON);
        let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
        progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_or_else(|error| panic!("{bounds:?}: {error}"));
        person.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(person.history().document().revision(), proposal.result());
    }
    // A view that leaves out row 0, one tile from row 1, or takes in
    // column 8, one tile from column 7, or columns 8 and 9: another
    // transaction along its edge, so diverged. (Two tiles out: below.)
    for bounds in [[0, 1, 8, 5], [0, 0, 9, 6], [0, 0, 10, 6]] {
        let copy = viewed(bounds, true);
        let mut person = open(copy.path(), PERSON);
        let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
        let untouched = Untouched::of(&person, copy.path());
        let error = progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(
            error.code(),
            RefusalCode::ProposalStepDiverged,
            "{bounds:?}"
        );
        untouched.check(&person, copy.path(), "another view");
    }
    // A view that leaves out a covered cell: the engine refuses.
    let copy = viewed([0, 2, 8, 4], true);
    let mut person = open(copy.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::ScopeOutsideView));
    // No grant: refused with the engine's grant.missing.
    let copy = viewed([0, 0, 8, 6], false);
    let mut person = open(copy.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let untouched = Untouched::of(&person, copy.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::GrantMissing));
    untouched.check(&person, copy.path(), "no grant");
}

#[test]
fn a_forged_smooth_step_is_invalid_refused_or_diverged() {
    let (_agent, proposal) = smooth_proposal();
    let root = world(generated_heights);
    let mut records = Records::default();
    // Kernels the reader refuses: unknown versions, not an integer.
    for kernel in [
        json!(2),
        json!(0),
        json!(-1),
        json!(1.0),
        json!("1"),
        json!(null),
    ] {
        let mut body = body(proposal.bytes());
        body["steps"][0]["action"]["kernel"] = kernel.clone();
        fix_digests(&mut body);
        let error = Proposal::decode(&document(&body)).unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalInvalid, "{kernel}");
    }
    // Other cells in the view: another transaction.
    let forged_cells = forged(&proposal, |body| {
        body["steps"][0]["action"]["cells"] = json!([[0, 1], [0, 2]]);
    });
    // Cells outside the view: the engine refuses.
    let forged_outside = forged(&proposal, |body| {
        body["steps"][0]["action"]["cells"] = json!([[0, 1], [8, 1]]);
    });
    for (forged, code, engine) in [
        (&forged_cells, RefusalCode::ProposalStepDiverged, None),
        (
            &forged_outside,
            RefusalCode::ProposalStepRefused,
            Some(RefusalCode::ScopeOutsideView),
        ),
    ] {
        let mut person = open(root.path(), PERSON);
        let (mut progress, _) = Progress::begin(forged, &person).unwrap();
        let error = progress
            .prepare_step(forged, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(error.code(), code, "{error}");
        assert_eq!(error.engine_code(), engine);
        assert!(person.history().pending().is_none());
    }
}

/// Heights 12 in columns 2 and 7, 0 elsewhere, the range -16 to 16 and
/// the view `bounds`.
fn two_columns(bounds: [i64; 4]) -> tempfile::TempDir {
    world(|root| {
        let rows: Vec<Vec<i64>> = (0..32)
            .map(|_| {
                (0..32)
                    .map(|x| if x == 2 || x == 7 { 12 } else { 0 })
                    .collect()
            })
            .collect();
        common::edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
            chunk["terrain"]["height"] = json!({ "rows": rows });
        });
        common::edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["heights"] = json!({"min": -16, "max": 16});
            catalog["view"]["bounds"] = json!(bounds);
        });
    })
}

#[test]
fn a_smooth_step_diverges_where_the_view_moves_two_tiles_from_a_cell() {
    // Recorded in the view 0..7 × 0..6: a smooth of columns 3 to 5, rows 1
    // to 3. Column 5 reads up to column 7, two tiles away, which this view
    // leaves out: column 5 is even, column 4 takes 1 from column 2 two
    // tiles away, column 3 takes 3 from column 2 beside it.
    let agent_world = two_columns([0, 0, 7, 6]);
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let expected = revision(&session);
    let cells: Vec<[i64; 2]> = (3..6).flat_map(|x| (1..4).map(move |y| [x, y])).collect();
    accept(
        &mut session,
        Action::SmoothHeights {
            plane: "main".into(),
            layer: 0,
            cells,
            kernel: 1,
            expected,
        },
        &mut records,
    );
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    // A view one row taller reads nothing new within reach: applied.
    let copy = two_columns([0, 0, 7, 7]);
    let mut person = open(copy.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap();
    person.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(person.history().document().revision(), proposal.result());
    // A view one column wider takes in column 7, two tiles from column 5
    // and from no other covered cell: column 5 now moves, so diverged.
    let copy = two_columns([0, 0, 8, 6]);
    let mut person = open(copy.path(), PERSON);
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    let untouched = Untouched::of(&person, copy.path());
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepDiverged);
    untouched.check(&person, copy.path(), "two tiles wider");
}
