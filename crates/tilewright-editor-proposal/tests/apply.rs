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
use std::{collections::BTreeMap, fs, path::Path};
use tilewright_editor_proposal::{Notice, Progress, Proposal, Settled};
use tilewright_editor_ui::{Action, RefusalCode, Session};

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
fn a_recorded_session_applies_step_by_step_to_an_identical_copy() {
    let agent_world = world(|_| {});
    let person_world = world(|_| {});
    let (session, proposal) = recorded(agent_world.path());
    // The steps are the undo stack (pinned in build.rs): every kind once.
    let steps = proposal.steps();
    assert_eq!(steps.len(), KINDS);
    assert_eq!(proposal.result(), revision(&session));
    assert_eq!(proposal.actor().as_str(), AGENT);

    let mut person = open(person_world.path(), PERSON);
    assert_eq!(revision(&person), proposal.base());
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(notices.is_empty());
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
    let error = Progress::begin(&proposal, &person).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
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
        let error = progress
            .prepare_step(&forged, &mut person, &mut records)
            .unwrap_err();
        assert_eq!(
            error.code(),
            RefusalCode::ProposalStepDiverged,
            "{name}: {error}"
        );
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
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
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
    let error = Progress::begin(&proposal, &person).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBaseStale);
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
    let error = progress
        .prepare_step(&proposal, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::GrantMissing));
    assert_eq!(error.step(), Some(2));
    assert!(person.history().pending().is_none());
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
    let error = progress
        .prepare_step(&forged, &mut person, &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalStepRefused);
    assert_eq!(error.engine_code(), Some(RefusalCode::UidUnavailable));
    // The unforged proposal applies.
    let (mut progress, _) = Progress::begin(&proposal, &person).unwrap();
    apply_all(&proposal, &mut progress, &mut person);
    assert_eq!(revision(&person), proposal.result());
}
