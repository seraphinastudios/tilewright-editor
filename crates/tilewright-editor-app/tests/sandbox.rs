// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The whole loop with the binary: `tools/provision-sandbox.py` makes a
//! sandbox from a copy of the neutral fixture and starts the runner in
//! it; a script drives the runner over its standard input and output and
//! submits a proposal; a second provisioned copy, the person's world,
//! applies that proposal step by step under another actor; the two copies
//! end byte-identical and the source tree is untouched.
//!
//! The provisioner refuses volatile storage: run with
//! `TILEWRIGHT_CHECKED_TEST_ROOT` (or `TMPDIR`) on persistent local
//! storage, as CI and `tools/validate.sh` do.
#![cfg(target_os = "linux")]

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use tilewright_editor_commands::History;
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_proposal::{Progress, Settled, read_proposal};
use tilewright_editor_recovery::{DurableRecovery, RecoveryLimits};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Actor, Session};
use tilewright_source::SourceRoot;

const AGENT: &str = "agent:e2e";
const PERSON: &str = "human:e2e";
const LABEL: &str = "e2e client \u{1b}[31m\u{202e}";
const HOSTILE: &str = "Sign \u{1b}]0;title\u{7} \u{202e}desrever \"q\" \\ <b>";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("tilewright-sandbox-e2e-")
        .tempdir_in(
            std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
                .map_or_else(std::env::temp_dir, PathBuf::from),
        )
        .unwrap()
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Every entry of a tree by relative path: file bytes, or `None` for a
/// directory.
fn tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                pending.push(path);
                found.insert(relative, None);
            } else {
                found.insert(relative, Some(std::fs::read(&path).unwrap()));
            }
        }
    }
    found
}

/// The runner, through the provisioner, and its two streams.
struct Runner {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Runner {
    fn start(source: &Path, parent: &Path) -> Self {
        let mut child = Command::new("python3")
            .arg("-I")
            .arg(repository().join("tools/provision-sandbox.py"))
            .arg("--world")
            .arg(source)
            .arg("--destination-parent")
            .arg(parent)
            .args(["--actor", AGENT, "--client-label", LABEL, "--editor"])
            .arg(env!("CARGO_BIN_EXE_tilewright-editor"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }
    /// One request, and its one reply line, checked and parsed.
    fn send(&mut self, request: &Value) -> Value {
        writeln!(self.input, "{request}").unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        let read = self.output.read_line(&mut line).unwrap();
        assert!(
            read > 0,
            "the runner ended; is the test root on persistent storage? {request}"
        );
        let line = line.strip_suffix('\n').unwrap();
        assert!(
            !line
                .chars()
                .any(|character| character < ' ' || character == '\u{202e}'),
            "unescaped text in {line}"
        );
        serde_json::from_str(line).unwrap()
    }
    fn ok(&mut self, request: &Value) -> Value {
        let reply = self.send(request);
        assert_eq!(reply["ok"], true, "{request} {reply}");
        reply
    }
    fn simple(&mut self, request: &str) -> Value {
        self.ok(&json!({ "request": request }))
    }
    fn revision(&mut self) -> String {
        self.simple("status")["state"]["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn edit(&mut self, mut action: Value) -> Value {
        action["expected"] = Value::from(self.revision());
        self.send(&json!({"request": "edit", "action": action}))
    }
    /// An edit, accepted.
    fn step(&mut self, action: Value) -> Value {
        let edit = self.edit(action);
        assert_eq!(edit["ok"], true, "{edit}");
        assert_eq!(edit["pending"]["complete"], true);
        self.simple("accept")
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_sandbox_proposal_applies_step_by_step_to_a_persons_copy() {
    let scratch = scratch();
    let root = scratch.path();
    let fixture = tree(&fixture_world("prototype-neutral"));
    let source = root.join("source");
    copy_tree(&fixture_world("prototype-neutral"), &source);
    let original = tree(&source);
    let parent = root.join("sandboxes");
    std::fs::create_dir(&parent).unwrap();

    let mut runner = Runner::start(&source, &parent);
    let status = runner.simple("status");
    assert_eq!(status["actor"], AGENT);
    assert_eq!(status["client"], LABEL);
    let base = status["state"]["revision"].as_str().unwrap().to_owned();
    runner.step(json!({"kind": "move", "uid": "marker_1", "x": 4, "y": 2}));
    let hostile = runner
        .edit(json!({"kind": "set_field", "uid": "marker_1", "field": "label", "value": HOSTILE}));
    assert!(
        hostile["pending"]["review"]
            .to_string()
            .contains("desrever")
    );
    runner.simple("accept");
    // A missing grant: stone is not a floor value the catalog grants.
    let refused = runner.edit(json!({"kind": "paint", "plane": "main", "layer": 0,
        "grid": "floor", "x": 0, "y": 0, "proto": "stone"}));
    assert_eq!(refused["code"], "grant.missing", "{refused}");
    runner.step(
        json!({"kind": "paint_stroke", "plane": "main", "layer": 0, "grid": "floor",
        "cells": [[0, 1], [1, 1]], "proto": "sand"}),
    );
    let placed = runner.step(
        json!({"kind": "place", "plane": "main", "layer": 0, "x": 2, "y": 2, "proto": "stone"}),
    );
    let uid = placed["applied"]["placed"].as_str().unwrap().to_owned();
    runner.step(
        json!({"kind": "create_zone", "plane": "main", "zone": "camp", "name": "Camp",
        "parent": null}),
    );
    runner
        .step(json!({"kind": "create_anchor", "plane": "main", "anchor": "camp", "x": 2, "y": 2}));
    // Cancel, then an undo and a redo, each reviewed and accepted.
    let pending = runner.edit(json!({"kind": "paint", "plane": "main", "layer": 0,
        "grid": "floor", "x": 0, "y": 0, "proto": "sand"}));
    assert_eq!(pending["ok"], true, "{pending}");
    assert_eq!(runner.simple("cancel")["state"]["pending"], false);
    assert_eq!(runner.simple("undo")["state"]["pending"], true);
    assert_eq!(runner.simple("accept")["state"]["undo"], 5);
    runner.simple("redo");
    assert_eq!(runner.simple("accept")["state"]["undo"], 6);
    runner.step(json!({"kind": "set_portal", "uid": "gate_west",
        "target": {"kind": "anchor", "plane": "main", "anchor": "camp"}}));
    let submitted = runner.simple("submit");
    let proposal = &submitted["proposal"];
    assert_eq!(proposal["steps"], 7);
    assert_eq!(proposal["base"], base.as_str());
    let name = proposal["file"].as_str().unwrap().to_owned();
    runner.simple("quit");
    drop(runner.input);
    let status = runner.child.wait().unwrap();
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut runner.child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert!(status.success(), "{stderr}");
    // The provisioner's notice and the runner's ready line, escaped.
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert!(!stderr.contains('\u{1b}'));
    let notice: Value =
        serde_json::from_str(lines[0].strip_prefix("tilewright sandbox: ").unwrap()).unwrap();
    assert!(lines[1].starts_with("tilewright-editor sandbox: ready actor=agent:e2e revision="));
    let sandbox = PathBuf::from(notice["sandbox"].as_str().unwrap());
    assert_eq!(sandbox.parent().unwrap(), parent);
    let receipt = std::fs::read(sandbox.join("provision.json")).unwrap();
    assert_eq!(
        status_receipt(&notice),
        tilewright_editor_proposal::hex(&sha256(&receipt))
    );

    // The sandbox holds its receipt and three directories; the proposal
    // directory holds the one proposal.
    let entries: Vec<String> = tree(&sandbox)
        .into_keys()
        .filter(|path| !path.contains('/'))
        .collect();
    assert_eq!(
        entries,
        ["proposals", "provision.json", "recovery", "world"]
    );
    let proposals = tree(&sandbox.join("proposals"));
    assert_eq!(proposals.keys().collect::<Vec<_>>(), [&name]);
    let proposal = read_proposal(&SourceRoot::new(&sandbox.join("proposals")), &name).unwrap();
    assert_eq!(proposal.actor().as_str(), AGENT);
    assert_eq!(proposal.client(), Some(LABEL));
    assert_eq!(proposal.receipt(), sha256(&receipt));
    assert_eq!(proposal.editor(), env!("CARGO_PKG_VERSION"));
    assert_eq!(proposal.steps()[3].placed(), Some(uid.as_str()));
    // The source was copied, never written.
    assert_eq!(tree(&source), original);
    assert_ne!(tree(&sandbox.join("world")), original);

    // The person's world: a second provisioned copy of the same source.
    let person = root.join("person");
    let provisioned = Command::new("python3")
        .arg("-I")
        .arg(repository().join("tools/provision-prototype.py"))
        .arg("--world")
        .arg(&source)
        .arg("--destination")
        .arg(&person)
        .output()
        .unwrap();
    assert!(provisioned.status.success(), "{provisioned:?}");
    assert_eq!(tree(&person.join("world")), original);
    let document = WorldDocument::open_prototype(&person.join("world")).unwrap();
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
        Actor::new(PERSON).unwrap(),
    );
    let mut recovery = DurableRecovery::from_provisioned_session_root(
        SourceRoot::new(&person.join("recovery")),
        RecoveryLimits::default(),
    )
    .unwrap();
    let (mut progress, notices) = Progress::begin(&proposal, &session).unwrap();
    assert!(notices.is_empty());
    while !progress.finished(&proposal) {
        let step = progress
            .prepare_step(&proposal, &mut session, &mut recovery)
            .unwrap_or_else(|error| panic!("{} {error}", error.code()));
        assert!(session.review().complete);
        session.dispatch(Action::Accept, &mut recovery).unwrap();
        assert_eq!(
            progress.settle(&proposal, &session).unwrap(),
            Settled::Applied { step }
        );
    }
    assert_eq!(session.history().document().revision(), proposal.result());
    for applied in session.applied_steps() {
        assert_eq!(applied.origin.unwrap().actor().as_str(), PERSON);
    }
    assert_eq!(tree(&person.join("world")), tree(&sandbox.join("world")));
    // Neither the source nor the fixture changed.
    assert_eq!(tree(&source), original);
    assert_eq!(tree(&fixture_world("prototype-neutral")), fixture);
    // Every directory the provisioners made is private.
    for directory in [&sandbox, &sandbox.join("proposals"), &person] {
        let mode = std::fs::metadata(directory).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{}", directory.display());
    }
}

fn status_receipt(notice: &Value) -> String {
    notice["receipt_sha256"].as_str().unwrap().to_owned()
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).into()
}
