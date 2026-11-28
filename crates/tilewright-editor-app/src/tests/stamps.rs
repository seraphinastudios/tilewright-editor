// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Place mode's catalog stamps (`editing.stamps`) through real SDL pointer
//! and key events, on the CPU scene path: the Stamps rows, R and M, the
//! hover preview, the one `place_objects` change a click prepares, its
//! refusals and the keys it must not take. Every world is a temporary copy
//! of the MIT neutral sample (view 8 x 6) with a stamps section; the
//! committed sample has none.
// Colours are copied, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::proposal::edit_json;
use super::zones::{button, key, route, sidebar_texts, zone_application};
use super::{acknowledge_ui, cell_pixel, input_key};
use crate::prototype::{Application, ROAD_IN_PROGRESS};
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::{event::Event, keyboard::Keycode};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::stamp::StampObject;
use tilewright_editor_ui::stamps::StampTurn;
use tilewright_editor_ui::tools::ToolMode;
use tilewright_editor_ui::uid::SeededUidSource;
use tilewright_editor_ui::{Action, SessionPhase};

const MAIN: &str = "world/planes/main/layers/0/0_0.jsonc";
const SEED: u64 = 7;
/// What the session says after R reopened it.
const REOPENED: &str = "Reopen verified committed source revision";

/// The catalog's `camp`, an asymmetric layout every turn and mirror moves
/// differently, as `(prototype, offset)`.
const CAMP: [(&str, [i64; 2]); 4] = [
    ("crate", [0, 0]),
    ("lamp", [1, 0]),
    ("bush", [0, 1]),
    ("stone", [2, 1]),
];

/// The sample with `editing.stamps`: `camp`, a three-marker `line` and a
/// one-object `post`, listed out of name order.
pub(super) fn with_stamps(root: &Path) {
    edit_json(root, "preview.jsonc", |catalog| {
        let camp: Vec<_> = CAMP
            .iter()
            .map(|(proto, offset)| serde_json::json!({"proto": proto, "offset": offset}))
            .collect();
        catalog["editing"]["stamps"] = serde_json::json!([
            {"name": "line", "objects": [
                {"proto": "marker", "offset": [-1, 0]},
                {"proto": "marker", "offset": [0, 0]},
                {"proto": "marker", "offset": [1, 0]}]},
            {"name": "camp", "objects": camp},
            {"name": "post", "objects": [{"proto": "lamp", "offset": [0, 0]}]},
        ]);
    });
}

/// The 1280x800 application on the sample with stamps, seeded, in Place
/// mode with the frame acknowledged.
pub(super) fn stamp_application() -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = zone_application(with_stamps);
    app.session
        .set_uid_source(Box::new(SeededUidSource::new(SEED)));
    press(&mut app, Keycode::P).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Place);
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// A real key press on a displayed frame.
pub(super) fn press(app: &mut Application, keycode: Keycode) -> Result<(), String> {
    acknowledge_ui(app);
    route(app, &key(keycode))
}

/// A real press and release on tile `at`, on a displayed frame.
pub(super) fn click(app: &mut Application, at: [i64; 2]) -> Result<(), String> {
    acknowledge_ui(app);
    let pixel = cell_pixel(app, at[0], at[1]);
    let down = route(app, &button(true, pixel));
    route(app, &button(false, pixel)).unwrap();
    down
}

/// A real press and release on the widget of `control`.
pub(super) fn press_widget(app: &mut Application, control: &Control) -> Result<(), String> {
    acknowledge_ui(app);
    let point = super::widget_point(app, control);
    let down = route(app, &button(true, point));
    route(app, &button(false, point)).unwrap();
    down
}

/// Pointer motion with no button held over tile `at`.
pub(super) fn hover(app: &mut Application, at: [i64; 2]) {
    let [x, y] = cell_pixel(app, at[0], at[1]);
    route(
        app,
        &Event::MouseMotion {
            timestamp: 0,
            window_id: 7,
            which: 0,
            mousestate: sdl3::mouse::MouseState::from_sdl_state(0),
            x,
            y,
            xrel: 0.0,
            yrel: 0.0,
        },
    )
    .unwrap();
}

/// The tiles the stamp preview marks, read back through the prepared cell
/// picks: every copy is unpickable and in the stroke preview's tint, and
/// the composed frame holds exactly these copies.
pub(super) fn marked(app: &mut Application) -> BTreeSet<[i64; 2]> {
    app.refresh_ui();
    let overlay = crate::prototype::stamp_overlay(app);
    let cells: BTreeSet<[i64; 2]> = overlay
        .iter()
        .map(|quad| {
            assert!(!quad.pickable);
            assert_eq!(quad.color, crate::workspace_render::STROKE_PREVIEW_TINT);
            match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a preview copy of no cell"),
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
    assert_eq!(
        cells.len(),
        overlay.len(),
        "one copy per tile on this flat fixture"
    );
    cells
}

/// `camp` as the stated convention moves it, computed here on its own: an
/// offset is mirrored (`[-dx, dy]`) when `mirrored`, then turned clockwise
/// (`[-dy, dx]`) `turns` times.
fn camp(turns: u8, mirrored: bool) -> Vec<StampObject> {
    CAMP.iter()
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

/// The stamp a click on `anchor` records, as a direct action.
pub(super) fn place_objects(
    app: &Application,
    anchor: [i64; 2],
    objects: Vec<StampObject>,
) -> Action {
    Action::PlaceObjects {
        plane: "main".into(),
        layer: 0,
        x: anchor[0],
        y: anchor[1],
        objects,
        expected: app.session.history().document().revision(),
    }
}

pub(super) fn tiles(anchor: [i64; 2], objects: &[StampObject]) -> BTreeSet<[i64; 2]> {
    objects
        .iter()
        .map(|object| [anchor[0] + object.offset[0], anchor[1] + object.offset[1]])
        .collect()
}

/// The recorded proposal of `app`'s applied steps.
pub(super) fn proposal(app: &Application) -> tilewright_editor_proposal::Proposal {
    tilewright_editor_proposal::Proposal::from_session(&app.session, "test-editor", [1; 32], None)
        .unwrap()
}

pub(super) fn turn_row(app: &mut Application) -> Vec<(String, bool, bool)> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .filter(|widget| {
            matches!(
                widget.control,
                Some(Control::StampTurn | Control::StampMirror)
            )
        })
        .map(|widget| (widget.text.clone(), widget.enabled, widget.active))
        .collect()
}

fn stamp_rows(app: &mut Application) -> Vec<(String, bool, bool)> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .filter(|widget| matches!(widget.control, Some(Control::StampPalette(_))))
        .map(|widget| (widget.text.clone(), widget.enabled, widget.active))
        .collect()
}

/// Scroll the sidebar until the Stamps rows are laid out.
fn scroll_to_stamps(app: &mut Application) {
    for scroll in 0..40 {
        app.sidebar_scroll = scroll;
        if stamp_rows(app).len() == 3 {
            return;
        }
    }
    panic!("the Stamps rows never came into view");
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_stamp_row_r_m_and_a_click_review_exactly_a_direct_place_objects() {
    let (temporary, mut app) = stamp_application();
    let (twin_temporary, mut twin) = stamp_application();
    let chunk = temporary.path().join(MAIN);
    let original = fs::read(&chunk).unwrap();
    // The Stamps rows follow the prototypes, in name order, with counts.
    scroll_to_stamps(&mut app);
    assert!(sidebar_texts(&mut app).contains(&"Stamps: R turns, M mirrors".to_owned()));
    assert_eq!(
        stamp_rows(&mut app),
        [
            ("4 objects: camp".to_owned(), true, false),
            ("3 objects: line".to_owned(), true, false),
            ("1 object: post".to_owned(), true, false),
        ]
    );
    // Without a stamp R and M neither turn nor mirror anything.
    press(&mut app, Keycode::M).unwrap();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
    assert!(turn_row(&mut app).is_empty());

    // A row click chooses the stamp; the pinned rows name it and its turn.
    press_widget(&mut app, &Control::StampPalette("camp".into())).unwrap();
    assert_eq!(app.tools.stamp_choice(), Some("camp"));
    assert!(app.session.history().pending().is_none());
    let texts = sidebar_texts(&mut app);
    assert!(
        texts.contains(&"Stamping 4 objects: camp".to_owned()),
        "{texts:?}"
    );
    assert_eq!(
        turn_row(&mut app),
        [
            ("Turn: 0\u{b0}  R".to_owned(), true, false),
            ("Mirror: off  M".to_owned(), true, false)
        ]
    );
    // The prototype rows are no longer marked: a click stamps.
    app.refresh_ui();
    assert!(
        app.ui
            .widgets
            .iter()
            .filter(|widget| matches!(widget.control, Some(Control::PlacePalette(_))))
            .all(|widget| !widget.active)
    );
    assert_eq!(
        app.status_label(),
        "Ready. Click a tile to stamp camp (4 objects); R turns, M mirrors."
    );

    // R four times and M twice come back to the catalog layout.
    for _ in 0..4 {
        press(&mut app, Keycode::R).unwrap();
    }
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
    for _ in 0..2 {
        press(&mut app, Keycode::M).unwrap();
    }
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
    // R, then M: the turned layout mirrored. The buttons do what the keys do.
    press(&mut app, Keycode::R).unwrap();
    assert_eq!(
        turn_row(&mut app)[0],
        ("Turn: 90\u{b0}  R".to_owned(), true, true)
    );
    press_widget(&mut app, &Control::StampMirror).unwrap();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE.turned().flipped());
    assert_eq!(
        turn_row(&mut app),
        [
            ("Turn: 270\u{b0}  R".to_owned(), true, true),
            ("Mirror: on  M".to_owned(), true, true)
        ]
    );
    let objects = camp(3, true);
    // The stated convention by hand: crate stays, lamp goes south, bush
    // east and stone two south and one east.
    assert_eq!(
        objects
            .iter()
            .map(|object| object.offset)
            .collect::<Vec<_>>(),
        [[0, 0], [0, 1], [1, 0], [1, 2]]
    );
    assert!(app.session.history().pending().is_none());
    assert_eq!(fs::read(&chunk).unwrap(), original);

    // The hover preview marks exactly the tiles a click records.
    let anchor = [3, 2];
    acknowledge_ui(&mut app);
    hover(&mut app, anchor);
    assert_eq!(marked(&mut app), tiles(anchor, &objects));
    assert_eq!(
        app.status_label(),
        "Stamp camp at (3, 2): 4 objects; click to review."
    );
    click(&mut app, anchor).unwrap();
    let pending = app.session.history().pending().unwrap();
    assert_eq!(
        pending.sealed().matches(r#""op":"object.insert""#).count(),
        4
    );
    let sealed = pending.sealed().to_owned();
    let review = app.diff_lines();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    // While it is pending, nothing previews and no click prepares.
    assert!(app.stamp_preview().is_none());

    // The twin dispatches the same stamp directly, with offsets computed
    // on their own: the same sealed transaction and the same review.
    let direct = place_objects(&twin, anchor, objects.clone());
    twin.action(direct).unwrap();
    assert_eq!(twin.session.history().pending().unwrap().sealed(), sealed);
    assert_eq!(twin.diff_lines(), review);
    // The engine's own warning: crate lands on marker_1 at (3, 2).
    assert!(
        review[0].starts_with("Warning: 1 of the 4 new objects lands on a tile"),
        "{review:?}"
    );

    for app in [&mut app, &mut twin] {
        acknowledge_ui(app);
        route(app, &key(Keycode::Return)).unwrap();
        assert!(app.session.history().pending().is_none());
        assert_eq!(app.recovery.records().len(), 1);
    }
    // The same proposal, step for step and byte for byte; its action is
    // the stamp with the turned and mirrored offsets.
    let (mine, theirs) = (proposal(&app), proposal(&twin));
    assert_eq!(mine.bytes(), theirs.bytes());
    let Action::PlaceObjects {
        x,
        y,
        objects: recorded,
        ..
    } = mine.steps()[0].action()
    else {
        panic!("the step is a stamp");
    };
    assert_eq!(([*x, *y], recorded), (anchor, &objects));
    assert_eq!(mine.steps()[0].placed_objects().len(), 4);
    assert_eq!(
        fs::read(&chunk).unwrap(),
        fs::read(twin_temporary.path().join(MAIN)).unwrap()
    );
    // Undo restores the source exactly.
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

#[test]
fn every_turn_and_mirror_previews_exactly_the_tiles_a_click_records() {
    let (_temporary, mut app) = stamp_application();
    app.control(Control::StampPalette("camp".into())).unwrap();
    // Every state the keys reach, by a short key sequence, with the state's
    // own arithmetic.
    let sequences: [(&[Keycode], u8, bool); 8] = [
        (&[], 0, false),
        (&[Keycode::R], 1, false),
        (&[Keycode::R, Keycode::R], 2, false),
        (&[Keycode::R, Keycode::R, Keycode::R], 3, false),
        (&[Keycode::M], 0, true),
        // M after one R mirrors the turned layout: three turns of the
        // mirrored one.
        (&[Keycode::R, Keycode::M], 3, true),
        (&[Keycode::M, Keycode::R], 1, true),
        (&[Keycode::R, Keycode::R, Keycode::M], 2, true),
    ];
    let mut seen = BTreeSet::new();
    for (keys, turns, mirrored) in sequences {
        // Back to the catalog layout: choosing another stamp resets it.
        app.control(Control::StampPalette("post".into())).unwrap();
        app.control(Control::StampPalette("camp".into())).unwrap();
        for &keycode in keys {
            press(&mut app, keycode).unwrap();
        }
        let turn = app.tools.stamp_turn();
        assert_eq!(
            (turn.quarter_turns(), turn.mirrored()),
            (turns, mirrored),
            "{keys:?}"
        );
        let objects = camp(turns, mirrored);
        seen.insert(tiles([0, 0], &objects));
        for anchor in [[3, 2], [2, 3], [4, 3]] {
            acknowledge_ui(&mut app);
            hover(&mut app, anchor);
            let preview = app.stamp_preview().unwrap();
            assert_eq!(preview.outside, 0, "{keys:?} at {anchor:?}");
            assert_eq!(marked(&mut app), tiles(anchor, &objects), "{keys:?}");
            // The preview's action is what the click prepares.
            assert_eq!(preview.action, place_objects(&app, anchor, objects.clone()));
            let cell = SelectionKey::Cell {
                plane: "main".into(),
                layer: 0,
                grid: "floor".into(),
                x: anchor[0],
                y: anchor[1],
            };
            let action = app
                .tools
                .place_action(
                    app.session.catalog(),
                    &cell,
                    app.session.history().document().revision(),
                    false,
                )
                .unwrap();
            assert_eq!(action, preview.action);
        }
    }
    // Eight distinct layouts.
    assert_eq!(seen.len(), 8);
}

/// The sample with stamps, drawn isometric (`+x` down-right, `+y`
/// down-left).
fn with_isometric_stamps(root: &Path) {
    with_stamps(root);
    edit_json(root, "profile/profile.jsonc", |profile| {
        profile["projection"] =
            serde_json::json!({"kind": "isometric", "tile_width": 64, "tile_height": 32});
    });
}

/// The centre on screen of every tile the stamp preview marks, from the
/// corners of its copies of the exact cell quads.
fn marked_centres(app: &mut Application) -> std::collections::BTreeMap<[i64; 2], [f32; 2]> {
    app.refresh_ui();
    let mut corners = std::collections::BTreeMap::<[i64; 2], Vec<[f32; 2]>>::new();
    for quad in crate::prototype::stamp_overlay(app) {
        let Some(SelectionKey::Cell { x, y, .. }) = app.prepared.cell_picks.get(&quad.semantic_id)
        else {
            panic!("a preview copy of no cell");
        };
        corners.entry([*x, *y]).or_default().extend(quad.positions);
    }
    corners
        .into_iter()
        .map(|(tile, points)| {
            #[allow(clippy::cast_precision_loss)]
            let count = points.len() as f32;
            let sum = points
                .iter()
                .fold([0.0, 0.0], |[x, y], [px, py]| [x + px, y + py]);
            (tile, [sum[0] / count, sum[1] / count])
        })
        .collect()
}

/// The marked tiles' centres relative to the anchor's, in `objects` order.
fn relative_centres(
    app: &mut Application,
    anchor: [i64; 2],
    objects: &[StampObject],
) -> Vec<[f32; 2]> {
    let centres = marked_centres(app);
    let origin = centres[&anchor];
    objects
        .iter()
        .map(|object| {
            let centre = centres[&[anchor[0] + object.offset[0], anchor[1] + object.offset[1]]];
            [centre[0] - origin[0], centre[1] - origin[1]]
        })
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)]
fn isometric_m_flips_the_shown_stamp_left_to_right_and_a_click_records_it() {
    let (_temporary, mut app) = zone_application(with_isometric_stamps);
    app.session
        .set_uid_source(Box::new(SeededUidSource::new(SEED)));
    press(&mut app, Keycode::P).unwrap();
    assert!(matches!(
        app.session.world().profile().projection,
        tilewright_source::Projection::Isometric { .. }
    ));
    app.control(Control::StampPalette("camp".into())).unwrap();
    let anchor = [3, 2];
    // Key sequences, with the shown offsets computed here on their own: R
    // is `[-dy, dx]` and M, left to right on screen, `[dy, dx]`, each on the
    // layout as shown.
    let sequences: [&[Keycode]; 10] = [
        &[],
        &[Keycode::M],
        &[Keycode::M, Keycode::M],
        &[Keycode::R],
        &[Keycode::R, Keycode::R],
        &[Keycode::R, Keycode::M],
        &[Keycode::M, Keycode::R],
        &[Keycode::R, Keycode::M, Keycode::R],
        &[Keycode::M, Keycode::R, Keycode::M],
        &[Keycode::R, Keycode::R, Keycode::M],
    ];
    let mut seen = BTreeSet::new();
    for keys in sequences {
        app.control(Control::StampPalette("post".into())).unwrap();
        app.control(Control::StampPalette("camp".into())).unwrap();
        let mut objects: Vec<StampObject> = CAMP
            .iter()
            .map(|(proto, offset)| StampObject {
                proto: (*proto).into(),
                offset: *offset,
            })
            .collect();
        // The buttons read as the top-down keys would: the same turn and
        // mirror for the same keys.
        let mut top_down = StampTurn::NONE;
        for &keycode in keys {
            press(&mut app, keycode).unwrap();
            for object in &mut objects {
                let [dx, dy] = object.offset;
                object.offset = if keycode == Keycode::R {
                    [-dy, dx]
                } else {
                    [dy, dx]
                };
            }
            top_down = if keycode == Keycode::R {
                top_down.turned()
            } else {
                top_down.flipped()
            };
        }
        let degrees = top_down.degrees();
        let mirror = if top_down.mirrored() {
            "Mirror: on  M"
        } else {
            "Mirror: off  M"
        };
        assert_eq!(
            turn_row(&mut app),
            [
                (format!("Turn: {degrees}\u{b0}  R"), true, degrees != 0),
                (mirror.to_owned(), true, top_down.mirrored())
            ],
            "{keys:?}"
        );
        seen.insert(tiles([0, 0], &objects));
        acknowledge_ui(&mut app);
        hover(&mut app, anchor);
        let preview = app.stamp_preview().unwrap();
        assert_eq!(preview.outside, 0, "{keys:?}");
        assert_eq!(marked(&mut app), tiles(anchor, &objects), "{keys:?}");
        assert_eq!(preview.action, place_objects(&app, anchor, objects.clone()));

        // On screen, one more M mirrors every marked tile across the
        // anchor's vertical: across negated, down kept.
        let before = relative_centres(&mut app, anchor, &objects);
        press(&mut app, Keycode::M).unwrap();
        let flipped: Vec<StampObject> = objects
            .iter()
            .map(|object| StampObject {
                proto: object.proto.clone(),
                offset: [object.offset[1], object.offset[0]],
            })
            .collect();
        acknowledge_ui(&mut app);
        hover(&mut app, anchor);
        let after = relative_centres(&mut app, anchor, &flipped);
        for ([bx, by], [ax, ay]) in before.iter().zip(&after) {
            assert!(
                (ax + bx).abs() < 0.01 && (ay - by).abs() < 0.01,
                "{keys:?}: {before:?} -> {after:?}"
            );
        }
        // At least one tile moved sideways, so the check has teeth.
        assert!(before.iter().any(|[x, _]| x.abs() > 1.0));
    }
    // M M is the catalog layout again and R M R is M (as M R M is three
    // R), so eight distinct layouts.
    assert_eq!(seen.len(), 8);

    // A copy made with C follows the same rule: the sample's `marker_1`,
    // `gate_west` and `marker_2` copied from (0, 2) to (3, 4), then M or
    // R M, previewed and clicked on tiles outside that rectangle, so the
    // second copy copies the same objects.
    let captured = [("marker", [3, 0]), ("gate", [0, 1]), ("marker", [1, 2])];
    let mut steps = 0;
    for (keys, at, offsets) in [
        // M swaps: [0, 3], [1, 0], [2, 1].
        (&[Keycode::M][..], [4, 1], [[0, 3], [1, 0], [2, 1]]),
        // R turns to [0, 3], [-1, 0], [-2, 1]; M then swaps.
        (
            &[Keycode::R, Keycode::M][..],
            [4, 3],
            [[3, 0], [0, -1], [1, -2]],
        ),
    ] {
        super::capture::copy(&mut app, [0, 2], [3, 4]);
        let as_copied: Vec<StampObject> = captured
            .iter()
            .map(|(proto, offset)| StampObject {
                proto: (*proto).into(),
                offset: *offset,
            })
            .collect();
        assert_eq!(
            app.tools.copy_choice().unwrap().objects(),
            as_copied.as_slice()
        );
        for &keycode in keys {
            press(&mut app, keycode).unwrap();
        }
        let expected: Vec<StampObject> = captured
            .iter()
            .zip(offsets)
            .map(|((proto, _), offset)| StampObject {
                proto: (*proto).into(),
                offset,
            })
            .collect();
        acknowledge_ui(&mut app);
        hover(&mut app, at);
        let preview = app.stamp_preview().unwrap();
        assert_eq!(preview.outside, 0, "{keys:?}");
        assert_eq!(marked(&mut app), tiles(at, &expected), "{keys:?}");
        assert_eq!(preview.action, place_objects(&app, at, expected.clone()));
        click(&mut app, at).unwrap();
        acknowledge_ui(&mut app);
        route(&mut app, &key(Keycode::Return)).unwrap();
        assert!(app.session.history().pending().is_none());
        steps += 1;
        let recorded = proposal(&app);
        assert_eq!(recorded.steps().len(), steps);
        let Action::PlaceObjects { x, y, objects, .. } = recorded.steps()[steps - 1].action()
        else {
            panic!("the step is a stamp");
        };
        assert_eq!(([*x, *y], objects), (at, &expected), "{keys:?}");
    }

    // R then M, clicked: the recorded step is the previewed offsets.
    app.control(Control::StampPalette("post".into())).unwrap();
    app.control(Control::StampPalette("camp".into())).unwrap();
    press(&mut app, Keycode::R).unwrap();
    press(&mut app, Keycode::M).unwrap();
    acknowledge_ui(&mut app);
    hover(&mut app, anchor);
    let preview = app.stamp_preview().unwrap();
    let Action::PlaceObjects {
        objects: previewed, ..
    } = preview.action.clone()
    else {
        panic!("the preview is a stamp");
    };
    // Turned to [0, 0], [0, 1], [-1, 0], [-1, 2], then swapped.
    assert_eq!(
        previewed
            .iter()
            .map(|object| object.offset)
            .collect::<Vec<_>>(),
        [[0, 0], [1, 0], [0, -1], [2, -1]]
    );
    assert_eq!(marked(&mut app), tiles(anchor, &previewed));
    click(&mut app, anchor).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert!(app.session.history().pending().is_none());
    let recorded = proposal(&app);
    let Action::PlaceObjects { x, y, objects, .. } = recorded.steps()[steps].action() else {
        panic!("the step is a stamp");
    };
    assert_eq!(([*x, *y], objects), (anchor, &previewed));
}

#[test]
fn a_stamp_reaching_past_the_view_is_refused_whole_in_the_engines_words() {
    let (temporary, mut app) = stamp_application();
    let (_twin_temporary, mut twin) = stamp_application();
    let chunk = temporary.path().join(MAIN);
    let original = fs::read(&chunk).unwrap();
    app.control(Control::StampPalette("camp".into())).unwrap();
    // From the view's last tile only the crate is inside the view.
    let corner = [7, 5];
    acknowledge_ui(&mut app);
    hover(&mut app, corner);
    assert_eq!(marked(&mut app), BTreeSet::from([corner]));
    assert_eq!(
        app.status_label(),
        "Stamp camp at (7, 5): 3 of 4 objects outside the view."
    );
    let refused = click(&mut app, corner).unwrap_err();
    // The engine's refusal, as a direct dispatch gives it.
    let direct = twin
        .action(place_objects(&twin, corner, camp(0, false)))
        .unwrap_err();
    assert_eq!(refused, direct);
    assert!(refused.contains("outside"), "{refused}");
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.status().phase, SessionPhase::Ready);
    assert_eq!(fs::read(&chunk).unwrap(), original);
    // Turned half way it reaches back into the view and is prepared.
    press(&mut app, Keycode::R).unwrap();
    press(&mut app, Keycode::R).unwrap();
    acknowledge_ui(&mut app);
    hover(&mut app, corner);
    assert_eq!(marked(&mut app), tiles(corner, &camp(2, false)));
    click(&mut app, corner).unwrap();
    assert!(app.session.history().pending().is_some());
    // Every view edge refuses whatever leaves it.
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());
    for (anchor, turns) in [([0, 0], 2), ([0, 5], 1), ([7, 0], 3), ([6, 4], 0)] {
        let refused = click_turned(&mut app, anchor, turns).unwrap_err();
        assert!(refused.contains("outside"), "{anchor:?}: {refused}");
        assert!(app.session.history().pending().is_none());
    }
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

/// Choose `camp` afresh, turn it `turns` times with R and click `anchor`.
fn click_turned(app: &mut Application, anchor: [i64; 2], turns: u8) -> Result<(), String> {
    app.control(Control::StampPalette("post".into())).unwrap();
    app.control(Control::StampPalette("camp".into())).unwrap();
    for _ in 0..turns {
        press(app, Keycode::R).unwrap();
    }
    click(app, anchor)
}

#[test]
fn while_a_change_is_pending_or_editing_is_paused_the_place_rules_hold() {
    let (temporary, mut app) = stamp_application();
    let chunk = temporary.path().join(MAIN);
    app.control(Control::StampPalette("line".into())).unwrap();
    click(&mut app, [5, 1]).unwrap();
    let sealed = app.session.history().pending().unwrap().sealed().to_owned();
    // Pending: no preview, a click prepares nothing, the rows are disabled
    // and the placing row dimmed, as for a prototype.
    acknowledge_ui(&mut app);
    hover(&mut app, [3, 3]);
    assert!(app.stamp_preview().is_none());
    assert!(marked(&mut app).is_empty());
    let refused = click(&mut app, [3, 3]).unwrap_err();
    assert!(refused.contains("pending"), "{refused}");
    assert_eq!(app.session.history().pending().unwrap().sealed(), sealed);
    scroll_to_stamps(&mut app);
    assert!(stamp_rows(&mut app).iter().all(|(_, enabled, _)| !enabled));
    app.refresh_ui();
    let placing = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.text.starts_with("Stamping 3 objects: line"))
        .unwrap();
    assert!(!placing.enabled);
    // R and M touch no World: they still turn the layout, and the pending
    // change stays exactly as it was.
    press(&mut app, Keycode::R).unwrap();
    press(&mut app, Keycode::M).unwrap();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE.turned().flipped());
    assert!(turn_row(&mut app).iter().all(|(_, enabled, _)| *enabled));
    assert_eq!(app.session.history().pending().unwrap().sealed(), sealed);
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());

    // Paused: no preview, and a click is refused with the pause's words.
    let original = fs::read(&chunk).unwrap();
    app.blocked = Some(crate::prototype::Blocked::new(
        crate::prototype::BlockCause::Present,
        "test pause",
    ));
    acknowledge_ui(&mut app);
    hover(&mut app, [3, 3]);
    assert!(app.stamp_preview().is_none());
    let refused = click(&mut app, [3, 3]).unwrap_err();
    assert_eq!(refused, "rendering blocked editing: test pause");
    assert!(app.session.history().pending().is_none());
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

#[test]
fn r_and_m_turn_and_mirror_only_a_chosen_stamp_in_place_mode() {
    let (_temporary, mut app) = stamp_application();
    // With a prototype chosen R is still Reopen: the session reopens and
    // nothing turns.
    assert!(app.tools.stamp_choice().is_none());
    press(&mut app, Keycode::R).unwrap();
    assert_eq!(app.session.status().message, REOPENED);
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);

    app.control(Control::StampPalette("camp".into())).unwrap();
    // Choosing a prototype ends the stamp.
    app.control(Control::PlacePalette("crate".into())).unwrap();
    assert!(app.tools.stamp_choice().is_none());
    assert_eq!(app.tools.placement_choice(), Some("crate"));
    app.control(Control::StampPalette("camp".into())).unwrap();
    press(&mut app, Keycode::R).unwrap();
    // The same stamp chosen again keeps its turn.
    app.control(Control::StampPalette("camp".into())).unwrap();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE.turned());

    // The focused filter takes R and M as text.
    let mut input = crate::native_input::NativeInput::new(7);
    press(&mut app, Keycode::Slash).unwrap();
    assert!(app.tools.filter_focused());
    acknowledge_ui(&mut app);
    for keycode in [Keycode::R, Keycode::M] {
        input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
        crate::prototype::native_text_event(&mut app, &input_key(7, keycode, false), &mut input)
            .unwrap()
            .unwrap();
    }
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE.turned());
    // The filter keeps the stamps whose name matches, by the same rule as
    // the prototypes.
    input.begin_batch(true, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(
        &mut app,
        &Event::TextInput {
            timestamp: 1,
            window_id: 7,
            text: "IN".into(),
        },
        &mut input,
    )
    .unwrap()
    .unwrap();
    scroll_stamps_into_view(&mut app);
    assert_eq!(
        stamp_rows(&mut app),
        [("3 objects: line".to_owned(), true, false)]
    );
    assert!(sidebar_texts(&mut app).contains(&"Stamps: 1 of 3 match".to_owned()));
    // The chosen stamp stays chosen while the filter hides it.
    assert_eq!(app.tools.stamp_choice(), Some("camp"));
    acknowledge_ui(&mut app);
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(
        &mut app,
        &input_key(7, Keycode::Escape, false),
        &mut input,
    )
    .unwrap_or_else(|| crate::prototype::native_key(&mut app, Keycode::Escape))
    .unwrap();
    assert!(!app.tools.filter_focused());

    // Outside Place mode R reopens and M does nothing; the stamp and its
    // turn stay chosen for the next visit.
    press(&mut app, Keycode::T).unwrap();
    press(&mut app, Keycode::R).unwrap();
    press(&mut app, Keycode::M).unwrap();
    assert_eq!(app.session.status().message, REOPENED);
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE.turned());
    assert_eq!(app.tools.stamp_choice(), Some("camp"));
    assert!(app.control(Control::StampTurn).is_err());
    press(&mut app, Keycode::P).unwrap();
    assert_eq!(app.tools.stamp_choice(), Some("camp"));
}

/// Scroll the sidebar until a Stamps row or header is laid out.
fn scroll_stamps_into_view(app: &mut Application) {
    for scroll in 0..40 {
        app.sidebar_scroll = scroll;
        if sidebar_texts(app)
            .iter()
            .any(|text| text.starts_with("Stamps"))
        {
            return;
        }
    }
    panic!("no Stamps rows");
}

#[test]
fn the_road_fence_refuses_every_stamp_control() {
    for control in [
        Control::StampPalette("camp".into()),
        Control::StampTurn,
        Control::StampMirror,
    ] {
        assert!(!crate::prototype::road_allows(&control), "{control:?}");
    }
    let (_temporary, mut app) = super::road::road_application(SEED, with_stamps);
    super::road::click(&mut app, [1, 1]).unwrap();
    assert!(app.tools.road().is_some());
    for control in [
        Control::StampPalette("camp".into()),
        Control::StampTurn,
        Control::StampMirror,
    ] {
        assert_eq!(app.control(control).unwrap_err(), ROAD_IN_PROGRESS);
    }
    assert!(app.tools.stamp_choice().is_none());
    // R is refused while the road is drawn, and M keeps it.
    assert_eq!(
        super::road::press(&mut app, Keycode::R).unwrap_err(),
        ROAD_IN_PROGRESS
    );
    super::road::press(&mut app, Keycode::M).unwrap();
    assert!(app.tools.road().is_some());
}

#[test]
#[allow(clippy::too_many_lines)]
fn the_stamp_rows_fit_at_1280_by_800_720_by_480_and_double_scale() {
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        let physical = (logical.0 * scale, logical.1 * scale);
        let (_temporary, mut app) = zone_application(with_stamps);
        app.ui = WorkspaceUi::new(logical, physical);
        app.view.camera.width = physical.0;
        app.view.camera.height = physical.1;
        app.rebuild().unwrap();
        app.control(Control::Place).unwrap();
        app.control(Control::StampPalette("camp".into())).unwrap();
        for _ in 0..3 {
            app.control(Control::StampTurn).unwrap();
        }
        app.control(Control::StampMirror).unwrap();
        let case = format!("{logical:?} x{scale}");
        app.refresh_ui();
        let sidebar = app.ui.sidebar;
        let inside = |bounds: crate::workspace_ui::Rect| {
            bounds.x >= sidebar.x
                && bounds.y >= sidebar.y
                && bounds.x + bounds.width <= sidebar.x + sidebar.width
                && bounds.y + bounds.height <= sidebar.y + sidebar.height
        };
        // The pinned rows: whole, inside the sidebar, one beside the other.
        let pinned: Vec<_> = app
            .ui
            .widgets
            .iter()
            .filter(|widget| {
                matches!(
                    widget.control,
                    Some(Control::StampTurn | Control::StampMirror)
                ) || widget.text.starts_with("Stamping")
            })
            .map(|widget| (widget.text.clone(), widget.bounds))
            .collect();
        assert_eq!(
            pinned
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            [
                "Stamping 4 objects: camp",
                "Turn: 90\u{b0}  R",
                "Mirror: on  M"
            ],
            "{case}"
        );
        assert!(pinned.iter().all(|(_, bounds)| inside(*bounds)), "{case}");
        let (turn, mirror) = (pinned[1].1, pinned[2].1);
        assert_eq!(turn.y, mirror.y, "{case}");
        assert!(turn.x + turn.width <= mirror.x, "{case}");
        assert!(pinned[0].1.y + pinned[0].1.height <= turn.y, "{case}");
        // Hits land on their own control at this scale.
        for (control, bounds) in [(Control::StampTurn, turn), (Control::StampMirror, mirror)] {
            assert_eq!(
                app.ui
                    .hit(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2),
                crate::workspace_ui::Hit::Control(control),
                "{case}"
            );
        }
        // The stamp rows scroll into view whole and untruncated.
        scroll_to_stamps(&mut app);
        let rows: Vec<_> = app
            .ui
            .widgets
            .iter()
            .filter(|widget| matches!(widget.control, Some(Control::StampPalette(_))))
            .map(|widget| (widget.text.clone(), widget.bounds))
            .collect();
        assert_eq!(
            rows.iter()
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>(),
            ["4 objects: camp", "3 objects: line", "1 object: post"],
            "{case}"
        );
        assert!(rows.iter().all(|(_, bounds)| inside(*bounds)), "{case}");
        // And the frame composes with the hover preview at this size.
        acknowledge_ui(&mut app);
        hover_at(&mut app, [3, 2], logical, physical);
        // Three turns, then a mirror: one turn of the mirrored layout.
        assert_eq!(marked(&mut app), tiles([3, 2], &camp(1, true)), "{case}");
    }
}

/// Pointer motion over tile `at` in a window of `logical` size drawn at
/// `physical` pixels.
pub(super) fn hover_at(
    app: &mut Application,
    at: [i64; 2],
    logical: (u32, u32),
    physical: (u32, u32),
) {
    let [x, y] = cell_pixel(app, at[0], at[1]);
    // `cell_pixel` gives a physical pixel; SDL reports logical positions.
    #[allow(clippy::cast_precision_loss)]
    let scale = physical.0 as f32 / logical.0 as f32;
    crate::prototype::native_input_event(
        app,
        &Event::MouseMotion {
            timestamp: 0,
            window_id: 7,
            which: 0,
            mousestate: sdl3::mouse::MouseState::from_sdl_state(0),
            x: x / scale,
            y: y / scale,
            xrel: 0.0,
            yrel: 0.0,
        },
        logical,
        physical,
        false,
        false,
    )
    .unwrap()
    .unwrap();
}

#[test]
fn without_a_stamps_section_place_mode_is_as_before() {
    let (_temporary, mut app) = zone_application(|_| {});
    press(&mut app, Keycode::P).unwrap();
    assert!(app.session.catalog().editing().stamps.is_empty());
    for scroll in 0..40 {
        app.sidebar_scroll = scroll;
        assert!(
            !sidebar_texts(&mut app)
                .iter()
                .any(|text| text.starts_with("Stamp")),
            "scroll {scroll}"
        );
    }
    assert!(turn_row(&mut app).is_empty());
    assert!(app.control(Control::StampPalette("camp".into())).is_err());
    assert!(app.control(Control::StampTurn).is_err());
    acknowledge_ui(&mut app);
    let masks = app.hover.masks;
    hover(&mut app, [3, 2]);
    assert!(app.stamp_preview().is_none());
    assert_eq!(
        app.hover.masks, masks,
        "no hover mask is built for a prototype"
    );
    press(&mut app, Keycode::R).unwrap();
    assert_eq!(app.session.status().message, REOPENED);
    assert_eq!(app.status_label(), "Ready. Click a tile to place bush.");
}

#[test]
fn in_a_blocked_session_r_stays_reopen_with_a_stamp_chosen() {
    let (temporary, mut app) = stamp_application();
    app.control(Control::StampPalette("camp".into())).unwrap();
    // The catalog changes on disk, and Reopen blocks the session: only
    // Reopen ends that.
    let catalog = temporary.path().join("world/preview.jsonc");
    let original = fs::read(&catalog).unwrap();
    fs::write(&catalog, [original.as_slice(), b"\n"].concat()).unwrap();
    assert!(app.action(Action::Reopen).is_err());
    assert_eq!(app.session.status().phase, SessionPhase::Blocked);
    let message = app.session.status().message.clone();
    // R reopens again (and is refused again); nothing turns.
    let refused = press(&mut app, Keycode::R).unwrap_err();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
    assert_eq!(app.session.status().phase, SessionPhase::Blocked);
    assert!(
        refused.contains("catalog") || refused.contains("eopen"),
        "{refused} / {message}"
    );
    // Nothing can be placed there, so M does nothing either, and the
    // Turn and Mirror buttons are disabled and refuse.
    press(&mut app, Keycode::M).unwrap();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
    let row = turn_row(&mut app);
    assert_eq!(row.len(), 2);
    assert!(row.iter().all(|(_, enabled, _)| !enabled), "{row:?}");
    for control in [Control::StampTurn, Control::StampMirror] {
        // A press on a disabled button is a panel press.
        press_widget(&mut app, &control).unwrap();
        assert_eq!(
            app.control(control).unwrap_err(),
            crate::prototype::STAMP_BLOCKED
        );
    }
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
}

#[test]
fn an_open_portal_pick_step_shows_no_stamp_preview_and_the_click_picks() {
    let (_temporary, mut app) = stamp_application();
    app.control(Control::StampPalette("camp".into())).unwrap();
    // The gate's portal tile pick, open in Place mode.
    app.action(Action::Select {
        generation: app.session.generation(),
        key: SelectionKey::Object {
            uid: "gate_west".into(),
        },
    })
    .unwrap();
    app.control(Control::PortalPick).unwrap();
    assert!(app.tools.portal_picking());
    assert_eq!(app.tools.mode(), ToolMode::Place);
    acknowledge_ui(&mut app);
    hover(&mut app, [3, 2]);
    assert!(app.stamp_preview().is_none());
    assert!(marked(&mut app).is_empty());
    let status = app.status_label();
    assert!(
        !status.contains("stamp") && !status.contains("Stamp"),
        "{status}"
    );
    // The click picks the portal's target: no object is inserted.
    click(&mut app, [3, 2]).unwrap();
    let pending = app.session.history().pending().unwrap();
    assert!(!pending.sealed().contains("object.insert"));
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());
    // With the step over the stamp previews again.
    assert!(!app.tools.portal_picking());
    acknowledge_ui(&mut app);
    hover(&mut app, [3, 3]);
    assert!(app.stamp_preview().is_some());
}

#[test]
fn a_text_draft_takes_r_and_m_as_text_with_a_stamp_chosen() {
    let (_temporary, mut app) = stamp_application();
    app.control(Control::StampPalette("camp".into())).unwrap();
    app.action(Action::Select {
        generation: app.session.generation(),
        key: SelectionKey::Object {
            uid: "marker_1".into(),
        },
    })
    .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    assert!(app.tools.draft_text().is_some());
    assert_eq!(app.tools.mode(), ToolMode::Place);
    acknowledge_ui(&mut app);
    let mut input = crate::native_input::NativeInput::new(7);
    for keycode in [Keycode::R, Keycode::M] {
        input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
        crate::prototype::native_text_event(&mut app, &input_key(7, keycode, false), &mut input)
            .unwrap()
            .unwrap();
    }
    // The key itself never reaches the world keys, even when routed there.
    crate::prototype::native_key(&mut app, Keycode::R).unwrap();
    crate::prototype::native_key(&mut app, Keycode::M).unwrap();
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
    assert_ne!(app.session.status().message, REOPENED);
    // The text arrives as text.
    let before = app.tools.draft_text().unwrap().to_owned();
    input.begin_batch(true, app.tools.input_revision().map(|r| r.draft));
    crate::prototype::native_text_event(
        &mut app,
        &Event::TextInput {
            timestamp: 1,
            window_id: 7,
            text: "rm".into(),
        },
        &mut input,
    )
    .unwrap()
    .unwrap();
    assert_eq!(app.tools.draft_text().unwrap(), format!("{before}rm"));
    assert_eq!(app.tools.stamp_turn(), StampTurn::NONE);
    assert!(app.session.history().pending().is_none());
}

/// A proposal whose one step paints `[3, 2]`, `[4, 2]` and `[5, 2]`, open in
/// the person's application; both worlds have the stamps.
fn stamp_proposal() -> super::proposal::Twin {
    use super::proposal::{AGENT, RECEIPT, Twin, accept, open, open_session, person_application};
    let agent_root = tempfile::tempdir_in(super::proposal::scratch()).unwrap();
    let world = agent_root.path().join("world");
    super::copy_private(
        &tilewright_editor_testkit::fixture_world("prototype-neutral"),
        &world,
    );
    with_stamps(&world);
    let mut session = open_session(&world, AGENT);
    let expected = session.history().document().revision();
    accept(
        &mut session,
        Action::PaintStroke {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            cells: vec![[3, 2], [4, 2], [5, 2]],
            proto: "sand".into(),
            expected,
        },
    );
    let proposal = tilewright_editor_proposal::Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
    let (person, app) = person_application(with_stamps);
    let mut twin = Twin {
        person,
        agent: agent_root,
        app,
        proposal,
    };
    open(&mut twin);
    twin
}

/// The tiles the proposal highlight draws now, read back through the
/// prepared cell picks.
fn highlighted(app: &Application) -> BTreeSet<[i64; 2]> {
    crate::prototype::proposal_overlay(app)
        .iter()
        .map(
            |quad| match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a highlight copy of no cell"),
            },
        )
        .collect()
}

#[test]
fn the_stamp_preview_wins_over_the_proposal_highlight_on_a_shared_tile() {
    let mut twin = stamp_proposal();
    let app = &mut twin.app;
    let all = BTreeSet::from([[3, 2], [4, 2], [5, 2]]);
    assert_eq!(highlighted(app), all);
    press(app, Keycode::P).unwrap();
    app.control(Control::StampPalette("post".into())).unwrap();
    // One lamp on (4, 2): its tile is left to the preview.
    acknowledge_ui(app);
    hover(app, [4, 2]);
    assert_eq!(marked(app), BTreeSet::from([[4, 2]]));
    assert_eq!(highlighted(app), BTreeSet::from([[3, 2], [5, 2]]));
    // The line covers all three: the highlight draws none of them.
    app.control(Control::StampPalette("line".into())).unwrap();
    acknowledge_ui(app);
    hover(app, [4, 3]);
    hover(app, [4, 2]);
    assert_eq!(marked(app), all);
    assert!(highlighted(app).is_empty());
    // Off the map again, the highlight takes its tiles back.
    app.forget_hover();
    assert_eq!(highlighted(app), all);
}
