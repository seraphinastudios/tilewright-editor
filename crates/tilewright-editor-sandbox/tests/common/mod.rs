// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A provisioned-like sandbox on disk, and a driver that sends one request
//! line at a time and reads its one reply.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tilewright_editor_commands::RecoveryStore;
use tilewright_editor_recovery::DurableRecovery;
use tilewright_editor_sandbox::{Sandbox, Settings};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::Actor;
use tilewright_source::SourceRoot;

pub const ACTOR: &str = "agent:tester";
pub const RECEIPT: [u8; 32] = [9; 32];
pub const EDITOR: &str = "0.1.0-test";

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

/// Parse, change and rewrite a JSON file.
pub fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// A sandbox laid out as the provisioner lays it out: `world/` (the
/// neutral fixture, changed by `configure`), `recovery/` and `proposals/`,
/// each private.
pub struct Layout {
    pub root: tempfile::TempDir,
}

impl Layout {
    pub fn new(configure: impl FnOnce(&Path)) -> Self {
        let root = tempfile::Builder::new()
            .prefix("tilewright-sandbox-")
            .tempdir_in(
                std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
                    .map_or_else(std::env::temp_dir, PathBuf::from),
            )
            .unwrap();
        copy_tree(
            &fixture_world("prototype-neutral"),
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
    pub fn settings(client: Option<&str>) -> Settings {
        Settings {
            client: client.map(str::to_owned),
            receipt: RECEIPT,
            editor: EDITOR.into(),
        }
    }
    /// The sandbox, opened as the runner opens it.
    pub fn open(&self, client: Option<&str>) -> Driver<DurableRecovery> {
        let sandbox = Sandbox::open(
            &self.path("world"),
            SourceRoot::new(&self.path("recovery")),
            SourceRoot::new(&self.path("proposals")),
            Actor::new(ACTOR).unwrap(),
            Self::settings(client),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        Driver { sandbox }
    }
    /// Every file under `child`, by relative path.
    pub fn files(&self, child: &str) -> BTreeMap<String, Vec<u8>> {
        files(&self.path(child))
    }
}

/// Give a tree the provisioner's access: directories 0700, files 0600.
fn private(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            private(&path);
        } else {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
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

/// Sends request lines to a sandbox and reads its replies.
pub struct Driver<R> {
    pub sandbox: Sandbox<R>,
}

impl<R: RecoveryStore> Driver<R> {
    /// Serves `input` and returns every reply line and the log.
    pub fn serve(&mut self, input: &[u8]) -> (Vec<String>, String) {
        let mut output = Vec::new();
        let mut log = Vec::new();
        self.sandbox
            .serve(Cursor::new(input), &mut output, &mut log)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.is_empty() || output.ends_with('\n'), "{output}");
        (
            output.lines().map(str::to_owned).collect(),
            String::from_utf8(log).unwrap(),
        )
    }
    /// One request line, and its one reply, parsed.
    pub fn send(&mut self, line: &str) -> Value {
        let (replies, _) = self.serve(format!("{line}\n").as_bytes());
        assert_eq!(replies.len(), 1, "{line}: {replies:?}");
        reply(&replies[0])
    }
    pub fn request(&mut self, request: &Value) -> Value {
        self.send(&request.to_string())
    }
    /// An edit of `action`, its `expected` the current revision.
    pub fn edit(&mut self, mut action: Value) -> Value {
        action["expected"] = Value::from(self.revision());
        self.request(&json!({"request": "edit", "action": action}))
    }
    pub fn simple(&mut self, request: &str) -> Value {
        self.request(&json!({ "request": request }))
    }
    pub fn revision(&mut self) -> String {
        self.simple("status")["state"]["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

/// A reply line, parsed, after checking it is one line of JSON with no
/// control character, no character outside ASCII left unescaped that the
/// writer escapes, and the members every reply has.
pub fn reply(line: &str) -> Value {
    assert!(
        !line
            .chars()
            .any(|character| character < ' ' || character == '\u{7f}'),
        "raw control character in {line}"
    );
    assert!(
        !line.contains('\u{202e}')
            && !line.contains('\u{200b}')
            && !line.contains('\u{2028}')
            && !line.contains('\u{2029}'),
        "raw invisible or bidirectional character in {line}"
    );
    let value: Value = serde_json::from_str(line).unwrap_or_else(|error| panic!("{error}: {line}"));
    assert!(value["ok"].is_boolean(), "{line}");
    assert!(value["state"]["revision"].is_string(), "{line}");
    if value["ok"] == false {
        assert!(value["code"].is_string(), "{line}");
        assert!(value["message"].is_string(), "{line}");
        assert!(
            value["engine"].is_string() || value["engine"].is_null(),
            "{line}"
        );
        assert!(value["step"].is_u64() || value["step"].is_null(), "{line}");
    }
    value
}

/// A reply that must be a success.
pub fn ok(reply: Value) -> Value {
    assert_eq!(reply["ok"], true, "{reply}");
    reply
}

/// A refusal with `code`.
pub fn refused(reply: &Value, code: &str) {
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["code"], code, "{reply}");
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
