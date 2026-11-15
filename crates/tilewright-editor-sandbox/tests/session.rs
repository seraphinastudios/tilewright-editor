// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The runner's requests on a sandbox laid out as the provisioner lays it
//! out: edits, review, accept, cancel, undo and redo, submit, and what
//! each leaves on disk. Linux only, as the stores are.
#![cfg(target_os = "linux")]

mod common;

use common::{ACTOR, EDITOR, Layout, RECEIPT, label, move_marker, ok, paint, refused};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tilewright_editor_proposal::{hex, read_proposal};
use tilewright_editor_sandbox::Sandbox;
use tilewright_editor_ui::Actor;
use tilewright_source::SourceRoot;

const HOSTILE: &str = "Bell\u{7}\u{1b}[2J \u{202e}reversed\u{200b} \"quoted\" \\ </tag>";

#[test]
#[allow(clippy::too_many_lines)]
fn a_session_edits_reviews_accepts_and_submits_only_in_its_sandbox() {
    let layout = Layout::new(|_| {});
    let original = layout.files("world");
    let mut driver = layout.open(Some("client \u{1b}[31m"));
    let status = ok(driver_status(&mut driver));
    assert_eq!(status["actor"], ACTOR);
    assert_eq!(status["client"], "client \u{1b}[31m");
    assert_eq!(status["editor"], EDITOR);
    assert_eq!(status["receipt"], hex(&RECEIPT));
    assert_eq!(status["catalog_verified"], true);
    assert_eq!(
        status["view"],
        json!({"plane": "main", "layer": 0, "bounds": [0, 0, 8, 6]})
    );
    assert_eq!(status["proposals"], 0);
    let base = status["state"]["revision"].as_str().unwrap().to_owned();
    assert_eq!(
        status["state"],
        json!({"revision": base, "phase": "ready", "pending": false, "undo": 0, "redo": 0})
    );

    // An edit prepares a pending change and returns its review.
    let edit = ok(driver.edit(move_marker(4, 2)));
    let pending = &edit["pending"];
    assert_eq!(pending["base"], base.as_str());
    assert_ne!(pending["result"], base.as_str());
    assert_eq!(pending["complete"], true);
    assert_eq!(pending["cut"], false);
    assert_eq!(
        pending["files"],
        json!([{"path": "planes/main/layers/0/0_0.jsonc", "change": "modified",
            "before_bytes": pending["files"][0]["before_bytes"], "after_bytes": pending["files"][0]["after_bytes"]}])
    );
    assert!(!pending["review"].as_array().unwrap().is_empty());
    assert_eq!(edit["state"]["phase"], "pending");
    assert_eq!(edit["state"]["pending"], true);
    // Nothing is written before Accept.
    assert_eq!(layout.files("world"), original);
    assert_eq!(ok(driver.simple("review"))["pending"], *pending);
    let accepted = ok(driver.simple("accept"));
    assert_eq!(accepted["applied"]["before"], base.as_str());
    assert_eq!(accepted["applied"]["after"], pending["result"]);
    assert_eq!(accepted["applied"]["placed"], Value::Null);
    assert_eq!(accepted["state"]["undo"], 1);
    assert_ne!(layout.files("world"), original);

    // World text comes back as escaped data.
    let hostile = ok(driver.edit(label(HOSTILE)));
    let review = hostile["pending"]["review"].to_string();
    assert!(review.contains("reversed"), "{review}");
    ok(driver.simple("accept"));

    // A grant the catalog does not give is refused, and nothing changes.
    let before = layout.files("world");
    refused(&driver.edit(paint(0, 0, "stone")), "grant.missing");
    assert_eq!(layout.files("world"), before);

    // Cancel drops a pending change without writing anything.
    ok(driver.edit(paint(0, 0, "sand")));
    let cancelled = ok(driver.simple("cancel"));
    assert_eq!(cancelled["pending"], Value::Null);
    assert_eq!(cancelled["state"]["pending"], false);
    assert_eq!(layout.files("world"), before);

    // Undo and redo are reviewed changes: each prepares, and Accept
    // publishes it.
    let undo = ok(driver.simple("undo"));
    assert_eq!(undo["state"]["phase"], "pending");
    assert_eq!(undo["state"]["undo"], 2);
    assert_eq!(layout.files("world"), before);
    let undone = ok(driver.simple("accept"));
    assert_eq!(undone["applied"], Value::Null);
    assert_eq!(undone["state"]["undo"], 1);
    assert_eq!(undone["state"]["redo"], 1);
    ok(driver.simple("redo"));
    let redone = ok(driver.simple("accept"));
    assert_eq!(redone["state"]["undo"], 2);
    assert_eq!(redone["state"]["redo"], 0);
    assert_eq!(layout.files("world"), before);

    // A placement reports the uid it assigned.
    ok(driver.edit(
        json!({"kind": "place", "plane": "main", "layer": 0, "x": 2, "y": 2, "proto": "stone"}),
    ));
    let placed = ok(driver.simple("accept"));
    let assigned = placed["applied"]["placed"].as_str().unwrap().to_owned();
    assert!(assigned.starts_with("o."), "a generated object id");

    // Submit retains one proposal file of the applied steps.
    let submitted = ok(driver.simple("submit"));
    let proposal = &submitted["proposal"];
    assert_eq!(proposal["steps"], 3);
    assert_eq!(proposal["base"], base.as_str());
    assert_eq!(proposal["result"], submitted["state"]["revision"]);
    assert_eq!(proposal["existed"], false);
    let name = proposal["file"].as_str().unwrap().to_owned();
    assert_eq!(
        name,
        format!(
            "proposal-{}.json",
            proposal["body_sha256"].as_str().unwrap()
        )
    );
    let files = layout.files("proposals");
    assert_eq!(files.keys().collect::<Vec<_>>(), [&name]);
    assert_eq!(proposal["bytes"], files[&name].len());
    let read = read_proposal(&SourceRoot::new(&layout.path("proposals")), &name).unwrap();
    assert_eq!(read.actor().as_str(), ACTOR);
    assert_eq!(read.client(), Some("client \u{1b}[31m"));
    assert_eq!(read.receipt(), RECEIPT);
    assert_eq!(read.editor(), EDITOR);
    assert_eq!(read.steps()[2].placed(), Some(assigned.as_str()));
    // Submitting the same steps again is the same file, written once.
    let again = ok(driver.simple("submit"));
    assert_eq!(again["proposal"]["file"], name.as_str());
    assert_eq!(again["proposal"]["existed"], true);
    assert_eq!(layout.files("proposals"), files);

    // The session goes on after a submit; a later submit is a new proposal
    // of every applied step from the sandbox's start.
    ok(driver.edit(paint(1, 0, "sand")));
    refused(&driver.simple("submit"), "pending.exists");
    ok(driver.simple("accept"));
    let later = ok(driver.simple("submit"));
    assert_eq!(later["proposal"]["steps"], 4);
    assert_eq!(later["proposal"]["base"], base.as_str());
    assert_ne!(later["proposal"]["file"], name.as_str());
    assert_eq!(layout.files("proposals").len(), 2);
    assert_eq!(ok(driver_status(&mut driver))["proposals"], 3);

    // The sandbox holds nothing but the three directories, and recovery
    // holds one inverse per accepted change.
    let mut entries: Vec<_> = fs::read_dir(layout.root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    entries.sort();
    assert_eq!(entries, ["proposals", "recovery", "world"]);
    assert_eq!(layout.files("recovery").len(), 6);
    // Quit ends serving: the request after it is not read.
    let (replies, _) = driver.serve(b"{\"request\":\"quit\"}\n{\"request\":\"status\"}\n");
    assert_eq!(replies.len(), 1);
    assert_eq!(common::reply(&replies[0])["request"], "quit");
}

fn driver_status<R: tilewright_editor_commands::RecoveryStore>(
    driver: &mut common::Driver<R>,
) -> Value {
    driver.simple("status")
}

#[test]
fn refusals_are_replies_with_the_engines_codes_and_change_nothing() {
    let layout = Layout::new(|_| {});
    let original = layout.files("world");
    let mut driver = layout.open(None);
    let revision = driver.revision();
    refused(&driver.simple("accept"), "pending.none");
    refused(&driver.simple("submit"), "proposal.empty");
    refused(&driver.simple("undo"), "history.nothing_to_undo");
    refused(&driver.simple("redo"), "history.nothing_to_redo");
    // Cancel with nothing pending is what the engine makes of it: nothing.
    ok(driver.simple("cancel"));
    // A stale revision.
    let mut stale = move_marker(4, 2);
    stale["expected"] = Value::from("0".repeat(64));
    refused(
        &driver.request(&json!({"request": "edit", "action": stale})),
        "revision.stale",
    );
    // An edit while one is pending, and undo or redo then.
    ok(driver.edit(move_marker(4, 2)));
    refused(&driver.edit(move_marker(5, 2)), "pending.exists");
    refused(&driver.simple("undo"), "pending.exists");
    refused(&driver.simple("submit"), "pending.exists");
    ok(driver.simple("cancel"));
    // Targets, scope and input the engine refuses.
    refused(
        &driver.edit(json!({"kind": "remove", "uid": "missing"})),
        "target.unknown",
    );
    refused(&driver.edit(paint(100, 0, "sand")), "scope.outside_view");
    // Every refusal names the state it left.
    let reply = driver.edit(move_marker(4, 2));
    assert_eq!(reply["state"]["revision"], revision.as_str());
    ok(driver.simple("cancel"));
    assert_eq!(driver.revision(), revision);
    assert_eq!(layout.files("world"), original);
    assert!(layout.files("recovery").is_empty());
    assert!(layout.files("proposals").is_empty());
}

#[test]
fn malformed_oversized_and_unknown_lines_each_get_one_refusal_and_serving_goes_on() {
    let layout = Layout::new(|_| {});
    let mut driver = layout.open(None);
    let oversized = format!(
        "{{\"request\":\"edit\",\"action\":{}}}",
        label(&"x".repeat(tilewright_editor_sandbox::MAXIMUM_REQUEST_BYTES))
    );
    let mut input = Vec::new();
    for line in [
        "not json",
        "",
        "[1]",
        "{\"request\":\"status\",}",
        "{\"request\":\"status\"} {}",
        "{\"request\":\"shell\"}",
        "{\"request\":\"status\",\"path\":\"/etc\"}",
        "{\"request\":\"edit\",\"action\":{\"kind\":\"move\"}}",
        oversized.as_str(),
        "{\"request\":\"status\"}",
    ] {
        input.extend_from_slice(line.as_bytes());
        input.push(b'\n');
    }
    input.extend_from_slice(b"{\"request\":\"st\xffatus\"}\n{\"request\":\"status\"}");
    let (replies, log) = driver.serve(&input);
    let codes: Vec<Value> = replies
        .iter()
        .map(|line| {
            let reply = common::reply(line);
            if reply["ok"] == true {
                Value::Null
            } else {
                reply["code"].clone()
            }
        })
        .collect();
    assert_eq!(
        codes,
        [
            json!("request.parse"),
            json!("request.parse"),
            json!("request.invalid"),
            json!("request.parse"),
            json!("request.parse"),
            json!("request.unknown"),
            json!("request.invalid"),
            json!("proposal.invalid"),
            json!("request.too_large"),
            Value::Null,
            json!("request.not_utf8"),
            Value::Null,
        ]
    );
    assert_eq!(
        log.lines().count(),
        2,
        "the startup line and the oversized line: {log}"
    );
}

#[test]
fn an_incomplete_review_cannot_be_accepted() {
    // `marker_1`'s label is too large for a complete review.
    let layout = Layout::new(|world| {
        common::edit_json(world, "profile/schema/object.jsonc", |schema| {
            schema["fields"]["label"]["max_length"] = Value::from(80_000);
        });
        common::edit_json(world, "planes/main/layers/0/0_0.jsonc", |chunk| {
            chunk["objects"][0]["fields"]["label"] = Value::String("a\"".repeat(35_000));
        });
    });
    let original = layout.files("world");
    let mut driver = layout.open(None);
    let edit = ok(driver.edit(label(&"b".repeat(100))));
    let pending = &edit["pending"];
    assert_eq!(pending["complete"], false);
    assert_eq!(
        pending["review"][0],
        tilewright_editor_ui::review::REVIEW_LIMIT_MESSAGE
    );
    refused(&driver.simple("accept"), "review.incomplete");
    assert_eq!(driver.simple("status")["state"]["pending"], true);
    assert_eq!(layout.files("world"), original);
    ok(driver.simple("cancel"));
    assert_eq!(layout.files("world"), original);
    assert!(layout.files("recovery").is_empty());
}

#[test]
fn replies_over_the_cap_keep_the_first_entries_and_count_the_rest() {
    let layout = Layout::new(|_| {});
    let full = {
        let mut driver = layout.open(None);
        let reply = ok(driver.edit(label(HOSTILE)));
        ok(driver.simple("cancel"));
        reply
    };
    let lines = full["pending"]["review"].as_array().unwrap().len();
    assert!(lines >= 3, "{full}");
    assert_eq!(full["pending"]["cut"], false);
    let mut driver = layout.open(None);
    let mut partial = 0;
    // From room for the fixed members only, up to room for everything.
    for cap in (4096..4096 + 1024).step_by(8) {
        driver.sandbox.set_reply_limit(cap);
        let line = {
            let mut action = label(HOSTILE);
            action["expected"] = Value::from(driver.revision());
            json!({"request": "edit", "action": action}).to_string()
        };
        let (replies, _) = driver.serve(format!("{line}\n").as_bytes());
        assert!(replies[0].len() <= cap, "{cap}: {}", replies[0].len());
        let reply = common::reply(&replies[0]);
        let pending = &reply["pending"];
        let kept = pending["review"].as_array().unwrap();
        let omitted = usize::try_from(pending["review_omitted"].as_u64().unwrap()).unwrap();
        assert_eq!(kept.len() + omitted, lines, "{cap}");
        // What is kept is the start of the full review, in order, and the
        // reply says whether anything was cut.
        assert_eq!(
            kept[..],
            full["pending"]["review"].as_array().unwrap()[..kept.len()]
        );
        let files = pending["files"].as_array().unwrap().len();
        let files_omitted = usize::try_from(pending["files_omitted"].as_u64().unwrap()).unwrap();
        assert_eq!(files + files_omitted, 1, "{cap}");
        assert_eq!(pending["cut"], omitted + files_omitted > 0, "{cap}");
        if omitted > 0 && !kept.is_empty() {
            partial += 1;
        }
        ok(driver.simple("cancel"));
    }
    assert!(partial > 0);
    driver.sandbox.set_reply_limit(usize::MAX);
    assert_eq!(ok(driver.edit(label(HOSTILE)))["pending"], full["pending"]);
}

#[test]
fn a_sandbox_opens_only_on_provisioned_stores_with_an_agent_actor() {
    let open = |layout: &Layout, actor: &str, client: Option<&str>| {
        Sandbox::open(
            &layout.path("world"),
            SourceRoot::new(&layout.path("recovery")),
            SourceRoot::new(&layout.path("proposals")),
            Actor::new(actor).unwrap(),
            Layout::settings(client),
        )
        .map(|_| ())
        .map_err(|error| error.0)
    };
    let layout = Layout::new(|_| {});
    assert_eq!(open(&layout, ACTOR, None), Ok(()));
    for actor in ["human:tester", "editor:prototype", "agent:", "Agent:x"] {
        let error = open(&layout, actor, None).unwrap_err();
        assert!(error.contains("agent:<name>"), "{actor}: {error}");
    }
    assert!(open(&layout, ACTOR, Some(&"x".repeat(257))).is_err());
    assert!(open(&layout, ACTOR, Some("")).is_err());
    assert_eq!(open(&layout, ACTOR, Some(&"x".repeat(256))), Ok(()));
    // A recovery store that is not private, or already has records, as a
    // tree that was not freshly provisioned would.
    let shared = Layout::new(|_| {});
    fs::set_permissions(shared.path("recovery"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(open(&shared, ACTOR, None).unwrap_err().contains("recovery"));
    let used = Layout::new(|_| {});
    fs::write(used.path("recovery/inverse-0001.json"), b"{}").unwrap();
    assert!(open(&used, ACTOR, None).unwrap_err().contains("recovery"));
    let public = Layout::new(|_| {});
    fs::set_permissions(public.path("proposals"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        open(&public, ACTOR, None)
            .unwrap_err()
            .contains("proposal store")
    );
    let missing = Layout::new(|_| {});
    fs::remove_dir(missing.path("proposals")).unwrap();
    assert!(open(&missing, ACTOR, None).is_err());
    // One process per store: a second runner on the same sandbox is
    // refused while the first holds it.
    let first = layout.open(None);
    assert!(open(&layout, ACTOR, None).is_err());
    drop(first);
    assert_eq!(open(&layout, ACTOR, None), Ok(()));
}
