// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Exact UTF-8 drafts. Geometry and visual navigation belong to the renderer.
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

use crate::tools::ToolError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextAffinity {
    Before,
    #[default]
    After,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextCursor {
    pub byte: usize,
    pub affinity: TextAffinity,
}

impl TextCursor {
    pub fn new(byte: usize) -> Self {
        Self {
            byte,
            affinity: TextAffinity::After,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextSelection {
    pub anchor: TextCursor,
    pub focus: TextCursor,
}

impl TextSelection {
    pub fn caret(cursor: TextCursor) -> Self {
        Self {
            anchor: cursor,
            focus: cursor,
        }
    }

    pub fn range(self) -> Range<usize> {
        self.anchor.byte.min(self.focus.byte)..self.anchor.byte.max(self.focus.byte)
    }
}

/// Both identities must match the frame used for a pointer or visual movement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextRevision {
    pub draft: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionSnapshot {
    pub text: String,
    /// Original committed selection; never updated by subsequent preedits.
    pub replacement: Range<usize>,
    /// SDL scalar offsets converted to UTF-8 bytes; None means unset.
    pub selection: Option<Range<usize>>,
    /// Preedit's exact byte range in the virtual display string.
    pub display_range: Range<usize>,
}

/// One stored line break or control character (a CR LF pair counts as one)
/// shown as a visible marker. Display only: the committed text keeps it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayMarker {
    /// Its bytes in the text before marking: the committed text with any
    /// preedit spliced in.
    pub source: Range<usize>,
    /// The whole marker unit in `display_text`, including any zero-width
    /// guard that keeps a neighbour from joining the marker's grapheme.
    pub display: Range<usize>,
    /// The visible marker glyph inside `display`.
    pub glyph: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputSnapshot {
    pub committed: String,
    /// Complete UTF-8 for shared shaping, never escaped or elided, except
    /// that each stored line break or control character is shown as one
    /// marker grapheme (see [`mark_stored_breaks`]) so the field stays on one
    /// line.
    pub display_text: String,
    pub selection: TextSelection,
    /// Grapheme-safe cursors in `display_text`, including the preedit splice.
    pub display_selection: TextSelection,
    pub composition: Option<CompositionSnapshot>,
    /// Markers in `display_text`, in order. Empty when nothing is marked.
    pub markers: Vec<DisplayMarker>,
    pub revision: TextRevision,
}

impl TextInputSnapshot {
    /// Map a displayed cursor back to committed text. Interior preedit positions
    /// have no committed counterpart; callers must cancel composition first.
    /// A position inside a marker unit (beside its zero-width guard) maps to
    /// the edge of the stored character on the same side of the glyph.
    pub fn committed_cursor(&self, cursor: TextCursor) -> Result<TextCursor, ToolError> {
        validate_cursor(&self.display_text, cursor)?;
        let cursor = TextCursor {
            byte: unmarked_byte(&self.markers, cursor.byte),
            ..cursor
        };
        let byte = if let Some(composition) = &self.composition {
            // Compare in unmarked coordinates, as the cursor now is.
            let display = unmarked_byte(&self.markers, composition.display_range.start)
                ..unmarked_byte(&self.markers, composition.display_range.end);
            if cursor.byte < display.start {
                cursor.byte
            } else if cursor.byte == display.start {
                composition.replacement.start
            } else if cursor.byte < display.end {
                return Err(ToolError("Cursor is inside transient composition"));
            } else {
                composition
                    .replacement
                    .end
                    .checked_add(cursor.byte - display.end)
                    .ok_or(ToolError("Invalid composition mapping"))?
            }
        } else {
            cursor.byte
        };
        let result = TextCursor { byte, ..cursor };
        validate_cursor(&self.committed, result)?;
        Ok(result)
    }
}

/// SDL reports Unicode scalar counts, never UTF-8 bytes or UTF-16 units.
pub fn scalar_selection_bytes(
    text: &str,
    start: i32,
    length: i32,
) -> Result<Option<Range<usize>>, ToolError> {
    if start < -1 || length < -1 {
        return Err(ToolError("Invalid composition selection"));
    }
    if start == -1 || length == -1 {
        let count = text.chars().count();
        for known in [start, length].into_iter().filter(|value| *value >= 0) {
            if usize::try_from(known).map_err(|_| ToolError("Invalid composition selection"))?
                > count
            {
                return Err(ToolError("Composition selection is out of bounds"));
            }
        }
        return Ok(None);
    }
    let start = usize::try_from(start).map_err(|_| ToolError("Invalid composition selection"))?;
    let length = usize::try_from(length).map_err(|_| ToolError("Invalid composition selection"))?;
    let end = start
        .checked_add(length)
        .ok_or(ToolError("Invalid composition selection"))?;
    let byte_at = |index: usize| {
        text.char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(text.len()))
            .nth(index)
            .ok_or(ToolError("Composition selection is out of bounds"))
    };
    Ok(Some(byte_at(start)?..byte_at(end)?))
}

/// Characters a single-line field never accepts as new input. Text layout
/// breaks lines at every control line break (LF, CR, VT, FF, NEL) and also at
/// U+2028 LINE SEPARATOR and U+2029 PARAGRAPH SEPARATOR, which are not
/// controls.
fn breaks_single_line(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

/// Marker for a line break: LF, CR, a CR LF pair, VT, FF, NEL and U+2028.
pub const LINE_BREAK_MARKER: char = '\u{23ce}';
/// Marker for U+2029 PARAGRAPH SEPARATOR.
pub const PARAGRAPH_MARKER: char = '\u{b6}';
/// Marker for a tab.
pub const TAB_MARKER: char = '\u{2192}';
/// Marker for every other control character.
pub const CONTROL_MARKER: char = '\u{2b1a}';
/// Zero-width, grapheme-breaking guard placed beside a marker only when a
/// neighbour (a prepended or combining character) would otherwise join the
/// marker's grapheme, which the stored character never does.
const MARKER_GUARD: char = '\u{200b}';

/// The marker shown for one stored character (or CR LF pair) that a
/// single-line field never accepts as new input.
fn marker_for(stored: &str) -> char {
    match stored {
        "\r\n" | "\n" | "\r" | "\u{b}" | "\u{c}" | "\u{85}" | "\u{2028}" => LINE_BREAK_MARKER,
        "\u{2029}" => PARAGRAPH_MARKER,
        "\t" => TAB_MARKER,
        _ => CONTROL_MARKER,
    }
}

/// Stored characters a single-line field shows as markers, as byte ranges
/// in `text`. A CR LF pair is one range, as it is one grapheme to editing.
fn stored_breaks(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (start, character) = chars.next()?;
            if !breaks_single_line(character) {
                continue;
            }
            let mut end = start + character.len_utf8();
            if character == '\r' && chars.next_if(|(_, next)| *next == '\n').is_some() {
                end += 1;
            }
            return Some(start..end);
        }
    })
}

/// Display text for a single-line value: each stored line break or control
/// character (a CR LF pair counts as one) becomes one visible marker
/// grapheme, so nothing breaks the line. Returns the markers in order. The
/// stored value is never changed; this is presentation only.
pub fn mark_stored_breaks(text: &str) -> (String, Vec<DisplayMarker>) {
    let mut display = String::with_capacity(text.len());
    let mut markers = Vec::new();
    let mut copied = 0;
    for source in stored_breaks(text) {
        display.push_str(&text[copied..source.start]);
        let glyph = marker_for(&text[source.clone()]);
        let mut glyph_text = [0; 4];
        let glyph_text: &str = glyph.encode_utf8(&mut glyph_text);
        let start = display.len();
        // The stored character is its own grapheme (UAX #29 GB4/GB5); the
        // marker must stay one too, or a caret could not sit beside it.
        let joins_before = display
            .graphemes(true)
            .next_back()
            .is_some_and(|before| format!("{before}{glyph}").graphemes(true).count() == 1);
        let joins_after = text[source.end..]
            .graphemes(true)
            .next()
            .is_some_and(|after| {
                format!("{glyph}{after}").graphemes(true).next() != Some(glyph_text)
            });
        if joins_before {
            display.push(MARKER_GUARD);
        }
        let glyph_range = display.len()..display.len() + glyph.len_utf8();
        display.push(glyph);
        if joins_after {
            display.push(MARKER_GUARD);
        }
        markers.push(DisplayMarker {
            source: source.clone(),
            display: start..display.len(),
            glyph: glyph_range,
        });
        copied = source.end;
    }
    display.push_str(&text[copied..]);
    (display, markers)
}

/// A short note on the stored line breaks (paragraph separators included)
/// and other control characters a single-line value holds, or `None`
/// without any. A CR LF pair counts once, as it has one marker.
pub fn stored_break_hint(text: &str) -> Option<String> {
    let (breaks, controls) = stored_breaks(text).fold((0, 0), |(breaks, controls), range| {
        match marker_for(&text[range]) {
            LINE_BREAK_MARKER | PARAGRAPH_MARKER => (breaks + 1, controls),
            _ => (breaks, controls + 1),
        }
    });
    let count =
        |count: usize, noun: &str| format!("{count} {noun}{}", if count == 1 { "" } else { "s" });
    match (breaks, controls) {
        (0, 0) => None,
        (breaks, 0) => Some(format!("Contains {}", count(breaks, "line break"))),
        (0, controls) => Some(format!("Contains {}", count(controls, "control character"))),
        (breaks, controls) => Some(format!(
            "Contains {} and {}",
            count(breaks, "line break"),
            count(controls, "control character")
        )),
    }
}

/// A position in the marked text to its position before marking. Inside a
/// marker unit (next to a guard) it maps to the stored character's edge on
/// the same side of the glyph; the guard has no width, so that is also where
/// the caret is drawn.
fn unmarked_byte(markers: &[DisplayMarker], byte: usize) -> usize {
    let index = markers.partition_point(|marker| marker.display.start < byte);
    match index.checked_sub(1).map(|index| &markers[index]) {
        Some(marker) if byte < marker.display.end => {
            if byte <= marker.glyph.start {
                marker.source.start
            } else {
                marker.source.end
            }
        }
        Some(marker) => marker.source.end + (byte - marker.display.end),
        None => byte,
    }
}

fn validate_cursor(text: &str, cursor: TextCursor) -> Result<(), ToolError> {
    if cursor.byte == text.len()
        || text
            .grapheme_indices(true)
            .any(|(byte, _)| byte == cursor.byte)
    {
        Ok(())
    } else {
        Err(ToolError("Cursor is not an extended grapheme boundary"))
    }
}
