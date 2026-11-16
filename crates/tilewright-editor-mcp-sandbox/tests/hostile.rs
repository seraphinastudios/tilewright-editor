// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! World text and hostile requests: every string an editing result holds
//! that the world, the engine's review of it or the client label wrote is
//! at a position the output schema marks `x-world-text`, and every other
//! string is in a closed vocabulary of this editor's own; random and
//! mutated messages each get exactly the replies the protocol's rules give.
#![cfg(target_os = "linux")]

mod common;

use common::{
    ACTOR, Called, EDITOR, Layout, call_line, checked_call, checked_reply, label, move_marker,
};
use serde_json::{Value, json};

const MARKER: &str = "QZXW";

/// A string of this editor's own: a code, a request, a phase, a change
/// kind, a revision, the operator's actor, the editor version, or a
/// proposal file name.
fn own(text: &str) -> bool {
    let hex = |text: &str| {
        text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    let code = |text: &str| {
        text.split_once('.').is_some_and(|(area, rest)| {
            !area.is_empty()
                && !rest.is_empty()
                && text
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'.')
        })
    };
    let requests = [
        "status", "edit", "review", "accept", "cancel", "undo", "redo", "submit",
    ];
    let words = [
        "ready",
        "pending",
        "retaining",
        "publishing",
        "blocked",
        "created",
        "modified",
        "removed",
    ];
    hex(text)
        || code(text)
        || requests.contains(&text)
        || words.contains(&text)
        || text == ACTOR
        || text == EDITOR
        || text
            .strip_prefix("proposal-")
            .and_then(|rest| rest.strip_suffix(".json"))
            .is_some_and(hex)
}

fn check_marking(called: &Called) {
    for (text, world) in &called.strings {
        if text.contains(MARKER) {
            assert!(world, "unmarked world text {text:?} in {}", called.line);
        }
        if !world {
            assert!(
                own(text),
                "{text:?} is neither marked nor this editor's own: {}",
                called.line
            );
        }
    }
}

#[test]
fn world_text_is_only_at_marked_positions() {
    let hostile = format!("{MARKER} \u{1b}[2J\u{7} \u{202e}desrever\u{200b} \"q\" \\ </b>");
    let layout = Layout::new(|_| {});
    let mut client = layout.open(Some(&format!("client {MARKER} \u{1b}]0;t\u{7}")));
    let mut seen = 0;
    let mut check = |called: Called| {
        check_marking(&called);
        seen += called
            .strings
            .iter()
            .filter(|(text, _)| text.contains(MARKER))
            .count();
        for character in ['\u{1b}', '\u{202e}', '\u{200b}', '\u{7}'] {
            assert!(!called.line.contains(character), "raw {character:?}");
        }
    };
    check(client.simple("status"));
    check(client.edit(label(&hostile)));
    check(client.simple("review"));
    check(client.simple("accept"));
    // A removal's review warns about what it deletes.
    check(client.edit(json!({"kind": "remove", "uid": "marker_1"})));
    check(client.simple("cancel"));
    check(
        client.edit(
            json!({"kind": "set_field", "uid": "marker_1", "field": MARKER, "value": MARKER}),
        ),
    );
    check(client.edit(json!({"kind": "move", "uid": MARKER, "x": 1, "y": 1})));
    check(client.edit(
        json!({"kind": "place", "plane": "main", "layer": 0, "x": 1, "y": 1, "proto": MARKER}),
    ));
    check(client.call("edit", &json!({"action": {"kind": MARKER}})));
    check(client.simple("submit"));
    check(client.simple("undo"));
    check(client.simple("accept"));
    check(client.simple("status"));
    // The marker reached the results: in the label, the review, and the
    // engine's messages where they quote what was asked.
    assert!(seen >= 3, "{seen}");
}

/// A small deterministic generator (xorshift64*), so a failure replays.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap()
    }
}

/// How many replies a line gets, and with which ID, by the revision's
/// rules as the read-only server applies them: none for a notification,
/// a response or a blank line; one, without ID, for a line that is not a
/// JSON object or whose ID is not a string or an integer.
enum Expected {
    Nothing,
    /// One reply, with this ID or none.
    Reply(Option<Value>),
}

fn expected(line: &str) -> Expected {
    if line.trim().is_empty() {
        return Expected::Nothing;
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Expected::Reply(None);
    };
    let Some(object) = value.as_object() else {
        return Expected::Reply(None);
    };
    let method = object.get("method");
    if method.is_none() && (object.contains_key("result") || object.contains_key("error")) {
        return Expected::Nothing;
    }
    match object.get("id") {
        None => Expected::Nothing,
        Some(id @ Value::String(_)) => Expected::Reply(Some(id.clone())),
        Some(id) if id.is_i64() => Expected::Reply(Some(id.clone())),
        Some(_) => Expected::Reply(None),
    }
}

#[test]
fn random_and_mutated_messages_get_exactly_one_reply_per_request() {
    let layout = Layout::new(|_| {});
    let mut client = layout.open(None);
    let revision = client.revision();
    let mut action = move_marker(4, 2);
    action["expected"] = json!(revision);
    let mut templates = vec![
        common::request(1, "server/discover", json!({})),
        common::request(2, "tools/list", json!({})),
        call_line(3, "edit", &json!({ "action": action })),
        call_line(4, "status", &json!({})),
        call_line(5, "accept", &json!({})),
        call_line(6, "undo", &json!({})),
        call_line(
            7,
            "tile",
            &json!({"plane": "main", "layer": 0, "x": 4, "y": 2}),
        ),
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {}}).to_string(),
        json!({"jsonrpc": "2.0", "method": 1}).to_string(),
        json!({"jsonrpc": "2.0", "id": 9, "result": {}}).to_string(),
    ];
    for tool in ["submit", "cancel", "redo", "review"] {
        templates.push(call_line(8, tool, &json!({})));
    }
    let alphabet = b"{}[]\":,0123456789.-eE \\nulltruefalseidmethodparams_meta\x00\xff\r";
    let mut random = Random(0x5eed_1234_abcd_0001);
    let mut tool_results = 0;
    for round in 0..1500 {
        let template = templates[random.below(templates.len())].clone();
        let mut bytes = template.into_bytes();
        if round % 3 != 0 {
            for _ in 0..=random.below(4) {
                let at = random.below(bytes.len().max(1));
                match random.below(3) {
                    0 if !bytes.is_empty() => {
                        bytes.remove(at);
                    }
                    1 => bytes.insert(at, alphabet[random.below(alphabet.len())]),
                    _ if !bytes.is_empty() => bytes[at] = alphabet[random.below(alphabet.len())],
                    _ => {}
                }
            }
        }
        bytes.retain(|byte| *byte != b'\n');
        let (output, _) = client.serve_raw(&[bytes.as_slice(), b"\n"].concat());
        let replies: Vec<&str> = output.lines().collect();
        let text = String::from_utf8_lossy(&bytes);
        // A line that is not UTF-8 is a parse error: one reply, no ID.
        let wanted = if std::str::from_utf8(&bytes).is_err() {
            Expected::Reply(None)
        } else {
            expected(&text)
        };
        match wanted {
            Expected::Nothing => assert!(replies.is_empty(), "{text}: {replies:?}"),
            Expected::Reply(id) => {
                assert_eq!(replies.len(), 1, "{text}: {replies:?}");
                let reply = checked_reply(replies[0]);
                assert_eq!(reply.get("id").cloned(), id, "{text}");
                if let Some(name) = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|value| value["params"]["name"].as_str().map(str::to_owned))
                    && reply["result"]["content"].is_array()
                {
                    checked_call(&name, replies[0].to_owned(), reply);
                    tool_results += 1;
                }
            }
        }
    }
    assert!(tool_results > 200, "{tool_results}");
}
