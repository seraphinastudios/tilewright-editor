// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The typed JSON form of the edit Actions: exact round trips and strict,
//! fail-closed reading.

mod common;

use common::{KINDS, edit};
use serde_json::{Value, json};
use tilewright_editor_proposal::{decode_action, encode_action, limits};
use tilewright_editor_ui::{Action, PortalTarget, RefusalCode, SelectionKey};
use tilewright_jsonc as jsonc;

fn read(text: &str) -> Result<Action, RefusalCode> {
    let document = jsonc::Document::parse(text, &jsonc::Limits::default()).unwrap();
    decode_action(document.root()).map_err(|error| error.code())
}

fn round_trip(action: &Action) {
    let text = encode_action(action).unwrap().to_string();
    assert_eq!(read(&text).as_ref(), Ok(action), "{text}");
    // One encoding: writing what was read gives the same bytes.
    assert_eq!(
        encode_action(&read(&text).unwrap()).unwrap().to_string(),
        text
    );
}

const MAX: i64 = limits::INTEGER;

#[test]
fn every_edit_kind_round_trips_exactly_with_edge_values() {
    let revision: [u8; 32] = std::array::from_fn(|index| u8::try_from(index * 7).unwrap());
    for kind in 0..KINDS {
        round_trip(&edit(kind, revision));
    }
    let odd = "quote \" backslash \\ newline \n bidi \u{202e} snow \u{2603}".to_owned();
    let long = "x".repeat(limits::TEXT_BYTES);
    let edges = vec![
        Action::Move {
            uid: odd.clone(),
            x: MAX,
            y: -MAX,
            expected: [0xff; 32],
        },
        Action::SetField {
            uid: String::new(),
            field: long.clone(),
            value: json!({"b": [1.5, -0.0, 1e300, null, true, MAX, -MAX], "a": {"deep": [[[[[["7"]]]]]]}}),
            expected: [0; 32],
        },
        Action::Paint {
            plane: odd.clone(),
            layer: i32::MIN,
            grid: "g".into(),
            x: 0,
            y: -1,
            proto: long.clone(),
            expected: revision,
        },
        Action::PaintStroke {
            plane: "p".into(),
            layer: i32::MAX,
            grid: "g".into(),
            cells: (0..i64::try_from(limits::CELLS).unwrap())
                .map(|x| [x, -x])
                .collect(),
            proto: "t".into(),
            expected: revision,
        },
        Action::PaintZones {
            plane: "p".into(),
            layer: 0,
            cells: Vec::new(),
            zone: None,
            expected: revision,
        },
        Action::CreateZone {
            plane: "p".into(),
            zone: "z".into(),
            name: None,
            parent: Some(odd.clone()),
            expected: revision,
        },
        Action::RenameZone {
            plane: "p".into(),
            zone: "z".into(),
            name: None,
            expected: revision,
        },
        Action::SetZoneParent {
            plane: "p".into(),
            zone: "z".into(),
            parent: None,
            expected: revision,
        },
        Action::SetPortal {
            uid: "u".into(),
            target: None,
            expected: revision,
        },
        Action::SetPortal {
            uid: "u".into(),
            target: Some(PortalTarget::Location {
                plane: odd,
                layer: -3,
                at: [-MAX, MAX],
            }),
            expected: revision,
        },
    ];
    for action in &edges {
        round_trip(action);
    }
}

#[test]
fn actions_that_are_not_edits_have_no_form() {
    let select = Action::Select {
        generation: 1,
        key: SelectionKey::Object { uid: "u".into() },
    };
    for action in [
        select,
        Action::ClearSelection,
        Action::Accept,
        Action::Cancel,
        Action::Undo,
        Action::Redo,
        Action::Reopen,
    ] {
        let error = encode_action(&action).unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalInvalid, "{action:?}");
    }
}

#[test]
fn values_outside_the_bounds_are_refused_when_writing() {
    let revision = [1; 32];
    let moved = |x| Action::Move {
        uid: "u".into(),
        x,
        y: 0,
        expected: revision,
    };
    let field = |value: Value| Action::SetField {
        uid: "u".into(),
        field: "f".into(),
        value,
        expected: revision,
    };
    let stroke = |cells: usize| Action::PaintStroke {
        plane: "p".into(),
        layer: 0,
        grid: "g".into(),
        cells: vec![[0, 0]; cells],
        proto: "t".into(),
        expected: revision,
    };
    let mut deep = json!(1);
    for _ in 0..=limits::FIELD_VALUE_DEPTH {
        deep = json!([deep]);
    }
    let cases = [
        (moved(MAX + 1), RefusalCode::ProposalInvalid),
        (moved(i64::MIN), RefusalCode::ProposalInvalid),
        (field(json!(u64::MAX)), RefusalCode::ProposalInvalid),
        (
            field(json!({"a": ["nul \u{0}"]})),
            RefusalCode::ProposalInvalid,
        ),
        (field(json!({"nul \u{0}": 1})), RefusalCode::ProposalInvalid),
        (
            Action::Remove {
                uid: "nul \u{0}".into(),
                expected: revision,
            },
            RefusalCode::ProposalInvalid,
        ),
        (field(json!([MAX + 1])), RefusalCode::ProposalInvalid),
        (field(deep), RefusalCode::ProposalBudget),
        (
            field(json!("x".repeat(limits::FIELD_VALUE_BYTES))),
            RefusalCode::ProposalBudget,
        ),
        (stroke(limits::CELLS + 1), RefusalCode::ProposalBudget),
        (
            Action::Remove {
                uid: "x".repeat(limits::TEXT_BYTES + 1),
                expected: revision,
            },
            RefusalCode::ProposalBudget,
        ),
        (
            Action::RenameZone {
                plane: "p".into(),
                zone: "z".into(),
                name: Some("x".repeat(limits::TEXT_BYTES + 1)),
                expected: revision,
            },
            RefusalCode::ProposalBudget,
        ),
    ];
    for (action, code) in cases {
        let text = format!("{action:?}");
        let error = encode_action(&action).unwrap_err();
        assert_eq!(error.code(), code, "{}", &text[..text.len().min(80)]);
    }
    // The bounds themselves are allowed.
    round_trip(&stroke(limits::CELLS));
    let mut deepest = json!(1);
    for _ in 0..limits::FIELD_VALUE_DEPTH {
        deepest = json!([deepest]);
    }
    round_trip(&field(deepest));
}

#[test]
fn reading_refuses_every_other_form() {
    let good = encode_action(&edit(14, [3; 32])).unwrap();
    assert!(read(&good.to_string()).is_ok());
    let paint = encode_action(&edit(3, [3; 32])).unwrap();
    let place = encode_action(&edit(6, [3; 32])).unwrap();
    let field = encode_action(&edit(1, [3; 32])).unwrap();
    let with = |base: &Value, key: &str, value: Value| {
        let mut changed = base.clone();
        changed[key] = value;
        changed.to_string()
    };
    let without = |base: &Value, key: &str| {
        let mut changed = base.clone();
        changed.as_object_mut().unwrap().remove(key);
        changed.to_string()
    };
    let invalid = [
        with(&good, "kind", json!("select")),
        with(&good, "kind", json!("Set_Portal")),
        with(&good, "kind", json!(1)),
        without(&good, "kind"),
        with(&good, "extra", json!(null)),
        without(&good, "target"),
        without(&good, "expected"),
        with(&good, "expected", json!("AB".repeat(32))),
        with(&good, "expected", json!("ab".repeat(31))),
        with(&good, "expected", json!(null)),
        with(&good, "uid", json!(1)),
        with(&good, "uid", json!(null)),
        with(&good, "target", json!({"kind": "anchor", "plane": "p"})),
        with(
            &good,
            "target",
            json!({"kind": "anchor", "plane": "p", "anchor": "a", "x": 1}),
        ),
        with(
            &good,
            "target",
            json!({"kind": "tile", "plane": "p", "anchor": "a"}),
        ),
        with(
            &good,
            "target",
            json!({"kind": "location", "plane": "p", "layer": 0, "at": [1]}),
        ),
        with(&good, "target", json!("anchor")),
        with(&paint, "layer", json!(i64::from(i32::MAX) + 1)),
        with(&paint, "layer", json!(1.0)),
        with(&paint, "layer", json!("0")),
        with(&paint, "cells", json!([[1, 2, 3]])),
        with(&paint, "cells", json!([[1]])),
        with(&paint, "cells", json!([["1", 2]])),
        with(&paint, "cells", json!([[1.5, 2]])),
        with(&paint, "cells", json!({"0": [1, 2]})),
        with(&place, "x", json!(2.0)),
        with(&place, "x", json!(null)),
        without(&field, "value"),
        "[]".into(),
        "\"move\"".into(),
    ];
    for text in &invalid {
        assert_eq!(read(text), Err(RefusalCode::ProposalInvalid), "{text}");
    }
    let mut deep = json!(1);
    for _ in 0..=limits::FIELD_VALUE_DEPTH {
        deep = json!([deep]);
    }
    let over = [
        with(&field, "value", deep),
        with(&good, "uid", json!("x".repeat(limits::TEXT_BYTES + 1))),
        with(&paint, "cells", json!(vec![[0, 0]; limits::CELLS + 1])),
    ];
    for text in &over {
        assert_eq!(
            read(text),
            Err(RefusalCode::ProposalBudget),
            "{}",
            &text[..80]
        );
    }
}
