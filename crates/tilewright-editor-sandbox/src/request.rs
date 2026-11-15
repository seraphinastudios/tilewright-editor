// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Reading one request line: strict JSON, one object, a known `request`
//! and exactly its members. An edit's action is read by the proposal
//! crate's decoder, so the runner and proposals share one Action form.
use tilewright_editor_proposal::{decode_action, limits};
use tilewright_editor_ui::Action;
use tilewright_jsonc::{Document, Limits, Value};

/// Every request, in documentation order.
pub const REQUESTS: [&str; 9] = [
    "status", "edit", "review", "accept", "cancel", "undo", "redo", "submit", "quit",
];

/// The reader's limits for one request: one line of JSON without
/// comments, large enough for every action the proposal format records (a
/// stroke of 4,096 cells, a field value of 4,096 values nested 8 deep).
const REQUEST_LIMITS: Limits = Limits {
    max_input_bytes: crate::MAXIMUM_REQUEST_BYTES,
    // The request, the action, then a field value its own bound deep.
    max_depth: 4 + limits::FIELD_VALUE_DEPTH,
    max_values: 3 * limits::CELLS + 64,
    max_array_items: limits::CELLS,
    max_object_keys: limits::OBJECT_KEYS,
    max_string_bytes: crate::MAXIMUM_REQUEST_BYTES,
    // As the proposal reader allows.
    max_number_bytes: 128,
    max_comments: 0,
    max_comment_bytes: 0,
    max_lines: 1,
    max_line_bytes: crate::MAXIMUM_REQUEST_BYTES,
};

/// A request that was read.
#[derive(Debug, PartialEq)]
pub enum Request {
    Status,
    Edit(Action),
    Review,
    Accept,
    Cancel,
    Undo,
    Redo,
    Submit,
    Quit,
}

/// A request line that was refused: the request it names, when it names
/// a known one, a stable code and a message written here.
#[derive(Debug, Eq, PartialEq)]
pub struct Refused {
    pub request: Option<&'static str>,
    pub code: &'static str,
    pub message: String,
}

fn refused(
    request: Option<&'static str>,
    code: &'static str,
    message: impl Into<String>,
) -> Refused {
    Refused {
        request,
        code,
        message: message.into(),
    }
}

/// Reads one request line, without its newline.
pub fn read(line: &[u8]) -> Result<Request, Refused> {
    // A line ending in CR LF is read as if it ended in LF.
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let text = std::str::from_utf8(line)
        .map_err(|_| refused(None, "request.not_utf8", "a request is UTF-8 text"))?;
    let document = Document::parse(text, &REQUEST_LIMITS).map_err(|error| {
        refused(
            None,
            "request.parse",
            format!(
                "a request is one JSON object on one line ({})",
                error.code.as_str()
            ),
        )
    })?;
    let members = document
        .root()
        .as_object()
        .ok_or_else(|| refused(None, "request.invalid", "a request is a JSON object"))?;
    let name = document
        .root()
        .get("request")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            refused(
                None,
                "request.invalid",
                "a request has a string member \"request\"",
            )
        })?;
    let Some(&known) = REQUESTS.iter().find(|known| **known == name) else {
        return Err(refused(
            None,
            "request.unknown",
            "unknown request; see docs/SANDBOX.md for the list",
        ));
    };
    let expected: &[&str] = if known == "edit" {
        &["request", "action"]
    } else {
        &["request"]
    };
    // The reader refuses duplicate keys, so equal counts and every member
    // known means exactly these members.
    if members.len() != expected.len()
        || !members
            .iter()
            .all(|member| expected.contains(&member.key.as_str()))
    {
        return Err(refused(
            Some(known),
            "request.invalid",
            if known == "edit" {
                "an edit request has exactly the members request and action"
            } else {
                "this request has exactly the member request"
            },
        ));
    }
    Ok(match known {
        "status" => Request::Status,
        "edit" => {
            let action = document.root().get("action").expect("members were checked");
            let action = decode_action(action).map_err(|error| Refused {
                request: Some("edit"),
                code: error.code().as_str(),
                message: format!("the action is not in the proposal format's form: {error}"),
            })?;
            Request::Edit(action)
        }
        "review" => Request::Review,
        "accept" => Request::Accept,
        "cancel" => Request::Cancel,
        "undo" => Request::Undo,
        "redo" => Request::Redo,
        "submit" => Request::Submit,
        _ => Request::Quit,
    })
}

#[cfg(test)]
mod tests {
    use super::{Request, read};

    #[test]
    fn requests_are_one_object_with_exactly_their_members() {
        assert_eq!(read(br#"{"request":"status"}"#), Ok(Request::Status));
        assert_eq!(read(b"{\"request\":\"quit\"}\r"), Ok(Request::Quit));
        assert_eq!(read(br#" { "request" : "undo" } "#), Ok(Request::Undo));
        for (line, code) in [
            (&b""[..], "request.parse"),
            (b"[]", "request.invalid"),
            (b"{\"request\":\"status\"} // note", "request.parse"),
            (
                b"{\"request\":\"status\",\"request\":\"quit\"}",
                "request.parse",
            ),
            (b"{\"request\":1}", "request.invalid"),
            (b"{}", "request.invalid"),
            (b"{\"request\":\"\"}", "request.unknown"),
            (b"{\"request\":\"Status\"}", "request.unknown"),
            (b"{\"request\":\"status\",\"action\":{}}", "request.invalid"),
            (b"{\"request\":\"edit\"}", "request.invalid"),
            (
                b"{\"request\":\"edit\",\"action\":{},\"x\":1}",
                "request.invalid",
            ),
            (b"{\"request\":\"edit\",\"action\":{}}", "proposal.invalid"),
            (
                b"{\"request\":\"edit\",\"action\":{\"kind\":\"accept\"}}",
                "proposal.invalid",
            ),
            (b"{\"request\":\"\xff\"}", "request.not_utf8"),
        ] {
            let refused = read(line).unwrap_err();
            assert_eq!(refused.code, code, "{}", String::from_utf8_lossy(line));
        }
    }
}
