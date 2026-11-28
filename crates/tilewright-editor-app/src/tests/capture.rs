// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Copy area in Place mode through real SDL pointer and key events, on the
//! CPU scene path: C arms it, a drag spans the rectangle, the release
//! copies, and the copy is then stamped exactly as a catalog stamp is (the
//! same preview, review, proposal step and undo). Every world is a
//! temporary copy of the MIT neutral sample (view 8 x 6); the catalog
//! stamps of `stamps::with_stamps` are there so a copy can be seen
//! replacing one.
// Colours are copied, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::proposal::edit_json;
use super::stamps::{
    click, hover, hover_at, marked, place_objects, press, press_widget, proposal,
    stamp_application, tiles, turn_row, with_stamps,
};
use super::zones::{button, key, route, sidebar_texts, zone_application};
use super::{acknowledge_ui, cell_pixel, input_key};
use crate::prototype::capture_view::{
    CAPTURE_ARMED, CAPTURE_OFF_VIEW, CAPTURE_PENDING, capture_overlay,
};
use crate::prototype::{Application, ROAD_IN_PROGRESS, STAMP_BLOCKED};
use crate::workspace_ui::{Control, Hit, WorkspaceUi};
use sdl3::{event::Event, keyboard::Keycode};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::capture::{CaptureGesture, CaptureRefusal};
use tilewright_editor_ui::stamp::StampObject;
use tilewright_editor_ui::stamps::StampTurn;
use tilewright_editor_ui::uid::SeededUidSource;
use tilewright_editor_ui::{Action, SessionPhase};

const MAIN: &str = "world/planes/main/layers/0/0_0.jsonc";
const SEED: u64 = 7;

/// What a drag over (0, 2)-(3, 4) copies from the sample: `marker_1` at
/// (3, 2), `gate_west` at (0, 3) and `marker_2` at (1, 4), an asymmetric L
/// that every turn and mirror moves differently.
const COPIED: [(&str, [i64; 2]); 3] = [("marker", [3, 0]), ("gate", [0, 1]), ("marker", [1, 2])];

/// Pointer motion over physical pixel `at`, with the left button held or
/// not.
fn motion(app: &mut Application, at: [f32; 2], held: bool) -> Result<(), String> {
    route(
        app,
        &Event::MouseMotion {
            timestamp: 0,
            window_id: 7,
            which: 0,
            mousestate: sdl3::mouse::MouseState::from_sdl_state(u32::from(held)),
            x: at[0],
            y: at[1],
            xrel: 0.0,
            yrel: 0.0,
        },
    )
}

/// A real drag: press on tile `from`, move with the button held over each
/// of `through`, release on the last. Returns the press's result and the
/// release's.
fn drag(
    app: &mut Application,
    from: [i64; 2],
    through: &[[i64; 2]],
) -> (Result<(), String>, Result<(), String>) {
    acknowledge_ui(app);
    let start = cell_pixel(app, from[0], from[1]);
    let down = route(app, &button(true, start));
    let mut last = start;
    for &at in through {
        last = cell_pixel(app, at[0], at[1]);
        motion(app, last, true).unwrap();
    }
    let up = route(app, &button(false, last));
    (down, up)
}

/// C, then a drag from `from` to `to`; the copy must be made.
pub(super) fn copy(app: &mut Application, from: [i64; 2], to: [i64; 2]) {
    press(app, Keycode::C).unwrap();
    let (down, up) = drag(app, from, &[to]);
    down.unwrap();
    up.unwrap();
    assert!(app.tools.copy_choice().is_some());
}

/// The copy as the convention moves it, computed here on its own.
fn copied(turns: u8, mirrored: bool) -> Vec<StampObject> {
    COPIED
        .iter()
        .map(|(proto, [dx, dy])| {
            let mut offset = if mirrored { [-dx, *dy] } else { [*dx, *dy] };
            for _ in 0..turns {
                offset = [-offset[1], offset[0]];
            }
            StampObject {
                proto: (*proto).into(),
                offset,
            }
        })
        .collect()
}

/// The tiles Copy area's rectangle marks, read back through the prepared
/// cell picks; the composed frame holds exactly these copies.
fn rectangle(app: &mut Application) -> BTreeSet<[i64; 2]> {
    app.refresh_ui();
    let overlay = capture_overlay(app);
    let cells: BTreeSet<[i64; 2]> = overlay
        .iter()
        .map(|quad| {
            assert!(!quad.pickable);
            assert_eq!(quad.color, crate::workspace_render::STROKE_PREVIEW_TINT);
            match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a rectangle copy of no cell"),
            }
        })
        .collect();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    let presented = presentation
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.color == crate::workspace_render::STROKE_PREVIEW_TINT && !quad.pickable)
        .count();
    assert_eq!(presented, overlay.len());
    cells
}

fn span(from: [i64; 2], to: [i64; 2]) -> BTreeSet<[i64; 2]> {
    (from[1]..=to[1])
        .flat_map(|y| (from[0]..=to[0]).map(move |x| [x, y]))
        .collect()
}

/// The Copy area and Drop copy buttons: text, enabled, active.
fn capture_row(app: &mut Application) -> Vec<(String, bool, bool)> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .filter(|widget| matches!(widget.control, Some(Control::CopyArea | Control::DropCopy)))
        .map(|widget| (widget.text.clone(), widget.enabled, widget.active))
        .collect()
}

fn every_file(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else {
                let path = entry.path();
                files.push((
                    path.strip_prefix(root).unwrap().display().to_string(),
                    fs::read(&path).unwrap(),
                ));
            }
        }
    }
    files.sort();
    files
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_dragged_copy_stamps_exactly_a_direct_place_objects_in_every_turn_and_mirror() {
    let (temporary, mut app) = stamp_application();
    let (twin_temporary, mut twin) = stamp_application();
    let planes = temporary.path().join("world/planes");
    let original = every_file(&planes);
    // Nothing of Copy area shows until C.
    assert!(capture_row(&mut app).is_empty());
    press(&mut app, Keycode::C).unwrap();
    assert_eq!(app.tools.capture_gesture(), Some(CaptureGesture::Armed));
    assert_eq!(app.status_label(), CAPTURE_ARMED);
    assert_eq!(
        capture_row(&mut app),
        [
            ("Copy area  C".to_owned(), true, true),
            ("Drop copy  Esc".to_owned(), false, false),
        ]
    );
    // While armed, no stamp or prototype is previewed.
    acknowledge_ui(&mut app);
    hover(&mut app, [3, 2]);
    assert!(app.stamp_preview().is_none());
    // The drag: from (3, 4) up and left to (0, 2), through (2, 3); the
    // rectangle follows the pointer and the status line counts what
    // release copies.
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 3, 4);
    route(&mut app, &button(true, start)).unwrap();
    assert_eq!(rectangle(&mut app), BTreeSet::from([[3, 4]]));
    assert_eq!(
        app.status_label(),
        "Copy (3, 4) to (3, 4), 1 x 1 tiles: no objects; release copies nothing."
    );
    // Every moved corner asks for a new frame, as a stroke sample does;
    // motion within the same tile does not.
    let middle = cell_pixel(&mut app, 2, 3);
    acknowledge_ui(&mut app);
    motion(&mut app, middle, true).unwrap();
    assert!(!app.displayed_input_ready());
    acknowledge_ui(&mut app);
    motion(&mut app, [middle[0] + 1.0, middle[1]], true).unwrap();
    assert!(app.displayed_input_ready());
    assert_eq!(rectangle(&mut app), span([2, 3], [3, 4]));
    let end = cell_pixel(&mut app, 0, 2);
    motion(&mut app, end, true).unwrap();
    assert_eq!(rectangle(&mut app), span([0, 2], [3, 4]));
    assert_eq!(
        app.status_label(),
        "Copy (0, 2) to (3, 4), 4 x 3 tiles: 3 objects; release to copy."
    );
    route(&mut app, &button(false, end)).unwrap();
    // The copy is the chosen stamp now, in its captured layout, and Copy
    // area has ended; nothing was written or prepared.
    assert_eq!(app.tools.capture_gesture(), None);
    assert!(rectangle(&mut app).is_empty());
    assert_eq!(
        app.tools.copy_choice().unwrap().objects(),
        copied(0, false).as_slice()
    );
    assert!(app.session.history().pending().is_none());
    assert_eq!(every_file(&planes), original);
    let texts = sidebar_texts(&mut app);
    assert!(texts.contains(&"Copied 3 objects".to_owned()), "{texts:?}");
    assert_eq!(
        turn_row(&mut app),
        [
            ("Turn: 0\u{b0}  R".to_owned(), true, false),
            ("Mirror: off  M".to_owned(), true, false)
        ]
    );
    assert_eq!(
        capture_row(&mut app),
        [
            ("Copy area  C".to_owned(), true, false),
            ("Drop copy  Esc".to_owned(), true, false),
        ]
    );
    assert_eq!(
        app.status_label(),
        "Ready. Click a tile to stamp the copy (3 objects); R turns, M mirrors, Esc drops it."
    );

    // Every state the keys reach, one key at a time: three turns, a
    // mirror (one turn of the mirrored layout), three more turns.
    let keys = [
        None,
        Some(Keycode::R),
        Some(Keycode::R),
        Some(Keycode::R),
        Some(Keycode::M),
        Some(Keycode::R),
        Some(Keycode::R),
        Some(Keycode::R),
    ];
    let mut states = BTreeSet::new();
    let mut steps = Vec::new();
    for keycode in keys {
        if let Some(keycode) = keycode {
            press(&mut app, keycode).unwrap();
        }
        let turn = app.tools.stamp_turn();
        let (turns, mirrored) = (turn.quarter_turns(), turn.mirrored());
        states.insert((turns, mirrored));
        let objects = copied(turns, mirrored);
        // An anchor that keeps every object inside the view.
        let anchor = [
            1 - objects.iter().map(|object| object.offset[0]).min().unwrap(),
            1 - objects.iter().map(|object| object.offset[1]).min().unwrap(),
        ];
        acknowledge_ui(&mut app);
        hover(&mut app, anchor);
        assert_eq!(marked(&mut app), tiles(anchor, &objects), "{turn:?}");
        assert_eq!(
            app.status_label(),
            format!(
                "Stamp the copy at ({}, {}): 3 objects; click to review.",
                anchor[0], anchor[1]
            )
        );
        let preview = app.stamp_preview().unwrap();
        assert_eq!(preview.action, place_objects(&app, anchor, objects.clone()));
        click(&mut app, anchor).unwrap();
        // The twin dispatches the same objects directly: the same sealed
        // transaction and the same review.
        let direct = place_objects(&twin, anchor, objects.clone());
        twin.action(direct).unwrap();
        let sealed = app.session.history().pending().unwrap().sealed().to_owned();
        assert_eq!(
            twin.session.history().pending().unwrap().sealed(),
            sealed,
            "{turn:?}"
        );
        assert_eq!(
            sealed.matches(r#""op":"object.insert""#).count(),
            3,
            "{turn:?}"
        );
        assert_eq!(twin.diff_lines(), app.diff_lines(), "{turn:?}");
        for app in [&mut app, &mut twin] {
            acknowledge_ui(app);
            route(app, &key(Keycode::Return)).unwrap();
            assert!(app.session.history().pending().is_none());
        }
        // The copy stays chosen for the next stamp.
        assert!(app.tools.copy_choice().is_some());
        steps.push((anchor, objects));
    }
    assert_eq!(states.len(), 8);
    // The same proposal, step for step and byte for byte; every step is
    // the copy with the turned and mirrored offsets.
    let (mine, theirs) = (proposal(&app), proposal(&twin));
    assert_eq!(mine.bytes(), theirs.bytes());
    assert_eq!(mine.steps().len(), 8);
    for (step, (anchor, objects)) in mine.steps().iter().zip(&steps) {
        let Action::PlaceObjects {
            x,
            y,
            objects: recorded,
            ..
        } = step.action()
        else {
            panic!("every step is a stamp");
        };
        assert_eq!(([*x, *y], recorded), (*anchor, objects));
        assert_eq!(step.placed_objects().len(), 3);
    }
    assert_eq!(
        every_file(&planes),
        every_file(&twin_temporary.path().join("world/planes"))
    );
    // The copies carry the grant's layer and defaults, never the
    // originals' label, settings or portal.
    let world = app.session.world();
    for uid in mine
        .steps()
        .iter()
        .flat_map(tilewright_editor_proposal::ProposalStep::placed_objects)
    {
        let object = world.object(uid).unwrap();
        // The proto names the object in a failure; a uid is never printed.
        assert!(object.fields.is_empty(), "{}", object.proto);
        assert!(object.portal.is_none(), "{}", object.proto);
        assert_eq!(object.layer.as_deref(), Some("item"));
    }
    // Undo restores the source exactly.
    for _ in 0..8 {
        press(&mut app, Keycode::Z).unwrap();
        press(&mut app, Keycode::Return).unwrap();
    }
    assert_eq!(every_file(&planes), original);
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_rectangle_is_any_shape_clipped_to_the_view_and_refusals_copy_nothing() {
    let (temporary, mut app) = stamp_application();
    let chunk = temporary.path().join(MAIN);
    let original = fs::read(&chunk).unwrap();
    // One tile: a press and release on marker_1.
    press(&mut app, Keycode::C).unwrap();
    let (down, up) = drag(&mut app, [3, 2], &[]);
    down.unwrap();
    up.unwrap();
    assert_eq!(
        app.tools.copy_choice().unwrap().objects(),
        [StampObject {
            proto: "marker".into(),
            offset: [0, 0]
        }]
    );
    // A whole row and a whole column.
    copy(&mut app, [0, 3], [7, 3]);
    assert_eq!(app.tools.copy_choice().unwrap().objects().len(), 1);
    copy(&mut app, [7, 5], [7, 0]);
    assert_eq!(
        app.tools.copy_choice().unwrap().objects(),
        [StampObject {
            proto: "gate".into(),
            offset: [0, 0]
        }]
    );
    let held = app.tools.copy_choice().cloned();

    // An empty rectangle: said before the release, and nothing is copied;
    // the earlier copy stays and Copy area stays armed.
    press(&mut app, Keycode::C).unwrap();
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 5, 1);
    route(&mut app, &button(true, start)).unwrap();
    let end = cell_pixel(&mut app, 6, 2);
    motion(&mut app, end, true).unwrap();
    assert_eq!(
        app.status_label(),
        "Copy (5, 1) to (6, 2), 2 x 2 tiles: no objects; release copies nothing."
    );
    let refused = route(&mut app, &button(false, end)).unwrap_err();
    assert_eq!(refused, CaptureRefusal::Empty.to_string());
    assert_eq!(app.tools.copy_choice().cloned(), held);
    assert_eq!(app.tools.capture_gesture(), Some(CaptureGesture::Armed));

    // Off the map the rectangle's corner stays on the last view tile: a
    // drag out over the sidebar, released there, spans (5, 3)-(7, 5).
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 5, 3);
    route(&mut app, &button(true, start)).unwrap();
    let corner = cell_pixel(&mut app, 7, 5);
    motion(&mut app, corner, true).unwrap();
    app.refresh_ui();
    let sidebar = app.ui.sidebar;
    let (px, py) = (
        sidebar.x + sidebar.width / 2,
        sidebar.y + sidebar.height - 4,
    );
    #[allow(clippy::cast_precision_loss)]
    let outside = [px as f32, py as f32];
    assert!(!matches!(app.ui.hit(px, py), Hit::World(..)));
    motion(&mut app, outside, true).unwrap();
    assert_eq!(rectangle(&mut app), span([5, 3], [7, 5]));
    // A map pixel with no tile of the view keeps it too.
    let background = map_background(&mut app);
    motion(&mut app, background, true).unwrap();
    assert_eq!(rectangle(&mut app), span([5, 3], [7, 5]));
    assert_eq!(
        route(&mut app, &button(false, outside)).unwrap_err(),
        CaptureRefusal::Empty.to_string()
    );
    // A press off the view's tiles starts nothing.
    acknowledge_ui(&mut app);
    assert_eq!(
        route(&mut app, &button(true, background)).unwrap_err(),
        CAPTURE_OFF_VIEW
    );
    assert_eq!(app.tools.capture_gesture(), Some(CaptureGesture::Armed));

    // A missed release (motion without the button) copies where it ended,
    // and so does a press while the rectangle is dragged: the press only
    // ends it, on the last tile the drag reached.
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 0, 0);
    route(&mut app, &button(true, start)).unwrap();
    let far = cell_pixel(&mut app, 7, 5);
    motion(&mut app, far, true).unwrap();
    motion(&mut app, far, false).unwrap();
    assert_eq!(app.tools.copy_choice().unwrap().objects().len(), 4);
    assert_eq!(app.tools.capture_gesture(), None);
    press(&mut app, Keycode::C).unwrap();
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 3, 2);
    route(&mut app, &button(true, start)).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, far)).unwrap();
    assert_eq!(app.tools.copy_choice().unwrap().objects().len(), 1);
    assert_eq!(app.tools.capture_gesture(), None);
    // No copy ever wrote anything.
    assert!(app.session.history().pending().is_none());
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

/// A pixel of the map that shows no tile of the view.
fn map_background(app: &mut Application) -> [f32; 2] {
    app.refresh_ui();
    let frame = app.frame(true).unwrap();
    let width = usize::try_from(frame.width).unwrap();
    let cells: BTreeSet<_> = app.prepared.cell_picks.keys().map(|id| id.0).collect();
    let index = frame
        .semantic_ids
        .iter()
        .enumerate()
        .position(|(index, sample)| {
            let px = u32::try_from(index % width).unwrap();
            let py = u32::try_from(index / width).unwrap();
            !cells.contains(sample) && app.ui.hit(px, py) == Hit::World(px, py)
        })
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    [(index % width) as f32 + 0.5, (index / width) as f32 + 0.5]
}

/// The sample with 64 more crates over the view's first two rows, four to
/// a tile: with `gate_east`, 65 objects in (0, 0)-(7, 1).
fn with_crates(root: &Path) {
    with_stamps(root);
    edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
        let list = chunk["objects"].as_array_mut().unwrap();
        for index in 0..64_u16 {
            let tile = index % 16;
            list.push(serde_json::json!({
                "uid": format!("crate_{index}"), "at": [tile % 8, tile / 8],
                "proto": "crate", "layer": "item"
            }));
        }
    });
}

/// A 1280x800 application on `prepare`'s world, in Place mode.
fn place_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = zone_application(prepare);
    app.session
        .set_uid_source(Box::new(SeededUidSource::new(SEED)));
    press(&mut app, Keycode::P).unwrap();
    (temporary, app)
}

#[test]
fn too_many_objects_or_an_ungranted_prototype_are_said_before_the_release() {
    let (_temporary, mut app) = place_application(with_crates);
    press(&mut app, Keycode::C).unwrap();
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 0, 0);
    route(&mut app, &button(true, start)).unwrap();
    let end = cell_pixel(&mut app, 7, 1);
    motion(&mut app, end, true).unwrap();
    assert_eq!(
        app.status_label(),
        "Copy (0, 0) to (7, 1), 8 x 2 tiles: 65 objects, more than 64; release copies nothing."
    );
    let refused = route(&mut app, &button(false, end)).unwrap_err();
    assert_eq!(refused, CaptureRefusal::TooMany(65).to_string());
    assert!(app.tools.copy_choice().is_none());
    // Copy area stays armed and the earlier choice is as it was. The
    // window shows the refusal as a notice; the next rectangle's press
    // clears it.
    assert_eq!(app.tools.capture_gesture(), Some(CaptureGesture::Armed));
    assert_eq!(app.tools.placement_choice(), Some("bush"));
    app.notice = Some(refused);
    assert_eq!(app.status_label(), "Input not applied. Details below.");
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, start)).unwrap();
    assert!(app.notice.is_none());
    assert_eq!(
        app.status_label(),
        "Copy (0, 0) to (0, 0), 1 x 1 tiles: 4 objects; release to copy."
    );
    route(&mut app, &key(Keycode::Escape)).unwrap();
    press(&mut app, Keycode::C).unwrap();
    // Without gate_east's column: 56 crates copy.
    let (down, up) = drag(&mut app, [0, 0], &[[6, 1]]);
    down.unwrap();
    up.unwrap();
    assert_eq!(app.tools.copy_choice().unwrap().objects().len(), 56);

    // marker without its grant: every rectangle with a marker refuses.
    let (_temporary, mut app) = place_application(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            let objects = catalog["editing"]["objects"].as_array_mut().unwrap();
            objects.retain(|grant| grant["proto"] != "marker");
        });
    });
    press(&mut app, Keycode::C).unwrap();
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 0, 2);
    route(&mut app, &button(true, start)).unwrap();
    let end = cell_pixel(&mut app, 3, 4);
    motion(&mut app, end, true).unwrap();
    assert_eq!(
        app.status_label(),
        "Copy (0, 2) to (3, 4), 4 x 3 tiles: not granted: marker; release copies nothing."
    );
    assert_eq!(
        route(&mut app, &button(false, end)).unwrap_err(),
        "Nothing copied: Place cannot put down marker (not granted by editing.objects)."
    );
    assert!(app.tools.copy_choice().is_none());
    // gate_west alone copies.
    let (down, up) = drag(&mut app, [0, 3], &[]);
    down.unwrap();
    up.unwrap();
    assert_eq!(app.tools.copy_choice().unwrap().objects().len(), 1);
}

#[test]
#[allow(clippy::too_many_lines)]
fn c_and_esc_act_only_where_they_should() {
    let (_temporary, mut app) = stamp_application();
    // Outside Place mode C does nothing.
    press(&mut app, Keycode::T).unwrap();
    press(&mut app, Keycode::C).unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    assert!(app.control(Control::CopyArea).is_err());
    press(&mut app, Keycode::P).unwrap();
    // C toggles; Esc stops it, and so does a mode change.
    press(&mut app, Keycode::C).unwrap();
    press(&mut app, Keycode::C).unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    press(&mut app, Keycode::C).unwrap();
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    // A mode key during the drag ends it; the release then copies nothing.
    press(&mut app, Keycode::C).unwrap();
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 0, 2);
    route(&mut app, &button(true, start)).unwrap();
    press(&mut app, Keycode::T).unwrap();
    route(&mut app, &button(false, start)).unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    assert!(app.tools.copy_choice().is_none());
    press(&mut app, Keycode::P).unwrap();
    // Esc during the drag stops it too.
    press(&mut app, Keycode::C).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, start)).unwrap();
    route(&mut app, &key(Keycode::Escape)).unwrap();
    route(&mut app, &button(false, start)).unwrap();
    assert!(app.tools.copy_choice().is_none());

    copy(&mut app, [0, 2], [3, 4]);
    // R and M turn the copy as they turn a catalog stamp.
    press(&mut app, Keycode::R).unwrap();
    press(&mut app, Keycode::M).unwrap();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE.turned().flipped());
    // A pending change: Esc cancels it first, and the copy stays.
    click(&mut app, [4, 1]).unwrap();
    assert!(app.session.history().pending().is_some());
    // No rectangle starts while it is pending.
    press(&mut app, Keycode::C).unwrap();
    let (down, _) = drag(&mut app, [1, 1], &[]);
    assert_eq!(down.unwrap_err(), CAPTURE_PENDING);
    press(&mut app, Keycode::C).unwrap();
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());
    assert!(app.tools.copy_choice().is_some());
    // A non-empty filter is cleared first.
    press(&mut app, Keycode::Slash).unwrap();
    let mut input = crate::native_input::NativeInput::new(7);
    input.begin_batch(true, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(
        &mut app,
        &Event::TextInput {
            timestamp: 1,
            window_id: 7,
            text: "c".into(),
        },
        &mut input,
    )
    .unwrap()
    .unwrap();
    // The focused filter took C as text: Copy area is not armed.
    acknowledge_ui(&mut app);
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(&mut app, &input_key(7, Keycode::C, false), &mut input)
        .unwrap()
        .unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(app.tools.palette_filter(), "c");
    // Esc ends Copy area before it clears the filter.
    press(&mut app, Keycode::C).unwrap();
    press(&mut app, Keycode::Escape).unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    assert_eq!(app.tools.palette_filter(), "c");
    press(&mut app, Keycode::Escape).unwrap();
    assert_eq!(app.tools.palette_filter(), "");
    assert!(app.tools.copy_choice().is_some());
    // With nothing pending, Esc drops the copy: back to the prototype.
    press(&mut app, Keycode::Escape).unwrap();
    assert!(app.tools.copy_choice().is_none());
    assert!(capture_row(&mut app).is_empty());
    assert!(sidebar_texts(&mut app).contains(&"Placing: bush".to_owned()));
    // The Drop copy button does the same; a prototype row or a catalog
    // stamp replaces the copy.
    copy(&mut app, [0, 2], [3, 4]);
    press_widget(&mut app, &Control::DropCopy).unwrap();
    assert!(app.tools.copy_choice().is_none());
    copy(&mut app, [0, 2], [3, 4]);
    app.control(Control::PlacePalette("crate".into())).unwrap();
    assert!(app.tools.copy_choice().is_none());
    copy(&mut app, [0, 2], [3, 4]);
    app.control(Control::StampPalette("camp".into())).unwrap();
    assert!(app.tools.copy_choice().is_none());
    assert_eq!(app.tools.stamp_choice(), Some("camp"));
    // A copy replaces the catalog stamp.
    copy(&mut app, [0, 2], [3, 4]);
    assert_eq!(app.tools.stamp_choice(), None);

    // A text draft takes C as text.
    app.action(Action::Select {
        generation: app.session.generation(),
        key: SelectionKey::Object {
            uid: "marker_1".into(),
        },
    })
    .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    assert!(app.tools.draft_text().is_some());
    crate::prototype::native_key(&mut app, Keycode::C).unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    app.control(Control::CancelDraft).unwrap();
}

#[test]
fn a_portal_pick_step_and_copy_area_end_each_other() {
    let (_temporary, mut app) = stamp_application();
    app.action(Action::Select {
        generation: app.session.generation(),
        key: SelectionKey::Object {
            uid: "gate_west".into(),
        },
    })
    .unwrap();
    app.control(Control::PortalPick).unwrap();
    assert!(app.tools.portal_picking());
    // C ends the pick step, so the next press starts the rectangle.
    press(&mut app, Keycode::C).unwrap();
    assert!(!app.tools.portal_picking());
    // The pick step ends Copy area, so the next press picks.
    app.control(Control::PortalPick).unwrap();
    assert_eq!(app.tools.capture_gesture(), None);
    click(&mut app, [3, 2]).unwrap();
    let pending = app.session.history().pending().unwrap();
    assert!(!pending.sealed().contains("object.insert"));
    assert!(app.tools.copy_choice().is_none());
}

#[test]
fn the_road_fence_a_blocked_session_and_a_pause_refuse_copy_area() {
    for control in [Control::CopyArea, Control::DropCopy] {
        assert!(!crate::prototype::road_allows(&control), "{control:?}");
    }
    let (_temporary, mut app) = super::road::road_application(SEED, with_stamps);
    super::road::click(&mut app, [1, 1]).unwrap();
    assert!(app.tools.road().is_some());
    for control in [Control::CopyArea, Control::DropCopy] {
        assert_eq!(app.control(control).unwrap_err(), ROAD_IN_PROGRESS);
    }
    // C is Place mode's own: the road keeps it.
    super::road::press(&mut app, Keycode::C).unwrap();
    assert!(app.tools.road().is_some());

    // Paused: a press while armed is refused in the pause's words.
    let (temporary, mut app) = stamp_application();
    copy(&mut app, [0, 2], [3, 4]);
    press(&mut app, Keycode::C).unwrap();
    app.blocked = Some(crate::prototype::Blocked::new(
        crate::prototype::BlockCause::Present,
        "test pause",
    ));
    let (down, _) = drag(&mut app, [1, 1], &[]);
    assert_eq!(down.unwrap_err(), "rendering blocked editing: test pause");
    app.blocked = None;
    route(&mut app, &key(Keycode::Escape)).unwrap();

    // Blocked: Copy area is inert and its button disabled; Drop copy still
    // drops the session's own copy.
    let catalog = temporary.path().join("world/preview.jsonc");
    let original = fs::read(&catalog).unwrap();
    fs::write(&catalog, [original.as_slice(), b"\n"].concat()).unwrap();
    assert!(app.action(Action::Reopen).is_err());
    assert_eq!(app.session.status().phase, SessionPhase::Blocked);
    assert_eq!(app.control(Control::CopyArea).unwrap_err(), STAMP_BLOCKED);
    assert_eq!(
        capture_row(&mut app),
        [
            ("Copy area  C".to_owned(), false, false),
            ("Drop copy  Esc".to_owned(), true, false),
        ]
    );
    assert_eq!(app.tools.capture_gesture(), None);
}

#[test]
#[allow(clippy::too_many_lines)]
fn the_copy_rows_fit_at_1280_by_800_720_by_480_and_double_scale() {
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        let physical = (logical.0 * scale, logical.1 * scale);
        let (_temporary, mut app) = zone_application(with_stamps);
        app.ui = WorkspaceUi::new(logical, physical);
        app.view.camera.width = physical.0;
        app.view.camera.height = physical.1;
        app.rebuild().unwrap();
        app.control(Control::Place).unwrap();
        app.control(Control::CopyArea).unwrap();
        // The drag, at this size.
        acknowledge_ui(&mut app);
        let scaled = |app: &mut Application, at: [i64; 2]| {
            let [x, y] = cell_pixel(app, at[0], at[1]);
            #[allow(clippy::cast_precision_loss)]
            let factor = scale as f32;
            [x / factor, y / factor]
        };
        let route_sized = |app: &mut Application, event: &Event| {
            crate::prototype::native_input_event(app, event, logical, physical, false, false)
                .unwrap_or(Ok(()))
        };
        let start = scaled(&mut app, [0, 2]);
        route_sized(&mut app, &button(true, start)).unwrap();
        let end = scaled(&mut app, [3, 4]);
        route_sized(
            &mut app,
            &Event::MouseMotion {
                timestamp: 0,
                window_id: 7,
                which: 0,
                mousestate: sdl3::mouse::MouseState::from_sdl_state(1),
                x: end[0],
                y: end[1],
                xrel: 0.0,
                yrel: 0.0,
            },
        )
        .unwrap();
        let case = format!("{logical:?} x{scale}");
        assert_eq!(rectangle(&mut app), span([0, 2], [3, 4]), "{case}");
        route_sized(&mut app, &button(false, end)).unwrap();
        assert_eq!(
            app.tools.copy_choice().unwrap().objects().len(),
            3,
            "{case}"
        );
        app.control(Control::StampTurn).unwrap();
        app.control(Control::CopyArea).unwrap();
        app.refresh_ui();
        let sidebar = app.ui.sidebar;
        let inside = |bounds: crate::workspace_ui::Rect| {
            bounds.x >= sidebar.x
                && bounds.y >= sidebar.y
                && bounds.x + bounds.width <= sidebar.x + sidebar.width
                && bounds.y + bounds.height <= sidebar.y + sidebar.height
        };
        let pinned: Vec<_> = app
            .ui
            .widgets
            .iter()
            .filter(|widget| {
                matches!(
                    widget.control,
                    Some(
                        Control::StampTurn
                            | Control::StampMirror
                            | Control::CopyArea
                            | Control::DropCopy
                    )
                ) || widget.text.starts_with("Copied")
            })
            .map(|widget| (widget.text.clone(), widget.control.clone(), widget.bounds))
            .collect();
        assert_eq!(
            pinned
                .iter()
                .map(|(text, ..)| text.as_str())
                .collect::<Vec<_>>(),
            [
                "Copied 3 objects",
                "Turn: 90\u{b0}  R",
                "Mirror: off  M",
                "Copy area  C",
                "Drop copy  Esc"
            ],
            "{case}"
        );
        assert!(pinned.iter().all(|(.., bounds)| inside(*bounds)), "{case}");
        let (turn, copy_area, drop) = (pinned[1].2, pinned[3].2, pinned[4].2);
        assert_eq!(copy_area.y, drop.y, "{case}");
        assert!(copy_area.x + copy_area.width <= drop.x, "{case}");
        assert!(turn.y + turn.height <= copy_area.y, "{case}");
        // Hits land on their own control at this scale.
        for (_, control, bounds) in &pinned[1..] {
            assert_eq!(
                app.ui
                    .hit(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2),
                Hit::Control(control.clone().unwrap()),
                "{case}"
            );
        }
        // And the copy's hover preview composes at this size.
        app.control(Control::CopyArea).unwrap();
        acknowledge_ui(&mut app);
        hover_at(&mut app, [4, 1], logical, physical);
        assert_eq!(marked(&mut app), tiles([4, 1], &copied(1, false)), "{case}");
    }
}

#[test]
fn without_copy_area_place_mode_is_as_before() {
    let (_temporary, mut app) = zone_application(|_| {});
    press(&mut app, Keycode::P).unwrap();
    for scroll in 0..40 {
        app.sidebar_scroll = scroll;
        assert!(
            !sidebar_texts(&mut app)
                .iter()
                .any(|text| text.starts_with("Cop") || text.starts_with("Drop")),
            "scroll {scroll}"
        );
    }
    assert!(capture_row(&mut app).is_empty());
    assert_eq!(app.status_label(), "Ready. Click a tile to place bush.");
    acknowledge_ui(&mut app);
    hover(&mut app, [3, 2]);
    assert!(capture_overlay(&app).is_empty());
    // Esc with nothing to drop cancels as before: nothing is pending.
    route(&mut app, &key(Keycode::Escape)).unwrap_or_default();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.status_label(), "Ready. Click a tile to place bush.");
}
