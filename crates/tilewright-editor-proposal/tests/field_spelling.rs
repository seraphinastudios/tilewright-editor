// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A recorded `set_field` on a number stored as a whole number (`2`, not
//! `2.0`) applies to the person's copy: the step's transaction expects the
//! value as the source spells it.
#![cfg(target_os = "linux")]

mod common;

use common::{EDITOR, PERSON, RECEIPT, Records, accept, open, revision, sandbox, world};
use serde_json::json;
use std::{fs, path::Path};
use tilewright_editor_proposal::{Progress, Proposal, Settled};
use tilewright_editor_ui::Action;

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

/// A number field `weight`, granted on `marker_1` and stored there as `2`.
fn whole_weight(root: &Path) {
    common::edit_json(root, "profile/schema/object.jsonc", |schema| {
        schema["fields"]["weight"] = json!({"type": "number"});
    });
    common::edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["fields"] = json!([{"uid": "marker_1", "field": "weight"}]);
    });
    let path = root.join(CHUNK);
    let mut chunk: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    chunk["objects"][0]["fields"] = json!("FIELDS");
    let text = serde_json::to_string_pretty(&chunk).unwrap().replacen(
        "\"FIELDS\"",
        "{\"label\": \"Trail marker\", \"weight\": 2}",
        1,
    );
    fs::write(path, text).unwrap();
}

#[test]
fn a_recorded_change_of_a_whole_number_applies_to_the_persons_copy() {
    let agent_world = world(whole_weight);
    let person_world = world(whole_weight);
    let mut session = sandbox(agent_world.path());
    let mut records = Records::default();
    let expected = revision(&session);
    let action = |value, expected| Action::SetField {
        uid: "marker_1".into(),
        field: "weight".into(),
        value,
        expected,
    };
    accept(&mut session, action(json!(1.25), expected), &mut records);
    let expected = revision(&session);
    // Back to a whole number, now spelled `2.0`, and changed once more.
    accept(&mut session, action(json!(2.0), expected), &mut records);
    let expected = revision(&session);
    accept(&mut session, action(json!(-0.5), expected), &mut records);
    let proposal = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    assert_eq!(proposal.steps().len(), 3);

    let mut person = open(person_world.path(), PERSON);
    let (mut progress, notices) = Progress::begin(&proposal, &person).unwrap();
    assert!(notices.is_empty());
    let mut records = Records::default();
    while !progress.finished(&proposal) {
        let step = progress
            .prepare_step(&proposal, &mut person, &mut records)
            .unwrap_or_else(|error| panic!("{} {error}", error.code()));
        person.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(
            progress.settle(&proposal, &person).unwrap(),
            Settled::Applied { step }
        );
    }
    assert_eq!(revision(&person), proposal.result());
    assert_eq!(
        fs::read(person_world.path().join(CHUNK)).unwrap(),
        fs::read(agent_world.path().join(CHUNK)).unwrap()
    );
}
