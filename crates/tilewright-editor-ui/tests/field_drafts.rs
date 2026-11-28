// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Every text draft on the shared field: the inspector's string field, its
//! boolean, integer, number and reference fields, both zone name drafts, the
//! palette filter, the coordinates, a new anchor's id and a typed portal
//! location open on it, refuse text past their byte limits in the editor's
//! words, review what fills the limit, and end an open portal step.
use std::{fs, path::Path};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewCatalog, PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, SelectionKey,
    inspector::inspect,
    text_input::{TextCursor, TextInputSnapshot, TextRevision, TextSelection},
    tools::{PortalStep, ToolError, ToolMode, ToolState, ZoneChoice, ZoneDraft},
};
use tilewright_source::World;

/// Everything but the revision numbers, which each opening counts afresh.
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

/// The shared field edits every text draft, from a bare tool state on: the
/// string field, the coordinates, both zone name drafts, a new anchor's
/// id, a portal's typed location and the palette filter. (The sample
/// grants no typed inspector field; those are checked in
/// [`every_typed_inspector_field_opens_on_the_shared_field_and_ends_an_open_portal_step`].)
#[test]
fn every_text_draft_opens_on_the_shared_field() {
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
    // The coordinates too.
    tools.begin_move(&view, false).unwrap();
    assert!(tools.shared_draft().is_some(), "the coordinates");
    // Both zone name drafts.
    let (world, catalog) = (document.world(), &catalog);
    let mut tools = zone_tools(world, catalog);
    for draft in [ZoneDraft::Name, ZoneDraft::Create] {
        begin_zone(&mut tools, world, catalog, draft, document.revision()).unwrap();
        assert_eq!(tools.zone_draft(), Some(draft));
        assert!(tools.shared_draft().is_some(), "{draft:?}");
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
    assert!(tools.shared_draft().is_some(), "an anchor id");
    let gate = SelectionKey::Object {
        uid: "gate_east".into(),
    };
    tools
        .begin_portal_location(world, catalog, Some(&gate), document.revision(), false)
        .unwrap();
    assert!(tools.portal_draft().is_some());
    assert!(tools.shared_draft().is_some(), "a portal location");
    tools.begin_move(&view, false).unwrap();
    assert!(tools.shared_draft().is_some(), "the coordinates");
    // And the palette filter.
    tools.set_mode(ToolMode::Place);
    tools.focus_filter().unwrap();
    assert!(tools.filter_focused());
    assert!(tools.shared_draft().is_some(), "the palette filter");
}

/// A tool state in Zone mode with meadow chosen.
fn zone_tools(world: &World, catalog: &PreviewCatalog) -> ToolState {
    let mut tools = ToolState::default();
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

/// Each zone draft's byte limit, refused in the editor's words, and what
/// review makes of text at the limit.
#[test]
fn zone_drafts_are_refused_at_their_byte_limits_in_the_editors_words() {
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
        let case = format!("{draft:?} {}", text.len());
        let mut tools = zone_tools(world, &catalog);
        begin_zone(&mut tools, world, &catalog, draft, document.revision()).unwrap();
        assert!(tools.shared_draft().is_some());
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

/// The filter's 64-byte limit, refused in the editor's words, and a
/// composition dropped on release.
#[test]
fn the_filter_is_refused_past_its_byte_limit_and_drops_a_composition_on_release() {
    const REFUSED: ToolError =
        ToolError("Input exceeds byte budget or contains control characters or line breaks");
    let full = "😀".repeat(16);
    assert_eq!(full.len(), tilewright_editor_ui::tools::MAX_FILTER_BYTES);
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Place);
    tools.focus_filter().unwrap();
    assert!(tools.shared_draft().is_some());
    // One byte over, at once: refused and nothing changes.
    assert_eq!(tools.input(&format!("{full}a")), Err(REFUSED));
    assert_eq!(tools.palette_filter(), "");
    tools.input(&full).unwrap();
    assert_eq!(tools.input("a"), Err(REFUSED));
    assert_eq!(tools.update_preedit("a", 1, 0), Err(REFUSED));
    assert_eq!(tools.palette_filter(), full);
    // Released and focused again, it holds the whole limit, and still
    // nothing more.
    tools.release_filter();
    tools.focus_filter().unwrap();
    assert_eq!(tools.palette_filter(), full);
    assert_eq!(tools.input("a"), Err(REFUSED));
    // A composition is not part of the filter, and release drops it.
    tools.select_all().unwrap();
    tools.input("lam").unwrap();
    tools.update_preedit("p", 1, 0).unwrap();
    assert!(tools.is_composing());
    assert_eq!(tools.palette_filter(), "lam");
    tools.release_filter();
    assert_eq!(tools.palette_filter(), "lam");
    tools.focus_filter().unwrap();
    assert!(!tools.is_composing());
    assert_eq!(tools.draft_text(), Some("lam"));
    // Esc clears and releases, a composition included.
    tools.update_preedit("p", 1, 0).unwrap();
    assert!(tools.clear_filter());
    assert_eq!(
        (tools.palette_filter(), tools.filter_focused()),
        ("", false)
    );
    assert!(!tools.clear_filter(), "nothing left to clear");
}

/// Focusing the filter ends an open portal step, as every other draft does,
/// and the next tile click places instead of picking.
#[test]
fn focusing_the_filter_ends_an_open_portal_step() {
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
    for step in [PortalStep::PickTile, PortalStep::Anchors] {
        let mut tools = ToolState::default();
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
        assert!(tools.filter_focused(), "{step:?}");
        assert_eq!(tools.portal_step(), None, "{step:?}");
        assert!(!tools.portal_picking());
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

/// Each of these drafts' byte limits, refused in the editor's words, a
/// character straddling the limit refused whole, and
/// what review makes of text that fills the limit exactly.
#[test]
fn coordinate_anchor_and_location_drafts_are_refused_at_their_byte_limits() {
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
        let case = format!("{typed:?}");
        let mut tools = ToolState::default();
        sample.begin(&mut tools, typed).unwrap();
        assert!(tools.shared_draft().is_some(), "{case}");
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
        let review = sample.review(&tools, typed);
        // Deleting one makes room for one.
        tools.backspace();
        tools.input("0").unwrap();
        assert_eq!(tools.draft_text().map(str::len), Some(limit), "{case}");
        let reviewed = review.as_ref().unwrap();
        match (typed, reviewed) {
            (Typed::Move, Action::Move { x: 1, y: 0, .. })
            | (Typed::Anchor, Action::CreateAnchor { .. })
            | (Typed::Location, Action::SetPortal { .. }) => {}
            other => panic!("{other:?}"),
        }
    }
}

/// Opening any of these drafts ends an open portal step,
/// as every other draft does: a tile click then no longer picks.
#[test]
fn opening_a_coordinate_anchor_or_location_draft_ends_an_open_portal_step() {
    let sample = Sample::new();
    for typed in Typed::ALL {
        for step in [PortalStep::PickTile, PortalStep::Anchors] {
            let case = format!("{typed:?} {step:?}");
            let mut tools = ToolState::default();
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
            assert!(tools.shared_draft().is_some(), "{case}");
            assert_eq!(tools.portal_step(), None, "{case}");
            assert!(!tools.portal_picking(), "{case}");
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

/// The typed fields' 4,096-byte limit, refused in the editor's words, a
/// character straddling the limit refused whole, and
/// what review makes of text that fills the limit exactly.
#[test]
fn typed_inspector_fields_are_refused_at_their_byte_limit() {
    const REFUSED: ToolError =
        ToolError("Input exceeds byte budget or contains control characters or line breaks");
    let sample = TypedSample::new(None);
    let view = sample.view();
    let limit = tilewright_editor_ui::inspector::MAX_INPUT_BYTES;
    assert_eq!(limit, 4096);
    let full = "0".repeat(limit);
    for scalar in Scalar::ALL {
        let case = format!("{scalar:?}");
        let mut tools = ToolState::default();
        tools.begin_field(&view, scalar.name(), false).unwrap();
        assert!(tools.shared_draft().is_some(), "{case}");
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
        let review = tools.build_action(&view, false);
        // Deleting one makes room for one.
        tools.backspace();
        tools.input("0").unwrap();
        assert_eq!(tools.draft_text().map(str::len), Some(limit), "{case}");
        match (scalar, &review) {
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

/// Every typed inspector field opens on the shared field, from absent and
/// from a stored value, with the same text with or without an open portal
/// step, and ends that step as every other draft does.
#[test]
fn every_typed_inspector_field_opens_on_the_shared_field_and_ends_an_open_portal_step() {
    for stored in typed_stored() {
        let sample = TypedSample::new(stored.clone());
        let view = sample.view();
        for scalar in Scalar::ALL {
            let mut opened = Vec::new();
            for step in [None, Some(PortalStep::PickTile), Some(PortalStep::Anchors)] {
                let case = format!("{stored:?} {scalar:?} {step:?}");
                let mut tools = ToolState::default();
                tools.begin_field(&view, scalar.name(), false).unwrap();
                assert!(tools.shared_draft().is_some(), "{case}");
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
                assert!(tools.shared_draft().is_some(), "{case}");
                assert_eq!(tools.portal_step(), None, "{case}");
                assert!(!tools.portal_picking(), "{case}");
                opened.push(comparable(tools.draft_snapshot().unwrap()));
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
