// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sculpt mode's Flatten tool through real SDL pointer and key events, on
//! the CPU scene path: F reaches it after Raise and Lower and goes back to
//! Raise, the target is the committed height under the press and stays
//! fixed for the stroke, the marks are exactly the cells the release
//! changes, the release is one reviewed `flatten_heights`, and presses
//! without a height to sample, or on one outside the range, are refused in
//! words. Every world is a temporary copy of the MIT neutral sample with a
//! grant and generated heights; the committed sample has neither.
use super::proposal::edit_json;
use super::sculpt_mode::{
    MAIN, drag, engine, height, heights, marked, marks, release, sculpt_application, wide_isometric,
};
use super::zones::{key, route, sidebar_texts, zone_application};
use super::{BRUSH_WINDOW, acknowledge_ui, cell_pixel};
use crate::prototype::Application;
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::tools::{SculptTool, ToolMode};

/// Sculpt mode with the Flatten tool, chosen by F twice.
fn flatten_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = sculpt_application(prepare);
    route(&mut app, &key(Keycode::F)).unwrap();
    route(&mut app, &key(Keycode::F)).unwrap();
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Flatten);
    acknowledge_ui(&mut app);
    (temporary, app)
}

fn widget(app: &mut Application, text: &str) -> (bool, bool) {
    app.refresh_ui();
    let found = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.text == text)
        .unwrap_or_else(|| panic!("{text}"));
    (found.enabled, found.active)
}

/// The pending `terrain.set` on `height`: `at` to (expect, value).
fn pending(app: &Application) -> BTreeSet<([i64; 2], i64, i64)> {
    let sealed: serde_json::Value =
        serde_json::from_str(app.session.history().pending().unwrap().sealed()).unwrap();
    let operation = &sealed["operations"][0];
    assert_eq!(operation["grid"], "height");
    operation["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            (
                [
                    cell["at"][0].as_i64().unwrap(),
                    cell["at"][1].as_i64().unwrap(),
                ],
                cell["expect"].as_i64().unwrap(),
                cell["value"].as_i64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn f_cycles_raise_lower_flatten_smooth_and_back_and_the_steps_do_nothing_for_flatten() {
    let (_temporary, mut app) = sculpt_application(|root| heights(root, -4, 4, |x, y| x - y));
    route(&mut app, &key(Keycode::_8)).unwrap();
    let pinned = |app: &mut Application| {
        sidebar_texts(app)
            .into_iter()
            .find(|text| text.starts_with("Sculpt: "))
            .unwrap()
    };
    let mut seen = Vec::new();
    for _ in 0..5 {
        seen.push((app.tools.sculpt_tool(), pinned(&mut app)));
        route(&mut app, &key(Keycode::F)).unwrap();
    }
    assert_eq!(
        seen,
        [
            (SculptTool::Raise, "Sculpt: Raise by 8  F".to_owned()),
            (SculptTool::Lower, "Sculpt: Lower by 8  F".to_owned()),
            // No target until a press samples one.
            (SculptTool::Flatten, "Sculpt: Flatten  F".to_owned()),
            (SculptTool::Smooth, "Sculpt: Smooth  F".to_owned()),
            (SculptTool::Raise, "Sculpt: Raise by 8  F".to_owned()),
        ]
    );
    // The pinned row moves on too.
    app.control(Control::SculptTool(SculptTool::Lower.next()))
        .unwrap();
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Flatten);
    assert_eq!(
        widget(&mut app, "Flatten: drag levels to the start tile"),
        (true, true)
    );
    assert_eq!(
        app.status_label(),
        "Ready. Drag to flatten heights to the start tile's height."
    );
    // The steps are shown dimmed; their keys change nothing and refuse
    // nothing.
    assert_eq!(widget(&mut app, "Step 8  8"), (false, true));
    assert_eq!(widget(&mut app, "Step 1  1"), (false, false));
    route(&mut app, &key(Keycode::_1)).unwrap();
    assert_eq!(app.tools.sculpt_step(), 8);
    assert!(app.notice.is_none());
    // Past Smooth, back at Raise, the kept step applies again.
    route(&mut app, &key(Keycode::F)).unwrap();
    route(&mut app, &key(Keycode::F)).unwrap();
    assert_eq!(app.tools.sculpt_amount(), 8);
    assert_eq!(widget(&mut app, "Step 1  1"), (true, false));
}

/// A drag of a 3 x 3 brush pressed at (3, 2) (height 1 on the `x - y`
/// ramp) and through (5, 2) and (1, 2) (heights 3 and -1), in the
/// projection `prepare` sets up: the target stays 1, the marks are exactly
/// the engine's changes, the release prepares exactly those, Accept writes
/// them, and undo and redo take the whole stroke.
fn flatten_previews_and_applies(prepare: impl FnOnce(&Path)) {
    let (temporary, mut app) = flatten_application(|root| {
        heights(root, -64, 64, |x, y| x - y);
        prepare(root);
    });
    let chunk = temporary.path().join("world").join(MAIN);
    let original = fs::read(&chunk).unwrap();
    drag(&mut app, [3, 2], &[[5, 2], [1, 2], [3, 2]]);
    let covered: Vec<[i64; 2]> = app.tools.stroke().unwrap().cells().collect();
    // x 0 to 6, y 1 to 3; of those, three are already at 1.
    assert_eq!(covered.len(), 21);
    let there: BTreeSet<[i64; 2]> = covered
        .iter()
        .copied()
        .filter(|&[x, y]| x - y == 1)
        .collect();
    assert_eq!(there, BTreeSet::from([[2, 1], [3, 2], [4, 3]]));
    let preview = marked(&mut app);
    assert_eq!(preview, engine(&app));
    assert_eq!(
        preview,
        covered
            .iter()
            .copied()
            .filter(|at| !there.contains(at))
            .collect()
    );
    assert_eq!(marks(&mut app).len(), 3 * preview.len());
    assert_eq!(
        app.status_label(),
        "Flattening 21 cells to 1, 18 to change, 3 already there; release to review."
    );
    assert!(sidebar_texts(&mut app).contains(&"Sculpt: Flatten to 1  F".to_owned()));
    let before: Vec<i32> = covered.iter().map(|&[x, y]| height(&app, x, y)).collect();
    release(&mut app, [3, 2]).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(marks(&mut app).is_empty());
    // Preview, pending and (below) Accept are the same cells.
    assert_eq!(
        pending(&app),
        covered
            .iter()
            .zip(&before)
            .filter(|(at, _)| preview.contains(*at))
            .map(|(&at, &old)| (at, i64::from(old), 1))
            .collect()
    );
    for &[x, y] in &covered {
        assert_eq!(height(&app, x, y), 1, "({x}, {y})");
    }
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_ne!(fs::read(&chunk).unwrap(), original);
    assert_eq!(height(&app, 0, 0), 0, "outside the stroke");
    // The inspector line follows.
    app.action(Action::Select {
        generation: app.view.frame_revision,
        key: SelectionKey::Cell {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            x: 5,
            y: 2,
        },
    })
    .unwrap();
    assert!(sidebar_texts(&mut app).contains(&"Height: 1".to_owned()));
    for (history, shown) in [(Keycode::Z, 3), (Keycode::Y, 1)] {
        route(&mut app, &key(history)).unwrap();
        acknowledge_ui(&mut app);
        route(&mut app, &key(Keycode::Return)).unwrap();
        assert_eq!(height(&app, 5, 2), shown);
        assert_eq!(height(&app, 1, 2), if shown == 1 { 1 } else { -1 });
    }
}

#[test]
fn a_top_down_flatten_previews_exactly_what_accept_applies() {
    flatten_previews_and_applies(|_| {});
}

#[test]
fn an_isometric_flatten_previews_exactly_what_accept_applies() {
    flatten_previews_and_applies(wide_isometric);
}

#[test]
fn the_target_is_sampled_once_at_the_press_and_keys_do_not_move_it() {
    let (_temporary, mut app) = flatten_application(|root| heights(root, -64, 64, |x, _| x * 2));
    app.tools
        .set_brush(tilewright_editor_ui::brush::Brush::default());
    // Pressed at (2, 0), height 4; the drag ends over (6, 0), height 12.
    drag(&mut app, [2, 0], &[[4, 0], [6, 0]]);
    assert_eq!(
        app.status_label(),
        "Flattening 5 cells to 4, 4 to change, 1 already there; release to review."
    );
    // Neither the step keys nor the tool keys reach a running stroke.
    for pressed in [Keycode::_8, Keycode::_1, Keycode::F] {
        route(&mut app, &key(pressed)).unwrap();
        assert!(app.tools.stroke().is_some());
    }
    release(&mut app, [6, 0]).unwrap();
    for x in 2..=6 {
        assert_eq!(height(&app, x, 0), 4, "{x}");
    }
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    // The keys were ignored mid-stroke: still Flatten, step 1. The next
    // press samples the now-committed height 4 at (6, 0), not the 12
    // before.
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Flatten);
    assert_eq!(app.tools.sculpt_step(), 1);
    acknowledge_ui(&mut app);
    drag(&mut app, [6, 0], &[[7, 0]]);
    assert_eq!(
        app.status_label(),
        "Flattening 2 cells to 4, 1 to change, 1 already there; release to review."
    );
}

#[test]
fn presses_without_a_height_in_the_range_are_refused_in_words() {
    // A view over chunk 0_0 and the chunk 1_0 that does not exist; (30, 2)
    // stands at 30, outside the range -8 to 8.
    let (_temporary, mut app) = flatten_application(|root| {
        heights(root, -8, 8, |x, y| if [x, y] == [30, 2] { 30 } else { 0 });
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([28, 0, 8, 6]);
        });
    });
    let press = |app: &mut Application, [x, y]: [i64; 2]| {
        let pixel = cell_pixel(app, x, y);
        crate::prototype::native_pointer(app, pixel, BRUSH_WINDOW, BRUSH_WINDOW)
    };
    assert_eq!(
        press(&mut app, [30, 2]).unwrap_err(),
        "Flatten refused: this tile's height 30 is outside the granted range -8 to 8; start on a tile inside it."
    );
    assert!(app.tools.stroke().is_none());
    acknowledge_ui(&mut app);
    assert_eq!(
        press(&mut app, [33, 1]).unwrap_err(),
        "Flatten needs a starting tile with ground: tile (33, 1) has no map chunk."
    );
    assert!(app.tools.stroke().is_none());
    assert!(app.session.history().pending().is_none());
    // Started inside the range, a stroke over the far tile brings it in,
    // and the tiles without a chunk are counted and left out.
    acknowledge_ui(&mut app);
    drag(&mut app, [31, 2], &[]);
    assert_eq!(
        app.status_label(),
        "Flattening 9 cells to 0, 1 to change, 5 already there, 3 without a chunk; release to review."
    );
    release(&mut app, [31, 2]).unwrap();
    assert_eq!(height(&app, 30, 2), 0);
}

#[test]
fn a_stroke_already_at_its_target_says_why_and_prepares_nothing() {
    let (_temporary, mut app) = flatten_application(|root| heights(root, -4, 4, |_, _| 2));
    drag(&mut app, [3, 3], &[]);
    let label = app.status_label();
    assert_eq!(
        label,
        "Flattening 9 cells to 2 refused: flatten changes no cell: 9 covered cells already at the target 2, 0 in map chunks that do not exist (painting terrain there creates them)"
    );
    let error = release(&mut app, [3, 3]).unwrap_err();
    assert!(label.ends_with(&error), "{error}");
    assert!(app.session.history().pending().is_none());
}

#[test]
fn a_running_flatten_stroke_is_recorded_in_the_frame_evidence() {
    let (temporary, mut app) = flatten_application(|root| heights(root, -64, 64, |x, _| x));
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_slice(
            &fs::read(temporary.path().join(format!("evidence/{name}.json"))).unwrap(),
        )
        .unwrap()
    };
    drag(&mut app, [1, 1], &[]);
    app.capture("during").unwrap();
    assert_eq!(
        read("during")["sculpt_preview"],
        json!({"target": 1, "cells": 9, "changes": 6, "at_target": 3, "missing": 0,
            "first": [[0, 0], [0, 1], [0, 2], [2, 0], [2, 1], [2, 2]]})
    );
    release(&mut app, [1, 1]).unwrap();
    app.capture("after").unwrap();
    assert!(read("after").get("sculpt_preview").is_none());
}

#[test]
fn flatten_presses_are_refused_in_the_pauses_words_and_work_between_proposal_steps() {
    use super::proposal::{open, prepare_and_accept, state, twin};
    use crate::prototype::proposal_view::State;
    // Paused (#92): the press is refused before anything is sampled.
    let (_temporary, mut app) = zone_application(super::sculpt_mode::steep);
    route(&mut app, &key(Keycode::U)).unwrap();
    app.tools.set_sculpt_tool(SculptTool::Flatten);
    let pixel = cell_pixel(&mut app, 5, 4);
    app.view.camera.zoom = 8.0;
    assert_eq!(app.rebuild().unwrap_err(), "invalid world view");
    acknowledge_ui(&mut app);
    let error =
        crate::prototype::native_pointer(&mut app, pixel, BRUSH_WINDOW, BRUSH_WINDOW).unwrap_err();
    assert_eq!(error, "rendering blocked editing: invalid world view");
    assert!(app.tools.stroke().is_none());
    // With a proposal open, the person's own flatten is theirs.
    let grant = |root: &Path| heights(root, -64, 64, |x, _| x);
    let mut twin = twin(2, grant, grant, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    let app = &mut twin.app;
    route(app, &key(Keycode::U)).unwrap();
    app.tools.set_sculpt_tool(SculptTool::Flatten);
    app.tools
        .set_brush(tilewright_editor_ui::brush::Brush::default());
    acknowledge_ui(app);
    drag(app, [6, 5], &[[7, 5]]);
    release(app, [7, 5]).unwrap();
    assert_eq!(state(app), State::OtherPending);
    acknowledge_ui(app);
    route(app, &key(Keycode::Return)).unwrap();
    assert_eq!(height(app, 7, 5), 6);
    assert_eq!(app.tools.mode(), ToolMode::Sculpt);
}

#[test]
fn changed_cells_this_view_does_not_draw_are_counted_not_marked_and_written() {
    // Top-down, range -8 to 8: (3, 0) at i32::MAX and (4, 0) at 100000 are
    // so far off that their tiles are not drawn; (1, 0) at 5 is.
    let (temporary, mut app) = flatten_application(|root| {
        heights(root, -8, 8, |x, y| match [x, y] {
            [1, 0] => 5,
            [3, 0] => i64::from(i32::MAX),
            [4, 0] => 100_000,
            _ => 0,
        });
    });
    // Pressed at (2, 1), height 0; the 3 x 3 brush reaches row 0.
    drag(&mut app, [2, 1], &[[3, 1]]);
    let changes = engine(&app);
    assert_eq!(changes, BTreeSet::from([[1, 0], [3, 0], [4, 0]]));
    assert_eq!(marked(&mut app), BTreeSet::from([[1, 0]]));
    assert_eq!(
        app.status_label(),
        "Flattening 12 cells to 0, 3 to change, 2 not drawn at this view, 9 already there; release to review."
    );
    app.capture("undrawn").unwrap();
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(temporary.path().join("evidence/undrawn.json")).unwrap())
            .unwrap();
    assert_eq!(
        record["sculpt_preview"],
        json!({"target": 0, "cells": 12, "changes": 3, "at_target": 9, "missing": 0,
            "first": [[1, 0], [3, 0], [4, 0]],
            "undrawn": {"count": 2, "first": [[3, 0], [4, 0]]}})
    );
    release(&mut app, [3, 1]).unwrap();
    assert_eq!(
        pending(&app),
        BTreeSet::from([
            ([1, 0], 5, 0),
            ([3, 0], i64::from(i32::MAX), 0),
            ([4, 0], 100_000, 0)
        ])
    );
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert!(app.session.history().pending().is_none());
    for x in [1, 3, 4] {
        assert_eq!(height(&app, x, 0), 0, "{x}");
    }
}
