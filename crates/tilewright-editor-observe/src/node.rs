// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The one document shape every report is built as, and its two printed
//! forms. Member names are always this crate's own text; world text only
//! ever appears as a string value, clipped when it is built and escaped
//! when it is printed.
use crate::limits::MAXIMUM_TEXT_CHARACTERS;
use std::fmt::Write as _;
use tilewright_editor_document::{terminal_field, terminal_safe, unprintable};

/// A value of a report.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Integer(i64),
    /// A JSON number written exactly as it is given (a World field value).
    Number(f64),
    Text(String),
    List(Vec<Node>),
    Map(Map),
}

impl Node {
    /// A count, saturated at `i64::MAX`.
    pub fn count(count: usize) -> Self {
        Self::Integer(i64::try_from(count).unwrap_or(i64::MAX))
    }

    fn scalar(&self) -> bool {
        !matches!(self, Self::List(_) | Self::Map(_))
    }
}

impl From<bool> for Node {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<i64> for Node {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<i32> for Node {
    fn from(value: i32) -> Self {
        Self::Integer(value.into())
    }
}
impl From<u32> for Node {
    fn from(value: u32) -> Self {
        Self::Integer(value.into())
    }
}
impl From<u16> for Node {
    fn from(value: u16) -> Self {
        Self::Integer(value.into())
    }
}
impl From<i16> for Node {
    fn from(value: i16) -> Self {
        Self::Integer(value.into())
    }
}
impl From<[i64; 2]> for Node {
    fn from(value: [i64; 2]) -> Self {
        Self::List(value.into_iter().map(Self::Integer).collect())
    }
}
impl From<Map> for Node {
    fn from(value: Map) -> Self {
        Self::Map(value)
    }
}

/// `text` cut to [`MAXIMUM_TEXT_CHARACTERS`] characters, and whether it was
/// cut.
pub fn clip(text: &str) -> (String, bool) {
    match text.char_indices().nth(MAXIMUM_TEXT_CHARACTERS) {
        Some((end, _)) => (text[..end].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

/// An object with members in a fixed order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(Vec<(String, Node)>);

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds member `key`.
    #[must_use]
    pub fn with(mut self, key: &'static str, value: impl Into<Node>) -> Self {
        self.0.push((key.to_owned(), value.into()));
        self
    }

    /// Adds a count.
    #[must_use]
    pub fn count(self, key: &'static str, count: usize) -> Self {
        self.with(key, Node::count(count))
    }

    /// Adds text, clipped to [`MAXIMUM_TEXT_CHARACTERS`]; a clipped value is
    /// followed by the member `<key>_clipped: true`.
    #[must_use]
    pub fn text(mut self, key: &'static str, text: &str) -> Self {
        let (value, clipped) = clip(text);
        self.0.push((key.to_owned(), Node::Text(value)));
        if clipped {
            self.0.push((format!("{key}_clipped"), Node::Bool(true)));
        }
        self
    }

    /// Adds text, or null.
    #[must_use]
    pub fn optional_text(self, key: &'static str, text: Option<&str>) -> Self {
        match text {
            Some(text) => self.text(key, text),
            None => self.with(key, Node::Null),
        }
    }

    /// Adds a list of at most `maximum` items and the member `<key>_more`
    /// with how many were left out. `item` runs only for the items listed;
    /// the rest are only counted, so callers build nodes in `item`.
    #[must_use]
    pub fn bounded<T>(
        mut self,
        key: &'static str,
        items: impl IntoIterator<Item = T>,
        maximum: usize,
        mut item: impl FnMut(T) -> Node,
    ) -> Self {
        let mut items = items.into_iter();
        let listed: Vec<Node> = items.by_ref().take(maximum).map(&mut item).collect();
        let more = items.count();
        self.0.push((key.to_owned(), Node::List(listed)));
        self.0.push((format!("{key}_more"), Node::count(more)));
        self
    }

    /// Adds list `key`, found at `path` in its report (`catalog.grants.fields`
    /// says the member `fields` of `grants` of `catalog`), as `pager`
    /// decides: [`Bounded`], as the CLI prints it, is exactly
    /// [`Map::bounded`]; the MCP server's pager takes a window of the
    /// items within a byte budget and adds members of its own. `item` runs
    /// only for the items inside the window; those before it are skipped
    /// and those after it counted, unbuilt.
    #[must_use]
    pub fn paged<T>(
        mut self,
        pager: &dyn Pager,
        path: &'static str,
        key: &'static str,
        items: impl IntoIterator<Item = T>,
        maximum: usize,
        mut item: impl FnMut(T) -> Node,
    ) -> Self {
        let Window { offset, limit } = pager.window(path, maximum);
        let mut items = items.into_iter();
        let skipped = items.by_ref().take(offset).count();
        let mut listed = Vec::new();
        let mut refused = false;
        while listed.len() < limit {
            let Some(raw) = items.next() else { break };
            let node = item(raw);
            if !pager.admit(path, &node) {
                refused = true;
                break;
            }
            listed.push(node);
        }
        let more = items.count() + usize::from(refused);
        let next = (more > 0).then_some(skipped + listed.len());
        let total = skipped + listed.len() + more;
        let members = pager.members(Listed {
            path,
            key,
            items: listed,
            more,
            next,
            total,
        });
        self.0.extend(members);
        self
    }

    /// Adds a list of identifiers, which World bounds itself.
    #[must_use]
    pub fn ids<'a>(self, key: &'static str, ids: impl IntoIterator<Item = &'a str>) -> Self {
        self.with(
            key,
            Node::List(ids.into_iter().map(|id| Node::Text(clip(id).0)).collect()),
        )
    }
}

/// The items [`Map::paged`] lists of one list, and where the rest are.
pub struct Listed {
    /// Where the list is in its report.
    pub path: &'static str,
    /// Its member name.
    pub key: &'static str,
    /// The items listed.
    pub items: Vec<Node>,
    /// How many items follow the listed ones.
    pub more: usize,
    /// The index of the first item not listed, when there is one.
    pub next: Option<usize>,
    /// How many items the list has.
    pub total: usize,
}

/// Which items of a list [`Map::paged`] builds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    /// Items skipped, unbuilt, before the first one listed.
    pub offset: usize,
    /// Most items listed.
    pub limit: usize,
}

/// How [`Map::paged`] lists a list: which items, within what budget, and
/// written as which members.
pub trait Pager {
    /// The window of the list at `path`, whose printed bound is `maximum`.
    fn window(&self, path: &'static str, maximum: usize) -> Window;
    /// Whether `item`, the next item of the list at `path`, still fits;
    /// once it does not, the list ends before it.
    fn admit(&self, path: &'static str, item: &Node) -> bool;
    /// The members the list adds to its map.
    fn members(&self, listed: Listed) -> Vec<(String, Node)>;
}

/// The printed reports' pager: every list holds its first `maximum` items,
/// followed by `<key>_more`, as [`Map::bounded`] writes it.
pub struct Bounded;

impl Pager for Bounded {
    fn window(&self, _: &'static str, maximum: usize) -> Window {
        Window {
            offset: 0,
            limit: maximum,
        }
    }

    fn admit(&self, _: &'static str, _: &Node) -> bool {
        true
    }

    fn members(&self, listed: Listed) -> Vec<(String, Node)> {
        vec![
            (listed.key.to_owned(), Node::List(listed.items)),
            (format!("{}_more", listed.key), Node::count(listed.more)),
        ]
    }
}

/// Whether the printers write `character` as an escape rather than as
/// itself. This is the one definition of the escaped set:
///
/// - everything [`unprintable`] matches: every control character (C0, DEL
///   and C1), U+2028 and U+2029, and the bidirectional formatting
///   characters U+061C, U+200E, U+200F, U+202A to U+202E and U+2066 to
///   U+2069;
/// - and invisible or format characters a reader would not see: U+00AD
///   (soft hyphen), U+034F (combining grapheme joiner), the Hangul fillers
///   U+115F, U+1160, U+3164 and U+FFA0, the Khmer inherent vowels U+17B4
///   and U+17B5, U+180E, U+200B to U+200F, U+2060 to U+206F (word joiner,
///   invisible operators and the deprecated format controls), U+FEFF,
///   U+FFF0 to U+FFFB, the shorthand format controls U+1BCA0 to U+1BCA3,
///   the musical formatting characters U+1D173 to U+1D17A, and U+E0000 to
///   U+E0FFF (the tag characters and the unassigned default-ignorable code
///   points around them) except the variation selectors U+E0100 to
///   U+E01EF.
///
/// Variation selectors (U+FE00 to U+FE0F and U+E0100 to U+E01EF) are not
/// escaped: they change how a character is drawn, not what text says.
/// [`ESCAPED_RANGES`] lists the ranges this adds to [`unprintable`].
pub fn escaped(character: char) -> bool {
    unprintable(character)
        || ESCAPED_RANGES
            .iter()
            .any(|(first, last)| (*first..=*last).contains(&character))
}

/// The invisible or format characters [`escaped`] adds to [`unprintable`],
/// as inclusive ranges in code point order.
pub const ESCAPED_RANGES: [(char, char); 15] = [
    ('\u{ad}', '\u{ad}'),
    ('\u{34f}', '\u{34f}'),
    ('\u{115f}', '\u{1160}'),
    ('\u{17b4}', '\u{17b5}'),
    ('\u{180e}', '\u{180e}'),
    ('\u{200b}', '\u{200f}'),
    ('\u{2060}', '\u{206f}'),
    ('\u{3164}', '\u{3164}'),
    ('\u{feff}', '\u{feff}'),
    ('\u{ffa0}', '\u{ffa0}'),
    ('\u{fff0}', '\u{fffb}'),
    ('\u{1bca0}', '\u{1bca3}'),
    ('\u{1d173}', '\u{1d17a}'),
    ('\u{e0000}', '\u{e00ff}'),
    ('\u{e01f0}', '\u{e0fff}'),
];

/// The JSON form: one document, two-space indented, ending with a newline.
/// Every [`escaped`] character is written as a `\u` escape (a UTF-16
/// surrogate pair above U+FFFF), so the document is safe to print as it is.
pub fn to_json(root: &Map) -> String {
    let mut output = String::new();
    map_json(root, 0, &mut output);
    output.push('\n');
    output
}

/// Writes `text` as a JSON string.
pub fn json_string(text: &str, output: &mut String) {
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            character if escaped(character) => {
                let mut units = [0; 2];
                for unit in character.encode_utf16(&mut units) {
                    let _ = write!(output, "\\u{unit:04x}");
                }
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

/// `text` as one field of the text form: every [`escaped`] character
/// written as its `\u{...}` escape, then [`terminal_field`] over the rest.
pub fn text_field(text: &str) -> String {
    let marked: String = text.chars().fold(
        String::with_capacity(text.len()),
        |mut marked, character| {
            if escaped(character) && !unprintable(character) {
                marked.extend(character.escape_unicode());
            } else {
                marked.push(character);
            }
            marked
        },
    );
    terminal_field(&marked)
}

/// The compact JSON form, the MCP server's: the same document as
/// [`to_json`] with no whitespace between tokens and no final newline.
pub fn to_compact_json(root: &Map) -> String {
    let mut output = String::new();
    compact_json(&Node::Map(root.clone()), &mut output);
    output
}

/// Writes `node` in the compact JSON form.
pub fn compact_json(node: &Node, output: &mut String) {
    match node {
        Node::Map(map) => {
            output.push('{');
            for (index, (key, value)) in map.0.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                json_string(key, output);
                output.push(':');
                compact_json(value, output);
            }
            output.push('}');
        }
        Node::List(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                compact_json(item, output);
            }
            output.push(']');
        }
        scalar => node_json(scalar, 0, output),
    }
}

fn indent(depth: usize, output: &mut String) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

fn map_json(map: &Map, depth: usize, output: &mut String) {
    if map.0.is_empty() {
        output.push_str("{}");
        return;
    }
    output.push_str("{\n");
    for (index, (key, value)) in map.0.iter().enumerate() {
        if index > 0 {
            output.push_str(",\n");
        }
        indent(depth + 1, output);
        json_string(key, output);
        output.push_str(": ");
        node_json(value, depth + 1, output);
    }
    output.push('\n');
    indent(depth, output);
    output.push('}');
}

fn node_json(node: &Node, depth: usize, output: &mut String) {
    match node {
        Node::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Node::Integer(value) => {
            let _ = write!(output, "{value}");
        }
        // Rust's shortest round-trip form of a finite number is JSON.
        Node::Number(value) if value.is_finite() => {
            let _ = write!(output, "{value:?}");
        }
        Node::Null | Node::Number(_) => output.push_str("null"),
        Node::Text(text) => json_string(text, output),
        Node::Map(map) => map_json(map, depth, output),
        Node::List(items) if items.iter().all(Node::scalar) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                node_json(item, depth, output);
            }
            output.push(']');
        }
        Node::List(items) => {
            output.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(",\n");
                }
                indent(depth + 1, output);
                node_json(item, depth + 1, output);
            }
            output.push('\n');
            indent(depth, output);
            output.push(']');
        }
    }
}

/// The text form: one `name: value` line per member, nested members
/// indented, list items marked `- `. Every string value is one escaped
/// field ([`text_field`]), so world text cannot add, forge or reorder
/// lines, and the whole is passed through [`terminal_safe`] as a backstop.
pub fn to_text(root: &Map) -> String {
    let mut output = String::new();
    map_text(root, 0, &mut output);
    terminal_safe(&output)
}

fn scalar_text(node: &Node) -> String {
    match node {
        Node::Null => "none".into(),
        Node::Bool(value) => value.to_string(),
        Node::Integer(value) => value.to_string(),
        Node::Number(value) => format!("{value:?}"),
        Node::Text(text) if text.is_empty() => "\"\"".into(),
        Node::Text(text) => text_field(text),
        Node::List(items) => format!(
            "[{}]",
            items.iter().map(scalar_text).collect::<Vec<_>>().join(", ")
        ),
        Node::Map(_) => "{...}".into(),
    }
}

fn map_text(map: &Map, depth: usize, output: &mut String) {
    for (key, value) in &map.0 {
        indent(depth, output);
        output.push_str(key);
        match value {
            Node::Map(inner) if inner.0.is_empty() => output.push_str(": (none)\n"),
            Node::Map(inner) => {
                output.push_str(":\n");
                map_text(inner, depth + 1, output);
            }
            Node::List(items) if items.is_empty() => output.push_str(": (none)\n"),
            Node::List(items) if items.iter().all(Node::scalar) => {
                let _ = writeln!(output, ": {}", scalar_text(value));
            }
            Node::List(items) => {
                output.push_str(":\n");
                for item in items {
                    let mut lines = String::new();
                    match item {
                        Node::Map(inner) if !inner.0.is_empty() => {
                            map_text(inner, depth + 2, &mut lines);
                        }
                        other => {
                            indent(depth + 2, &mut lines);
                            lines.push_str(&scalar_text(other));
                            lines.push('\n');
                        }
                    }
                    // The first line's indent becomes the item marker.
                    let start = 2 * (depth + 1);
                    lines.replace_range(start..start + 2, "- ");
                    output.push_str(&lines);
                }
            }
            scalar => {
                let _ = writeln!(output, ": {}", scalar_text(scalar));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Map, Node, clip, to_json, to_text};
    use crate::limits::MAXIMUM_TEXT_CHARACTERS;

    #[test]
    fn both_forms_escape_world_text_and_keep_its_shape() {
        let hostile = "a\"b\\c\n\u{1b}[2J\u{7f}\u{85}\u{2028}\u{202e}d";
        let map = Map::new()
            .text("name", hostile)
            .with("tile", [-2_i64, 3])
            .with(
                "items",
                Node::List(vec![
                    Map::new().text("id", "x").count("n", 2).into(),
                    Map::new().text("id", "y").into(),
                ]),
            )
            .with("nothing", Node::List(Vec::new()));
        let json = to_json(&map);
        assert!(
            json.contains(r#""name": "a\"b\\c\u000a\u001b[2J\u007f\u0085\u2028\u202ed""#),
            "{json}"
        );
        assert!(json.contains(r#""tile": [-2, 3]"#), "{json}");
        let text = to_text(&map);
        assert_eq!(
            text,
            "name: a\"b\\c\\u{a}\\u{1b}[2J\\u{7f}\\u{85}\\u{2028}\\u{202e}d\ntile: [-2, 3]\nitems:\n  - id: x\n    n: 2\n  - id: y\nnothing: (none)\n"
        );
    }

    #[test]
    fn invisible_characters_are_escaped_and_astral_ones_as_surrogate_pairs() {
        // A tag character, BOM, zero-width space, soft hyphen, word joiner
        // and an annotation anchor are escaped; a variation selector is not.
        let text = "a\u{e0041}\u{feff}\u{200b}\u{ad}\u{2060}\u{fff9}\u{180e}\u{fe0f}b";
        let json = to_json(&Map::new().text("v", text));
        assert!(
            json.contains(r#""v": "a\udb40\udc41\ufeff\u200b\u00ad\u2060\ufff9\u180e"#),
            "{json}"
        );
        assert!(json.contains("\u{fe0f}b\""), "{json}");
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["v"], text);
        let shown = to_text(&Map::new().text("v", text));
        assert_eq!(
            shown,
            "v: a\\u{e0041}\\u{feff}\\u{200b}\\u{ad}\\u{2060}\\u{fff9}\\u{180e}\u{fe0f}b\n"
        );
        assert!(!super::escaped('\u{fe0f}'));
        assert!(super::escaped('\u{e007f}') && super::escaped('\u{e0080}'));
    }

    #[test]
    fn every_code_point_of_every_escaped_range_is_escaped_in_both_forms() {
        let mut count = 0;
        for (first, last) in super::ESCAPED_RANGES {
            for character in first..=last {
                count += 1;
                assert!(super::escaped(character), "U+{:04X}", u32::from(character));
                let text = format!("a{character}b");
                let json = to_json(&Map::new().text("v", &text));
                assert!(json.is_ascii(), "{json}");
                let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert_eq!(parsed["v"], text);
                assert_eq!(
                    to_text(&Map::new().text("v", &text)),
                    format!("v: a\\u{{{:x}}}b\n", u32::from(character))
                );
            }
        }
        assert_eq!(
            count,
            1 + 1 + 2 + 2 + 1 + 5 + 16 + 1 + 1 + 1 + 12 + 4 + 8 + 256 + 3600
        );
        // The neighbours of each range, and the variation selectors, are
        // not.
        for character in [
            '\u{ac}',
            '\u{ae}',
            '\u{34e}',
            '\u{350}',
            '\u{115e}',
            '\u{1161}',
            '\u{17b3}',
            '\u{17b6}',
            '\u{180d}',
            '\u{180f}',
            '\u{2070}',
            '\u{3163}',
            '\u{3165}',
            '\u{fe00}',
            '\u{fe0f}',
            '\u{fefe}',
            '\u{ff9f}',
            '\u{ffa1}',
            '\u{ffef}',
            '\u{fffc}',
            '\u{1bc9f}',
            '\u{1bca4}',
            '\u{1d172}',
            '\u{1d17b}',
            '\u{e0100}',
            '\u{e01ef}',
            '\u{e1000}',
            'é',
            '\u{1f600}',
        ] {
            assert!(!super::escaped(character), "U+{:04X}", u32::from(character));
        }
    }

    #[test]
    fn the_compact_form_is_the_same_document() {
        let map = Map::new()
            .text("name", "a\u{1d173}\"")
            .with("tile", [-2_i64, 3])
            .with("number", Node::Number(0.5))
            .with("nothing", Node::Null)
            .with(
                "items",
                Node::List(vec![Map::new().count("n", 2).into(), Map::new().into()]),
            );
        let compact = super::to_compact_json(&map);
        assert_eq!(
            compact,
            r#"{"name":"a\ud834\udd73\"","tile":[-2,3],"number":0.5,"nothing":null,"items":[{"n":2},{}]}"#
        );
        let pretty: serde_json::Value = serde_json::from_str(&to_json(&map)).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&compact).unwrap();
        assert_eq!(parsed, pretty);
    }

    #[test]
    fn paged_with_the_printed_pager_is_bounded() {
        let pager = super::Bounded;
        for maximum in [0, 3, 10, 11] {
            let through_pager =
                Map::new().paged(&pager, "values", "values", 0..10, maximum, |value: i64| {
                    value.into()
                });
            let bounded = Map::new().bounded("values", 0..10, maximum, |value: i64| value.into());
            assert_eq!(through_pager, bounded, "{maximum}");
        }
    }

    #[test]
    fn long_text_is_clipped_and_flagged() {
        let long = "é".repeat(MAXIMUM_TEXT_CHARACTERS + 5);
        let (value, clipped) = clip(&long);
        assert!(clipped);
        assert_eq!(value.chars().count(), MAXIMUM_TEXT_CHARACTERS);
        let json = to_json(&Map::new().text("label", &long));
        assert!(json.contains("\"label_clipped\": true"), "{json}");
        assert!(!to_json(&Map::new().text("label", "short")).contains("clipped"));
    }

    #[test]
    fn bounded_lists_count_what_they_leave_out() {
        let map = Map::new().bounded("values", 0..10, 3, |value: i64| value.into());
        assert_eq!(
            to_json(&map),
            "{\n  \"values\": [0, 1, 2],\n  \"values_more\": 7\n}\n"
        );
    }
}
