// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Budgets, cursors and the pager every paged list shares (`docs/MCP.md`,
//! "Paging" and "Limits").
//!
//! A tool's result is sent twice: as `structuredContent`, and serialized as
//! its text content. A list holds at most `limit` items and stops early,
//! before the item that would take the two copies together past the page
//! budget. A cursor names the tool, the snapshot it was issued for, a
//! fingerprint of the arguments and section it was issued with, and the
//! offset of the next item.

use std::cell::{Cell, RefCell};
use tilewright_editor_observe::node::{Listed, Node, Pager, Window, compact_json, escaped};

/// The `limit` of a call that gives none.
pub const DEFAULT_LIMIT: usize = 50;
/// The largest `limit` a call may give.
pub const MAXIMUM_LIMIT: usize = 500;
/// Most bytes of a tool call's `result` as sent: its structured result and
/// the text copy of it together, with the fields around them. `render`'s
/// image is outside it, under [`MAXIMUM_IMAGE_BYTES`].
pub const MAXIMUM_RESULT_BYTES: usize = 256 * 1024;
/// Room kept for the members of a result outside its paged lists, both
/// copies counted.
pub const ENVELOPE_BYTES: usize = 64 * 1024;
/// The bytes the items of a result's paged lists may cost together.
pub const PAGE_BYTES: usize = MAXIMUM_RESULT_BYTES - ENVELOPE_BYTES;
/// Most bytes of a cursor a client sends back; a longer one is invalid.
pub const MAXIMUM_CURSOR_BYTES: usize = 256;

/// `render`: most bytes of the PNG, before base64 makes it a third larger.
pub const MAXIMUM_IMAGE_BYTES: usize = 192 * 1024;
/// `render`: most pixels on either side of the image, and in all; far
/// below the CLI's, so that one call draws at most a mebipixel.
pub const MAXIMUM_IMAGE_SIDE: u32 = 2_048;
pub const MAXIMUM_IMAGE_PIXELS: u64 = 1 << 20;
/// `render`: the largest `scale`.
pub const MAXIMUM_SCALE: u32 = 4;

/// What `json` costs once sent: as written, and again as a JSON string in
/// the text copy, where `"` and `\` take two bytes and an escaped
/// character six. The structured JSON this server writes holds no raw
/// escaped character, so the second sum is exact for it.
pub fn cost(json: &str) -> usize {
    json.len()
        + json
            .chars()
            .map(|character| match character {
                '"' | '\\' => 2,
                character if escaped(character) => 6,
                character => character.len_utf8(),
            })
            .sum::<usize>()
}

/// `text` cut to at most `maximum` bytes at a character boundary.
pub fn clip(text: &str, maximum: usize) -> &str {
    if text.len() <= maximum {
        return text;
    }
    let mut end = maximum;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// FNV-1a, 64 bits: a fingerprint for cursors and snapshots, not a
/// security measure. A cursor only has to tell the arguments and snapshot
/// it was issued for from others a client sends by mistake.
pub fn fingerprint(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.iter().copied().chain(
            // Each part is followed by its length, so ["ab", "c"] and
            // ["a", "bc"] differ.
            (part.len() as u64).to_le_bytes(),
        ) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// Why a cursor was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorError {
    /// Not a cursor this server issued for this tool, these arguments and
    /// this section, or past the end of the list.
    Invalid,
    /// Issued for another snapshot: the world was loaded again since, and
    /// changed.
    Stale,
}

impl CursorError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Invalid => "cursor.invalid",
            Self::Stale => "cursor.stale",
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::Invalid => {
                "the cursor was not issued by this server for this tool, these arguments and this section; call again without a cursor"
            }
            Self::Stale => {
                "the cursor was issued for another snapshot of the world; call again without a cursor"
            }
        }
    }
}

/// The prefix of this server's cursors, with the cursor format's version.
const CURSOR_PREFIX: &str = "tw1";

/// A cursor for the item at `offset` of `tool` with the arguments and
/// section whose fingerprint is `arguments`, on the snapshot `binding`.
pub fn cursor(tool: &str, binding: &str, arguments: u64, offset: usize) -> String {
    format!("{CURSOR_PREFIX}.{tool}.{binding}.{arguments:016x}.{offset}")
}

/// The offset a cursor names, checked against the tool, snapshot and
/// arguments of the call it came back with. Whether the offset is inside
/// the list is checked once the list is counted.
pub fn resume(text: &str, tool: &str, binding: &str, arguments: u64) -> Result<usize, CursorError> {
    if text.len() > MAXIMUM_CURSOR_BYTES {
        return Err(CursorError::Invalid);
    }
    let parts: Vec<&str> = text.split('.').collect();
    let [prefix, named_tool, named_binding, named_arguments, offset] = parts.as_slice() else {
        return Err(CursorError::Invalid);
    };
    if *prefix != CURSOR_PREFIX
        || *named_tool != tool
        || !is_binding(named_binding)
        || named_arguments.len() != 16
        || u64::from_str_radix(named_arguments, 16).ok() != Some(arguments)
    {
        return Err(CursorError::Invalid);
    }
    // Digits only, without a leading zero, so one offset has one spelling.
    // Offset 0 is a list of a report that the byte budget left empty.
    let offset = (!offset.is_empty()
        && offset.bytes().all(|byte| byte.is_ascii_digit())
        && (offset.len() == 1 || !offset.starts_with('0')))
    .then(|| offset.parse::<usize>().ok())
    .flatten()
    .ok_or(CursorError::Invalid)?;
    if *named_binding != binding {
        return Err(CursorError::Stale);
    }
    Ok(offset)
}

fn hex(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether `text` is a snapshot binding: 80 hex digits for a world that
/// loads (its revision and a fingerprint of its catalog), or `x` and 16
/// hex digits for one that does not.
pub fn is_binding(text: &str) -> bool {
    match text.strip_prefix('x') {
        Some(rest) => rest.len() == 16 && hex(rest),
        None => text.len() == 80 && hex(text),
    }
}

/// Whether `text` is a revision: 64 lowercase hex digits.
pub fn is_revision(text: &str) -> bool {
    text.len() == 64 && hex(text)
}

/// Which lists a call lists, and how.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// The whole report: each paged list holds its first items, at most
    /// `limit` and at most its printed bound.
    Report { limit: usize },
    /// One page of one list; the rest of the report is not sent.
    Section {
        section: &'static str,
        offset: usize,
        limit: usize,
    },
}

/// The page of the section a [`Mode::Section`] call asked for.
#[derive(Debug, Default)]
pub struct Page {
    pub items: Vec<Node>,
    pub next: Option<usize>,
    pub total: usize,
    pub byte_budget_reached: bool,
}

/// The MCP server's [`Pager`]: windows by [`Mode`], one shared byte budget
/// for every list of the result, and a `<key>_cursor` after each list of a
/// report.
pub struct Paging<'a> {
    mode: Mode,
    tool: &'static str,
    binding: &'a str,
    /// The arguments' fingerprint parts, without the section.
    arguments: Vec<Vec<u8>>,
    /// Whether reports carry cursors: not in a load failure.
    cursors: bool,
    remaining: Cell<usize>,
    /// The section whose item did not fit, if one did not.
    stopped: Cell<Option<&'static str>>,
    page: RefCell<Option<Page>>,
}

impl<'a> Paging<'a> {
    pub fn new(mode: Mode, tool: &'static str, binding: &'a str, arguments: Vec<Vec<u8>>) -> Self {
        Self {
            mode,
            tool,
            binding,
            arguments,
            cursors: true,
            remaining: Cell::new(PAGE_BYTES),
            stopped: Cell::new(None),
            page: RefCell::new(None),
        }
    }

    /// A pager for a document that carries no cursors.
    pub fn without_cursors(limit: usize) -> Self {
        Self {
            cursors: false,
            ..Self::new(Mode::Report { limit }, "", "", Vec::new())
        }
    }

    /// The fingerprint a cursor of `section` is bound to.
    pub fn fingerprint(&self, section: &str) -> u64 {
        let mut parts: Vec<&[u8]> = self.arguments.iter().map(Vec::as_slice).collect();
        parts.push(section.as_bytes());
        fingerprint(&parts)
    }

    /// The page of a [`Mode::Section`] call, once the report is built.
    pub fn take_page(&self) -> Option<Page> {
        self.page.borrow_mut().take()
    }
}

impl Pager for Paging<'_> {
    fn window(&self, path: &'static str, maximum: usize) -> Window {
        match self.mode {
            Mode::Report { limit } => Window {
                offset: 0,
                limit: limit.min(maximum),
            },
            Mode::Section {
                section,
                offset,
                limit,
            } if section == path => Window { offset, limit },
            Mode::Section { .. } => Window {
                offset: 0,
                limit: 0,
            },
        }
    }

    fn admit(&self, path: &'static str, item: &Node) -> bool {
        let mut json = String::new();
        compact_json(item, &mut json);
        // The comma before it, which the first item does not have, is
        // counted for every item.
        let item_cost = cost(&json) + cost(",");
        if let Some(remaining) = self.remaining.get().checked_sub(item_cost) {
            self.remaining.set(remaining);
            return true;
        }
        // Nothing is admitted once one item has not fitted, so a smaller
        // item of a later list cannot jump the queue.
        self.remaining.set(0);
        self.stopped.set(Some(path));
        false
    }

    fn members(&self, listed: Listed) -> Vec<(String, Node)> {
        let Listed {
            path,
            key,
            items,
            more,
            next,
            total,
        } = listed;
        if let Mode::Section { section, .. } = self.mode
            && section == path
        {
            *self.page.borrow_mut() = Some(Page {
                items,
                next,
                total,
                byte_budget_reached: self.stopped.get() == Some(path),
            });
            return Vec::new();
        }
        let mut members = vec![
            (key.to_owned(), Node::List(items)),
            (format!("{key}_more"), Node::count(more)),
        ];
        if self.cursors && matches!(self.mode, Mode::Report { .. }) {
            let cursor = next.map_or(Node::Null, |next| {
                Node::Text(cursor(
                    self.tool,
                    self.binding,
                    self.fingerprint(path),
                    next,
                ))
            });
            members.push((format!("{key}_cursor"), cursor));
        }
        members
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tilewright_editor_observe::node::{Map, json_string};

    fn binding() -> String {
        "a".repeat(80)
    }

    #[test]
    fn cursors_resume_only_where_issued() {
        let binding = binding();
        let text = cursor("catalog", &binding, 7, 50);
        assert_eq!(resume(&text, "catalog", &binding, 7), Ok(50));
        assert_eq!(
            resume(&text, "catalog", &"b".repeat(80), 7),
            Err(CursorError::Stale)
        );
        for (tool, arguments) in [("lint", 7), ("catalog", 8)] {
            assert_eq!(
                resume(&text, tool, &binding, arguments),
                Err(CursorError::Invalid)
            );
        }
        let invalid = format!("x{}", "0".repeat(16));
        for text in [
            String::new(),
            "tw1".to_owned(),
            format!("tw2.lint.{invalid}.0000000000000007.1"),
            format!("tw1.lint.{invalid}.0000000000000007.01"),
            format!("tw1.lint.{invalid}.0000000000000007.+1"),
            format!("tw1.lint.{invalid}.0000000000000007.99999999999999999999999"),
            "tw1.lint.x000000000000000.0000000000000007.1".to_owned(),
            "tw1.lint.X0000000000000000.0000000000000007.1".to_owned(),
            format!("tw1.lint.{invalid}.000000000000007.1"),
            format!("tw1.lint.{invalid}.0000000000000007.1.1"),
            format!("tw1.lint.{}.0000000000000007.1", "a".repeat(64)),
            "a".repeat(300),
        ] {
            assert_eq!(
                resume(&text, "lint", &invalid, 7),
                Err(CursorError::Invalid),
                "{text}"
            );
        }
        assert_eq!(
            resume(&cursor("lint", &invalid, 7, 1), "lint", &invalid, 7),
            Ok(1)
        );
        assert_eq!(
            resume(&cursor("lint", &invalid, 7, 0), "lint", &invalid, 7),
            Ok(0)
        );
    }

    #[test]
    fn separators_and_escapes_cost_what_they_are_sent_as() {
        let mut item = String::new();
        compact_json(&Node::Text("a\u{2028}b\u{e0041}\u{7}\"é".into()), &mut item);
        assert_eq!(item, "\"a\\u2028b\\udb40\\udc41\\u0007\\\"é\"");
        let mut text = String::new();
        json_string(&item, &mut text);
        assert_eq!(cost(&item), item.len() + text.len() - 2);
    }

    #[test]
    fn a_section_window_lists_one_page_and_counts_the_rest() {
        let binding = binding();
        let paging = Paging::new(
            Mode::Section {
                section: "b",
                offset: 3,
                limit: 4,
            },
            "catalog",
            &binding,
            vec![b"x".to_vec()],
        );
        let built = Cell::new(0);
        let item = |value: i64| {
            built.set(built.get() + 1);
            Node::Integer(value)
        };
        let map = Map::new().paged(&paging, "a", "a", 0..10, 100, item).paged(
            &paging,
            "b",
            "b",
            0..10,
            2,
            item,
        );
        // Only the page's items are built; the other list only counts.
        assert_eq!(built.get(), 4);
        assert_eq!(
            tilewright_editor_observe::node::to_compact_json(&map),
            r#"{"a":[],"a_more":10}"#
        );
        let page = paging.take_page().unwrap();
        assert_eq!(page.items, (3..7).map(Node::Integer).collect::<Vec<_>>());
        assert_eq!((page.next, page.total), (Some(7), 10));
        assert!(!page.byte_budget_reached);
    }

    #[test]
    fn a_report_caps_each_list_at_its_bound_and_gives_cursors() {
        let binding = binding();
        let paging = Paging::new(
            Mode::Report { limit: 3 },
            "catalog",
            &binding,
            vec![b"x".to_vec()],
        );
        let map = Map::new()
            .paged(&paging, "a", "a", 0..10, 2, Node::from)
            .paged(&paging, "b", "b", 0..3, 100, Node::from);
        let json = tilewright_editor_observe::node::to_compact_json(&map);
        let expected = format!(
            r#"{{"a":[0,1],"a_more":8,"a_cursor":"{}","b":[0,1,2],"b_more":0,"b_cursor":null}}"#,
            cursor("catalog", &binding, paging.fingerprint("a"), 2)
        );
        assert_eq!(json, expected);
        assert_ne!(paging.fingerprint("a"), paging.fingerprint("b"));
    }

    #[test]
    fn the_byte_budget_is_shared_and_ends_every_later_list() {
        let binding = binding();
        let paging = Paging::new(
            Mode::Report {
                limit: MAXIMUM_LIMIT,
            },
            "lint",
            &binding,
            Vec::new(),
        );
        let long = "\u{e0041}".repeat(256);
        let map = Map::new()
            .paged(&paging, "a", "a", 0..1000, 1000, |_: i32| {
                Node::Text(long.clone())
            })
            .paged(&paging, "b", "b", 0..5, 100, Node::from);
        let json = tilewright_editor_observe::node::to_compact_json(&map);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let listed = value["a"].as_array().unwrap().len();
        assert!(listed > 0 && listed < 1000);
        assert_eq!(value["a_more"], 1000 - listed);
        assert_eq!(value["b"].as_array().unwrap().len(), 0);
        assert_eq!(value["b_more"], 5);
        assert!(value["b_cursor"].is_string());
        // The items listed cost no more than the page budget.
        assert!(cost(&json) <= PAGE_BYTES + 1024, "{}", cost(&json));
    }
}
