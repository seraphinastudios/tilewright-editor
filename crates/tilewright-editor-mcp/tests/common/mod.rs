// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! What the MCP tests share: a server session over in-memory streams,
//! request builders, the checks every reply gets, and world copies.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tilewright_editor_mcp::{PROTOCOL_VERSION, Server};
use tilewright_editor_preview::ReferenceFrame;

/// A stand-in for the binary's PNG encoder, which lives in the app crate:
/// the image's size as text, and a fixed digest. The app's tests run the
/// real one through the binary.
#[allow(clippy::unnecessary_wraps)]
pub fn fake_encode(frame: &ReferenceFrame) -> Option<(Vec<u8>, [u8; 32])> {
    Some((
        format!(
            "{}x{} rgba {}",
            frame.width,
            frame.height,
            frame.rgba8.len()
        )
        .into_bytes(),
        [7; 32],
    ))
}

pub fn server(root: &Path) -> Server {
    Server::open(root, "0.0.0-test", fake_encode)
}

/// The `_meta` every request needs.
pub fn meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION,
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// A request line with `_meta` added to `params`.
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

/// Serves `lines` and returns the raw output and the log.
pub fn serve_raw(server: &mut Server, input: &[u8]) -> (String, String) {
    let mut output = Vec::new();
    let mut log = Vec::new();
    server
        .serve(Cursor::new(input.to_vec()), &mut output, &mut log)
        .unwrap();
    (
        String::from_utf8(output).expect("UTF-8 output"),
        String::from_utf8(log).expect("UTF-8 log"),
    )
}

/// Every check a reply line gets: nothing on it the CLI's escaping would
/// escape, it parses strictly, it is a JSON-RPC 2.0 response, and for a
/// tool result, the text copy is the structured result exactly as sent and
/// conforms to the tool's output schema.
pub fn checked_reply(line: &str) -> Value {
    for character in line.chars() {
        assert!(
            !tilewright_editor_observe::node::escaped(character),
            "raw U+{:04X} on stdout",
            u32::from(character)
        );
    }
    // World's strict parser: no duplicate keys, no trailing content.
    tilewright_jsonc::Document::parse(line, &tilewright_jsonc::Limits::default())
        .unwrap_or_else(|error| panic!("{error}: {line}"));
    let reply: Value = serde_json::from_str(line).unwrap();
    assert_eq!(reply["jsonrpc"], "2.0");
    assert_ne!(reply.get("result").is_some(), reply.get("error").is_some());
    if let Some(result) = reply.get("result") {
        assert_eq!(result["resultType"], "complete");
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "tilewright-editor"
        );
    }
    reply
}

/// The raw text of `structuredContent` in a reply line.
pub fn raw_structured(line: &str) -> &str {
    let start = line.find(",\"structuredContent\":").unwrap() + ",\"structuredContent\":".len();
    let end = line.rfind(",\"isError\":false").unwrap();
    &line[start..end]
}

/// One session: each line in, each reply out, checked. Lines that get no
/// reply must be marked `None` in `expect_reply`.
pub fn session(server: &mut Server, lines: &[String]) -> Vec<(String, Value)> {
    let mut input = String::new();
    for line in lines {
        input.push_str(line);
        input.push('\n');
    }
    let (output, _) = serve_raw(server, input.as_bytes());
    output
        .lines()
        .map(|line| (line.to_owned(), checked_reply(line)))
        .collect()
}

/// A tool call's result: checked against the tool's output schema (when it
/// succeeded), its text copy compared with the structured result as sent,
/// and the raw line kept.
pub struct Called {
    pub line: String,
    pub reply: Value,
    pub structured: Option<Value>,
    /// Every string of the structured result, and whether its schema marks
    /// it world text.
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
        self.structured.as_ref().expect("a successful result")
    }

    /// The error's first line, for a tool error.
    pub fn error_text(&self) -> &str {
        assert!(self.is_error(), "{}", self.line);
        self.result()["content"][0]["text"].as_str().unwrap()
    }
}

pub fn call(server: &mut Server, tool: &str, arguments: &Value) -> Called {
    let line = call_line(1, tool, arguments);
    let mut replies = session(server, &[line]);
    assert_eq!(replies.len(), 1);
    let (line, reply) = replies.remove(0);
    checked_call(tool, line, reply)
}

pub fn checked_call(tool: &str, line: String, reply: Value) -> Called {
    let result = &reply["result"];
    assert!(result.is_object(), "{line}");
    assert!(
        line.len()
            <= tilewright_editor_mcp::MAXIMUM_RESULT_BYTES
                + 512
                + if tool == "render" {
                    tilewright_editor_mcp::MAXIMUM_IMAGE_BYTES * 4 / 3 + 64
                } else {
                    0
                },
        "{tool}: {} bytes",
        line.len()
    );
    if result["isError"] == true {
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
    assert_eq!(result["isError"], false);
    let raw = raw_structured(&line).to_owned();
    let text = result["content"][0]["text"].as_str().unwrap();
    assert_eq!(result["content"][0]["type"], "text");
    assert_eq!(text, raw, "the text copy is the structured result as sent");
    let schema = tilewright_editor_mcp::output_schema(tool).unwrap();
    let parsed =
        tilewright_jsonc::Document::parse(raw.clone(), &tilewright_jsonc::Limits::default())
            .unwrap();
    let strings = schema
        .check(parsed.root())
        .unwrap_or_else(|problem| panic!("{tool}: {problem}\n{raw}"));
    let structured: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(structured, result["structuredContent"]);
    Called {
        line,
        reply,
        structured: Some(structured),
        strings,
    }
}

pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A writable copy of fixture `name`, in a directory of its own.
pub fn fixture_copy(name: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&tilewright_editor_testkit::fixture_world(name), &root);
    (directory, root)
}

pub fn read_json(root: &Path, path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join(path)).unwrap()).unwrap()
}

pub fn store_json(root: &Path, path: &str, value: &Value) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

pub fn edit(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let mut value = read_json(root, path);
    change(&mut value);
    store_json(root, path, &value);
}

pub const MAIN_CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

/// Every entry under `root`, and `root` itself under the empty path: its
/// kind, bytes, permissions and modification time. A directory's time
/// changes when an entry is made or removed in it, so an entry made and
/// removed again during a session is seen too.
pub type Tree = BTreeMap<PathBuf, (bool, Vec<u8>, std::fs::Permissions, std::time::SystemTime)>;

pub fn tree(root: &Path) -> Tree {
    let mut entries = BTreeMap::new();
    let own = std::fs::metadata(root).unwrap();
    entries.insert(
        PathBuf::new(),
        (true, Vec::new(), own.permissions(), own.modified().unwrap()),
    );
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
            let bytes = if metadata.is_dir() {
                pending.push(entry.path());
                Vec::new()
            } else {
                std::fs::read(entry.path()).unwrap()
            };
            entries.insert(
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                (
                    metadata.is_dir(),
                    bytes,
                    metadata.permissions(),
                    metadata.modified().unwrap(),
                ),
            );
        }
    }
    entries
}

/// Every item of `section` of `tool` with `arguments`, paged `limit` at a
/// time to the end, and how many calls that took.
pub fn page_through(
    server: &mut Server,
    tool: &str,
    arguments: &Value,
    section: &str,
    limit: usize,
) -> (Vec<Value>, usize) {
    let mut items = Vec::new();
    let mut cursor: Option<String> = None;
    let mut calls = 0;
    loop {
        let mut arguments = arguments.clone();
        arguments["section"] = json!(section);
        arguments["limit"] = json!(limit);
        if let Some(cursor) = &cursor {
            arguments["cursor"] = json!(cursor);
        }
        let called = call(server, tool, &arguments);
        calls += 1;
        let page = called.structured();
        assert_eq!(page["format"], "tilewright.editor-page/1");
        assert_eq!(page["section"], section);
        assert_eq!(page["offset"], items.len(), "{section}");
        let found = page["items"].as_array().unwrap();
        assert!(found.len() <= limit);
        items.extend(found.iter().cloned());
        let Some(next) = page["next_cursor"].as_str() else {
            assert_eq!(page["total"], items.len(), "{section}");
            return (items, calls);
        };
        assert!(!found.is_empty(), "a page always moves on");
        cursor = Some(next.to_owned());
        assert!(calls < 100_000);
    }
}
