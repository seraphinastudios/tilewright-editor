// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Differential parity: the editor's own field and the shared field, driven
//! by the same sequences from the same stored values, must agree on every
//! result and refusal wording, the committed and displayed text, the
//! selection in stored terms, and the action a review hands the engine. The
//! inspector's string field, its boolean, integer, number and reference
//! fields, both zone name drafts, the palette filter (with the rows it
//! keeps), the coordinates, a new anchor's id and a typed portal location
//! are compared.
use std::{fs, path::Path};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewCatalog, PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, SelectionKey,
    field_input::FieldImplementation,
    inspector::inspect,
    text_input::{TextCursor, TextInputSnapshot, TextRevision, TextSelection},
    tools::{PortalStep, ToolError, ToolMode, ToolState, ZoneChoice, ZoneDraft},
};
use tilewright_source::World;
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

/// Every text draft follows the switch: the string field, the coordinates,
/// both zone name drafts, a new anchor's id, a portal's typed location and
/// the palette filter. (The sample grants no typed inspector field; those
/// follow it in [`every_typed_inspector_field_follows_the_switch_and_ends_an_open_portal_step`].)
#[test]
fn every_text_draft_follows_the_switch_to_the_shared_field() {
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
    // The coordinates follow the switch too.
    tools.begin_move(&view, false).unwrap();
    assert!(tools.shared_draft().is_some(), "the coordinates");
    // Both zone name drafts follow the switch.
    let (world, catalog) = (document.world(), &catalog);
    for implementation in FieldImplementation::ALL {
        let shared = implementation == FieldImplementation::Shared;
        let mut tools = zone_tools(world, catalog, implementation);
        for draft in [ZoneDraft::Name, ZoneDraft::Create] {
            begin_zone(&mut tools, world, catalog, draft, document.revision()).unwrap();
            assert_eq!(tools.zone_draft(), Some(draft));
            assert_eq!(tools.shared_draft().is_some(), shared, "{draft:?}");
        }
        tools.set_mode(ToolMode::Anchor);
        assert!(tools.draft_text().is_none());
        // So do a new anchor's id and a portal's typed location.
        let tile = SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 3,
            y: 3,
        };
        tools
            .begin_anchor_create(catalog, Some(&tile), document.revision(), false)
            .unwrap();
        assert!(tools.anchor_draft().is_some());
        assert_eq!(tools.shared_draft().is_some(), shared, "an anchor id");
        let gate = SelectionKey::Object {
            uid: "gate_east".into(),
        };
        tools
            .begin_portal_location(world, catalog, Some(&gate), document.revision(), false)
            .unwrap();
        assert!(tools.portal_draft().is_some());
        assert_eq!(tools.shared_draft().is_some(), shared, "a portal location");
        tools.begin_move(&view, false).unwrap();
        assert_eq!(tools.shared_draft().is_some(), shared, "the coordinates");
        // The palette filter follows the switch too.
        tools.set_mode(ToolMode::Place);
        tools.focus_filter().unwrap();
        assert!(tools.filter_focused());
        assert_eq!(tools.shared_draft().is_some(), shared, "the palette filter");
    }
}

/// A tool state in Zone mode with meadow chosen, its zone name drafts
/// edited by `implementation`.
fn zone_tools(
    world: &World,
    catalog: &PreviewCatalog,
    implementation: FieldImplementation,
) -> ToolState {
    let mut tools = ToolState::default();
    tools.set_string_field(implementation);
    tools.set_mode(ToolMode::Zone);
    tools
        .choose_zone(world, catalog, ZoneChoice::Zone("meadow".into()))
        .unwrap();
    tools
}

/// Open `draft` for meadow: its display name, or a new zone under it.
fn begin_zone(
    tools: &mut ToolState,
    world: &World,
    catalog: &PreviewCatalog,
    draft: ZoneDraft,
    expected: [u8; 32],
) -> Result<(), ToolError> {
    match draft {
        ZoneDraft::Name => tools.begin_zone_name(world, catalog, expected, false),
        ZoneDraft::Create => {
            tools.begin_zone_create(world, catalog, Some("meadow".into()), expected, false)
        }
    }
}

/// The sample with meadow's display name set to `name` (`None`: no name).
fn zone_world(root: &Path, name: Option<&str>) {
    world_with(root, "Trail marker");
    let path = root.join("planes/main/zones.jsonc");
    let mut zones: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let meadow = zones["zones"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|zone| zone["id"] == "meadow")
        .unwrap()
        .as_object_mut()
        .unwrap();
    match name {
        Some(name) => meadow.insert("name".into(), name.into()),
        None => meadow.remove("name"),
    };
    fs::write(path, serde_json::to_vec_pretty(&zones).unwrap()).unwrap();
}

/// Text typed into a zone draft first: ids valid, invalid, used and too
/// long, with and without a name, and names past 256 characters.
fn zone_start(random: &mut Seeded) -> String {
    let starts = [
        "meadow.bridge Old bridge".to_owned(),
        "meadow.bridge".to_owned(),
        "Bridge".to_owned(),
        "path".to_owned(),
        " no id".to_owned(),
        "a  Spaced name  ".to_owned(),
        "x".repeat(128),
        "x".repeat(129),
        format!("grove {}", "é".repeat(257)),
        format!("grove {}", "😀".repeat(256)),
        String::new(),
    ];
    starts[random.below(starts.len())].clone()
}

/// The randomised parity run of [`both_fields_agree_on_every_step_and_on_the_reviewed_action`]
/// for both zone drafts: every step, refusal and the reviewed zone action.
#[test]
#[allow(clippy::too_many_lines)] // The stored names and outcomes are listed inline.
fn both_fields_agree_on_every_zone_draft_step_and_on_the_reviewed_zone_action() {
    let mut random = Seeded(0x2073_0e5e);
    let full = "😀".repeat(256);
    let names = [
        Some("Quiet meadow"),
        None,
        Some("Stone\npath\u{2028}way\t"),
        Some("e\u{301} 👨\u{200d}👩\u{200d}👧"),
        Some("עברית 123 العربية"),
        Some(full.as_str()),
    ];
    let (mut steps, mut refusals) = (0, 0);
    let mut reviewed = std::collections::BTreeMap::<String, usize>::new();
    for name in names {
        let dir = tempfile::tempdir().unwrap();
        zone_world(dir.path(), name);
        let document = WorldDocument::open(dir.path()).unwrap();
        let (catalog, _) = loader::load(
            document.source(),
            document.world(),
            PreviewLimits::default(),
        )
        .unwrap();
        let world = document.world();
        for round in 0..80 {
            let draft = if round % 2 == 0 {
                ZoneDraft::Name
            } else {
                ZoneDraft::Create
            };
            let [mut own, mut shared] = FieldImplementation::ALL.map(|implementation| {
                let mut tools = zone_tools(world, &catalog, implementation);
                begin_zone(&mut tools, world, &catalog, draft, document.revision()).unwrap();
                tools
            });
            assert!(own.shared_draft().is_none());
            assert!(shared.shared_draft().is_some());
            let stored = match draft {
                ZoneDraft::Name => name.unwrap_or_default(),
                ZoneDraft::Create => "",
            };
            assert_eq!(own.draft_text(), Some(stored));
            assert_eq!(shared.draft_text(), Some(stored));
            let stale = [
                own.input_revision().unwrap(),
                shared.input_revision().unwrap(),
            ];
            let mut ops = vec![Op::Input(zone_start(&mut random))];
            if draft == ZoneDraft::Name && random.below(2) == 0 {
                ops.insert(0, Op::SelectAll);
            }
            ops.extend((0..random.below(10)).map(|_| op(&mut random)));
            for op in &ops {
                let before = [own.input_revision(), shared.input_revision()];
                let results = [
                    apply(&mut own, op, stale[0]),
                    apply(&mut shared, op, stale[1]),
                ];
                assert_eq!(results[0], results[1], "{name:?} {draft:?} {op:?}");
                refusals += usize::from(results[0].is_err());
                assert_eq!(
                    comparable(own.draft_snapshot().unwrap()),
                    comparable(shared.draft_snapshot().unwrap()),
                    "{name:?} {draft:?} {op:?}"
                );
                assert_eq!(own.is_composing(), shared.is_composing());
                assert_eq!(
                    before[0] == own.input_revision(),
                    before[1] == shared.input_revision(),
                    "{name:?} {draft:?} {op:?}"
                );
                steps += 1;
            }
            let actions = [
                own.zone_action(world, &catalog, false),
                shared.zone_action(world, &catalog, false),
            ];
            assert_eq!(actions[0], actions[1], "{name:?} {draft:?} {ops:?}");
            let kind = match &actions[0] {
                Ok(Action::RenameZone { name: None, .. }) => "rename to no name".to_owned(),
                Ok(Action::RenameZone { .. }) => "rename".to_owned(),
                Ok(Action::CreateZone { name: None, .. }) => "create with no name".to_owned(),
                Ok(Action::CreateZone { .. }) => "create".to_owned(),
                Ok(other) => panic!("{other:?}"),
                Err(error) => error.0.to_owned(),
            };
            *reviewed.entry(kind).or_default() += 1;
        }
    }
    // Every outcome of a zone review was reached, refusals included.
    for kind in [
        "rename",
        "rename to no name",
        "create",
        "create with no name",
        "Type the new zone's id first",
        "A zone id is 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
        "This plane already has a zone with this id",
        "A zone's display name has at most 256 characters",
        "Finish or cancel composition before review",
    ] {
        assert!(
            reviewed.get(kind).is_some_and(|count| *count > 0),
            "{kind}: {reviewed:?}"
        );
    }
    assert!(steps > 2_000 && refusals > 200, "{steps} {refusals}");
}

/// Each zone draft's byte limit, refused in the editor's own wording in
/// both fields, and what review makes of text at the limit.
#[test]
fn both_fields_refuse_zone_drafts_at_their_byte_limits_in_the_editors_words() {
    const REFUSED: ToolError =
        ToolError("Input exceeds byte budget or contains control characters or line breaks");
    let dir = tempfile::tempdir().unwrap();
    zone_world(dir.path(), None);
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, _) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let world = document.world();
    // (draft, the limit, text filling it, what review gives)
    let full_name = "😀".repeat(256);
    let full_create = format!("meadow.b {}", "😀".repeat((1153 - 9) / 4));
    let cases = [
        (
            ZoneDraft::Name,
            1024,
            full_name.clone(),
            Ok(Some(full_name.clone())),
        ),
        (
            ZoneDraft::Name,
            1024,
            "a".repeat(1024),
            Err(ToolError(
                "A zone's display name has at most 256 characters",
            )),
        ),
        (
            ZoneDraft::Create,
            1153,
            format!("{full_create}{}", "a".repeat(1153 - full_create.len())),
            Err(ToolError(
                "A zone's display name has at most 256 characters",
            )),
        ),
        (
            ZoneDraft::Create,
            1153,
            format!("{} {full_name}", "x".repeat(128)),
            Ok(Some(full_name.clone())),
        ),
        (
            ZoneDraft::Create,
            1153,
            format!("{} {}", "x".repeat(129), "a".repeat(1023)),
            Err(ToolError(
                "A zone id is 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
            )),
        ),
    ];
    for (draft, limit, text, review) in cases {
        for implementation in FieldImplementation::ALL {
            let case = format!("{draft:?} {implementation:?} {}", text.len());
            let mut tools = zone_tools(world, &catalog, implementation);
            begin_zone(&mut tools, world, &catalog, draft, document.revision()).unwrap();
            assert_eq!(
                tools.shared_draft().is_some(),
                implementation == FieldImplementation::Shared
            );
            // One byte over, at once: refused and nothing changes.
            assert_eq!(tools.input(&format!("{text}a")), Err(REFUSED), "{case}");
            assert_eq!(tools.draft_text(), Some(""), "{case}");
            // Exactly the limit: taken.
            assert_eq!(text.len(), limit, "{case}");
            tools.input(&text).unwrap();
            // Then nothing more, typed or composed; deleting one makes room.
            assert_eq!(tools.input("a"), Err(REFUSED), "{case}");
            assert_eq!(tools.update_preedit("a", 1, 0), Err(REFUSED), "{case}");
            assert_eq!(tools.draft_text(), Some(text.as_str()), "{case}");
            let reviewed = tools
                .zone_action(world, &catalog, false)
                .map(|action| match action {
                    Action::RenameZone { name, .. } | Action::CreateZone { name, .. } => name,
                    other => panic!("{other:?}"),
                });
            assert_eq!(reviewed, review, "{case}");
            tools.backspace();
            tools.input("a").unwrap();
        }
    }
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

/// Text typed into the palette filter: parts of the sample's prototype ids
/// in either case, letters no id has, refused text and pastes of up to and
/// past the filter's 64 bytes.
const FILTER_PIECES: [&str; 13] = [
    "a", "St", "ONE", "r", "T", "ate", "x", "é", "青", "\n", "\t", "\u{2028}", "",
];

/// One step on the palette filter: an edit ([`op`]), or one of the filter's
/// own: release it (Enter or a click), clear it (Esc), focus it again (`/`).
#[derive(Clone, Debug)]
enum FilterOp {
    Edit(Op),
    Release,
    Clear,
    Focus,
}

fn filter_op(random: &mut Seeded) -> FilterOp {
    match random.below(20) {
        0..=5 => FilterOp::Edit(Op::Input(
            FILTER_PIECES[random.below(FILTER_PIECES.len())].to_owned(),
        )),
        6 => FilterOp::Edit(Op::Input("ab".repeat(28 + random.below(8)))),
        7 => FilterOp::Release,
        8 => FilterOp::Clear,
        9 | 10 => FilterOp::Focus,
        _ => FilterOp::Edit(op(random)),
    }
}

/// Apply `op` to a tool state in Place mode. An edit on an unfocused filter
/// is what the editor does with it: nothing reaches a draft.
fn apply_filter(
    tools: &mut ToolState,
    op: &FilterOp,
    stale: TextRevision,
) -> Result<(), ToolError> {
    match op {
        FilterOp::Edit(op) if tools.filter_focused() => apply(tools, op, stale),
        FilterOp::Edit(_) => Ok(()),
        FilterOp::Release => {
            tools.release_filter();
            Ok(())
        }
        FilterOp::Clear => {
            tools.clear_filter();
            Ok(())
        }
        FilterOp::Focus => tools.focus_filter(),
    }
}

/// The randomised parity run for the palette filter: after every step both
/// fields hold the same filter text, snapshot, focus and composition, keep
/// the same palette rows, and refuse review the same way.
#[test]
fn both_fields_agree_on_every_filter_step_and_on_the_rows_it_keeps() {
    let mut random = Seeded(0x5f11_7e25);
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
    let (mut steps, mut refusals, mut focused) = (0, 0, 0);
    let mut kept = std::collections::BTreeSet::new();
    for _ in 0..200 {
        let [mut own, mut shared] = FieldImplementation::ALL.map(|implementation| {
            let mut tools = ToolState::default();
            tools.set_string_field(implementation);
            tools.set_mode(ToolMode::Place);
            tools.focus_filter().unwrap();
            tools
        });
        let stale = [
            own.input_revision().unwrap(),
            shared.input_revision().unwrap(),
        ];
        for _ in 0..random.below(24) {
            // Each focus really is the implementation it is named for.
            assert!(own.shared_draft().is_none());
            assert_eq!(shared.shared_draft().is_some(), shared.filter_focused());
            let op = filter_op(&mut random);
            let before = [own.input_revision(), shared.input_revision()];
            let results = [
                apply_filter(&mut own, &op, stale[0]),
                apply_filter(&mut shared, &op, stale[1]),
            ];
            assert_eq!(results[0], results[1], "{op:?}");
            refusals += usize::from(results[0].is_err());
            assert_eq!(own.palette_filter(), shared.palette_filter(), "{op:?}");
            assert_eq!(own.filter_focused(), shared.filter_focused(), "{op:?}");
            assert_eq!(
                own.draft_snapshot().map(comparable),
                shared.draft_snapshot().map(comparable),
                "{op:?}"
            );
            assert_eq!(own.is_composing(), shared.is_composing(), "{op:?}");
            assert_eq!(
                before[0] == own.input_revision(),
                before[1] == shared.input_revision(),
                "{op:?}"
            );
            // The rows the filter keeps, after every step.
            let rows = own.filtered_palette(&catalog);
            assert_eq!(rows, shared.filtered_palette(&catalog), "{op:?}");
            kept.insert(rows.len());
            // The filter is never reviewed, and never makes a change.
            if own.filter_focused() {
                focused += 1;
                assert_eq!(
                    own.build_action(&view, false),
                    Err(ToolError("The palette filter is never reviewed"))
                );
                assert_eq!(
                    shared.build_action(&view, false),
                    Err(ToolError("The palette filter is never reviewed"))
                );
            }
            steps += 1;
        }
        // Leaving Place clears the filter in both.
        for tools in [&mut own, &mut shared] {
            tools.set_mode(ToolMode::Select);
            assert_eq!(tools.palette_filter(), "");
            assert!(!tools.filter_focused());
        }
    }
    // Every row count was reached, from none to the whole palette of six
    // (no text keeps exactly five of bush, crate, gate, lamp, marker and
    // stone).
    assert_eq!(kept, [0, 1, 2, 3, 4, 6].into(), "{kept:?}");
    assert!(
        steps > 2_000 && refusals > 100 && focused > 1_000,
        "{steps} {refusals} {focused}"
    );
}

/// The filter's 64-byte limit, refused in the editor's own wording in both
/// fields, and a composition dropped on release in both.
#[test]
fn both_fields_refuse_the_filter_past_its_byte_limit_and_drop_a_composition_on_release() {
    const REFUSED: ToolError =
        ToolError("Input exceeds byte budget or contains control characters or line breaks");
    let full = "😀".repeat(16);
    assert_eq!(full.len(), tilewright_editor_ui::tools::MAX_FILTER_BYTES);
    for implementation in FieldImplementation::ALL {
        let mut tools = ToolState::default();
        tools.set_string_field(implementation);
        tools.set_mode(ToolMode::Place);
        tools.focus_filter().unwrap();
        let case = format!("{implementation:?}");
        assert_eq!(
            tools.shared_draft().is_some(),
            implementation == FieldImplementation::Shared
        );
        // One byte over, at once: refused and nothing changes.
        assert_eq!(tools.input(&format!("{full}a")), Err(REFUSED), "{case}");
        assert_eq!(tools.palette_filter(), "", "{case}");
        tools.input(&full).unwrap();
        assert_eq!(tools.input("a"), Err(REFUSED), "{case}");
        assert_eq!(tools.update_preedit("a", 1, 0), Err(REFUSED), "{case}");
        assert_eq!(tools.palette_filter(), full, "{case}");
        // Released and focused again, it holds the whole limit, and still
        // nothing more.
        tools.release_filter();
        tools.focus_filter().unwrap();
        assert_eq!(tools.palette_filter(), full, "{case}");
        assert_eq!(tools.input("a"), Err(REFUSED), "{case}");
        // A composition is not part of the filter, and release drops it.
        tools.select_all().unwrap();
        tools.input("lam").unwrap();
        tools.update_preedit("p", 1, 0).unwrap();
        assert!(tools.is_composing(), "{case}");
        assert_eq!(tools.palette_filter(), "lam", "{case}");
        tools.release_filter();
        assert_eq!(tools.palette_filter(), "lam", "{case}");
        tools.focus_filter().unwrap();
        assert!(!tools.is_composing(), "{case}");
        assert_eq!(tools.draft_text(), Some("lam"), "{case}");
        // Esc clears and releases, a composition included.
        tools.update_preedit("p", 1, 0).unwrap();
        assert!(tools.clear_filter(), "{case}");
        assert_eq!(
            (tools.palette_filter(), tools.filter_focused()),
            ("", false)
        );
        assert!(!tools.clear_filter(), "{case}: nothing left to clear");
    }
}

/// Focusing the filter ends an open portal step, as every other draft does,
/// with either field: the next tile click places instead of picking.
#[test]
fn focusing_the_filter_ends_an_open_portal_step_with_either_field() {
    let dir = tempfile::tempdir().unwrap();
    world_with(dir.path(), "Trail marker");
    let document = WorldDocument::open(dir.path()).unwrap();
    let (catalog, _) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let gate = SelectionKey::Object {
        uid: "gate_west".into(),
    };
    for implementation in FieldImplementation::ALL {
        for step in [PortalStep::PickTile, PortalStep::Anchors] {
            let mut tools = ToolState::default();
            tools.set_string_field(implementation);
            tools.set_mode(ToolMode::Place);
            assert!(
                tools
                    .toggle_portal_step(
                        document.world(),
                        &catalog,
                        Some(&gate),
                        step,
                        document.revision(),
                        false,
                    )
                    .unwrap()
            );
            assert_eq!(tools.portal_step(), Some(("gate_west", step)));
            tools.focus_filter().unwrap();
            assert!(tools.filter_focused(), "{implementation:?} {step:?}");
            assert_eq!(tools.portal_step(), None, "{implementation:?} {step:?}");
            assert!(!tools.portal_picking());
        }
    }
}

/// The drafts typed as text but reviewed as something other than a field
/// value: the coordinates (`x,y`), a new anchor's id and a portal's typed
/// location.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Typed {
    Move,
    Anchor,
    Location,
}

impl Typed {
    const ALL: [Self; 3] = [Self::Move, Self::Anchor, Self::Location];

    /// The draft's own byte limit.
    fn limit(self) -> usize {
        match self {
            Self::Move => 4096,
            Self::Anchor => 128,
            Self::Location => 128 + 3 * 21,
        }
    }

    /// The text the draft opens with: `marker_1`'s position, or nothing.
    fn opened(self) -> &'static str {
        match self {
            Self::Move => "3,2",
            Self::Anchor | Self::Location => "",
        }
    }

    /// Text typed into the draft first: accepted, refused by each review
    /// check, and in between.
    fn start(self, random: &mut Seeded) -> String {
        let starts: &[&str] = match self {
            Self::Move => &[
                "4,4",
                " 5 , 6 ",
                "-3,-2",
                "+4,4",
                "4,x",
                "x,4",
                "44",
                "1,2,3",
                "9223372036854775808,1",
                "٣,٤",
                "",
            ],
            Self::Anchor => &[
                "camp",
                "camp.b_c-1",
                "Camp",
                "-x",
                "é",
                "arrival",
                "lookout",
                "a b",
                "",
            ],
            Self::Location => &[
                "0 5 5",
                "main 0, 5, 5",
                "0,,5,,5",
                "0 9007199254740991 -9007199254740991",
                "1 1",
                "main 0 5 5 5",
                "elsewhere 0 1 1",
                "main x 1 1",
                "0 1 99999999999999999",
                "0 -2 2",
                "٠ ١ ٢",
                "",
            ],
        };
        match random.below(starts.len() + 2) {
            index if index < starts.len() => starts[index].to_owned(),
            // An id or location of the most bytes World takes, or one more.
            index => match self {
                Self::Move => format!("1,{}", "0".repeat(4094 + index - starts.len())),
                Self::Anchor => "a".repeat(128 + index - starts.len()),
                Self::Location => format!("0 5 {}", "0".repeat(186 + index - starts.len())),
            },
        }
    }
}

/// The sample's document and catalog, with the inspector view of `marker_1`.
struct Sample {
    _dir: tempfile::TempDir,
    document: WorldDocument,
    catalog: PreviewCatalog,
}

impl Sample {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        world_with(dir.path(), "Trail marker");
        let document = WorldDocument::open(dir.path()).unwrap();
        let (catalog, _) = loader::load(
            document.source(),
            document.world(),
            PreviewLimits::default(),
        )
        .unwrap();
        Self {
            _dir: dir,
            document,
            catalog,
        }
    }

    fn marker() -> SelectionKey {
        SelectionKey::Object {
            uid: "marker_1".into(),
        }
    }

    fn gate() -> SelectionKey {
        SelectionKey::Object {
            uid: "gate_west".into(),
        }
    }

    fn tile() -> SelectionKey {
        SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 3,
            y: 3,
        }
    }

    /// Open `typed` in `tools`, as the window opens it.
    fn begin(&self, tools: &mut ToolState, typed: Typed) -> Result<(), ToolError> {
        let (world, catalog, revision) = (
            self.document.world(),
            &self.catalog,
            self.document.revision(),
        );
        match typed {
            Typed::Move => {
                let key = Self::marker();
                let view = inspect(world, catalog, Some(&key), 4, revision);
                tools.begin_move(&view, false)
            }
            Typed::Anchor => {
                tools.begin_anchor_create(catalog, Some(&Self::tile()), revision, false)
            }
            Typed::Location => {
                tools.begin_portal_location(world, catalog, Some(&Self::gate()), revision, false)
            }
        }
    }

    /// What Enter or Review hands the engine for the open `typed` draft.
    fn review(&self, tools: &ToolState, typed: Typed) -> Result<Action, ToolError> {
        let (world, catalog) = (self.document.world(), &self.catalog);
        match typed {
            Typed::Move => {
                let key = Self::marker();
                let view = inspect(world, catalog, Some(&key), 4, self.document.revision());
                tools.build_action(&view, false)
            }
            Typed::Anchor => tools.anchor_action(world, catalog, false),
            Typed::Location => {
                tools.portal_location_action(world, catalog, Some(&Self::gate()), false)
            }
        }
    }
}

/// The randomised parity run of [`both_fields_agree_on_every_step_and_on_the_reviewed_action`]
/// for the coordinates, a new anchor's id and a typed portal location:
/// every step and refusal, then the action review hands the engine, built
/// from each field's committed text.
#[test]
#[allow(clippy::too_many_lines)] // The outcomes are listed inline.
fn both_fields_agree_on_every_coordinate_anchor_and_location_step_and_on_the_reviewed_action() {
    let mut random = Seeded(0x7e57_ed17);
    let sample = Sample::new();
    let (mut steps, mut refusals) = (0, 0);
    let mut reviewed = std::collections::BTreeMap::<(Typed, String), usize>::new();
    for round in 0..900 {
        let typed = Typed::ALL[round % 3];
        let [mut own, mut shared] = FieldImplementation::ALL.map(|implementation| {
            let mut tools = ToolState::default();
            tools.set_string_field(implementation);
            sample.begin(&mut tools, typed).unwrap();
            tools
        });
        // Each side really is the implementation it is named for.
        assert!(own.shared_draft().is_none(), "{typed:?}");
        assert!(shared.shared_draft().is_some(), "{typed:?}");
        assert_eq!(own.draft_text(), Some(typed.opened()));
        assert_eq!(shared.draft_text(), Some(typed.opened()));
        let stale = [
            own.input_revision().unwrap(),
            shared.input_revision().unwrap(),
        ];
        let mut ops = vec![Op::Input(typed.start(&mut random))];
        if typed == Typed::Move && random.below(4) != 0 {
            ops.insert(0, Op::SelectAll);
        }
        ops.extend((0..random.below(6)).map(|_| op(&mut random)));
        for op in &ops {
            let before = [own.input_revision(), shared.input_revision()];
            let results = [
                apply(&mut own, op, stale[0]),
                apply(&mut shared, op, stale[1]),
            ];
            assert_eq!(results[0], results[1], "{typed:?} {op:?}");
            refusals += usize::from(results[0].is_err());
            assert_eq!(own.draft_text(), shared.draft_text(), "{typed:?} {op:?}");
            assert_eq!(
                comparable(own.draft_snapshot().unwrap()),
                comparable(shared.draft_snapshot().unwrap()),
                "{typed:?} {op:?}"
            );
            assert_eq!(own.is_composing(), shared.is_composing());
            assert_eq!(
                before[0] == own.input_revision(),
                before[1] == shared.input_revision(),
                "{typed:?} {op:?}"
            );
            steps += 1;
        }
        let actions = [sample.review(&own, typed), sample.review(&shared, typed)];
        assert_eq!(actions[0], actions[1], "{typed:?} {ops:?}");
        let kind = match &actions[0] {
            Ok(Action::Move { .. }) => "move".to_owned(),
            Ok(Action::CreateAnchor { .. }) => "create anchor".to_owned(),
            Ok(Action::SetPortal { .. }) => "set portal".to_owned(),
            Ok(other) => panic!("{other:?}"),
            Err(error) => error.0.to_owned(),
        };
        *reviewed.entry((typed, kind)).or_default() += 1;
    }
    // Every outcome of each review was reached, refusals included.
    let composing = "Finish or cancel composition before review";
    for (typed, kind) in [
        (Typed::Move, "move"),
        (Typed::Move, "Enter coordinates as x,y"),
        (Typed::Move, "Invalid x coordinate"),
        (Typed::Move, "Invalid y coordinate"),
        (Typed::Move, composing),
        (Typed::Anchor, "create anchor"),
        (Typed::Anchor, "Type the new anchor's id first"),
        (
            Typed::Anchor,
            "An anchor id is 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
        ),
        (
            Typed::Anchor,
            "This plane already has an anchor with this id",
        ),
        (Typed::Anchor, composing),
        (Typed::Location, "set portal"),
        (Typed::Location, "Type layer x y, or plane layer x y"),
        (
            Typed::Location,
            "Type three or four values: layer x y, or plane layer x y",
        ),
        (Typed::Location, "The world has no plane with this id"),
        (Typed::Location, "The layer must be a whole number (32-bit)"),
        (
            Typed::Location,
            "x and y must be whole numbers within plus or minus 9007199254740991",
        ),
        (Typed::Location, "The portal already leads there"),
        (Typed::Location, composing),
    ] {
        assert!(
            reviewed
                .get(&(typed, kind.to_owned()))
                .is_some_and(|count| *count > 0),
            "{typed:?} {kind}: {reviewed:?}"
        );
    }
    assert!(steps > 2_000 && refusals > 200, "{steps} {refusals}");
}

/// Each of these drafts' byte limits, refused in the editor's own wording
/// in both fields, a character straddling the limit refused whole, and
/// what review makes of text that fills the limit exactly.
#[test]
fn both_fields_refuse_coordinate_anchor_and_location_drafts_at_their_byte_limits() {
    const REFUSED: ToolError =
        ToolError("Input exceeds byte budget or contains control characters or line breaks");
    let sample = Sample::new();
    for typed in Typed::ALL {
        let limit = typed.limit();
        let full = match typed {
            Typed::Move => format!("1,{}", "0".repeat(limit - 2)),
            Typed::Anchor => "a".repeat(limit),
            Typed::Location => format!("0 5 {}5", "0".repeat(limit - 5)),
        };
        assert_eq!(full.len(), limit);
        let mut reviews = Vec::new();
        for implementation in FieldImplementation::ALL {
            let case = format!("{typed:?} {implementation:?}");
            let mut tools = ToolState::default();
            tools.set_string_field(implementation);
            sample.begin(&mut tools, typed).unwrap();
            assert_eq!(
                tools.shared_draft().is_some(),
                implementation == FieldImplementation::Shared,
                "{case}"
            );
            tools.select_all().unwrap();
            // One byte over, at once: refused and nothing changes.
            assert_eq!(tools.input(&format!("{full}a")), Err(REFUSED), "{case}");
            assert_eq!(tools.draft_text(), Some(typed.opened()), "{case}");
            // One byte short, then a two-byte character: refused whole.
            tools.select_all().unwrap();
            tools.input(&full[..limit - 1]).unwrap();
            assert_eq!(tools.input("é"), Err(REFUSED), "{case}");
            assert_eq!(tools.update_preedit("é", 1, 0), Err(REFUSED), "{case}");
            // Exactly the limit: taken, then nothing more.
            tools.input(&full[limit - 1..]).unwrap();
            assert_eq!(tools.draft_text(), Some(full.as_str()), "{case}");
            assert_eq!(tools.input("a"), Err(REFUSED), "{case}");
            assert_eq!(tools.update_preedit("a", 1, 0), Err(REFUSED), "{case}");
            reviews.push(sample.review(&tools, typed));
            // Deleting one makes room for one.
            tools.backspace();
            tools.input("0").unwrap();
            assert_eq!(tools.draft_text().map(str::len), Some(limit), "{case}");
        }
        assert_eq!(reviews[0], reviews[1], "{typed:?}");
        let reviewed = reviews[0].as_ref().unwrap();
        match (typed, reviewed) {
            (Typed::Move, Action::Move { x: 1, y: 0, .. })
            | (Typed::Anchor, Action::CreateAnchor { .. })
            | (Typed::Location, Action::SetPortal { .. }) => {}
            other => panic!("{other:?}"),
        }
    }
}

/// Opening any of these drafts ends an open portal step with either field,
/// as every other draft does: a tile click then no longer picks.
#[test]
fn opening_a_coordinate_anchor_or_location_draft_ends_an_open_portal_step() {
    let sample = Sample::new();
    for implementation in FieldImplementation::ALL {
        for typed in Typed::ALL {
            for step in [PortalStep::PickTile, PortalStep::Anchors] {
                let case = format!("{implementation:?} {typed:?} {step:?}");
                let mut tools = ToolState::default();
                tools.set_string_field(implementation);
                assert!(
                    tools
                        .toggle_portal_step(
                            sample.document.world(),
                            &sample.catalog,
                            Some(&Sample::gate()),
                            step,
                            sample.document.revision(),
                            false,
                        )
                        .unwrap(),
                    "{case}"
                );
                sample.begin(&mut tools, typed).unwrap();
                assert_eq!(
                    tools.shared_draft().is_some(),
                    implementation == FieldImplementation::Shared,
                    "{case}"
                );
                assert_eq!(tools.portal_step(), None, "{case}");
                assert!(!tools.portal_picking(), "{case}");
            }
        }
    }
}

/// The fields the hand test adds to a copy of the sample (the docs' step
/// 16): a boolean, an integer, a number and a reference field in the
/// object schema (`profile/schema/object.jsonc`), each granted on
/// `marker_1` in `preview.jsonc`'s `editing.fields`.
const TYPED_SCHEMA: [(&str, &str); 4] = [
    ("flag", r#"{"type": "boolean"}"#),
    ("count", r#"{"type": "integer", "min": -2, "max": 7}"#),
    ("weight", r#"{"type": "number", "min": -1, "max": 2}"#),
    ("kind", r#"{"type": "reference", "target": "proto"}"#),
];

/// The sample with [`TYPED_SCHEMA`] added and granted, and `marker_1`'s
/// values for them set to `stored` (`None`: absent, as in the hand test).
fn typed_world(root: &Path, stored: Option<[serde_json::Value; 4]>) {
    world_with(root, "Trail marker");
    let path = root.join("profile/schema/object.jsonc");
    let mut schema: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (name, spec) in TYPED_SCHEMA {
        schema["fields"][name] = serde_json::from_str(spec).unwrap();
    }
    fs::write(path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
    let path = root.join("preview.jsonc");
    let mut catalog: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let grants = catalog["editing"]["fields"].as_array_mut().unwrap();
    for (name, _) in TYPED_SCHEMA {
        grants.push(serde_json::json!({"uid": "marker_1", "field": name}));
    }
    fs::write(path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
    if let Some(stored) = stored {
        let path = root.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let fields = &mut chunk["objects"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|object| object["uid"] == "marker_1")
            .unwrap()["fields"];
        for ((name, _), value) in TYPED_SCHEMA.into_iter().zip(stored) {
            fields[name] = value;
        }
        fs::write(path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
    }
}

/// The inspector's typed fields: a boolean, an integer, a number and a
/// reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum Scalar {
    Flag,
    Count,
    Weight,
    Kind,
}

impl Scalar {
    const ALL: [Self; 4] = [Self::Flag, Self::Count, Self::Weight, Self::Kind];

    fn name(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Count => "count",
            Self::Weight => "weight",
            Self::Kind => "kind",
        }
    }

    /// Text typed into the draft first: accepted, refused by each review
    /// check, at and past the schema's range, and in between.
    fn start(self, random: &mut Seeded) -> String {
        let starts: &[&str] = match self {
            Self::Flag => &[
                "true", "false", "TRUE", "True", "1", "yes", " true", "false ", "",
            ],
            Self::Count => &[
                "7",
                "-2",
                "8",
                "-3",
                "0",
                "-0",
                "+3",
                "3.0",
                "1e1",
                "٣",
                "9223372036854775807",
                "9223372036854775808",
                " 3",
                "",
            ],
            Self::Weight => &[
                "2",
                "-1",
                "1.5",
                "2.01",
                "-1.01",
                "2.0000001",
                "NaN",
                "inf",
                "-inf",
                "1e0",
                ".5",
                "5.",
                "+1",
                "1,5",
                "٣",
                "-0",
                "",
            ],
            Self::Kind => &[
                "marker", "stone", "grass", "Marker", "unknown", "marker ", "lamp", "",
            ],
        };
        match random.below(starts.len() + 2) {
            index if index < starts.len() => starts[index].to_owned(),
            // Zeros filling the draft's 4,096 bytes, or one more.
            index => "0".repeat(4095 + index - starts.len()),
        }
    }
}

/// The sample with [`TYPED_SCHEMA`], its document, catalog and `marker_1`.
struct TypedSample {
    _dir: tempfile::TempDir,
    document: WorldDocument,
    catalog: PreviewCatalog,
    marker: SelectionKey,
}

impl TypedSample {
    fn new(stored: Option<[serde_json::Value; 4]>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        typed_world(dir.path(), stored);
        let document = WorldDocument::open(dir.path()).unwrap();
        let (catalog, _) = loader::load(
            document.source(),
            document.world(),
            PreviewLimits::default(),
        )
        .unwrap();
        Self {
            _dir: dir,
            document,
            catalog,
            marker: Sample::marker(),
        }
    }

    fn view(&self) -> tilewright_editor_ui::inspector::Inspector<'_> {
        inspect(
            self.document.world(),
            &self.catalog,
            Some(&self.marker),
            4,
            self.document.revision(),
        )
    }
}

/// The stored values the typed runs open: absent, and each type's own.
fn typed_stored() -> [Option<[serde_json::Value; 4]>; 2] {
    [
        None,
        Some([
            serde_json::json!(true),
            serde_json::json!(3),
            serde_json::json!(0.5),
            serde_json::json!("marker"),
        ]),
    ]
}

/// The randomised parity run of [`both_fields_agree_on_every_step_and_on_the_reviewed_action`]
/// for the inspector's boolean, integer, number and reference fields:
/// every step and refusal, then the `SetField` (or the refusal, in each
/// type's own words) review builds from each field's committed text.
#[test]
#[allow(clippy::too_many_lines)] // The outcomes are listed inline.
fn both_fields_agree_on_every_typed_inspector_field_step_and_on_the_reviewed_value() {
    let mut random = Seeded(0x5ca1_a7ed);
    let (mut steps, mut refusals) = (0, 0);
    let mut reviewed = std::collections::BTreeMap::<(Scalar, String), usize>::new();
    for stored in typed_stored() {
        let sample = TypedSample::new(stored.clone());
        let view = sample.view();
        for round in 0..800 {
            let scalar = Scalar::ALL[round % 4];
            let [mut own, mut shared] = FieldImplementation::ALL.map(|implementation| {
                let mut tools = ToolState::default();
                tools.set_string_field(implementation);
                tools.begin_field(&view, scalar.name(), false).unwrap();
                tools
            });
            // Each side really is the implementation it is named for.
            assert!(own.shared_draft().is_none(), "{scalar:?}");
            assert!(shared.shared_draft().is_some(), "{scalar:?}");
            assert_eq!(own.draft_text(), shared.draft_text(), "{scalar:?}");
            let stale = [
                own.input_revision().unwrap(),
                shared.input_revision().unwrap(),
            ];
            let mut ops = vec![Op::Input(scalar.start(&mut random))];
            if random.below(4) != 0 {
                ops.insert(0, Op::SelectAll);
            }
            ops.extend((0..random.below(6)).map(|_| op(&mut random)));
            for op in &ops {
                let before = [own.input_revision(), shared.input_revision()];
                let results = [
                    apply(&mut own, op, stale[0]),
                    apply(&mut shared, op, stale[1]),
                ];
                assert_eq!(results[0], results[1], "{scalar:?} {op:?}");
                refusals += usize::from(results[0].is_err());
                assert_eq!(own.draft_text(), shared.draft_text(), "{scalar:?} {op:?}");
                assert_eq!(
                    comparable(own.draft_snapshot().unwrap()),
                    comparable(shared.draft_snapshot().unwrap()),
                    "{scalar:?} {op:?}"
                );
                assert_eq!(own.is_composing(), shared.is_composing());
                assert_eq!(
                    before[0] == own.input_revision(),
                    before[1] == shared.input_revision(),
                    "{scalar:?} {op:?}"
                );
                steps += 1;
            }
            let actions = [
                own.build_action(&view, false),
                shared.build_action(&view, false),
            ];
            assert_eq!(actions[0], actions[1], "{scalar:?} {ops:?}");
            let kind = match &actions[0] {
                Ok(Action::SetField { field, .. }) => {
                    assert_eq!(field, scalar.name());
                    "set field".to_owned()
                }
                Ok(other) => panic!("{other:?}"),
                Err(error) => error.0.to_owned(),
            };
            *reviewed.entry((scalar, kind)).or_default() += 1;
        }
    }
    // Every outcome of each type's review was reached, refusals included.
    let composing = "Finish or cancel composition before review";
    for (scalar, kind) in [
        (Scalar::Flag, "set field"),
        (Scalar::Flag, "Enter true or false"),
        (Scalar::Flag, composing),
        (Scalar::Count, "set field"),
        (Scalar::Count, "Enter an integer"),
        (Scalar::Count, "Integer is outside schema range"),
        (Scalar::Count, composing),
        (Scalar::Weight, "set field"),
        (Scalar::Weight, "Enter a finite number"),
        (
            Scalar::Weight,
            "Number is non-finite or outside schema range",
        ),
        (Scalar::Weight, composing),
        (Scalar::Kind, "set field"),
        (
            Scalar::Kind,
            "Reference does not name an authoritative definition",
        ),
        (Scalar::Kind, composing),
    ] {
        assert!(
            reviewed
                .get(&(scalar, kind.to_owned()))
                .is_some_and(|count| *count > 0),
            "{scalar:?} {kind}: {reviewed:?}"
        );
    }
    assert!(steps > 4_000 && refusals > 400, "{steps} {refusals}");
}

/// The typed fields' 4,096-byte limit, refused in the editor's own wording
/// in both fields, a character straddling the limit refused whole, and
/// what review makes of text that fills the limit exactly.
#[test]
fn both_fields_refuse_typed_inspector_fields_at_their_byte_limit() {
    const REFUSED: ToolError =
        ToolError("Input exceeds byte budget or contains control characters or line breaks");
    let sample = TypedSample::new(None);
    let view = sample.view();
    let limit = tilewright_editor_ui::inspector::MAX_INPUT_BYTES;
    assert_eq!(limit, 4096);
    let full = "0".repeat(limit);
    for scalar in Scalar::ALL {
        let mut reviews = Vec::new();
        for implementation in FieldImplementation::ALL {
            let case = format!("{scalar:?} {implementation:?}");
            let mut tools = ToolState::default();
            tools.set_string_field(implementation);
            tools.begin_field(&view, scalar.name(), false).unwrap();
            assert_eq!(
                tools.shared_draft().is_some(),
                implementation == FieldImplementation::Shared,
                "{case}"
            );
            assert_eq!(tools.draft_text(), Some(""), "{case}: absent");
            // One byte over, at once: refused and nothing changes.
            assert_eq!(tools.input(&format!("{full}0")), Err(REFUSED), "{case}");
            assert_eq!(tools.draft_text(), Some(""), "{case}");
            // One byte short, then a two-byte character: refused whole.
            tools.input(&full[..limit - 1]).unwrap();
            assert_eq!(tools.input("é"), Err(REFUSED), "{case}");
            assert_eq!(tools.update_preedit("é", 1, 0), Err(REFUSED), "{case}");
            // Exactly the limit: taken, then nothing more.
            tools.input("0").unwrap();
            assert_eq!(tools.draft_text(), Some(full.as_str()), "{case}");
            assert_eq!(tools.input("0"), Err(REFUSED), "{case}");
            assert_eq!(tools.update_preedit("0", 1, 0), Err(REFUSED), "{case}");
            reviews.push(tools.build_action(&view, false));
            // Deleting one makes room for one.
            tools.backspace();
            tools.input("0").unwrap();
            assert_eq!(tools.draft_text().map(str::len), Some(limit), "{case}");
        }
        assert_eq!(reviews[0], reviews[1], "{scalar:?}");
        match (scalar, &reviews[0]) {
            (Scalar::Count, Ok(Action::SetField { value, .. })) => {
                assert_eq!(value, &serde_json::json!(0));
            }
            (Scalar::Weight, Ok(Action::SetField { value, .. })) => {
                assert_eq!(value, &serde_json::json!(0.0));
            }
            (Scalar::Flag, Err(ToolError("Enter true or false")))
            | (
                Scalar::Kind,
                Err(ToolError("Reference does not name an authoritative definition")),
            ) => {}
            other => panic!("{other:?}"),
        }
    }
}

/// Every typed inspector field follows the switch, from absent and from a
/// stored value, opens with the same text in both fields, and ends an
/// open portal step as every other draft does.
#[test]
fn every_typed_inspector_field_follows_the_switch_and_ends_an_open_portal_step() {
    for stored in typed_stored() {
        let sample = TypedSample::new(stored.clone());
        let view = sample.view();
        for scalar in Scalar::ALL {
            let mut opened = Vec::new();
            for implementation in FieldImplementation::ALL {
                for step in [None, Some(PortalStep::PickTile), Some(PortalStep::Anchors)] {
                    let case = format!("{stored:?} {scalar:?} {implementation:?} {step:?}");
                    let mut tools = ToolState::default();
                    tools.begin_field(&view, scalar.name(), false).unwrap();
                    assert!(tools.shared_draft().is_none(), "{case}: off by default");
                    tools.set_string_field(implementation);
                    if let Some(step) = step {
                        assert!(
                            tools
                                .toggle_portal_step(
                                    sample.document.world(),
                                    &sample.catalog,
                                    Some(&Sample::gate()),
                                    step,
                                    sample.document.revision(),
                                    false,
                                )
                                .unwrap(),
                            "{case}"
                        );
                    }
                    tools.begin_field(&view, scalar.name(), false).unwrap();
                    assert_eq!(
                        tools.shared_draft().is_some(),
                        implementation == FieldImplementation::Shared,
                        "{case}"
                    );
                    assert_eq!(tools.portal_step(), None, "{case}");
                    assert!(!tools.portal_picking(), "{case}");
                    opened.push(comparable(tools.draft_snapshot().unwrap()));
                }
            }
            assert!(
                opened.windows(2).all(|pair| pair[0] == pair[1]),
                "{scalar:?}"
            );
            let expected = match (&stored, scalar) {
                (None, _) => "",
                (Some(_), Scalar::Flag) => "true",
                (Some(_), Scalar::Count) => "3",
                (Some(_), Scalar::Weight) => "0.5",
                (Some(_), Scalar::Kind) => "marker",
            };
            assert_eq!(opened[0].committed, expected, "{scalar:?}");
        }
    }
}
