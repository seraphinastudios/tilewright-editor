// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A proposal built from a session that accepted steps: publishing is
//! checked on Linux only; elsewhere World refuses it.
#![cfg(target_os = "linux")]

mod common;

use common::{
    AGENT, EDITOR, KINDS, RECEIPT, Records, accept, body, document, edit, revision, sandbox, world,
};
use serde_json::json;
use std::path::Path;
use tilewright_editor_proposal::{Proposal, encode_action};
use tilewright_editor_ui::{Action, RefusalCode, Session};

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

#[test]
fn a_proposal_records_the_undo_stack_of_every_edit_kind() {
    let root = world(|_| {});
    let (session, proposal) = recorded(root.path());
    // The steps are the undo stack: every kind once, the redone paint in
    // its place with its own Action, the undone edit absent.
    let steps = proposal.steps();
    assert_eq!(steps.len(), KINDS);
    let applied: Vec<_> = session.applied_steps().collect();
    for (kind, (step, applied)) in steps.iter().zip(&applied).enumerate() {
        assert_eq!(*step.action(), edit(kind, step.before()), "kind {kind}");
        assert_eq!(step.placed().is_some(), kind == 6);
        assert_eq!(step.placed(), applied.origin.unwrap().placed());
        assert_eq!(step.transaction(), applied.transaction);
        assert_eq!(
            (step.before(), step.after()),
            (applied.before, applied.after)
        );
        assert_eq!(
            step.transaction_sha256(),
            common::sha256(applied.transaction)
        );
    }
    assert_eq!(proposal.base(), steps[0].before());
    assert_eq!(proposal.result(), revision(&session));
    assert_eq!(proposal.catalog(), session.catalog().revision());
    assert_eq!(proposal.actor().as_str(), AGENT);
    assert_eq!(proposal.editor(), EDITOR);
    assert_eq!(proposal.client(), Some("client\u{1b}[31m"));
    assert!(!proposal.client_display().unwrap().contains('\u{1b}'));
    assert_eq!(proposal.receipt(), RECEIPT);
    assert_eq!(Proposal::decode(proposal.bytes()).unwrap(), proposal);
    // Encoded exactly: the recorded actions are the dispatched ones.
    let body = body(proposal.bytes());
    for (kind, step) in body["steps"].as_array().unwrap().iter().enumerate() {
        let expected = steps[kind].before();
        assert_eq!(
            step["action"],
            encode_action(&edit(kind, expected)).unwrap()
        );
    }
}

#[test]
fn building_refuses_a_pending_change_and_an_empty_history() {
    let root = world(|_| {});
    let mut session = sandbox(root.path());
    let mut records = Records::default();
    let error = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalEmpty);
    let expected = revision(&session);
    accept(&mut session, edit(0, expected), &mut records);
    assert!(Proposal::from_session(&session, EDITOR, RECEIPT, None).is_ok());
    let expected = revision(&session);
    session.dispatch(edit(1, expected), &mut records).unwrap();
    let error = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // A label over its bound.
    let error =
        Proposal::from_session(&session, EDITOR, RECEIPT, Some(&"x".repeat(257))).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalBudget);
    // An undone step leaves nothing to record.
    accept(&mut session, Action::Undo, &mut records);
    let error = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap_err();
    assert_eq!(error.code(), RefusalCode::ProposalEmpty);
}

#[test]
fn a_removed_or_reordered_step_breaks_the_chain() {
    let root = world(|_| {});
    let mut session = sandbox(root.path());
    let mut records = Records::default();
    for kind in 0..3 {
        let expected = revision(&session);
        accept(&mut session, edit(kind, expected), &mut records);
    }
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    let mut removed = body(proposal.bytes());
    removed["steps"].as_array_mut().unwrap().remove(1);
    let mut reordered = body(proposal.bytes());
    reordered["steps"].as_array_mut().unwrap().swap(1, 2);
    let mut last_dropped = body(proposal.bytes());
    last_dropped["steps"].as_array_mut().unwrap().pop();
    for (name, body) in [
        ("removed", removed),
        ("reordered", reordered),
        ("last dropped", last_dropped),
    ] {
        let error = Proposal::decode(&document(&body)).unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalChain, "{name}: {error}");
    }
    // The whole chain, with its result moved to the shorter end, reads.
    let mut shorter = body(proposal.bytes());
    shorter["steps"].as_array_mut().unwrap().pop();
    shorter["result"] = shorter["steps"][1]["after"].clone();
    assert_eq!(
        Proposal::decode(&document(&shorter)).unwrap().steps().len(),
        2
    );
}

#[test]
fn a_step_the_session_did_not_prepare_is_refused() {
    use tilewright_editor_commands::History;
    use tilewright_editor_document::WorldDocument;
    use tilewright_editor_preview::{PreviewLimits, loader};
    use tilewright_editor_ui::Actor;
    let root = world(|_| {});
    // A History that already has a step before the session is built.
    let mut history = History::new(WorldDocument::open(root.path()).unwrap());
    let mut first = sandbox(root.path());
    let expected = revision(&first);
    first
        .dispatch(edit(2, expected), &mut Records::default())
        .unwrap();
    let text = first.history().pending().unwrap().sealed().to_owned();
    history.prepare(expected, "earlier", &text).unwrap();
    history
        .commit(
            &mut Records::default(),
            &tilewright_edit::PublishOptions::default(),
        )
        .unwrap();
    let document = history.document();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let mut session = Session::new(history, catalog, resources, Actor::new(AGENT).unwrap());
    assert!(session.applied_steps().next().unwrap().origin.is_none());
    let error = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap_err();
    assert_eq!(
        (error.code(), error.step()),
        (RefusalCode::ProposalInvalid, Some(0))
    );
    // A step it prepared after that does not make the earlier one known.
    let expected = revision(&session);
    accept(&mut session, edit(0, expected), &mut Records::default());
    assert_eq!(session.applied_steps().len(), 2);
    let error = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap_err();
    assert_eq!(
        (error.code(), error.step()),
        (RefusalCode::ProposalInvalid, Some(0))
    );
}

#[test]
fn the_editor_version_is_the_callers_and_is_checked() {
    let root = world(|_| {});
    let mut session = sandbox(root.path());
    let expected = revision(&session);
    accept(&mut session, edit(0, expected), &mut Records::default());
    let long = "1".repeat(tilewright_editor_proposal::limits::EDITOR_BYTES + 1);
    for (editor, code) in [
        ("", RefusalCode::ProposalInvalid),
        ("1.2 beta", RefusalCode::ProposalInvalid),
        ("1.2.\u{e9}", RefusalCode::ProposalInvalid),
        (long.as_str(), RefusalCode::ProposalBudget),
    ] {
        let error = Proposal::from_session(&session, editor, RECEIPT, None).unwrap_err();
        assert_eq!(error.code(), code, "{editor:?}");
    }
    let longest = "1".repeat(tilewright_editor_proposal::limits::EDITOR_BYTES);
    let proposal = Proposal::from_session(&session, &longest, RECEIPT, None).unwrap();
    assert_eq!(proposal.editor(), longest);
}

/// The writer's exact bytes for a whole document: member order, framing
/// and escapes, so that a change of the JSON library's output is noticed.
#[test]
fn the_writer_emits_this_exact_document() {
    // The committed catalog: the golden names its digest.
    let root = common::plain_world(|_| {});
    let mut session = sandbox(root.path());
    let expected = revision(&session);
    accept(&mut session, edit(0, expected), &mut Records::default());
    let proposal = Proposal::from_session(
        &session,
        EDITOR,
        RECEIPT,
        Some("tab\t \u{1} \u{1f600} \"q\""),
    )
    .unwrap();
    let text = String::from_utf8(proposal.bytes().to_vec()).unwrap();
    // Written once from this writer; a change is a format change.
    assert_eq!(text, include_str!("golden/proposal-move.json"));
    assert_eq!(Proposal::decode(text.as_bytes()).unwrap(), proposal);
}
