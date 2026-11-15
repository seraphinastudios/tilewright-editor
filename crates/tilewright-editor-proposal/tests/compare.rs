// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A step prepared again equals its recorded transaction in everything
//! but the actor.

mod common;

use common::{Records, edit, sandbox, world};
use serde_json::{Value, json};
use tilewright_editor_proposal::compare_transactions;
use tilewright_jsonc as jsonc;

/// The sealed transaction of a move inside one chunk: tests, an expect
/// and a set, with base and result digests.
fn sealed() -> String {
    let root = world(|_| {});
    let mut session = sandbox(root.path());
    let expected = session.history().document().revision();
    session
        .dispatch(edit(0, expected), &mut Records::default())
        .unwrap();
    session.history().pending().unwrap().sealed().to_owned()
}

/// `text` with the value at `path` replaced by `value`, every other byte
/// kept.
fn changed(text: &str, path: &[jsonc::Segment<'_>], value: &Value) -> String {
    let document = jsonc::Document::parse(text, &jsonc::Limits::default()).unwrap();
    let span = document.root().pointer(path).unwrap().span();
    document
        .replace(span, &value.to_string(), &jsonc::Limits::default())
        .unwrap()
        .source()
        .to_owned()
}
use jsonc::Segment::{Index, Key};

#[test]
fn only_the_actor_may_differ() {
    let recorded = sealed();
    let value: Value = serde_json::from_str(&recorded).unwrap();
    let operations = value["operations"].as_array().unwrap().len();
    assert!(operations >= 4, "{recorded}");
    assert_eq!(compare_transactions(&recorded, &recorded), Ok(()));
    let person = recorded.replacen("agent:sandbox \\\"test\\\"", "human:tester", 1);
    assert_ne!(person, recorded);
    assert_eq!(compare_transactions(&recorded, &person), Ok(()));
    // Layout is not part of the value.
    let spaced = recorded
        .replace(",\"", ",\n   \"")
        .replacen('{', "{ // a comment\n", 1);
    assert_ne!(spaced, recorded);
    assert_eq!(compare_transactions(&recorded, &spaced), Ok(()));
    // Member order is.
    let reordered = serde_json::to_string(&value).unwrap();
    assert!(compare_transactions(&recorded, &reordered).is_err());
}

#[test]
fn any_operation_precondition_or_digest_that_differs_is_detected() {
    let recorded = sealed();
    let value: Value = serde_json::from_str(&recorded).unwrap();
    let operations = value["operations"].as_array().unwrap().len();
    let base_path = value["base"][0]["path"].as_str().unwrap().to_owned();
    // Every member of every operation, changed in turn.
    for index in 0..operations {
        let operation = value["operations"][index].as_object().unwrap().clone();
        for key in operation.keys() {
            let old = &value["operations"][index][key];
            let new = if *old == json!(0) { json!(1) } else { json!(0) };
            let other = changed(
                &recorded,
                &[Key("operations"), Index(index), Key(key)],
                &new,
            );
            let divergence = compare_transactions(&recorded, &other).unwrap_err();
            assert_eq!(divergence.members, ["operations"], "{index} {key}");
            assert_eq!(divergence.operation, Some(index), "{index} {key}");
        }
    }
    let mut more = value["operations"].clone();
    more.as_array_mut()
        .unwrap()
        .push(value["operations"][0].clone());
    let one_more = changed(&recorded, &[Key("operations")], &more);
    assert_eq!(
        compare_transactions(&recorded, &one_more)
            .unwrap_err()
            .operation,
        Some(operations)
    );
    for key in ["base", "result"] {
        let other = changed(
            &recorded,
            &[Key(key), Index(0), Key("sha256")],
            &json!("0".repeat(64)),
        );
        let divergence = compare_transactions(&recorded, &other).unwrap_err();
        assert_eq!(divergence.members, [key]);
        let files = if key == "base" {
            divergence.base_files
        } else {
            divergence.result_files
        };
        assert_eq!(files, 2, "{key}: one entry on each side");
        assert!(!divergence.to_string().contains(&base_path), "{divergence}");
    }
    let base = &recorded[jsonc::Document::parse(recorded.as_str(), &jsonc::Limits::default())
        .unwrap()
        .root()
        .member("base")
        .unwrap()
        .key_span
        .start..];
    let without_base = recorded.replacen(base, &base[base.find("\"result\"").unwrap()..], 1);
    let cases = [
        (
            "title",
            changed(&recorded, &[Key("title")], &json!("Other")),
        ),
        (
            "format",
            changed(
                &recorded,
                &[Key("format")],
                &json!("tilewright.transaction/2"),
            ),
        ),
        ("custom", recorded.replacen('{', "{\"custom\":{},", 1)),
        ("base", without_base),
    ];
    for (member, other) in cases {
        let divergence = compare_transactions(&recorded, &other).unwrap_err();
        assert!(
            divergence.members.contains(&member),
            "{member}: {divergence:?}"
        );
        assert!(!divergence.unreadable);
    }
}

#[test]
fn a_side_without_a_string_actor_or_strict_json_is_unreadable() {
    let recorded = sealed();
    let actor = changed(&recorded, &[Key("actor")], &json!(null));
    let cases = [
        actor
            .replacen("\"actor\":null,", "", 1)
            .replacen("\"actor\": null,", "", 1),
        actor,
        recorded.replacen('{', "{\"title\":\"x\",", 1),
        "[]".to_owned(),
        String::new(),
    ];
    assert!(!cases[0].contains("\"actor\""), "{}", cases[0]);
    for other in &cases {
        for (left, right) in [(recorded.as_str(), other.as_str()), (other, &recorded)] {
            assert!(
                compare_transactions(left, right).unwrap_err().unreadable,
                "{other}"
            );
        }
    }
}
