// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The whole loop over MCP with the binary: `tools/provision-sandbox.py
//! --mcp` makes a sandbox from a copy of the neutral fixture and starts
//! `tilewright-editor sandbox-mcp` in it; a client drives it over its
//! standard input and output with MCP tool calls, edits, undoes and redoes,
//! observes, and submits a proposal; the client closes the server's input;
//! a second provisioned copy, the person's world, applies the proposal step
//! by step under another actor; the two copies end byte-identical and the
//! source tree is untouched.
//!
//! The provisioner refuses volatile storage: run with
//! `TILEWRIGHT_CHECKED_TEST_ROOT` (or `TMPDIR`) on persistent local
//! storage, as CI and `tools/validate.sh` do.
#![cfg(target_os = "linux")]

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
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

const AGENT: &str = "agent:mcp-e2e";
const PERSON: &str = "human:mcp-e2e";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

/// The MCP editing server, through the provisioner, and its two streams.
struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    next: i64,
}

impl Client {
    fn start(source: &Path, parent: &Path) -> Self {
        let mut child = Command::new("python3")
            .arg("-I")
            .arg(repository().join("tools/provision-sandbox.py"))
            .arg("--world")
            .arg(source)
            .arg("--destination-parent")
            .arg(parent)
            .args([
                "--actor",
                AGENT,
                "--client-label",
                "an MCP client",
                "--mcp",
                "--editor",
            ])
            .arg(env!("CARGO_BIN_EXE_tilewright-editor"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
            next: 1,
        }
    }

    /// One request, and its one reply, checked and parsed.
    fn send(&mut self, method: &str, mut params: Value) -> Value {
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {}
        });
        let id = self.next;
        self.next += 1;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        let read = self.output.read_line(&mut line).unwrap();
        assert!(
            read > 0,
            "the server ended; is the test root on persistent storage? {request}"
        );
        assert!(
            !line
                .trim_end_matches('\n')
                .chars()
                .any(|character| character < ' ' || character == '\u{202e}'),
            "unescaped text in {line}"
        );
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], id, "{line}");
        reply
    }

    /// A tool call's structured content, after checking its text copy.
    #[allow(clippy::needless_pass_by_value)]
    fn call(&mut self, tool: &str, arguments: Value) -> (bool, Value) {
        let reply = self.send("tools/call", json!({"name": tool, "arguments": arguments}));
        let result = &reply["result"];
        let structured = result["structuredContent"].clone();
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, structured, "{reply}");
        (result["isError"] == true, structured)
    }

    fn ok(&mut self, tool: &str, arguments: Value) -> Value {
        let (error, structured) = self.call(tool, arguments);
        assert!(!error, "{tool}: {structured}");
        structured
    }

    fn revision(&mut self) -> String {
        self.ok("status", json!({}))["state"]["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// An edit, reviewed complete, and accepted.
    fn step(&mut self, mut action: Value) -> Value {
        action["expected"] = json!(self.revision());
        let edit = self.ok("edit", json!({ "action": action }));
        assert_eq!(edit["pending"]["complete"], true);
        self.ok("accept", json!({}))
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn an_mcp_sandbox_proposal_applies_step_by_step_to_a_persons_copy() {
    let scratch = tempfile::Builder::new()
        .prefix("tilewright-mcp-sandbox-e2e-")
        .tempdir_in(
            std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
                .map_or_else(std::env::temp_dir, PathBuf::from),
        )
        .unwrap();
    let root = scratch.path();
    let fixture = tree(&fixture_world("prototype-neutral"));
    let source = root.join("source");
    copy_tree(&fixture_world("prototype-neutral"), &source);
    // A scatter palette in the source catalog, so a scatter step can be
    // recorded and replayed.
    let catalog_path = source.join("preview.jsonc");
    let mut catalog: Value =
        serde_json::from_slice(&std::fs::read(&catalog_path).unwrap()).unwrap();
    catalog["editing"]["palettes"] = json!([{"name": "speckle", "grid": "floor",
        "entries": [{"value": "grass", "weight": 3}, {"value": "sand", "weight": 1}]}]);
    std::fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    let original = tree(&source);
    let parent = root.join("sandboxes");
    std::fs::create_dir(&parent).unwrap();

    let mut client = Client::start(&source, &parent);
    let discover = client.send("server/discover", json!({}));
    assert_eq!(
        discover["result"]["supportedVersions"],
        json!(["2026-07-28"])
    );
    let listed = client.send("tools/list", json!({}));
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 13);
    let status = client.ok("status", json!({}));
    assert_eq!(status["actor"], AGENT);
    assert_eq!(status["client"], "an MCP client");
    // The server and the sandbox report the version `version` prints.
    let version = Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
        .arg("version")
        .output()
        .unwrap()
        .stdout;
    let version = String::from_utf8(version).unwrap();
    let version = version.split_whitespace().nth(1).unwrap();
    assert_eq!(
        discover["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["version"],
        version
    );
    assert_eq!(status["editor"], version);
    let base = status["state"]["revision"].as_str().unwrap().to_owned();
    client.step(json!({"kind": "move", "uid": "marker_1", "x": 4, "y": 2}));
    // A refusal is an error result with the runner's code.
    let mut refused = json!({"kind": "paint", "plane": "main", "layer": 0, "grid": "floor",
        "x": 0, "y": 0, "proto": "stone"});
    refused["expected"] = json!(client.revision());
    let (error, refusal) = client.call("edit", json!({ "action": refused }));
    assert!(error);
    assert_eq!(refusal["code"], "grant.missing");
    client.step(
        json!({"kind": "paint_stroke", "plane": "main", "layer": 0, "grid": "floor",
        "cells": [[0, 1], [1, 1]], "proto": "sand"}),
    );
    let placed = client.step(
        json!({"kind": "place", "plane": "main", "layer": 0, "x": 2, "y": 2, "proto": "stone"}),
    );
    assert!(placed["applied"]["placed"].is_string());
    client.step(
        json!({"kind": "create_zone", "plane": "main", "zone": "camp", "name": "Camp",
        "parent": null}),
    );
    // An undo and a redo, each reviewed and accepted.
    client.ok("undo", json!({}));
    assert_eq!(client.ok("accept", json!({}))["state"]["undo"], 3);
    client.ok("redo", json!({}));
    assert_eq!(client.ok("accept", json!({}))["state"]["undo"], 4);
    // Every grass cell left in the view becomes sand.
    client.step(
        json!({"kind": "fill_terrain", "plane": "main", "layer": 0, "grid": "floor",
        "x": 3, "y": 3, "proto": "sand", "contiguous": false}),
    );
    // A scatter, recorded with its palette name and seed; the catalog tool
    // lists the palette.
    assert_eq!(
        client.ok("catalog", json!({}))["palettes"][0]["name"],
        "speckle"
    );
    client.step(
        json!({"kind": "scatter_terrain", "plane": "main", "layer": 0, "grid": "floor",
        "cells": [[0, 0], [0, 1], [1, 0], [1, 1], [2, 0], [2, 1]], "palette": "speckle", "seed": 1234}),
    );
    // The observe tools see the sandbox at its revision, and render
    // returns the real encoder's PNG.
    let now = client.revision();
    let tile = client.ok("tile", json!({"plane": "main", "layer": 0, "x": 2, "y": 2}));
    assert_eq!(tile["revision"], now.as_str());
    let reply = client.send(
        "tools/call",
        json!({"name": "render", "arguments": {"revision": now}}),
    );
    let image = &reply["result"]["content"][1];
    assert_eq!(image["mimeType"], "image/png");
    assert!(image["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    let submitted = client.ok("submit", json!({}));
    let proposal = &submitted["proposal"];
    assert_eq!(proposal["steps"], 6);
    assert_eq!(proposal["base"], base.as_str());
    let name = proposal["file"].as_str().unwrap().to_owned();
    // The client ends the session by closing the server's input.
    drop(client.input.take());
    let status = client.child.wait().unwrap();
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut client.child.stderr.take().unwrap(), &mut stderr).unwrap();
    assert!(status.success(), "{stderr}");
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    let notice: Value =
        serde_json::from_str(lines[0].strip_prefix("tilewright sandbox: ").unwrap()).unwrap();
    assert_eq!(
        lines[1],
        format!("tilewright-editor sandbox-mcp: ready actor={AGENT} revision={base}")
    );
    let sandbox = PathBuf::from(notice["sandbox"].as_str().unwrap());
    let proposal = read_proposal(&SourceRoot::new(&sandbox.join("proposals")), &name).unwrap();
    assert_eq!(proposal.actor().as_str(), AGENT);
    assert_eq!(proposal.client(), Some("an MCP client"));
    assert_eq!(tree(&source), original);

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
    assert_eq!(tree(&source), original);
    assert_eq!(tree(&fixture_world("prototype-neutral")), fixture);

    // A second start of the same command makes a new sandbox.
    let mut again = Client::start(&source, &parent);
    assert_eq!(again.revision(), base);
    drop(again.input.take());
    assert!(again.child.wait().unwrap().success());
    assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 2);
}
