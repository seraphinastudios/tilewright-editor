// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Differential parity: the editor's own field and the shared field, driven
//! by the same sequences from the same stored values, must agree on every
//! result and refusal wording, the committed and displayed text, the
//! selection in stored terms, and the action a review hands the engine. The
//! inspector's string field and both zone name drafts are compared.
use std::{fs, path::Path};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewCatalog, PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, SelectionKey,
    field_input::FieldImplementation,
    inspector::inspect,
    text_input::{TextCursor, TextInputSnapshot, TextRevision, TextSelection},
    tools::{ToolError, ToolMode, ToolState, ZoneChoice, ZoneDraft},
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

#[test]
fn only_string_fields_and_zone_names_use_the_shared_field() {
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
        // A new anchor's id and a portal's typed location keep the editor's
        // own field with the switch on.
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
        assert!(tools.shared_draft().is_none(), "an anchor id");
        let gate = SelectionKey::Object {
            uid: "gate_east".into(),
        };
        tools
            .begin_portal_location(world, catalog, Some(&gate), document.revision(), false)
            .unwrap();
        assert!(tools.portal_draft().is_some());
        assert!(tools.shared_draft().is_none(), "a portal location");
        tools.set_mode(ToolMode::Place);
        tools.focus_filter().unwrap();
        assert!(tools.shared_draft().is_none(), "the palette filter");
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
