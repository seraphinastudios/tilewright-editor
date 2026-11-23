// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Differential parity: the editor's own field and the shared field, driven
//! by the same sequences from the same stored values, must agree on every
//! result and refusal wording, the committed and displayed text, the
//! selection in stored terms, and the action a review hands the engine.
use std::{fs, path::Path};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    SelectionKey,
    field_input::FieldImplementation,
    inspector::inspect,
    text_input::{TextCursor, TextInputSnapshot, TextRevision, TextSelection},
    tools::{ToolError, ToolState},
};
use unicode_segmentation::UnicodeSegmentation;

/// Stored values to open: plain, empty, every stored-break class, joining
/// marks next to a break, right-to-left and emoji sequences.
const OPEN: [&str; 9] = [
    "Trail marker",
    "",
    "Trail\nmar\u{2028}ker\u{2029}",
    "\r\n\t\u{1}\u{85}x",
    "A\n\u{301}B",
    "e\u{301} 👨\u{200d}👩\u{200d}👧 🇺🇳",
    "עברית 123 العربية",
    "क्\u{200d}ष\u{2028}",
    "\u{600}\nZ",
];

/// Text typed, pasted or composed, refused characters included.
const PIECES: [&str; 14] = [
    "a", "é", "青", "e\u{301}", "\u{301}", "👍🏽", "עב", "\n", "\t", "\u{2028}", "\u{2029}",
    "x\u{1b}y", "", "\u{23ce}",
];

#[derive(Clone, Debug)]
enum Op {
    Input(String),
    Backspace,
    Delete,
    SelectAll,
    /// Grapheme indexes into the committed text (past its end: invalid),
    /// presented at the current revision or a stale one.
    Select(usize, usize, bool),
    /// A byte inside the committed text, maybe not a boundary.
    Caret(usize),
    Preedit(String, i32, i32),
    CancelPreedit,
}

struct Seeded(u64);
impl Seeded {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % bound as u64).unwrap()
    }
}

fn op(random: &mut Seeded) -> Op {
    match random.below(12) {
        0..=2 => Op::Input(PIECES[random.below(PIECES.len())].to_owned()),
        3 => Op::Input("ab".repeat(1 + random.below(3000))),
        4 => Op::Backspace,
        5 => Op::Delete,
        6 => Op::SelectAll,
        7 => Op::Select(random.below(24), random.below(24), random.below(6) == 0),
        8 => Op::Caret(random.below(40)),
        9 | 10 => {
            let text = PIECES[random.below(PIECES.len())].to_owned();
            let count = i32::try_from(text.chars().count()).unwrap();
            let start = i32::try_from(random.below(4)).unwrap() - 1;
            let length = i32::try_from(random.below(3)).unwrap() - 1;
            Op::Preedit(text, start.min(count), length)
        }
        _ => Op::CancelPreedit,
    }
}

fn apply(tools: &mut ToolState, op: &Op, stale: TextRevision) -> Result<(), ToolError> {
    match op {
        Op::Input(text) => tools.input(text),
        Op::Backspace => {
            tools.backspace();
            Ok(())
        }
        Op::Delete => tools.delete(),
        Op::SelectAll => tools.select_all(),
        Op::Select(anchor, focus, old) => {
            let text = tools.draft_text().unwrap().to_owned();
            let boundary = |index: usize| {
                text.grapheme_indices(true)
                    .map(|(byte, _)| byte)
                    .chain([text.len()])
                    .nth(index)
                    .unwrap_or(text.len() + 1)
            };
            let presented = if *old {
                stale
            } else {
                tools.input_revision().unwrap()
            };
            tools.set_selection(
                TextSelection {
                    anchor: TextCursor::new(boundary(*anchor)),
                    focus: TextCursor::new(boundary(*focus)),
                },
                presented,
            )
        }
        Op::Caret(byte) => {
            let presented = tools.input_revision().unwrap();
            tools.set_selection(TextSelection::caret(TextCursor::new(*byte)), presented)
        }
        Op::Preedit(text, start, length) => tools.update_preedit(text, *start, *length),
        Op::CancelPreedit => tools.cancel_preedit(),
    }
}

/// Everything but the revision numbers, which each field counts its own way.
fn comparable(mut snapshot: TextInputSnapshot) -> TextInputSnapshot {
    snapshot.revision = TextRevision::default();
    snapshot
}

fn world_with(root: &Path, stored: &str) {
    fn copy(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    copy(&fixture_world("prototype-neutral"), root);
    let path = root.join("planes/main/layers/0/0_0.jsonc");
    let mut chunk: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    chunk["objects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|object| object["uid"] == "marker_1")
        .unwrap()["fields"]["label"] = serde_json::json!(stored);
    fs::write(path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
}

#[test]
fn both_fields_agree_on_every_step_and_on_the_reviewed_action() {
    let mut random = Seeded(0x0e1f_1e1d);
    let (mut steps, mut refusals, mut actions) = (0, 0, 0);
    for stored in OPEN {
        let dir = tempfile::tempdir().unwrap();
        world_with(dir.path(), stored);
        let document = WorldDocument::open(dir.path()).unwrap();
        let (catalog, _) = loader::load(
            document.source(),
            document.world(),
            PreviewLimits::default(),
        )
        .unwrap();
        let key = SelectionKey::Object {
            uid: "marker_1".into(),
        };
        let view = inspect(
            document.world(),
            &catalog,
            Some(&key),
            4,
            document.revision(),
        );
        for _ in 0..120 {
            let [mut own, mut shared] = FieldImplementation::ALL.map(|implementation| {
                let mut tools = ToolState::default();
                tools.set_string_field(implementation);
                tools.begin_field(&view, "label", false).unwrap();
                tools
            });
            // Each side really is the implementation it is named for, so a
            // build that ignores the switch cannot compare a field with itself.
            assert!(own.shared_draft().is_none());
            assert!(shared.shared_draft().is_some());
            assert_eq!(own.draft_text(), Some(stored));
            assert_eq!(shared.draft_text(), Some(stored));
            let stale = [
                own.input_revision().unwrap(),
                shared.input_revision().unwrap(),
            ];
            for _ in 0..random.below(16) {
                let op = op(&mut random);
                let before = [own.input_revision(), shared.input_revision()];
                let results = [
                    apply(&mut own, &op, stale[0]),
                    apply(&mut shared, &op, stale[1]),
                ];
                assert_eq!(results[0], results[1], "{stored:?} {op:?}");
                refusals += usize::from(results[0].is_err());
                assert_eq!(own.draft_text(), shared.draft_text(), "{stored:?} {op:?}");
                assert_eq!(
                    comparable(own.draft_snapshot().unwrap()),
                    comparable(shared.draft_snapshot().unwrap()),
                    "{stored:?} {op:?}"
                );
                assert_eq!(own.is_composing(), shared.is_composing());
                // A change moves both revisions; a no-op or refusal neither.
                assert_eq!(
                    before[0] == own.input_revision(),
                    before[1] == shared.input_revision(),
                    "{stored:?} {op:?}"
                );
                steps += 1;
            }
            // Review: the same engine action, or the same refusal, built
            // from each field's committed text byte for byte.
            let reviewed = [
                own.build_action(&view, false),
                shared.build_action(&view, false),
            ];
            assert_eq!(reviewed[0], reviewed[1], "{stored:?}");
            actions += usize::from(reviewed[0].is_ok());
        }
    }
    assert!(
        steps > 5_000 && refusals > 500 && actions > 200,
        "{steps} {refusals} {actions}"
    );
}

#[test]
fn only_string_fields_use_the_shared_field() {
    let dir = tempfile::tempdir().unwrap();
    world_with(dir.path(), "Trail marker");
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, _) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let key = SelectionKey::Object {
        uid: "marker_1".into(),
    };
    let view = inspect(
        document.world(),
        &catalog,
        Some(&key),
        4,
        document.revision(),
    );
    let mut tools = ToolState::default();
    tools.begin_field(&view, "label", false).unwrap();
    assert!(tools.shared_draft().is_none(), "off by default");
    tools.set_string_field(FieldImplementation::Shared);
    tools.begin_field(&view, "label", false).unwrap();
    let shared = tools.shared_draft().expect("the string field is shared");
    assert_eq!(shared.snapshot().composer.committed, "Trail marker");
    // A revision presented for an earlier shared draft never selects in a
    // later one, even where the two revisions are equal.
    let earlier = tools.input_revision().unwrap();
    tools.begin_field(&view, "label", false).unwrap();
    let later = tools.input_revision().unwrap();
    assert_eq!(earlier.revision, later.revision);
    assert_ne!(earlier.draft, later.draft);
    assert_eq!(
        tools.set_selection(TextSelection::caret(TextCursor::new(0)), earlier),
        Err(ToolError("Input presentation is stale"))
    );
    tools
        .set_selection(TextSelection::caret(TextCursor::new(0)), later)
        .unwrap();
    // Coordinates, the palette filter and every other draft keep the
    // editor's own field.
    tools.begin_move(&view, false).unwrap();
    assert!(tools.shared_draft().is_none());
}

#[test]
fn the_frame_records_name_the_pinned_shared_field() {
    // A pin move has to rename the field in the frame records too.
    let manifest = include_str!("../../../Cargo.toml");
    let version = FieldImplementation::SHARED_NAME
        .strip_prefix("tilewright-ui ")
        .unwrap();
    // Line by line: a Windows checkout may end lines with CR LF.
    let pin = format!("tilewright-ui = \"={version}\"");
    assert!(
        manifest.lines().any(|line| line.trim_end() == pin),
        "{}",
        FieldImplementation::SHARED_NAME
    );
}
