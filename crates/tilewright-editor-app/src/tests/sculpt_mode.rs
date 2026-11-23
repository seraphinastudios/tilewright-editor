// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sculpt mode through real SDL pointer and key events, on the CPU scene
//! path: offered only with `editing.heights`, it shows the height tint and
//! puts it back, a drag previews with centre marks exactly the cells its
//! release changes, the release is one reviewed `adjust_heights` by the
//! signed step, and the status line says what is left out or why the
//! engine refuses. Every world is a temporary copy of the MIT neutral
//! sample with a grant and generated heights; the committed sample has
//! neither.
// Colours are copied, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::proposal::edit_json;
use super::zones::{key, route, sidebar_texts, zone_application};
use super::{BRUSH_WINDOW, acknowledge_ui, cell_pixel};
use crate::prototype::Application;
use crate::workspace_render::{SCULPT_MARK, SCULPT_MARK_OUTLINE, STROKE_PREVIEW_TINT};
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::brush::{Brush, BrushShape};
use tilewright_editor_ui::tools::{SculptTool, ToolMode};

const MAIN: &str = "planes/main/layers/0/0_0.jsonc";

/// The range `min` to `max`, and `height(x, y)` in the main chunk.
fn heights(root: &Path, min: i32, max: i32, height: impl Fn(i64, i64) -> i64) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = json!({"min": min, "max": max});
    });
    let rows: Vec<Vec<i64>> = (0..32)
        .map(|y| (0..32).map(|x| height(x, y)).collect())
        .collect();
    edit_json(root, MAIN, |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}

/// The range -4 to 4 over heights `x - y` (-5 to 7 in the 8 x 6 view), so
/// some cells start at or beyond an end.
fn ramped(root: &Path) {
    heights(root, -4, 4, |x, y| x - y);
}

fn isometric(root: &Path) {
    edit_json(root, "profile/profile.jsonc", |profile| {
        profile["projection"] = json!({"kind": "isometric", "tile_width": 32, "tile_height": 16});
    });
}

/// The 1280x800 application in Sculpt mode, entered by U, with a 3 x 3
/// square brush.
fn sculpt_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = zone_application(prepare);
    route(&mut app, &key(Keycode::U)).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Sculpt);
    app.tools.set_brush(Brush {
        shape: BrushShape::Square,
        radius: 1,
    });
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The committed (or, while pending, the shown) height of tile `x, y`.
fn height(app: &Application, x: i64, y: i64) -> i32 {
    let plane = &app.session.world().planes()["main"];
    tilewright_editor_ui::heights::cell_height(plane, 0, x, y).unwrap()
}

/// Press at `from`, drag through `through`, keep the button down.
fn drag(app: &mut Application, from: [i64; 2], through: &[[i64; 2]]) {
    let start = cell_pixel(app, from[0], from[1]);
    crate::prototype::native_pointer(app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    for &[x, y] in through {
        let at = cell_pixel(app, x, y);
        crate::prototype::native_stroke_motion(app, at, true, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    }
}

fn release(app: &mut Application, at: [i64; 2]) -> Result<(), String> {
    let pixel = cell_pixel(app, at[0], at[1]);
    crate::prototype::native_stroke_release(app, pixel, BRUSH_WINDOW, BRUSH_WINDOW)
}

/// The sculpt overlay of the frame being shown.
fn marks(app: &mut Application) -> Vec<tilewright_scene::Quad> {
    app.refresh_ui();
    crate::prototype::stroke_overlay(app)
}

/// The tiles the sculpt preview marks, read back through the cell picks.
fn marked(app: &mut Application) -> BTreeSet<[i64; 2]> {
    marks(app)
        .iter()
        .map(
            |quad| match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a mark of no cell"),
            },
        )
        .collect()
}

/// The engine's own answer for the running stroke.
fn engine(app: &Application) -> BTreeSet<[i64; 2]> {
    app.session
        .stroke_changes(app.tools.stroke().unwrap())
        .unwrap()
        .into_iter()
        .collect()
}

#[test]
fn sculpt_is_offered_only_with_the_height_grant() {
    // The sample itself has no grant: no row, and U is refused in words.
    let (_temporary, mut app) = zone_application(|_| {});
    let texts = sidebar_texts(&mut app);
    assert!(texts.contains(&"Show heights  H".to_owned()), "{texts:?}");
    assert!(
        !texts.iter().any(|text| text.contains("Sculpt")),
        "{texts:?}"
    );
    assert_eq!(
        route(&mut app, &key(Keycode::U)).unwrap_err(),
        crate::prototype::NOTHING_TO_SCULPT
    );
    assert!(app.control(Control::Sculpt).is_err());
    assert_eq!(app.tools.mode(), ToolMode::Select);
    assert!(!app.tools.heights_visible());
    // With the grant: the row shows in every mode, after the height rows.
    let (_temporary, mut app) = zone_application(ramped);
    for mode in [Control::Select, Control::Terrain, Control::Zone] {
        app.control(mode).unwrap();
        let texts = sidebar_texts(&mut app);
        let toggle = texts.iter().position(|text| text == "Show heights  H");
        let row = texts.iter().position(|text| text == "Sculpt heights  U");
        assert!(toggle.is_some() && row > toggle, "{texts:?}");
    }
    // The toolbar is unchanged: no Sculpt button.
    app.refresh_ui();
    assert!(
        !app.ui
            .widgets
            .iter()
            .any(|widget| widget.layout.y == 8 && widget.text.contains("Sculpt"))
    );
}

#[test]
fn the_mode_shows_its_controls_and_keys_choose_the_tool_and_step() {
    let (_temporary, mut app) = sculpt_application(ramped);
    let texts = sidebar_texts(&mut app);
    for line in [
        "Sculpt: Raise by 1  F",
        "Hide heights  H",
        "Heights -5 to 7",
        "Sculpt heights  U",
        "Granted heights -4 to 4",
        "Brush: square 3x3",
        "Raise: drag lifts each cell once",
        "Lower: drag sinks each cell once",
        "Step 1  1",
        "Step 8  8",
    ] {
        assert!(texts.contains(&line.to_owned()), "{line}: {texts:?}");
    }
    // The pinned row comes before the scrolled ones.
    assert_eq!(
        texts
            .iter()
            .position(|text| text == "Sculpt: Raise by 1  F"),
        texts.iter().position(|text| text.starts_with("Sculpt")),
    );
    let active = |app: &mut Application, text: &str| {
        app.refresh_ui();
        app.ui
            .widgets
            .iter()
            .find(|widget| widget.text == text)
            .unwrap_or_else(|| panic!("{text}"))
            .active
    };
    assert!(active(&mut app, "Sculpt heights  U"));
    assert!(active(&mut app, "Raise: drag lifts each cell once"));
    assert!(active(&mut app, "Step 1  1"));
    assert_eq!(app.status_label(), "Ready. Drag to raise heights by 1.");
    route(&mut app, &key(Keycode::F)).unwrap();
    route(&mut app, &key(Keycode::_8)).unwrap();
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Lower);
    assert_eq!(app.tools.sculpt_amount(), -8);
    assert!(active(&mut app, "Lower: drag sinks each cell once"));
    assert!(active(&mut app, "Step 8  8"));
    assert!(sidebar_texts(&mut app).contains(&"Sculpt: Lower by 8  F".to_owned()));
    assert_eq!(app.status_label(), "Ready. Drag to lower heights by 8.");
    // The pinned row and the rows choose too.
    app.control(Control::SculptTool(SculptTool::Lower.next()))
        .unwrap();
    app.control(Control::SculptStep(1)).unwrap();
    assert_eq!(app.tools.sculpt_amount(), 1);
    assert!(app.control(Control::SculptStep(4)).is_err());
    // The brush keys work here.
    route(&mut app, &key(Keycode::RightBracket)).unwrap();
    route(&mut app, &key(Keycode::B)).unwrap();
    assert!(sidebar_texts(&mut app).contains(&"Brush: circle 5x5".to_owned()));
    // Outside Sculpt mode F is Terrain's rule key, and 1 and 8 do nothing.
    app.control(Control::Select).unwrap();
    route(&mut app, &key(Keycode::F)).unwrap();
    route(&mut app, &key(Keycode::_8)).unwrap();
    assert_eq!(app.tools.sculpt_amount(), 1);
}

#[test]
fn entering_shows_the_height_tint_and_leaving_puts_it_back() {
    let (_temporary, mut app) = zone_application(ramped);
    assert!(!app.tools.heights_visible());
    route(&mut app, &key(Keycode::U)).unwrap();
    assert!(app.tools.heights_visible());
    app.refresh_ui();
    assert!(!crate::prototype::height_overlay(&app).is_empty());
    // H still hides it here, for the ground art.
    route(&mut app, &key(Keycode::H)).unwrap();
    assert!(!app.tools.heights_visible());
    route(&mut app, &key(Keycode::H)).unwrap();
    route(&mut app, &key(Keycode::T)).unwrap();
    assert!(!app.tools.heights_visible(), "the tint was off before");
    // Shown before: still shown after.
    route(&mut app, &key(Keycode::H)).unwrap();
    route(&mut app, &key(Keycode::U)).unwrap();
    route(&mut app, &key(Keycode::H)).unwrap();
    app.control(Control::Select).unwrap();
    assert!(app.tools.heights_visible(), "the tint was on before");
}

/// A drag of a 3 x 3 brush from (1, 2) to (3, 2), raising by 8, in the
/// projection `prepare` sets up: the marks are exactly the engine's
/// changes, the release prepares exactly those and Accept writes them.
fn raise_previews_and_applies(prepare: impl FnOnce(&Path)) {
    let (temporary, mut app) = sculpt_application(|root| {
        heights(root, -64, 64, |x, y| x - y);
        prepare(root);
    });
    let chunk = temporary.path().join("world").join(MAIN);
    let original = fs::read(&chunk).unwrap();
    route(&mut app, &key(Keycode::_8)).unwrap();
    acknowledge_ui(&mut app);
    // Back and forth: each covered cell is raised once.
    drag(&mut app, [1, 2], &[[3, 2], [1, 2], [3, 2]]);
    let covered: Vec<[i64; 2]> = app.tools.stroke().unwrap().cells().collect();
    assert_eq!(covered.len(), 15);
    let preview = marked(&mut app);
    assert_eq!(preview, engine(&app));
    assert_eq!(preview.len(), 15, "every covered cell moves");
    assert_eq!(
        app.status_label(),
        "Raising 15 cells by 8, 15 to change; release to review."
    );
    assert!(app.session.history().pending().is_none());
    let before: Vec<i32> = covered.iter().map(|&[x, y]| height(&app, x, y)).collect();
    release(&mut app, [3, 2]).unwrap();
    // One pending reviewed change, nothing written; the marks are gone.
    assert!(app.session.history().pending().is_some());
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(marks(&mut app).is_empty());
    for (&[x, y], before) in covered.iter().zip(&before) {
        assert_eq!(height(&app, x, y), before + 8, "({x}, {y})");
    }
    // During Review the tint shows the pending heights.
    assert!(
        sidebar_texts(&mut app).contains(&"Heights -5 to 11".to_owned()),
        "{:?}",
        sidebar_texts(&mut app)
    );
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.recovery.records().len(), 1);
    assert_ne!(fs::read(&chunk).unwrap(), original);
    // The inspector line follows.
    app.action(Action::Select {
        generation: app.view.frame_revision,
        key: SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 2,
            y: 2,
        },
    })
    .unwrap();
    assert!(sidebar_texts(&mut app).contains(&"Height: 8".to_owned()));
    // Undo and redo, one reviewed step each.
    for (history, shown) in [(Keycode::Z, 0), (Keycode::Y, 8)] {
        route(&mut app, &key(history)).unwrap();
        assert_eq!(height(&app, 2, 2), shown);
        acknowledge_ui(&mut app);
        route(&mut app, &key(Keycode::Return)).unwrap();
        assert!(app.session.history().pending().is_none());
        assert_eq!(height(&app, 2, 2), shown);
        assert!(sidebar_texts(&mut app).contains(&format!("Height: {shown}")));
    }
}

#[test]
fn a_top_down_raise_previews_exactly_what_accept_applies() {
    raise_previews_and_applies(|_| {});
}

#[test]
fn an_isometric_raise_previews_exactly_what_accept_applies() {
    raise_previews_and_applies(isometric);
}

#[test]
fn a_lower_stroke_is_one_adjustment_by_the_negative_step() {
    let (_temporary, mut app) = sculpt_application(|root| heights(root, -64, 64, |_, _| 0));
    route(&mut app, &key(Keycode::F)).unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [4, 3], &[]);
    assert_eq!(
        app.status_label(),
        "Lowering 9 cells by 1, 9 to change; release to review."
    );
    release(&mut app, [4, 3]).unwrap();
    assert_eq!(height(&app, 4, 3), -1);
    assert_eq!(height(&app, 5, 4), -1);
    assert_eq!(height(&app, 6, 3), 0);
}

#[test]
fn the_marks_are_centre_marks_no_band_tint_or_highlight_looks_like() {
    let (_temporary, mut app) = sculpt_application(|root| heights(root, -64, 64, |x, y| x - y));
    drag(&mut app, [2, 2], &[]);
    let overlay = marks(&mut app);
    // An outline and a mark per marked tile, in that order.
    assert!(!overlay.is_empty());
    let colours: BTreeSet<[u32; 4]> = overlay
        .iter()
        .map(|quad| quad.color.map(f32::to_bits))
        .collect();
    assert_eq!(
        colours,
        [SCULPT_MARK, SCULPT_MARK_OUTLINE]
            .iter()
            .map(|colour| colour.map(f32::to_bits))
            .collect()
    );
    let others: Vec<[f32; 3]> = tilewright_editor_ui::heights::RAMP
        .iter()
        .chain(tilewright_editor_ui::zones::TINTS.iter())
        .copied()
        .chain([
            [
                STROKE_PREVIEW_TINT[0],
                STROKE_PREVIEW_TINT[1],
                STROKE_PREVIEW_TINT[2],
            ],
            [0.0, 0.0, 0.0],
        ])
        .collect();
    for other in others {
        let distance: f32 = (0..3)
            .map(|c| (other[c] - SCULPT_MARK[c]).powi(2))
            .sum::<f32>()
            .sqrt();
        assert!(distance > 0.3, "{other:?}");
    }
    // Each mark sits inside its tile, half its size, about its centre.
    let bounds = |quads: &[&tilewright_scene::Quad]| {
        let points = || quads.iter().flat_map(|quad| quad.positions);
        [
            points().map(|p| p[0]).fold(f32::MAX, f32::min),
            points().map(|p| p[1]).fold(f32::MAX, f32::min),
            points().map(|p| p[0]).fold(f32::MIN, f32::max),
            points().map(|p| p[1]).fold(f32::MIN, f32::max),
        ]
    };
    for tile in marked(&mut app) {
        let of = |colour: [f32; 4]| -> Vec<&tilewright_scene::Quad> {
            overlay
                .iter()
                .filter(|quad| {
                    quad.color == colour
                        && matches!(
                            app.prepared.cell_picks.get(&quad.semantic_id),
                            Some(SelectionKey::Cell { x, y, .. }) if [*x, *y] == tile
                        )
                })
                .collect()
        };
        let cell: Vec<&tilewright_scene::Quad> = app
            .prepared
            .cell_scene
            .quads()
            .iter()
            .filter(|quad| {
                matches!(
                    app.prepared.cell_picks.get(&quad.semantic_id),
                    Some(SelectionKey::Cell { x, y, .. }) if [*x, *y] == tile
                )
            })
            .collect();
        let [cl, ct, cr, cb] = bounds(&cell);
        let [ml, mt, mr, mb] = bounds(&of(SCULPT_MARK));
        let [ol, ot, or, ob] = bounds(&of(SCULPT_MARK_OUTLINE));
        assert!((mr - ml - (cr - cl) / 2.0).abs() < 0.01, "{tile:?}");
        assert!((mb - mt - (cb - ct) / 2.0).abs() < 0.01, "{tile:?}");
        assert!(((ml + mr) - (cl + cr)).abs() < 0.01, "{tile:?}");
        assert!(ol < ml && or > mr && ot < mt && ob > mb, "{tile:?}");
        assert!(ol > cl && or < cr, "{tile:?}");
    }
}

#[test]
fn cells_at_an_end_and_without_a_chunk_are_left_out_and_counted() {
    // At the top: raising 1 over x 0 to 2 of row 0 (heights 0, 1, 2) with
    // the range -4 to 1 leaves (1, 0) and (2, 0) out.
    let (_temporary, mut app) = sculpt_application(|root| heights(root, -4, 1, |x, y| x - y));
    app.tools.set_brush(Brush::default());
    drag(&mut app, [0, 0], &[[2, 0]]);
    assert_eq!(
        app.status_label(),
        "Raising 3 cells by 1, 1 to change, 2 at or above the top; release to review."
    );
    assert_eq!(marked(&mut app), BTreeSet::from([[0, 0]]));
    release(&mut app, [2, 0]).unwrap();
    assert_eq!(
        [height(&app, 0, 0), height(&app, 1, 0), height(&app, 2, 0)],
        [1, 1, 2]
    );
    // At the bottom, lowering.
    app.action(Action::Cancel).unwrap();
    route(&mut app, &key(Keycode::F)).unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [0, 5], &[[2, 5]]);
    assert_eq!(
        app.status_label(),
        "Lowering 3 cells by 1, 1 to change, 2 at or below the bottom; release to review."
    );
    app.interrupt_stroke();
    // Without a chunk: the view straddles chunk 0_0 and the missing 1_0.
    let (_temporary, mut app) = sculpt_application(|root| {
        heights(root, -64, 64, |_, _| 0);
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([28, 0, 8, 6]);
        });
    });
    app.tools.set_brush(Brush::default());
    drag(&mut app, [30, 1], &[[33, 1]]);
    assert_eq!(
        app.status_label(),
        "Raising 4 cells by 1, 2 to change, 2 without a chunk; release to review."
    );
    assert_eq!(marked(&mut app), BTreeSet::from([[30, 1], [31, 1]]));
    let frame = crate::prototype::Application::sculpt_frame(&app).unwrap();
    assert_eq!(
        frame,
        json!({"amount": 1, "cells": 4, "changes": 2, "at_end": 0, "missing": 2,
            "first": [[30, 1], [31, 1]]})
    );
    release(&mut app, [33, 1]).unwrap();
    assert!(app.session.history().pending().is_some());
    // No chunk was created.
    assert!(
        !app.session.world().planes()["main"]
            .chunks
            .contains_key(&(0, 1, 0))
    );
}

#[test]
fn a_cell_the_step_cannot_bring_into_the_range_is_named_before_release() {
    // (6, 0) has height 6, above the range -4 to 4: lowering it by 1 does
    // not bring it in, by 8 does.
    let (_temporary, mut app) = sculpt_application(ramped);
    app.tools.set_brush(Brush::default());
    route(&mut app, &key(Keycode::F)).unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [5, 0], &[[6, 0]]);
    let reason = "cell (6, 0) has height 6, above the granted range -4 to 4: lowering it into the range needs an amount of at least 2";
    assert_eq!(
        app.status_label(),
        format!("Lowering 2 cells by 1 refused: {reason}")
    );
    assert!(marks(&mut app).is_empty());
    // The details give it whole, however narrow the status line.
    let bare = |text: &str| text.split_whitespace().collect::<String>();
    assert!(
        bare(&app.review_lines(300).join(" "))
            .contains(&bare(&format!("Release prepares nothing: {reason}")))
    );
    // The frame evidence records the refusal.
    assert_eq!(
        crate::prototype::Application::sculpt_frame(&app).unwrap(),
        json!({"amount": -1, "cells": 2, "refused": reason})
    );
    // The release is refused by the engine, in its words.
    let error = release(&mut app, [6, 0]).unwrap_err();
    assert!(error.contains(reason), "{error}");
    assert!(app.session.history().pending().is_none());
    // Step 8 brings it in.
    route(&mut app, &key(Keycode::_8)).unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [5, 0], &[[6, 0]]);
    assert_eq!(
        app.status_label(),
        "Lowering 2 cells by 8, 2 to change; release to review."
    );
    release(&mut app, [6, 0]).unwrap();
    assert_eq!([height(&app, 5, 0), height(&app, 6, 0)], [-3, -2]);
}

#[test]
fn a_stroke_that_changes_nothing_says_why_and_prepares_nothing() {
    let (_temporary, mut app) = sculpt_application(|root| heights(root, -4, 4, |_, _| 4));
    drag(&mut app, [3, 3], &[]);
    let label = app.status_label();
    assert_eq!(
        label,
        "Raising 9 cells by 1 refused: height adjustment changes no cell: 9 covered cells already at or above the top of the range, 0 in map chunks that do not exist (painting terrain there creates them)"
    );
    let error = release(&mut app, [3, 3]).unwrap_err();
    assert!(label.ends_with(&error), "{error}");
    assert!(app.session.history().pending().is_none());
}

#[test]
fn a_running_sculpt_stroke_is_recorded_in_the_frame_evidence() {
    let (temporary, mut app) = sculpt_application(|root| heights(root, -64, 64, |_, _| 0));
    route(&mut app, &key(Keycode::_8)).unwrap();
    acknowledge_ui(&mut app);
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_slice(
            &fs::read(temporary.path().join(format!("evidence/{name}.json"))).unwrap(),
        )
        .unwrap()
    };
    app.capture("before").unwrap();
    assert!(read("before").get("sculpt_preview").is_none());
    drag(&mut app, [1, 1], &[]);
    app.capture("during").unwrap();
    assert_eq!(
        read("during")["sculpt_preview"],
        json!({"amount": 8, "cells": 9, "changes": 9, "at_end": 0, "missing": 0,
            "first": [[0, 0], [0, 1], [0, 2], [1, 0], [1, 1], [1, 2], [2, 0], [2, 1]]})
    );
    release(&mut app, [1, 1]).unwrap();
    app.capture("after").unwrap();
    assert!(read("after").get("sculpt_preview").is_none());
}

#[test]
fn strokes_are_refused_in_the_pauses_words() {
    // A view that cannot be drawn pauses editing (#92).
    let (_temporary, mut app) = zone_application(steep);
    route(&mut app, &key(Keycode::U)).unwrap();
    let pixel = cell_pixel(&mut app, 5, 4);
    app.view.camera.zoom = 8.0;
    assert_eq!(app.rebuild().unwrap_err(), "invalid world view");
    acknowledge_ui(&mut app);
    let error =
        crate::prototype::native_pointer(&mut app, pixel, BRUSH_WINDOW, BRUSH_WINDOW).unwrap_err();
    assert_eq!(error, "rendering blocked editing: invalid world view");
    assert!(app.tools.stroke().is_none());
    assert!(app.session.history().pending().is_none());
    // Zoomed back out, the next stroke works.
    app.view.camera.zoom = 1.0;
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [5, 4], &[]);
    release(&mut app, [5, 4]).unwrap();
    assert!(app.session.history().pending().is_some());
}

/// The isometric sample with the range -1024 to 1024 and the steep
/// pattern of the view tests at ±512 in the 3 x 3 tiles from `at`.
fn steep_at(root: &Path, [ax, ay]: [i64; 2]) {
    let pattern = [[1, 1, -1], [1, -1, -1], [-1, -1, 1]];
    heights(root, -1024, 1024, |x, y| {
        if (ax..ax + 3).contains(&x) && (ay..ay + 3).contains(&y) {
            let (Ok(row), Ok(column)) = (usize::try_from(y - ay), usize::try_from(x - ax)) else {
                return 0;
            };
            pattern[row][column] * 512
        } else {
            0
        }
    });
    isometric(root);
}
fn steep(root: &Path) {
    steep_at(root, [1, 1]);
}

#[test]
fn a_release_that_cannot_be_drawn_is_cancelled_and_the_next_stroke_works() {
    // High tiles at (3, 2), (4, 2), (3, 3) and (5, 4), near the view's
    // centre, so (4, 2) and (3, 3) stay in the window at a high zoom.
    let (_temporary, mut app) = sculpt_application(|root| steep_at(root, [3, 2]));
    app.tools.set_brush(Brush::default());
    route(&mut app, &key(Keycode::_8)).unwrap();
    // The highest zoom at which the committed World still draws, to a part
    // in ten thousand.
    let draws = |app: &mut Application, zoom: f32| {
        app.view.camera.zoom = zoom;
        app.rebuild().is_ok()
    };
    let (mut low, mut high) = (1.0_f32, 64.0_f32);
    assert!(draws(&mut app, low) && !draws(&mut app, high));
    while high - low > low * 1e-4 {
        let middle = f32::midpoint(low, high);
        if draws(&mut app, middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    assert!(draws(&mut app, low));
    acknowledge_ui(&mut app);
    // Raising two high tiles by 8 makes the slope steeper than this zoom
    // can draw.
    drag(&mut app, [4, 2], &[[3, 3]]);
    assert_eq!(
        app.status_label(),
        "Raising 2 cells by 8, 2 to change; release to review."
    );
    let error = release(&mut app, [3, 3]).unwrap_err();
    let notice = format!(
        "{} Zoom out, or raise or lower by less. Renderer: invalid world view",
        crate::prototype::UNDRAWABLE_CHANGE
    );
    assert_eq!(error, notice);
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_none());
    assert_eq!(height(&app, 4, 2), 512);
    // Still in Sculpt mode; the next stroke, lowering, works here.
    assert_eq!(app.tools.mode(), ToolMode::Sculpt);
    route(&mut app, &key(Keycode::F)).unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [4, 2], &[]);
    release(&mut app, [4, 2]).unwrap();
    assert!(app.session.history().pending().is_some());
    assert_eq!(height(&app, 4, 2), 504);
}

#[test]
fn the_persons_own_sculpt_edit_between_proposal_steps_is_allowed() {
    use super::proposal::{open, prepare_and_accept, state, twin};
    use crate::prototype::proposal_view::State;
    let grant = |root: &Path| heights(root, -64, 64, |_, _| 0);
    let mut twin = twin(2, grant, grant, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    let app = &mut twin.app;
    route(app, &key(Keycode::U)).unwrap();
    app.tools.set_brush(Brush::default());
    acknowledge_ui(app);
    drag(app, [6, 5], &[]);
    release(app, [6, 5]).unwrap();
    assert_eq!(state(app), State::OtherPending);
    acknowledge_ui(app);
    route(app, &key(Keycode::Return)).unwrap();
    assert_eq!(height(app, 6, 5), 1);
    assert_eq!(state(app), State::Moved { resume: None });
    // Undone, the proposal continues.
    route(app, &key(Keycode::Z)).unwrap();
    assert_ne!(state(app), State::Moved { resume: None });
}

#[test]
fn the_guides_hand_test_reads_as_written() {
    // docs/OFFLINE-PROTOTYPE.md, "Sculpt mode": the sample with only the
    // range added.
    let (_temporary, mut app) = zone_application(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["heights"] = json!({"min": -8, "max": 8});
        });
    });
    route(&mut app, &key(Keycode::U)).unwrap();
    let texts = sidebar_texts(&mut app);
    assert!(texts.contains(&"Sculpt: Raise by 1  F".to_owned()));
    assert!(texts.contains(&"Granted heights -8 to 8".to_owned()));
    assert_eq!(app.status_label(), "Ready. Drag to raise heights by 1.");
    route(&mut app, &key(Keycode::_8)).unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [1, 1], &[[3, 1]]);
    assert_eq!(marked(&mut app).len(), 3);
    assert_eq!(
        app.status_label(),
        "Raising 3 cells by 8, 3 to change; release to review."
    );
    release(&mut app, [3, 1]).unwrap();
    assert!(sidebar_texts(&mut app).contains(&"Heights 0 to 8".to_owned()));
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    app.action(Action::Select {
        generation: app.view.frame_revision,
        key: SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 2,
            y: 1,
        },
    })
    .unwrap();
    assert!(sidebar_texts(&mut app).contains(&"Height: 8".to_owned()));
    acknowledge_ui(&mut app);
    drag(&mut app, [2, 1], &[[4, 1]]);
    assert_eq!(
        app.status_label(),
        "Raising 3 cells by 8, 1 to change, 2 at or above the top; release to review."
    );
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.tools.stroke().is_none());
    route(&mut app, &key(Keycode::Z)).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert_eq!(
        [height(&app, 1, 1), height(&app, 2, 1), height(&app, 3, 1)],
        [0, 0, 0]
    );
}
