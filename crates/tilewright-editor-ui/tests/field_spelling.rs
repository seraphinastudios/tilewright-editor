// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A `SetField` on a stored value expects the value as the source spells
//! it. World compares a `set`'s `expect` by kind and canonical text, and
//! canonical text keeps each number's spelling, so `2` is not `2.0`, `2.50`
//! is not `2.5` and `-0` is not `0`; objects keep their member order.
#![cfg(target_os = "linux")]

use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, Actor, SelectionKey, Session, inspector::inspect, tools::ToolState,
};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

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

#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}

/// The neutral fixture with a schema of every field type, each granted on
/// `marker_1`, whose `fields` are exactly `fields`, written as given.
fn fixture(fields: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    fs::write(
        dir.path().join("profile/schema/object.jsonc"),
        r#"{"format":"tilewright.schema/1","fields":{
          "flag":{"type":"boolean"},
          "count":{"type":"integer"},
          "weight":{"type":"number"},
          "label":{"type":"string","max_length":64},
          "settings":{"type":"object","fields":{"active":{"type":"boolean"},"visits":{"type":"integer"}}},
          "data":{"type":"json"}
        }}"#,
    )
    .unwrap();
    let path = dir.path().join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    catalog["editing"]["fields"] = serde_json::json!(
        ["flag", "count", "weight", "label", "settings", "data"]
            .map(|field| serde_json::json!({"uid": "marker_1", "field": field}))
    );
    fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    let path = dir.path().join(CHUNK);
    let mut chunk: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(chunk["objects"][0]["uid"], "marker_1");
    chunk["objects"][0]["fields"] = serde_json::json!("FIELDS");
    let text = serde_json::to_string_pretty(&chunk)
        .unwrap()
        .replacen("\"FIELDS\"", fields, 1);
    fs::write(path, text).unwrap();
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let session = Session::new(
        History::new(document),
        catalog,
        resources,
        Actor::new("editor:prototype").unwrap(),
    );
    (dir, session)
}

/// The canonical text of `text`, as World compares it.
fn canonical(text: &str) -> String {
    let document =
        tilewright_jsonc::Document::parse(text, &tilewright_jsonc::Limits::default()).unwrap();
    document.canonical(document.root())
}

/// The canonical text of the pending transaction's first `expect`.
fn pending_expect(session: &Session) -> String {
    let sealed = session.history().pending().unwrap().sealed();
    let document =
        tilewright_jsonc::Document::parse(sealed, &tilewright_jsonc::Limits::default()).unwrap();
    let operation = &document
        .root()
        .get("operations")
        .and_then(tilewright_jsonc::Value::as_array)
        .unwrap()[0];
    document.canonical(operation.get("expect").unwrap())
}

fn set(session: &Session, field: &str, value: serde_json::Value) -> Action {
    Action::SetField {
        uid: "marker_1".into(),
        field: field.into(),
        value,
        expected: session.history().document().revision(),
    }
}

#[test]
fn a_stored_value_is_expected_as_the_source_spells_it() {
    let cases: &[(&str, &str, serde_json::Value)] = &[
        // Whole numbers of a number field, as an integer token and as
        // number tokens of other spellings.
        ("weight", "2", serde_json::json!(1.25)),
        ("weight", "2.0", serde_json::json!(1.25)),
        ("weight", "2.50", serde_json::json!(1.25)),
        ("weight", "2e0", serde_json::json!(1.25)),
        ("weight", "1E+3", serde_json::json!(1.25)),
        ("weight", "0", serde_json::json!(1.25)),
        ("weight", "-0", serde_json::json!(1.25)),
        ("weight", "-0.0", serde_json::json!(1.25)),
        ("weight", "1e300", serde_json::json!(1.25)),
        ("weight", "-1.5e-300", serde_json::json!(1.25)),
        ("weight", "-9007199254740991", serde_json::json!(1.25)),
        // Above 2^53 a whole number is a number token; it rounds as an f64.
        ("weight", "9007199254740993.0", serde_json::json!(1.25)),
        (
            "weight",
            "0.1000000000000000055511151231257827",
            serde_json::json!(1.25),
        ),
        ("count", "7", serde_json::json!(5)),
        ("count", "-0", serde_json::json!(5)),
        ("count", "-9007199254740991", serde_json::json!(5)),
        ("flag", "false", serde_json::json!(true)),
        ("label", r#""tab\t quote\" é \/ é""#, serde_json::json!("x")),
        // An object keeps its member order, and its numbers their spelling.
        (
            "settings",
            r#"{"visits": 3, "active": true}"#,
            serde_json::json!({"active": false, "visits": 1}),
        ),
        (
            "data",
            r#"{"b": [2.50, -0, 1e3], "a": {"z": null, "y": 1.0}}"#,
            serde_json::json!([1]),
        ),
    ];
    for (field, stored, value) in cases {
        let (dir, mut session) = fixture(&format!("{{\"{field}\": {stored}}}"));
        let path = dir.path().join(CHUNK);
        let original = fs::read_to_string(&path).unwrap();
        let mut records = Records::default();
        session
            .dispatch(set(&session, field, value.clone()), &mut records)
            .unwrap_or_else(|error| panic!("{field} stored as {stored}: {error}"));
        assert_eq!(
            pending_expect(&session),
            canonical(stored),
            "{field} {stored}"
        );
        session.dispatch(Action::Accept, &mut records).unwrap();
        let written = serde_json::to_string(value).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original.replacen(
                &format!("\"{field}\": {stored}"),
                &format!("\"{field}\": {written}"),
                1
            ),
            "{field} {stored}"
        );
        // The inverse World built restores the exact spelling.
        session.dispatch(Action::Undo, &mut records).unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original,
            "{field} {stored}"
        );
        // And the stored spelling is expected again after the round trip.
        session
            .dispatch(set(&session, field, value.clone()), &mut records)
            .unwrap_or_else(|error| panic!("{field} restored as {stored}: {error}"));
        assert_eq!(
            pending_expect(&session),
            canonical(stored),
            "{field} {stored}"
        );
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
}

#[test]
fn a_value_changed_on_disk_since_the_frame_still_conflicts() {
    let (dir, mut session) = fixture(r#"{"weight": 2}"#);
    let path = dir.path().join(CHUNK);
    let original = fs::read_to_string(&path).unwrap();
    let mut records = Records::default();
    let action = set(&session, "weight", serde_json::json!(1.25));
    fs::write(
        &path,
        original.replacen("\"weight\": 2", "\"weight\": 2.0", 1),
    )
    .unwrap();
    // The spelling is read from the source the session holds; the full
    // source revision still fences an edit made outside it.
    assert!(session.dispatch(action, &mut records).is_err());
    assert!(records.0.is_empty());
}

#[test]
fn the_inspector_edits_a_whole_number_and_writes_numbers_as_today() {
    let (dir, mut session) = fixture(r#"{"weight": 2}"#);
    let path = dir.path().join(CHUNK);
    let original = fs::read_to_string(&path).unwrap();
    let key = SelectionKey::Object {
        uid: "marker_1".into(),
    };
    let mut records = Records::default();
    // A number draft's value is an f64: whole numbers are written with
    // `.0`, and an exponent is written out.
    for (typed, written) in [
        ("2", "2.0"),
        ("2.5", "2.5"),
        ("1e3", "1000.0"),
        ("-0", "-0.0"),
    ] {
        let revision = session.history().document().revision();
        let view = inspect(session.world(), session.catalog(), Some(&key), 4, revision);
        let mut tools = ToolState::default();
        tools.begin_field(&view, "weight", false).unwrap();
        while tools.draft_text().is_some_and(|text| !text.is_empty()) {
            tools.backspace();
        }
        tools.input(typed).unwrap();
        let action = tools.build_action(&view, false).unwrap();
        session
            .dispatch(action, &mut records)
            .unwrap_or_else(|error| panic!("{typed}: {error}"));
        assert_eq!(pending_expect(&session), "2");
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original.replacen("\"weight\": 2", &format!("\"weight\": {written}"), 1),
            "{typed}"
        );
        session.dispatch(Action::Undo, &mut records).unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
