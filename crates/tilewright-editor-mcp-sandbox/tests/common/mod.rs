// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! What the MCP editing server's tests share: a sandbox laid out as the
//! provisioner lays it out, the server over it on in-memory streams,
//! request builders, the checks every reply gets, and goldens.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_mcp::{Endpoint, PROTOCOL_VERSION};
use tilewright_editor_mcp_sandbox::{Editing, output_schema, server};
use tilewright_editor_observe::node::json_string;
use tilewright_editor_preview::{PreviewLimits, ReferenceFrame, loader};
use tilewright_editor_proposal::ProposalStore;
use tilewright_editor_recovery::DurableRecovery;
use tilewright_editor_sandbox::{Sandbox, Settings};
use tilewright_editor_ui::{Actor, Session};
use tilewright_source::SourceRoot;

pub const ACTOR: &str = "agent:mcp-tester";
pub const RECEIPT: [u8; 32] = [9; 32];
pub const EDITOR: &str = "0.1.0-test";

/// A stand-in for the binary's PNG encoder, as in the read-only server's
/// tests: the image's size as text, and a fixed digest.
#[allow(clippy::unnecessary_wraps)]
pub fn fake_encode(frame: &ReferenceFrame) -> Option<(Vec<u8>, [u8; 32])> {
    Some((
        format!("{}x{}", frame.width, frame.height).into_bytes(),
        [7; 32],
    ))
}

pub fn copy_tree(source: &Path, destination: &Path) {
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

pub fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// Every file of a tree, by relative path.
pub fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
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

fn private(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        if path.is_dir() {
            private(&path);
        }
    }
}

/// A sandbox laid out as the provisioner lays it out: `world/` (the
/// neutral fixture, changed by `configure`), `recovery/`, `proposals/`.
pub struct Layout {
    pub root: tempfile::TempDir,
}

impl Layout {
    pub fn new(configure: impl FnOnce(&Path)) -> Self {
        let root = tempfile::Builder::new()
            .prefix("tilewright-mcp-sandbox-")
            .tempdir_in(
                std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
                    .map_or_else(std::env::temp_dir, PathBuf::from),
            )
            .unwrap();
        copy_tree(
            &tilewright_editor_testkit::fixture_world("prototype-neutral"),
            &root.path().join("world"),
        );
        configure(&root.path().join("world"));
        fs::create_dir(root.path().join("recovery")).unwrap();
        fs::create_dir(root.path().join("proposals")).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        private(root.path());
        Self { root }
    }

    pub fn path(&self, child: &str) -> PathBuf {
        self.root.path().join(child)
    }

    pub fn files(&self, child: &str) -> BTreeMap<String, Vec<u8>> {
        files(&self.path(child))
    }

    pub fn settings(client: Option<&str>) -> Settings {
        Settings {
            client: client.map(str::to_owned),
            receipt: RECEIPT,
            editor: EDITOR.into(),
        }
    }

    /// The server on this sandbox, opened as `sandbox-mcp` opens it.
    pub fn open(&self, client: Option<&str>) -> Client<DurableRecovery> {
        let sandbox = Sandbox::open(
            &self.path("world"),
            SourceRoot::new(&self.path("recovery")),
            SourceRoot::new(&self.path("proposals")),
            Actor::new(ACTOR).unwrap(),
            Self::settings(client),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        Client::new(server(sandbox, &self.path("world"), EDITOR, fake_encode))
    }

    /// The server on this sandbox with another recovery store.
    pub fn open_with<R: RecoveryStore>(&self, recovery: R) -> Client<R> {
        let document = WorldDocument::open_prototype(&self.path("world")).unwrap();
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
            Actor::new(ACTOR).unwrap(),
        );
        let store = ProposalStore::open(SourceRoot::new(&self.path("proposals"))).unwrap();
        let sandbox = Sandbox::new(session, recovery, store, Self::settings(None)).unwrap();
        Client::new(server(sandbox, &self.path("world"), EDITOR, fake_encode))
    }
}

/// The `_meta` every request needs.
pub fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION,
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

pub fn request(id: i64, method: &str, mut params: Value) -> String {
    params["_meta"] = meta();
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

pub fn call_line(id: i64, tool: &str, arguments: &Value) -> String {
    request(
        id,
        "tools/call",
        json!({"name": tool, "arguments": arguments}),
    )
}

/// The raw text of `structuredContent` in a reply line.
pub fn raw_structured(line: &str) -> &str {
    let key = ",\"structuredContent\":";
    let start = line.find(key).unwrap() + key.len();
    let end = line.rfind(",\"isError\":").unwrap();
    &line[start..end]
}

/// Every check a reply line gets: nothing on it the CLI's escaping would
/// escape, it parses strictly, and it is a JSON-RPC 2.0 response whose
/// result names this server.
pub fn checked_reply(line: &str) -> Value {
    for character in line.chars() {
        assert!(
            !tilewright_editor_observe::node::escaped(character),
            "raw U+{:04X} on stdout",
            u32::from(character)
        );
    }
    tilewright_jsonc::Document::parse(line, &tilewright_jsonc::Limits::default())
        .unwrap_or_else(|error| panic!("{error}: {line}"));
    let reply: Value = serde_json::from_str(line).unwrap();
    assert_eq!(reply["jsonrpc"], "2.0");
    assert_ne!(reply.get("result").is_some(), reply.get("error").is_some());
    if let Some(result) = reply.get("result") {
        assert_eq!(result["resultType"], "complete");
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["title"],
            "Tilewright editor (sandbox editing)"
        );
    }
    reply
}

/// A tool call's result, checked: the text copy is the structured result
/// as sent, which conforms to the tool's output schema, and `isError` is
/// a refusal's.
pub struct Called {
    pub line: String,
    pub reply: Value,
    pub structured: Option<Value>,
    /// Every string of the structured result, and whether its schema
    /// marks it world text.
    pub strings: Vec<(String, bool)>,
}

impl Called {
    pub fn result(&self) -> &Value {
        &self.reply["result"]
    }
    pub fn is_error(&self) -> bool {
        self.result()["isError"] == true
    }
    pub fn structured(&self) -> &Value {
        self.structured.as_ref().expect("structured content")
    }
    /// A successful editing result, or an observe tool's.
    pub fn ok(&self) -> &Value {
        assert!(!self.is_error(), "{}", self.line);
        self.structured()
    }
    /// An editing tool's refusal with `code`.
    pub fn refused(&self, code: &str) -> &Value {
        assert!(self.is_error(), "{}", self.line);
        let structured = self.structured();
        assert_eq!(structured["ok"], false, "{}", self.line);
        assert_eq!(structured["code"], code, "{}", self.line);
        structured
    }
}

pub fn checked_call(tool: &str, line: String, reply: Value) -> Called {
    let result = &reply["result"];
    assert!(result.is_object(), "{line}");
    let image = if tool == "render" {
        tilewright_editor_mcp::MAXIMUM_IMAGE_BYTES * 4 / 3 + 64
    } else {
        0
    };
    assert!(
        line.len() <= tilewright_editor_mcp::MAXIMUM_RESULT_BYTES + 512 + image,
        "{tool}: {} bytes",
        line.len()
    );
    let observe = tilewright_editor_mcp::OBSERVE_TOOLS.contains(&tool);
    if observe && result["isError"] == true {
        // The read-only server's errors: one line, no structured content.
        assert!(result.get("structuredContent").is_none());
        let first = result["content"][0]["text"].as_str().unwrap();
        assert!(first.starts_with(&format!("{tool} failed: ")), "{first}");
        return Called {
            line,
            reply,
            structured: None,
            strings: Vec::new(),
        };
    }
    let raw = raw_structured(&line).to_owned();
    assert_eq!(result["content"][0]["type"], "text");
    assert_eq!(
        result["content"][0]["text"].as_str().unwrap(),
        raw,
        "the text copy is the structured result as sent"
    );
    let schema = output_schema(tool).unwrap();
    let parsed =
        tilewright_jsonc::Document::parse(raw.clone(), &tilewright_jsonc::Limits::default())
            .unwrap();
    let strings = schema
        .check(parsed.root())
        .unwrap_or_else(|problem| panic!("{tool}: {problem}\n{raw}"));
    let structured: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(structured, result["structuredContent"]);
    if !observe {
        assert_eq!(
            result["isError"],
            structured["ok"] == false,
            "isError is a refusal's: {line}"
        );
        assert_eq!(structured["request"], tool);
    }
    Called {
        line,
        reply,
        structured: Some(structured),
        strings,
    }
}

/// The server over in-memory streams.
pub struct Client<R> {
    pub endpoint: Endpoint<Editing<R>>,
}

impl<R: RecoveryStore> Client<R> {
    pub fn new(endpoint: Endpoint<Editing<R>>) -> Self {
        Self { endpoint }
    }

    /// Serves `input` and returns the raw output and the log.
    pub fn serve_raw(&mut self, input: &[u8]) -> (String, String) {
        let mut output = Vec::new();
        let mut log = Vec::new();
        self.endpoint
            .serve(Cursor::new(input.to_vec()), &mut output, &mut log)
            .unwrap();
        (
            String::from_utf8(output).expect("UTF-8 output"),
            String::from_utf8(log).expect("UTF-8 log"),
        )
    }

    /// Each line in, each reply out, checked.
    pub fn session(&mut self, lines: &[String]) -> Vec<(String, Value)> {
        let mut input = String::new();
        for line in lines {
            input.push_str(line);
            input.push('\n');
        }
        let (output, _) = self.serve_raw(input.as_bytes());
        output
            .lines()
            .map(|line| (line.to_owned(), checked_reply(line)))
            .collect()
    }

    pub fn call(&mut self, tool: &str, arguments: &Value) -> Called {
        let mut replies = self.session(&[call_line(1, tool, arguments)]);
        assert_eq!(replies.len(), 1);
        let (line, reply) = replies.remove(0);
        checked_call(tool, line, reply)
    }

    pub fn simple(&mut self, tool: &str) -> Called {
        self.call(tool, &json!({}))
    }

    pub fn revision(&mut self) -> String {
        self.simple("status").ok()["state"]["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// An edit of `action`, its `expected` the current revision.
    pub fn edit(&mut self, mut action: Value) -> Called {
        action["expected"] = Value::from(self.revision());
        self.call("edit", &json!({ "action": action }))
    }
}

/// A move of `marker_1`, which the neutral catalog grants.
pub fn move_marker(x: i64, y: i64) -> Value {
    json!({"kind": "move", "uid": "marker_1", "x": x, "y": y})
}

/// A paint of the floor grid.
pub fn paint(x: i64, y: i64, proto: &str) -> Value {
    json!({"kind": "paint", "plane": "main", "layer": 0, "grid": "floor", "x": x, "y": y, "proto": proto})
}

/// Set `marker_1`'s label.
pub fn label(text: &str) -> Value {
    json!({"kind": "set_field", "uid": "marker_1", "field": "label", "value": text})
}

/// Whether a directory's write permission binds this process: as root it
/// does not, and a test that relies on it has nothing to test.
pub fn permissions_bind(directory: &Path) -> bool {
    let probe = directory.join("probe");
    if fs::write(&probe, b"").is_ok() {
        fs::remove_file(&probe).unwrap();
        eprintln!("skipped: permissions do not bind this process (root?)");
        return false;
    }
    true
}

/// `value` indented two spaces a level, members in the order sent.
fn pretty(value: &tilewright_jsonc::Value, depth: usize, output: &mut String) {
    use tilewright_jsonc::Value as V;
    let indent = |depth: usize, output: &mut String| output.push_str(&"  ".repeat(depth));
    match value {
        V::Null { .. } => output.push_str("null"),
        V::Bool { value, .. } => output.push_str(&value.to_string()),
        V::Integer { value, .. } => output.push_str(&value.to_string()),
        V::Float { value, .. } => {
            let _ = write!(output, "{value:?}");
        }
        V::String { value, .. } => json_string(value, output),
        V::Array { items, .. } if items.is_empty() => output.push_str("[]"),
        V::Array { items, .. } => {
            output.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(",\n");
                }
                indent(depth + 1, output);
                pretty(item, depth + 1, output);
            }
            output.push('\n');
            indent(depth, output);
            output.push(']');
        }
        V::Object { members, .. } if members.is_empty() => output.push_str("{}"),
        V::Object { members, .. } => {
            output.push_str("{\n");
            for (index, member) in members.iter().enumerate() {
                if index > 0 {
                    output.push_str(",\n");
                }
                indent(depth + 1, output);
                json_string(&member.key, output);
                output.push_str(": ");
                pretty(&member.value, depth + 1, output);
            }
            output.push('\n');
            indent(depth, output);
            output.push('}');
        }
    }
}

/// `text` with every run of 64 lowercase hex digits (revisions, digests,
/// snapshot bindings, which line endings change) replaced by `<hex N>`,
/// numbered by first appearance, so equal digests stay visibly equal.
fn placeholders(text: &str, seen: &mut Vec<String>) -> String {
    let bytes = text.as_bytes();
    let mut output = String::new();
    let mut at = 0;
    while at < bytes.len() {
        let run = bytes[at..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
            .count();
        let starts_word = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        if run >= 64 && starts_word {
            let digest = &text[at..at + 64];
            let index = seen
                .iter()
                .position(|known| known == digest)
                .unwrap_or_else(|| {
                    seen.push(digest.to_owned());
                    seen.len() - 1
                });
            let _ = write!(output, "<hex {index}>");
            at += 64;
        } else {
            let step = text[at..].chars().next().unwrap().len_utf8();
            output.push_str(&text[at..at + step]);
            at += step;
        }
    }
    output
}

/// Compares reply lines with `tests/golden/<file>`, line endings aside,
/// members in the order sent; a tool result's text copy, already checked
/// equal to its structured content, is written as a placeholder.
/// `TILEWRIGHT_UPDATE_GOLDEN=1` rewrites the file instead.
pub fn golden(file: &str, lines: &[&str]) {
    let mut seen = Vec::new();
    let mut actual = String::new();
    for line in lines {
        let mut line = (*line).to_owned();
        if line.contains(",\"structuredContent\":") {
            let mut copy = String::new();
            json_string(raw_structured(&line), &mut copy);
            line = line.replacen(&copy, "\"<the structured content, serialized as sent>\"", 1);
        }
        let parsed =
            tilewright_jsonc::Document::parse(line, &tilewright_jsonc::Limits::default()).unwrap();
        let mut text = String::new();
        pretty(parsed.root(), 0, &mut text);
        actual.push_str(&placeholders(&text, &mut seen));
        actual.push('\n');
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    if std::env::var_os("TILEWRIGHT_UPDATE_GOLDEN").is_some() {
        fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "golden {file} differs");
}
