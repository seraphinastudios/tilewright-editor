// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A stamp (`place_objects`) in a proposal: `placed` is the list of its
//! uids in object order, verification accepts exactly the listed inserts,
//! and applying it prepares the recorded transaction with those uids.
//! Publishing is checked on Linux only; elsewhere World refuses it.
#![cfg(target_os = "linux")]

mod common;

use common::{
    EDITOR, PERSON, RECEIPT, Records, accept, body, document, edit, fix_digests, open, revision,
    sandbox, world,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use tilewright_editor_proposal::{Progress, Proposal, Settled};
use tilewright_editor_ui::{Action, RefusalCode, Session, uid::format_uid};

/// A change to a proposal body.
type Change = Box<dyn Fn(&mut Value)>;

/// The fixture's stamp, then a single placement, accepted in a sandbox.
const STAMP: usize = 20;
const PLACE: usize = 6;

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                found.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    found
}

fn recorded(root: &Path) -> Proposal {
    let mut session = sandbox(root);
    let mut records = Records::default();
    for kind in [STAMP, PLACE] {
        let expected = revision(&session);
        accept(&mut session, edit(kind, expected), &mut records);
    }
    Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap()
}

fn apply_all(proposal: &Proposal, person: &mut Session) {
    let (mut progress, _) = Progress::begin(proposal, person).unwrap();
    let mut records = Records::default();
    while !progress.finished(proposal) {
        let step = progress
            .prepare_step(proposal, person, &mut records)
            .unwrap_or_else(|error| panic!("{} {error}", error.code()));
        person.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(
            progress.settle(proposal, person).unwrap(),
            Settled::Applied { step }
        );
    }
}

/// The body of `proposal` changed by `change`, with digests made to match
/// again: only verification can refuse it.
fn tampered(proposal: &Proposal, change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut body = body(proposal.bytes());
    change(&mut body);
    fix_digests(&mut body);
    document(&body)
}

/// Edit step 0's transaction text; the edit must change it.
fn transaction(body: &mut Value, edit: impl FnOnce(&str) -> String) {
    let text = body["steps"][0]["transaction"].as_str().unwrap().to_owned();
    let edited = edit(&text);
    assert_ne!(edited, text);
    body["steps"][0]["transaction"] = Value::from(edited);
}

/// Edit step 0's operations, written back as World would read them.
fn operations(body: &mut Value, edit: impl FnOnce(&mut Vec<Value>)) {
    transaction(body, |text| {
        let mut value: Value = serde_json::from_str(text).unwrap();
        edit(value["operations"].as_array_mut().unwrap());
        value.to_string()
    });
}

#[test]
fn a_stamp_records_its_uids_as_a_list_and_applies_exactly() {
    let agent_world = world(|_| {});
    let person_world = world(|_| {});
    let proposal = recorded(agent_world.path());
    let stamp = &proposal.steps()[0];
    let uids = stamp.placed_objects().to_vec();
    assert_eq!(uids.len(), 4);
    assert_eq!(stamp.placed(), None);
    assert_eq!(proposal.steps()[1].placed_objects(), [] as [String; 0]);
    // The document: a list for the stamp, a string for the placement.
    let written = body(proposal.bytes());
    assert_eq!(written["steps"][0]["placed"], json!(uids));
    assert!(written["steps"][1]["placed"].is_string());
    assert_eq!(written["steps"][0]["action"]["kind"], "place_objects");
    // Read back, it is the same proposal.
    let read = Proposal::decode(proposal.bytes()).unwrap();
    assert_eq!(read, proposal);
    // Applied under another actor, it leaves the person's world with
    // exactly the agent's files: the same objects, uids and order.
    let mut person = open(person_world.path(), PERSON);
    apply_all(&proposal, &mut person);
    assert_eq!(revision(&person), proposal.result());
    assert_eq!(files(person_world.path()), files(agent_world.path()));
    let applied = person.applied_steps().next().unwrap();
    assert_eq!(applied.origin.unwrap().placed_objects(), uids.as_slice());
    // And an undo there restores the person's world byte for byte.
    let before = {
        let fresh = world(|_| {});
        files(fresh.path())
    };
    let mut records = Records::default();
    for _ in 0..2 {
        accept(&mut person, Action::Undo, &mut records);
    }
    assert_eq!(files(person_world.path()), before);
}

#[test]
#[allow(clippy::too_many_lines)]
fn verification_accepts_exactly_the_listed_inserts() {
    let agent_world = world(|_| {});
    let proposal = recorded(agent_world.path());
    let uids = proposal.steps()[0].placed_objects().to_vec();
    let place_uid = proposal.steps()[1].placed().unwrap().to_owned();
    let cases: Vec<(&str, Change)> = vec![
        (
            "one uid fewer",
            Box::new(|b| {
                b["steps"][0]["placed"].as_array_mut().unwrap().pop();
            }),
        ),
        (
            "one uid more",
            Box::new(|b| {
                b["steps"][0]["placed"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(format_uid([9; 16])));
            }),
        ),
        (
            "uids reordered",
            Box::new(|b| b["steps"][0]["placed"].as_array_mut().unwrap().swap(0, 1)),
        ),
        (
            "a uid of another object",
            Box::new(|b| b["steps"][0]["placed"][2] = json!(format_uid([9; 16]))),
        ),
        (
            "a uid given twice",
            Box::new({
                let uids = uids.clone();
                move |b| {
                    b["steps"][0]["placed"][1] = json!(uids[0]);
                    operations(b, |operations| {
                        operations[1]["object"]["uid"] = json!(uids[0]);
                    });
                }
            }),
        ),
        (
            "a uid the placement also places",
            Box::new({
                let place_uid = place_uid.clone();
                let uids = uids.clone();
                move |b| {
                    b["steps"][0]["placed"][3] = json!(place_uid);
                    transaction(b, |text| text.replace(&uids[3], &place_uid));
                }
            }),
        ),
        (
            "not generated, in its transaction too",
            Box::new({
                let uids = uids.clone();
                move |b| {
                    b["steps"][0]["placed"][0] = json!("marker_9");
                    transaction(b, |text| text.replace(&uids[0], "marker_9"));
                }
            }),
        ),
        (
            "a single uid",
            Box::new(|b| b["steps"][0]["placed"] = json!(b["steps"][0]["placed"][0])),
        ),
        (
            "no uid",
            Box::new(|b| b["steps"][0]["placed"] = Value::Null),
        ),
        (
            "an empty list",
            Box::new(|b| b["steps"][0]["placed"] = json!([])),
        ),
        (
            "a list of numbers",
            Box::new(|b| b["steps"][0]["placed"] = json!([1, 2, 3, 4])),
        ),
        (
            "a list on the placement",
            Box::new(|b| b["steps"][1]["placed"] = json!([b["steps"][1]["placed"]])),
        ),
        (
            "an extra insert",
            Box::new(|b| {
                operations(b, |operations| {
                    let mut extra = operations[0].clone();
                    extra["object"]["uid"] = json!(format_uid([9; 16]));
                    operations.push(extra);
                });
            }),
        ),
        (
            "an insert missing",
            Box::new(|b| {
                operations(b, |operations| {
                    operations.pop();
                });
            }),
        ),
        (
            "another operation in its place",
            Box::new(|b| {
                operations(b, |operations| {
                    operations[3] = json!({"op": "object.remove", "uid": "marker_1", "expect": {"proto": "marker"}});
                });
            }),
        ),
        (
            "inserts reordered",
            Box::new(|b| operations(b, |operations| operations.swap(1, 3))),
        ),
        (
            "a wrong prototype in the transaction",
            Box::new(|b| {
                operations(b, |operations| {
                    operations[1]["object"]["proto"] = json!("marker");
                });
            }),
        ),
        (
            "a wrong prototype in the action",
            Box::new(|b| b["steps"][0]["action"]["objects"][1]["proto"] = json!("lamp")),
        ),
        (
            "a wrong tile",
            Box::new(|b| operations(b, |operations| operations[2]["at"] = json!([5, 4]))),
        ),
        (
            "another offset in the action",
            Box::new(|b| b["steps"][0]["action"]["objects"][2]["offset"] = json!([0, 1])),
        ),
        (
            "another plane",
            Box::new(|b| operations(b, |operations| operations[0]["plane"] = json!("other"))),
        ),
        (
            "another spatial layer",
            Box::new(|b| operations(b, |operations| operations[0]["layer"] = json!(1))),
        ),
        (
            "an index",
            Box::new(|b| operations(b, |operations| operations[0]["index"] = json!(0))),
        ),
        (
            "an inventory",
            Box::new(|b| {
                operations(b, |operations| {
                    operations[2]["object"]["inventory"] =
                        json!([{"uid": "smuggled_1", "proto": "marker"}]);
                });
            }),
        ),
    ];
    for (name, change) in cases {
        let bytes = tampered(&proposal, change);
        let error = Proposal::decode(&bytes).unwrap_err();
        assert_eq!(
            error.code(),
            RefusalCode::ProposalInvalid,
            "{name}: {error}"
        );
        assert!(matches!(error.step(), Some(0 | 1)), "{name}");
    }
    // More uids than any stamp has objects is over a bound.
    let bytes = tampered(&proposal, |b| {
        b["steps"][0]["placed"] = json!(vec![format_uid([9; 16]); 65]);
    });
    assert_eq!(
        Proposal::decode(&bytes).unwrap_err().code(),
        RefusalCode::ProposalBudget
    );
}

#[test]
fn applying_a_stamp_is_refused_where_the_persons_grants_view_or_world_differ() {
    let agent_world = world(|_| {});
    let proposal = recorded(agent_world.path());
    let taken = proposal.steps()[0].placed_objects()[2].clone();
    for (name, configure, engine) in [
        (
            "no grant for one prototype",
            Box::new(|root: &Path| {
                common::edit_json(root, "preview.jsonc", |catalog| {
                    let objects = catalog["editing"]["objects"].as_array_mut().unwrap();
                    objects.retain(|grant| grant["proto"] != "bush");
                });
            }) as Box<dyn Fn(&Path)>,
            RefusalCode::GrantMissing,
        ),
        (
            "a smaller view",
            Box::new(|root: &Path| {
                common::edit_json(root, "preview.jsonc", |catalog| {
                    catalog["view"]["bounds"] = json!([0, 0, 6, 6]);
                });
            }),
            RefusalCode::ScopeOutsideView,
        ),
        (
            "a uid already in the world",
            Box::new(move |root: &Path| {
                common::edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
                    chunk["objects"].as_array_mut().unwrap().push(
                        json!({"uid": taken, "proto": "marker", "layer": "item", "at": [7, 5]}),
                    );
                });
            }),
            RefusalCode::UidUnavailable,
        ),
    ] {
        // A world that already holds the uid is another revision: the
        // forged proposal is recorded on a copy of that world, so it
        // begins there and only the engine can refuse it.
        let agent = world(|root| {
            if engine == RefusalCode::UidUnavailable {
                configure(root);
            }
        });
        let proposal = if engine == RefusalCode::UidUnavailable {
            forged_base(&proposal, agent.path())
        } else {
            proposal.clone()
        };
        let person_world = world(|root| configure(root));
        let mut person = open(person_world.path(), PERSON);
        let before = files(person_world.path());
        let (mut progress, _) = Progress::begin(&proposal, &person)
            .unwrap_or_else(|error| panic!("{name}: {} {error}", error.code()));
        let error = progress
            .prepare_step(&proposal, &mut person, &mut Records::default())
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalStepRefused, "{name}");
        assert_eq!(error.engine_code(), Some(engine), "{name}");
        assert!(person.history().pending().is_none(), "{name}");
        assert_eq!(files(person_world.path()), before, "{name}");
        assert!(!progress.stopped(), "{name}");
    }
}

/// The stamp step of `proposal` recorded again on the world at `root`
/// with the same uids, as a forger who knows them would: its revisions
/// are that world's, so only the engine can refuse it there.
fn forged_base(proposal: &Proposal, root: &Path) -> Proposal {
    let mut session = sandbox(root);
    let uids = proposal.steps()[0].placed_objects().to_vec();
    let expected = revision(&session);
    let mut action = edit(STAMP, expected);
    if let Action::PlaceObjects { expected: at, .. } = &mut action {
        *at = expected;
    }
    // The world holds the third uid: draw the stamp with fresh uids, then
    // write the recorded ones into the step.
    accept(&mut session, action, &mut Records::default());
    let honest = Proposal::from_session(&session, EDITOR, RECEIPT, None).unwrap();
    let drawn = honest.steps()[0].placed_objects().to_vec();
    let mut body = body(honest.bytes());
    let mut text = body["steps"][0]["transaction"].as_str().unwrap().to_owned();
    for (drawn, recorded) in drawn.iter().zip(&uids) {
        text = text.replace(drawn, recorded);
    }
    body["steps"][0]["transaction"] = Value::from(text);
    body["steps"][0]["placed"] = json!(uids);
    fix_digests(&mut body);
    Proposal::decode(&document(&body)).unwrap_or_else(|error| panic!("{} {error}", error.code()))
}
