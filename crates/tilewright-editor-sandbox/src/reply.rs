// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Replies, written by hand: fixed ASCII keys, and every string, world
//! text among them, through the observe crate's JSON string escaping.
//! Every reply ends with the session's state.
use std::fmt::Write as _;
use tilewright_editor_observe::node::json_string;
use tilewright_editor_proposal::{Proposal, ProposalError, StoredProposal, hex};
use tilewright_editor_ui::review::{ChangeKind, PendingSummary};
use tilewright_editor_ui::{AppliedStep, Session, SessionPhase};

/// Most bytes of a message taken into a reply, before escaping.
const MAXIMUM_MESSAGE_BYTES: usize = 4096;

/// Room kept in every reply for its fixed members and its state: a reply's
/// lists get what is left of the cap after it.
const RESERVED_BYTES: usize = 4096;

/// A reply being written: an open JSON object.
pub struct Reply {
    text: String,
}

/// The longest prefix of `text` of at most `maximum` bytes that ends on a
/// character boundary.
fn clip(text: &str, maximum: usize) -> &str {
    if text.len() <= maximum {
        return text;
    }
    let mut end = maximum;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn escaped(text: &str) -> String {
    let mut output = String::new();
    json_string(text, &mut output);
    output
}

const fn phase(phase: SessionPhase) -> &'static str {
    match phase {
        SessionPhase::Ready => "ready",
        SessionPhase::Pending => "pending",
        SessionPhase::Retaining => "retaining",
        SessionPhase::Publishing => "publishing",
        SessionPhase::Blocked => "blocked",
    }
}

impl Reply {
    pub fn ok(request: &str) -> Self {
        let mut reply = Self {
            text: String::from("{\"ok\":true"),
        };
        reply.string("request", request);
        reply
    }

    /// A refusal with a stable `code` and a `message`, clipped.
    pub fn refused(request: Option<&str>, code: &str, message: &str) -> Self {
        let mut reply = Self {
            text: String::from("{\"ok\":false"),
        };
        reply.optional_string("request", request);
        reply.string("code", code);
        reply.string("message", clip(message, MAXIMUM_MESSAGE_BYTES));
        reply
    }

    /// A refusal from the proposal crate: its code, the engine's code when
    /// the engine refused, and the step concerned.
    pub fn proposal_refused(request: &str, error: &ProposalError) -> Self {
        let mut reply = Self::refused(Some(request), error.code().as_str(), error.message());
        reply.optional_string(
            "engine",
            error
                .engine_code()
                .map(tilewright_editor_ui::RefusalCode::as_str),
        );
        reply.key("step");
        match error.step() {
            Some(step) => {
                let _ = write!(reply.text, "{step}");
            }
            None => reply.text.push_str("null"),
        }
        reply
    }

    fn key(&mut self, key: &str) {
        self.text.push(',');
        json_string(key, &mut self.text);
        self.text.push(':');
    }

    pub fn string(&mut self, key: &str, value: &str) {
        self.key(key);
        json_string(value, &mut self.text);
    }

    pub fn optional_string(&mut self, key: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.string(key, value);
        } else {
            self.key(key);
            self.text.push_str("null");
        }
    }

    pub fn boolean(&mut self, key: &str, value: bool) {
        self.key(key);
        self.text.push_str(if value { "true" } else { "false" });
    }

    pub fn number(&mut self, key: &str, value: usize) {
        self.key(key);
        let _ = write!(self.text, "{value}");
    }

    /// The catalog's active view.
    pub fn view(&mut self, plane: &str, layer: i32, bounds: [i64; 4]) {
        self.key("view");
        self.text.push_str("{\"plane\":");
        json_string(plane, &mut self.text);
        let [x, y, width, height] = bounds;
        let _ = write!(
            self.text,
            ",\"layer\":{layer},\"bounds\":[{x},{y},{width},{height}]}}"
        );
    }

    /// The pending change, or `null`: its revisions, completeness, files,
    /// notes and warnings, and review lines, each list cut to what fits in
    /// `cap` bytes with the count left out.
    pub fn pending(&mut self, summary: Option<&PendingSummary<'_>>, cap: usize) {
        self.key("pending");
        let Some(summary) = summary else {
            self.text.push_str("null");
            return;
        };
        let mut remaining = cap.saturating_sub(self.text.len() + RESERVED_BYTES);
        let _ = write!(
            self.text,
            "{{\"base\":\"{}\",\"result\":\"{}\",\"complete\":{}",
            hex(&summary.base),
            hex(&summary.result),
            summary.complete
        );
        let files = summary.files.iter().map(|file| {
            let mut item = String::from("{\"path\":");
            json_string(file.path, &mut item);
            let change = match file.kind {
                ChangeKind::Created => "created",
                ChangeKind::Modified => "modified",
                ChangeKind::Removed => "removed",
            };
            let size =
                |bytes: Option<usize>| bytes.map_or_else(|| "null".into(), |b| b.to_string());
            let _ = write!(
                item,
                ",\"change\":\"{change}\",\"before_bytes\":{},\"after_bytes\":{}}}",
                size(file.before_bytes),
                size(file.after_bytes)
            );
            item
        });
        let cut_files = self.list("files", files, &mut remaining);
        self.number("files_omitted", summary.files_omitted + cut_files);
        let cut_warnings = self.list(
            "warnings",
            summary.warnings.iter().map(|line| escaped(line)),
            &mut remaining,
        );
        self.number("warnings_omitted", summary.warnings_omitted + cut_warnings);
        let cut_review = self.list(
            "review",
            summary.review.lines.iter().map(|line| escaped(line)),
            &mut remaining,
        );
        self.number("review_omitted", cut_review);
        self.boolean("cut", cut_files + cut_warnings + cut_review > 0);
        self.text.push('}');
    }

    /// A JSON array of `items` (each already JSON), in order, until one
    /// does not fit in `remaining`; returns how many were left out.
    fn list(
        &mut self,
        key: &str,
        items: impl ExactSizeIterator<Item = String>,
        remaining: &mut usize,
    ) -> usize {
        self.key(key);
        self.text.push('[');
        let total = items.len();
        let mut kept = 0;
        for item in items {
            let cost = item.len() + 1;
            if cost > *remaining {
                break;
            }
            *remaining -= cost;
            if kept > 0 {
                self.text.push(',');
            }
            self.text.push_str(&item);
            kept += 1;
        }
        self.text.push(']');
        total - kept
    }

    /// The step an Accept added, or `null` when it removed one (an Undo).
    pub fn applied(&mut self, step: Option<&AppliedStep<'_>>) {
        self.key("applied");
        let Some(step) = step else {
            self.text.push_str("null");
            return;
        };
        let _ = write!(
            self.text,
            "{{\"before\":\"{}\",\"after\":\"{}\"",
            hex(&step.before),
            hex(&step.after)
        );
        self.optional_string("placed", step.origin.and_then(|origin| origin.placed()));
        self.text.push('}');
    }

    /// The proposal `submit` retained.
    pub fn proposal(&mut self, proposal: &Proposal, stored: &StoredProposal) {
        self.key("proposal");
        self.text.push('{');
        // The store names the file; its name is fixed ASCII.
        self.text.push_str("\"file\":");
        json_string(&stored.name, &mut self.text);
        self.string("body_sha256", &hex(&proposal.body_sha256()));
        let _ = write!(self.text, ",\"bytes\":{}", stored.bytes);
        self.number("steps", proposal.steps().len());
        self.string("base", &hex(&proposal.base()));
        self.string("result", &hex(&proposal.result()));
        self.boolean("existed", stored.existed);
        self.text.push('}');
    }

    /// The reply, closed with the session's state.
    pub fn finish(mut self, session: &Session) -> String {
        let history = session.history();
        let (undo, redo) = history.depths();
        let _ = write!(
            self.text,
            ",\"state\":{{\"revision\":\"{}\",\"phase\":\"{}\",\"pending\":{},\"undo\":{undo},\"redo\":{redo}}}}}",
            hex(&history.document().revision()),
            phase(session.status().phase),
            history.pending().is_some()
        );
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::{MAXIMUM_MESSAGE_BYTES, Reply, clip};

    #[test]
    fn messages_are_clipped_before_escaping() {
        let long = "\u{e9}".repeat(MAXIMUM_MESSAGE_BYTES);
        let reply = Reply::refused(None, "x.y", &long).text;
        let kept = reply.matches('\u{e9}').count();
        assert_eq!(kept, MAXIMUM_MESSAGE_BYTES / 2);
        assert!(reply.len() < MAXIMUM_MESSAGE_BYTES + 64, "{}", reply.len());
    }

    #[test]
    fn clips_end_on_a_character_boundary() {
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abc", 2), "ab");
        assert_eq!(clip("a\u{e9}", 2), "a");
        assert_eq!(clip("\u{e9}", 0), "");
    }
}
