// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Terrain mode's Road rule through real SDL pointer and key events, on the
//! CPU scene path: clicked points, the live corridor preview, Enter into
//! one ordinary paint or scatter change for Review, Backspace and Esc, the
//! keys a road keeps, ends or refuses, and the refusals. Every world is a
//! temporary copy of the MIT neutral sample.
// Colours are copied, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::proposal::edit_json;
use super::sculpt_mode::wide_isometric;
use super::zones::{button, key, route, sidebar_texts, zone_application};
use super::{acknowledge_ui, cell_pixel, terrain_value};
use crate::prototype::Application;
use crate::prototype::ROAD_IN_PROGRESS;
use crate::workspace_ui::Control;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::brush::{Brush, BrushShape};
use tilewright_editor_ui::tools::{TerrainRule, ToolMode};
use tilewright_editor_ui::uid::{SeededUidSource, UidSource};

const MAIN: &str = "world/planes/main/layers/0/0_0.jsonc";

pub(super) fn with_speckle(root: &Path) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["palettes"] = serde_json::json!([{"name": "speckle", "grid": "floor",
            "entries": [{"value": "grass", "weight": 3}, {"value": "sand", "weight": 1}]}]);
    });
}

/// The 1280x800 application on the sample changed by `prepare`, in Terrain
/// mode with sand chosen and the Road rule reached with F, its random
/// source seeded with `seed`.
pub(super) fn road_application(
    seed: u64,
    prepare: impl FnOnce(&Path),
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = zone_application(prepare);
    app.session
        .set_uid_source(Box::new(SeededUidSource::new(seed)));
    route(&mut app, &key(Keycode::T)).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    for _ in 0..3 {
        route(&mut app, &key(Keycode::F)).unwrap();
    }
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Road);
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// A real press and release on tile `at`, on a displayed frame.
pub(super) fn click(app: &mut Application, at: [i64; 2]) -> Result<(), String> {
    acknowledge_ui(app);
    let pixel = cell_pixel(app, at[0], at[1]);
    let down = route(app, &button(true, pixel));
    route(app, &button(false, pixel)).unwrap();
    down
}

/// A real key press on a displayed frame.
pub(super) fn press(app: &mut Application, keycode: Keycode) -> Result<(), String> {
    acknowledge_ui(app);
    route(app, &key(keycode))
}

pub(super) fn points(app: &Application) -> Vec<[i64; 2]> {
    app.tools
        .road()
        .map(|road| road.points().to_vec())
        .unwrap_or_default()
}

/// The cells the road preview marks, read back through the cell picks;
/// each copy is unpickable and in the stroke preview's tint.
fn marked(app: &mut Application) -> BTreeSet<[i64; 2]> {
    app.refresh_ui();
    crate::prototype::stroke_overlay(app)
        .iter()
        .map(|quad| {
            assert!(!quad.pickable);
            assert_eq!(quad.color, crate::workspace_render::STROKE_PREVIEW_TINT);
            match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a preview copy of no cell"),
            }
        })
        .collect()
}

/// The tiles the point marks sit on.
fn point_marks(app: &mut Application) -> usize {
    app.refresh_ui();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    presentation
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.color == crate::workspace_render::ROAD_POINT_MARK)
        .count()
}

fn view_values(app: &Application) -> Vec<Option<String>> {
    (0..6)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .map(|(x, y)| terrain_value(app, x, y))
        .collect()
}

fn changed(before: &[Option<String>], after: &[Option<String>]) -> BTreeSet<[i64; 2]> {
    before
        .iter()
        .zip(after)
        .enumerate()
        .filter(|(_, (b, a))| b != a)
        .map(|(index, _)| {
            let index = i64::try_from(index).unwrap();
            [index % 8, index / 8]
        })
        .collect()
}

/// A small fixed pseudo-random sequence, so every run checks the same lines.
pub(super) struct Lines(pub(super) u64);
impl Lines {
    pub(super) fn next(&mut self, below: i64) -> i64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        i64::try_from((self.0 >> 33) % u64::try_from(below).unwrap()).unwrap()
    }
}

/// For random roads, top-down and isometric, painting sand and scattering
/// speckle: the marked cells are exactly the engine's plan for the road's
/// stroke, the cells the pending change changes and the cells Accept
/// writes; the action's cells are those a drag through the same tiles in
/// the same order covers.
#[allow(clippy::too_many_lines)]
fn previews_what_enter_prepares(prepare: fn(&Path)) {
    let (temporary, mut app) = road_application(9, |root| {
        with_speckle(root);
        prepare(root);
    });
    let mut lines = Lines(0x5eed);
    let mut reviewed = [0; 2];
    for case in 0..12 {
        let scatter = case % 2 == 1;
        if scatter {
            app.control(Control::ScatterPalette("speckle".into()))
                .unwrap();
        } else {
            app.control(Control::Palette {
                grid: "floor".into(),
                prototype: "sand".into(),
            })
            .unwrap();
        }
        app.tools.set_brush(Brush {
            shape: if lines.next(2) == 0 {
                BrushShape::Square
            } else {
                BrushShape::Circle
            },
            radius: u16::try_from(lines.next(3)).unwrap(),
        });
        let count = 2 + lines.next(3);
        let mut clicked = Vec::new();
        for _ in 0..count {
            let at = [lines.next(8), lines.next(6)];
            click(&mut app, at).unwrap();
            if clicked.last() != Some(&at) {
                clicked.push(at);
            }
        }
        if clicked.len() < 2 {
            press(&mut app, Keycode::Escape).unwrap();
            continue;
        }
        assert_eq!(points(&app), clicked, "case {case}");
        let preview = marked(&mut app);
        let action = app
            .tools
            .road_action(
                app.session.catalog(),
                app.session.history().document().revision(),
                app.view.frame_revision,
                false,
            )
            .unwrap();
        let (cells, seed) = match &action {
            Action::PaintStroke { cells, proto, .. } => {
                assert!(!scatter && proto == "sand");
                (cells.clone(), None)
            }
            Action::ScatterTerrain {
                cells,
                palette,
                seed,
                ..
            } => {
                assert!(scatter && palette == "speckle");
                (cells.clone(), Some(*seed))
            }
            other => panic!("not a paint or scatter: {other:?}"),
        };
        // A drag pressed at the first tile through the others covers the
        // same cells: the corridor is the stroke code's.
        let mut drag = tilewright_editor_ui::tools::ToolState::default();
        drag.set_mode(ToolMode::Terrain);
        drag.set_brush(app.tools.brush());
        drag.choose_terrain(app.session.catalog(), "floor", "sand")
            .unwrap();
        let cell = |[x, y]: [i64; 2]| SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x,
            y,
        };
        let first = cell(clicked[0]);
        let inspected = tilewright_editor_ui::inspector::inspect(
            app.session.world(),
            app.session.catalog(),
            Some(&first),
            app.session.generation(),
            app.session.history().document().revision(),
        );
        drag.begin_stroke(&inspected, &first, false).unwrap();
        for &at in &clicked[1..] {
            drag.extend_stroke(&cell(at), app.session.generation())
                .unwrap();
        }
        let dragged: Vec<[i64; 2]> = drag.stroke().unwrap().cells().collect();
        assert_eq!(cells, dragged, "case {case}");
        let before = view_values(&app);
        let label = app.status_label();
        if preview.is_empty() {
            // Nothing to change: Enter prepares nothing and the road stays.
            assert!(label.contains("Enter prepares nothing"), "{label}");
            assert!(press(&mut app, Keycode::Return).is_err());
            assert_eq!(points(&app), clicked);
            press(&mut app, Keycode::Escape).unwrap();
            continue;
        }
        assert!(
            label.contains(&format!(
                "{} points, {} cells, {} to change; Enter to review",
                clicked.len(),
                cells.len(),
                preview.len()
            )),
            "{label}"
        );
        press(&mut app, Keycode::Return).unwrap();
        reviewed[usize::from(scatter)] += 1;
        assert!(app.tools.road().is_none());
        assert!(app.session.history().pending().is_some());
        assert_eq!(changed(&before, &view_values(&app)), preview, "case {case}");
        if let Some(seed) = seed {
            let palette = &app.session.catalog().editing().palettes["speckle"];
            for &[x, y] in &preview {
                let drawn = tilewright_editor_ui::scatter::value(palette, seed, 0, [x, y]);
                assert_eq!(terrain_value(&app, x, y).as_deref(), drawn);
            }
        }
        let pending = view_values(&app);
        press(&mut app, Keycode::Return).unwrap();
        assert!(app.session.history().pending().is_none());
        assert_eq!(view_values(&app), pending, "Accept writes the review");
        assert!(
            !fs::read_to_string(temporary.path().join(MAIN))
                .unwrap()
                .is_empty()
        );
        // Undo and accept: one history step puts the road back.
        press(&mut app, Keycode::Z).unwrap();
        press(&mut app, Keycode::Return).unwrap();
        assert_eq!(view_values(&app), before, "case {case}");
    }
    assert!(reviewed.iter().all(|&count| count >= 3), "{reviewed:?}");
}

#[test]
fn a_top_down_road_previews_exactly_what_enter_prepares_and_accept_writes() {
    previews_what_enter_prepares(|_| {});
}

#[test]
fn an_isometric_road_previews_exactly_what_enter_prepares_and_accept_writes() {
    previews_what_enter_prepares(wide_isometric);
}

#[test]
fn f_reaches_the_road_rule_and_its_rows_say_so() {
    let (_temporary, mut app) = road_application(1, |_| {});
    let texts = sidebar_texts(&mut app);
    assert!(texts.contains(&"Rule: Road  F".to_owned()), "{texts:?}");
    let replace = texts
        .iter()
        .position(|text| text.starts_with("Replace: click"))
        .unwrap();
    assert_eq!(texts[replace + 1], "Road: click points, Enter reviews");
    // The brush rows are live: width and shape are the road's.
    app.refresh_ui();
    assert!(
        app.ui
            .widgets
            .iter()
            .any(|widget| { widget.control == Some(Control::BrushSize(1)) && widget.enabled })
    );
    assert_eq!(
        app.status_label(),
        "Ready. Click tiles to draw a road; Enter reviews it."
    );
    route(&mut app, &key(Keycode::F)).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    assert!(sidebar_texts(&mut app).contains(&"Rule: Brush  F".to_owned()));
}

#[test]
fn one_enter_reviews_the_road_and_only_a_second_one_accepts_it() {
    let (temporary, mut app) = road_application(1, |_| {});
    let original = fs::read(temporary.path().join(MAIN)).unwrap();
    click(&mut app, [0, 0]).unwrap();
    click(&mut app, [3, 0]).unwrap();
    let label = app.status_label();
    assert_eq!(
        label,
        "Road of sand: 2 points, 4 cells, 4 to change; Enter to review, Backspace removes a point, Esc drops it."
    );
    assert_eq!(point_marks(&mut app), 2);
    press(&mut app, Keycode::Return).unwrap();
    assert!(app.tools.road().is_none());
    assert!(app.session.history().pending().is_some());
    assert_eq!(app.recovery.records().len(), 0);
    assert_eq!(fs::read(temporary.path().join(MAIN)).unwrap(), original);
    // The same key again, before the review is shown, accepts nothing.
    assert!(route(&mut app, &key(Keycode::Return)).is_err());
    assert!(app.session.history().pending().is_some());
    // On the displayed review it accepts, as for every edit.
    press(&mut app, Keycode::Return).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.recovery.records().len(), 1);
    assert_ne!(fs::read(temporary.path().join(MAIN)).unwrap(), original);
    for x in 0..4 {
        assert_eq!(terrain_value(&app, x, 0).as_deref(), Some("sand"));
    }
    assert_eq!(point_marks(&mut app), 0);
}

#[test]
fn backspace_takes_one_point_and_esc_drops_the_road_and_does_nothing_else() {
    let (_temporary, mut app) = road_application(1, |_| {});
    click(&mut app, [0, 0]).unwrap();
    // A click on the last point adds none.
    click(&mut app, [0, 0]).unwrap();
    assert_eq!(points(&app), [[0, 0]]);
    let label = app.status_label();
    assert!(
        label.starts_with("Road of sand: 1 point, 1 cell, 1 to change; click the next point"),
        "{label}"
    );
    // One point is not a road: Enter prepares nothing and keeps it.
    let refused = press(&mut app, Keycode::Return).unwrap_err();
    assert!(refused.contains("A road needs two points"), "{refused}");
    assert_eq!(points(&app), [[0, 0]]);
    assert!(app.session.history().pending().is_none());
    click(&mut app, [2, 1]).unwrap();
    click(&mut app, [2, 4]).unwrap();
    press(&mut app, Keycode::Backspace).unwrap();
    assert_eq!(points(&app), [[0, 0], [2, 1]]);
    press(&mut app, Keycode::Backspace).unwrap();
    press(&mut app, Keycode::Backspace).unwrap();
    assert!(app.tools.road().is_none());
    // With no road, Backspace does nothing at all.
    press(&mut app, Keycode::Backspace).unwrap();
    click(&mut app, [0, 0]).unwrap();
    click(&mut app, [3, 3]).unwrap();
    press(&mut app, Keycode::Escape).unwrap();
    assert!(app.tools.road().is_none());
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.tools.mode(), ToolMode::Terrain);
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Road);
    // The status line says so; the input did apply.
    assert!(app.notice.is_none());
    assert_eq!(app.status_label(), "Road dropped; nothing was prepared.");
    // The next road point clears those words.
    click(&mut app, [5, 5]).unwrap();
    assert!(app.status_label().starts_with("Road of sand: 1 point"));
    press(&mut app, Keycode::Escape).unwrap();
    // A pending change stays when Esc drops a road: while a change is
    // pending a click adds no point, so the road was drawn before.
    click(&mut app, [0, 0]).unwrap();
    click(&mut app, [1, 0]).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert!(app.session.history().pending().is_some());
    let refused = click(&mut app, [5, 5]).unwrap_err();
    assert_eq!(refused, "Accept or cancel the pending edit first");
    assert!(app.tools.road().is_none());
    press(&mut app, Keycode::Escape).unwrap();
    assert!(app.session.history().pending().is_none());
}

#[test]
fn the_brush_overlays_and_view_keys_keep_the_road_and_mode_or_rule_end_it() {
    let (_temporary, mut app) = road_application(1, with_speckle);
    click(&mut app, [0, 0]).unwrap();
    click(&mut app, [3, 3]).unwrap();
    let narrow = marked(&mut app);
    // Width and shape change the corridor; the points stay.
    press(&mut app, Keycode::RightBracket).unwrap();
    let wide = marked(&mut app);
    assert!(wide.len() > narrow.len() && narrow.is_subset(&wide));
    press(&mut app, Keycode::B).unwrap();
    assert_eq!(app.tools.brush().shape, BrushShape::Circle);
    press(&mut app, Keycode::LeftBracket).unwrap();
    assert_eq!(marked(&mut app), narrow);
    // Overlays, pans and zooms keep the points: they are tiles.
    for keycode in [
        Keycode::H,
        Keycode::O,
        Keycode::D,
        Keycode::A,
        Keycode::Equals,
        Keycode::Minus,
    ] {
        press(&mut app, keycode).unwrap();
        assert_eq!(points(&app), [[0, 0], [3, 3]], "{keycode:?}");
    }
    assert_eq!(marked(&mut app), narrow);
    // Keys that would edit or review otherwise wait for the road.
    for keycode in [Keycode::Z, Keycode::Y, Keycode::R, Keycode::Delete] {
        let refused = press(&mut app, keycode).unwrap_err();
        assert_eq!(refused, ROAD_IN_PROGRESS);
        assert_eq!(points(&app), [[0, 0], [3, 3]]);
        assert!(app.session.history().pending().is_none());
    }
    // A terrain or palette of the same grid keeps the road: a palette gets
    // its one seed now.
    let before = app.session.history().depths();
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    let seed = app.tools.road().unwrap().seed().unwrap();
    assert_eq!(points(&app), [[0, 0], [3, 3]]);
    assert!(
        app.status_label().starts_with(&format!(
            "Road from palette speckle (seed {seed}): 2 points"
        )),
        "{}",
        app.status_label()
    );
    assert_eq!(app.session.history().depths(), before);
    // T, and the Terrain button, in Terrain mode change nothing the road
    // depends on: it stays.
    press(&mut app, Keycode::T).unwrap();
    app.control(Control::Terrain).unwrap();
    assert_eq!(points(&app), [[0, 0], [3, 3]]);
    assert!(app.status_label().starts_with("Road from palette"));
    // F changes the rule: the road ends, and the status line says so (the
    // input applied, so not "Input not applied").
    press(&mut app, Keycode::F).unwrap();
    assert!(app.tools.road().is_none());
    assert!(app.notice.is_none());
    assert_eq!(
        app.status_label(),
        "Road dropped: the rule changed; nothing was prepared."
    );
    for keycode in [Keycode::F, Keycode::F, Keycode::F] {
        press(&mut app, keycode).unwrap();
    }
    // So does a key or button of another mode.
    for keycode in [Keycode::P, Keycode::N, Keycode::K] {
        if app.tools.mode() != ToolMode::Terrain {
            press(&mut app, Keycode::T).unwrap();
        }
        click(&mut app, [0, 0]).unwrap();
        press(&mut app, keycode).unwrap();
        assert!(app.tools.road().is_none(), "{keycode:?}");
        assert_eq!(
            app.status_label(),
            "Road dropped: the mode changed; nothing was prepared.",
            "{keycode:?}"
        );
    }
    press(&mut app, Keycode::T).unwrap();
    click(&mut app, [0, 0]).unwrap();
    app.control(Control::Select).unwrap();
    assert!(app.tools.road().is_none());
    // A later point clears the words.
    press(&mut app, Keycode::T).unwrap();
    click(&mut app, [0, 0]).unwrap();
    assert!(app.status_label().starts_with("Road from palette"));
}

#[test]
fn a_scatter_road_draws_its_seed_once_at_its_first_point() {
    let (_temporary, mut app) = road_application(21, with_speckle);
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    let mut source = SeededUidSource::new(21);
    let mut next = || {
        let mut bytes = [0; 16];
        source.fill(&mut bytes).unwrap();
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    };
    click(&mut app, [0, 0]).unwrap();
    let seed = app.tools.road().unwrap().seed().unwrap();
    assert_eq!(seed, next());
    click(&mut app, [3, 5]).unwrap();
    click(&mut app, [0, 5]).unwrap();
    assert_eq!(app.tools.road().unwrap().seed(), Some(seed));
    let preview = marked(&mut app);
    assert!(!preview.is_empty());
    press(&mut app, Keycode::Return).unwrap();
    assert!(app.session.history().pending().is_some());
    // The review writes the values of the seed the preview showed.
    let palette = &app.session.catalog().editing().palettes["speckle"];
    for &[x, y] in &preview {
        assert_eq!(
            terrain_value(&app, x, y).as_deref(),
            tilewright_editor_ui::scatter::value(palette, seed, 0, [x, y])
        );
    }
    // The next road draws the next seed: finishing drew none.
    press(&mut app, Keycode::Escape).unwrap();
    assert!(app.session.history().pending().is_none());
    click(&mut app, [1, 1]).unwrap();
    assert_eq!(app.tools.road().unwrap().seed(), Some(next()));
    // A terrain road draws none.
    press(&mut app, Keycode::Escape).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    click(&mut app, [1, 1]).unwrap();
    assert_eq!(app.tools.road().unwrap().seed(), None);
    press(&mut app, Keycode::Escape).unwrap();
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    click(&mut app, [1, 1]).unwrap();
    assert_eq!(app.tools.road().unwrap().seed(), Some(next()));
}

#[test]
fn a_road_is_refused_as_a_brush_is_and_never_while_editing_is_paused() {
    let (temporary, mut app) = road_application(1, |_| {});
    // Off the tiles: nothing added, and it says so.
    acknowledge_ui(&mut app);
    let off = off_tile_pixel(&mut app);
    let refused = route(&mut app, &button(true, off)).unwrap_err();
    assert!(
        refused.starts_with("Road points are tiles of the view"),
        "{refused}"
    );
    assert!(app.tools.road().is_none());
    // Nothing to change: the road stays so it can be extended.
    click(&mut app, [4, 0]).unwrap();
    click(&mut app, [7, 0]).unwrap();
    assert!(
        app.status_label()
            .ends_with("Enter prepares nothing: brush stroke changes no cell: every covered cell already has this terrain"),
        "{}",
        app.status_label()
    );
    assert!(marked(&mut app).is_empty());
    assert_eq!(point_marks(&mut app), 2);
    let refused = press(&mut app, Keycode::Return).unwrap_err();
    assert!(refused.contains("changes no cell"), "{refused}");
    assert_eq!(points(&app), [[4, 0], [7, 0]]);
    click(&mut app, [3, 0]).unwrap();
    assert_eq!(marked(&mut app), BTreeSet::from([[3, 0]]));
    // Paused: a click adds no point, and Enter and Backspace change
    // nothing, in the pause's words; the evidence says the pause; Esc still
    // drops the road.
    app.blocked = Some(crate::prototype::Blocked::new(
        crate::prototype::BlockCause::Present,
        "test pause",
    ));
    let refused = click(&mut app, [3, 3]).unwrap_err();
    assert_eq!(refused, "rendering blocked editing: test pause");
    assert_eq!(points(&app).len(), 3);
    let refused = press(&mut app, Keycode::Return).unwrap_err();
    assert_eq!(refused, "rendering blocked editing: test pause");
    assert!(app.session.history().pending().is_none());
    let refused = press(&mut app, Keycode::Backspace).unwrap_err();
    assert_eq!(refused, "rendering blocked editing: test pause");
    assert_eq!(points(&app).len(), 3);
    app.capture("paused").unwrap();
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(temporary.path().join("evidence/paused.json")).unwrap())
            .unwrap();
    assert_eq!(
        record["road"]["refused"],
        "rendering blocked editing: test pause"
    );
    assert!(record["road"].get("changes").is_none());
    press(&mut app, Keycode::Escape).unwrap();
    assert!(app.tools.road().is_none());
    app.blocked = None;
    // With no terrain or palette chosen: the brush's refusal.
    let (_plain, mut plain) = zone_application(|_| {});
    route(&mut plain, &key(Keycode::T)).unwrap();
    plain
        .control(Control::TerrainRule(TerrainRule::Road))
        .unwrap();
    let refused = click(&mut plain, [0, 0]).unwrap_err();
    assert_eq!(refused, "Choose an explicit terrain palette value");
    assert!(plain.tools.road().is_none());
}

/// A world pixel outside every panel that no tile covers.
pub(super) fn off_tile_pixel(app: &mut Application) -> [f32; 2] {
    use crate::workspace_ui::Hit;
    app.refresh_ui();
    let frame = app.frame(true).unwrap();
    let width = usize::try_from(frame.width).unwrap();
    let index = frame
        .semantic_ids
        .iter()
        .enumerate()
        .position(|(index, sample)| {
            let px = u32::try_from(index % width).unwrap();
            let py = u32::try_from(index / width).unwrap();
            !app.prepared
                .cell_picks
                .contains_key(&tilewright_scene::SemanticId(*sample))
                && app.ui.hit(px, py) == Hit::World(px, py)
        })
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    [(index % width) as f32 + 0.5, (index / width) as f32 + 0.5]
}

/// The Road rule's application with `gate_east`, whose portal the sample
/// lets the person edit, selected before Terrain mode: its inspector rows
/// stay laid out in Terrain mode.
fn gate_application(shared: bool) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = road_application(1, |_| {});
    if shared {
        app.share_string_field().unwrap();
    }
    app.select_object("gate_east").unwrap();
    route(&mut app, &key(Keycode::T)).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Road);
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// Whether the laid-out widget for `control`, if any, is enabled.
pub(super) fn enabled(app: &mut Application, control: &Control) -> Option<bool> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .find(|widget| widget.control.as_ref() == Some(control))
        .map(|widget| widget.enabled)
}

#[test]
fn nothing_else_is_prepared_or_opened_while_a_road_is_drawn() {
    for shared in [false, true] {
        let (_temporary, mut app) = gate_application(shared);
        let edits = [
            Control::PortalClear,
            Control::PortalAnchors,
            Control::PortalAnchor {
                plane: "main".into(),
                anchor: "lookout".into(),
            },
            Control::PortalPick,
            Control::PortalLocation,
            Control::Coordinates,
            Control::Field("label".into()),
            Control::Nudge([1, 0]),
            Control::Undo,
            Control::Redo,
            Control::Delete,
            Control::ApplyDraft,
            Control::ProposalPrepare,
            Control::GradeRoad(tilewright_editor_ui::road::RoadGrade::Flatten),
            Control::GradeRoad(tilewright_editor_ui::road::RoadGrade::Smooth),
        ];
        // Without a road the portal rows are live.
        assert_eq!(enabled(&mut app, &Control::PortalClear), Some(true));
        click(&mut app, [0, 0]).unwrap();
        click(&mut app, [3, 0]).unwrap();
        // The rows of these are laid out for the selected gate; they are
        // disabled while the road is drawn. The others have no row here.
        for control in [
            Control::PortalClear,
            Control::PortalAnchors,
            Control::PortalPick,
            Control::PortalLocation,
            Control::Coordinates,
            Control::Undo,
            Control::Redo,
            Control::Delete,
        ] {
            assert_eq!(enabled(&mut app, &control), Some(false), "{control:?}");
        }
        for control in &edits {
            let refused = app.control(control.clone()).unwrap_err();
            assert_eq!(refused, ROAD_IN_PROGRESS, "{control:?}");
            assert_eq!(points(&app), [[0, 0], [3, 0]], "{control:?}");
            assert!(app.session.history().pending().is_none(), "{control:?}");
            assert!(app.tools.draft_text().is_none(), "{control:?}");
            assert!(!app.tools.portal_picking(), "{control:?}");
        }
        // A press on a disabled row does nothing at all.
        for control in [Control::PortalClear, Control::Undo] {
            app.refresh_ui();
            let widget = app
                .ui
                .widgets
                .iter()
                .find(|widget| widget.control.as_ref() == Some(&control))
                .cloned()
                .unwrap();
            #[allow(clippy::cast_precision_loss)]
            let at = [
                (widget.bounds.x + widget.bounds.width / 2) as f32,
                (widget.bounds.y + widget.bounds.height / 2) as f32,
            ];
            acknowledge_ui(&mut app);
            route(&mut app, &button(true, at)).unwrap();
            assert_eq!(points(&app), [[0, 0], [3, 0]]);
            assert!(app.session.history().pending().is_none());
        }
        // The road's own controls stay live; Enter still finishes it.
        assert_eq!(enabled(&mut app, &Control::BrushSize(1)), Some(true));
        assert!(app.status_label().starts_with("Road of sand: 2 points"));
        press(&mut app, Keycode::Return).unwrap();
        assert!(app.session.history().pending().is_some());
        assert!(app.tools.road().is_none());
    }
}

#[test]
fn the_first_road_point_discards_a_half_typed_string_draft() {
    for shared in [false, true] {
        let (_temporary, mut app) = road_application(1, |_| {});
        if shared {
            app.share_string_field().unwrap();
        }
        app.select_visible_object(crate::prototype::Capability::Field)
            .unwrap();
        route(&mut app, &key(Keycode::T)).unwrap();
        let field = Control::Field("label".into());
        assert_eq!(enabled(&mut app, &field), Some(true));
        app.control(field.clone()).unwrap();
        app.tools.input("XYZ").unwrap();
        assert!(app.tools.draft_text().unwrap().ends_with("XYZ"));
        click(&mut app, [0, 0]).unwrap();
        assert!(app.tools.draft_text().is_none(), "shared {shared}");
        assert!(app.session.history().pending().is_none());
        assert_eq!(points(&app), [[0, 0]]);
        assert_eq!(enabled(&mut app, &field), Some(false));
        assert_eq!(app.control(field).unwrap_err(), ROAD_IN_PROGRESS);
        assert!(app.tools.draft_text().is_none());
    }
}

#[test]
fn the_admitted_controls_leave_a_road_alone() {
    let (_temporary, mut app) = gate_application(false);
    click(&mut app, [0, 0]).unwrap();
    click(&mut app, [3, 0]).unwrap();
    for control in [
        Control::EditDraft,
        Control::CancelDraft,
        Control::Accept,
        Control::Cancel,
        Control::Clear,
    ] {
        acknowledge_ui(&mut app);
        let _ = app.control(control.clone());
        assert_eq!(points(&app), [[0, 0], [3, 0]], "{control:?}");
        assert!(app.session.history().pending().is_none(), "{control:?}");
        assert!(app.tools.draft_text().is_none(), "{control:?}");
        assert!(!app.tools.portal_picking(), "{control:?}");
    }
    assert!(app.status_label().starts_with("Road of sand: 2 points"));
}

#[test]
fn a_terrain_of_another_grid_drops_the_road_and_says_the_grid() {
    let (_temporary, mut app) = road_application(1, |root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["terrain"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"grid": "edge", "values": ["grass_edge_1"]}));
        });
    });
    click(&mut app, [0, 0]).unwrap();
    click(&mut app, [3, 0]).unwrap();
    app.control(Control::Palette {
        grid: "edge".into(),
        prototype: "grass_edge_1".into(),
    })
    .unwrap();
    assert!(app.tools.road().is_none());
    assert_eq!(
        app.status_label(),
        "Road dropped: the grid changed; nothing was prepared."
    );
}

#[test]
fn a_road_and_a_draft_or_a_pick_step_never_coexist() {
    let (_temporary, mut app) = gate_application(false);
    // An open draft ends at the first road point, so it can never be
    // applied beside a road (nor a change be pending with one).
    app.control(Control::Coordinates).unwrap();
    assert!(app.tools.draft_text().is_some());
    click(&mut app, [0, 0]).unwrap();
    assert!(app.tools.draft_text().is_none());
    assert_eq!(points(&app), [[0, 0]]);
    let event = Event::TextInput {
        timestamp: 0,
        window_id: 7,
        text: "x".into(),
    };
    route(&mut app, &event).unwrap();
    assert_eq!(points(&app), [[0, 0]]);
    press(&mut app, Keycode::Escape).unwrap();
    // An open pick step takes the next tile click, as it always has: no
    // road begins. Esc ends the pick step first, then clicks draw a road.
    app.control(Control::PortalPick).unwrap();
    assert!(app.tools.portal_picking());
    press(&mut app, Keycode::Escape).unwrap();
    assert!(!app.tools.portal_picking());
    assert!(app.session.history().pending().is_none());
    click(&mut app, [1, 0]).unwrap();
    assert_eq!(points(&app), [[1, 0]]);
    press(&mut app, Keycode::Escape).unwrap();
    app.control(Control::PortalPick).unwrap();
    click(&mut app, [6, 4]).unwrap();
    assert!(app.tools.road().is_none());
    assert!(app.session.history().pending().is_some());
    // While that change is pending a click adds no point.
    assert!(click(&mut app, [0, 0]).is_err());
    assert!(app.tools.road().is_none());
}

#[test]
fn the_257th_point_is_refused() {
    let (_temporary, mut app) = road_application(1, |_| {});
    let cell = |x: i64| SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y: 0,
    };
    for index in 0..256 {
        assert!(
            app.tools
                .add_road_point(app.session.catalog(), &cell(index % 2), false)
                .unwrap()
        );
    }
    assert_eq!(points(&app).len(), 256);
    let refused = click(&mut app, [2, 0]).unwrap_err();
    assert_eq!(refused, "A road has at most 256 points");
    // The last point again adds nothing, silently.
    click(&mut app, [1, 0]).unwrap();
    assert_eq!(points(&app).len(), 256);
}

#[test]
fn a_frame_record_names_the_road_only_while_it_is_drawn() {
    let (temporary, mut app) = road_application(1, |_| {});
    app.capture("plain").unwrap();
    click(&mut app, [0, 2]).unwrap();
    click(&mut app, [5, 2]).unwrap();
    app.capture("drawn").unwrap();
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_slice(
            &fs::read(temporary.path().join(format!("evidence/{name}.json"))).unwrap(),
        )
        .unwrap()
    };
    assert!(read("plain").get("road").is_none());
    assert_eq!(
        read("drawn")["road"],
        serde_json::json!({
            "points": [[0, 2], [5, 2]],
            "brush": {"shape": "square", "width": 1},
            "proto": "sand",
            "cells": 6,
            "changes": 4,
            "first": [[0, 2], [1, 2], [2, 2], [3, 2]],
        })
    );
}
