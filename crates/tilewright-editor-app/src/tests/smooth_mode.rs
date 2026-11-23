// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sculpt mode's Smooth tool through real SDL pointer and key events, on
//! the CPU scene path: F reaches it after Flatten and goes back to Raise,
//! the step keys do nothing for it, the marks are exactly the cells the
//! release changes, the release is one reviewed `smooth_heights`, and the
//! status line and frame evidence count what is left out. Every world is a
//! temporary copy of the MIT neutral sample with a grant and generated
//! heights; the committed sample has neither.
use super::acknowledge_ui;
use super::proposal::edit_json;
use super::sculpt_mode::{
    MAIN, drag, engine, height, heights, marked, marks, release, sculpt_application, wide_isometric,
};
use super::zones::{key, route, sidebar_texts};
use crate::prototype::Application;
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::brush::{Brush, BrushShape};
use tilewright_editor_ui::tools::SculptTool;

/// Sculpt mode with the Smooth tool, chosen by F three times.
fn smooth_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = sculpt_application(prepare);
    for _ in 0..3 {
        route(&mut app, &key(Keycode::F)).unwrap();
    }
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Smooth);
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The range -8 to 8, a spike of 8 at (2, 2) and 0 elsewhere.
fn spike(root: &Path) {
    heights(root, -8, 8, |x, y| if [x, y] == [2, 2] { 8 } else { 0 });
}

/// The pending `terrain.set` on `height`: `at` to (expect, value).
fn pending(app: &Application) -> BTreeMap<[i64; 2], (i64, i64)> {
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
                (
                    cell["expect"].as_i64().unwrap(),
                    cell["value"].as_i64().unwrap(),
                ),
            )
        })
        .collect()
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

#[test]
fn f_reaches_smooth_after_flatten_and_goes_back_to_raise_and_the_steps_do_nothing() {
    let (_temporary, mut app) = sculpt_application(spike);
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
            (SculptTool::Flatten, "Sculpt: Flatten  F".to_owned()),
            (SculptTool::Smooth, "Sculpt: Smooth  F".to_owned()),
            (SculptTool::Raise, "Sculpt: Raise by 8  F".to_owned()),
        ]
    );
    // The pinned row moves on too.
    app.control(Control::SculptTool(SculptTool::Flatten.next()))
        .unwrap();
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Smooth);
    assert_eq!(
        widget(&mut app, "Smooth: drag evens out heights"),
        (true, true)
    );
    assert_eq!(
        app.status_label(),
        "Ready. Drag to smooth heights toward their neighbours."
    );
    // The steps are dimmed; their keys change nothing and refuse nothing.
    assert_eq!(widget(&mut app, "Step 8  8"), (false, true));
    assert_eq!(widget(&mut app, "Step 1  1"), (false, false));
    route(&mut app, &key(Keycode::_1)).unwrap();
    assert_eq!(app.tools.sculpt_step(), 8);
    assert!(app.notice.is_none());
    // Back at Raise, the kept step applies again.
    route(&mut app, &key(Keycode::F)).unwrap();
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Raise);
    assert_eq!(app.tools.sculpt_amount(), 8);
}

/// A 3 x 3 stroke over the spike at (2, 2), in the projection `prepare`
/// sets up: the spike comes down to (36 × 8) / 256 = 1.1, so 1, its four
/// edge neighbours go up to (24 × 8) / 256 = 0.75, so 1, and its diagonal
/// (3, 3) stays at 0 ((16 × 8) / 256, an exact half, toward its own
/// height), while the diagonals nearer the view's edge read less weight
/// (225 for (1, 1), 240 for (1, 3) and (3, 1)) and go up to 1. The marks
/// are exactly those, the release prepares exactly those, Accept writes
/// them, and undo and redo take the whole stroke.
fn smooth_previews_and_applies(prepare: impl FnOnce(&Path)) {
    let (temporary, mut app) = smooth_application(|root| {
        spike(root);
        prepare(root);
    });
    let chunk = temporary.path().join("world").join(MAIN);
    let original = fs::read(&chunk).unwrap();
    drag(&mut app, [2, 2], &[]);
    let changed = BTreeMap::from([
        ([1, 1], (0, 1)),
        ([1, 2], (0, 1)),
        ([1, 3], (0, 1)),
        ([2, 1], (0, 1)),
        ([2, 2], (8, 1)),
        ([2, 3], (0, 1)),
        ([3, 1], (0, 1)),
        ([3, 2], (0, 1)),
    ]);
    let cells: BTreeSet<[i64; 2]> = changed.keys().copied().collect();
    assert_eq!(marked(&mut app), cells);
    assert_eq!(engine(&app), cells);
    assert_eq!(marks(&mut app).len(), 3 * cells.len());
    assert_eq!(
        app.status_label(),
        "Smoothing 9 cells, 8 to change, 1 already even; release to review."
    );
    assert!(sidebar_texts(&mut app).contains(&"Sculpt: Smooth  F".to_owned()));
    release(&mut app, [2, 2]).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(marks(&mut app).is_empty());
    assert_eq!(pending(&app), changed);
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert!(app.session.history().pending().is_none());
    for (&[x, y], &(_, after)) in &changed {
        assert_eq!(i64::from(height(&app, x, y)), after, "({x}, {y})");
    }
    assert_eq!(height(&app, 3, 3), 0);
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
    assert!(sidebar_texts(&mut app).contains(&"Height: 1".to_owned()));
    for (history, shown) in [(Keycode::Z, 8), (Keycode::Y, 1)] {
        route(&mut app, &key(history)).unwrap();
        acknowledge_ui(&mut app);
        route(&mut app, &key(Keycode::Return)).unwrap();
        assert_eq!(height(&app, 2, 2), shown);
        assert_eq!(height(&app, 2, 1), i32::from(shown == 1));
    }
}

#[test]
fn a_top_down_smooth_previews_exactly_what_accept_applies() {
    smooth_previews_and_applies(|_| {});
}

#[test]
fn an_isometric_smooth_previews_exactly_what_accept_applies() {
    smooth_previews_and_applies(wide_isometric);
}

/// A small deterministic generator (xorshift).
struct Random(u64);
impl Random {
    fn below(&mut self, bound: u64) -> i64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        i64::try_from(self.0 % bound).unwrap()
    }
}

#[test]
fn random_strokes_mark_prepare_and_write_the_same_cells() {
    let (_temporary, mut app) = smooth_application(|root| {
        heights(root, -16, 16, |x, y| ((x * 5 + y * 3) % 9 - 4) * 3);
        wide_isometric(root);
    });
    let mut random = Random(0x0dd_ba11);
    let mut applied = 0;
    for round in 0..12 {
        let radius = random.below(3);
        app.tools.set_brush(Brush {
            shape: if random.below(2) == 0 {
                BrushShape::Square
            } else {
                BrushShape::Circle
            },
            radius: u16::try_from(radius).unwrap(),
        });
        let start = [random.below(8), random.below(6)];
        let through: Vec<[i64; 2]> = (0..random.below(3))
            .map(|_| [random.below(8), random.below(6)])
            .collect();
        let before: BTreeMap<[i64; 2], i32> = (0..8)
            .flat_map(|x| (0..6).map(move |y| [x, y]))
            .map(|[x, y]| ([x, y], height(&app, x, y)))
            .collect();
        drag(&mut app, start, &through);
        let preview = marked(&mut app);
        let planned: BTreeSet<[i64; 2]> = app
            .session
            .stroke_changes(app.tools.stroke().unwrap())
            .unwrap_or_default()
            .into_iter()
            .collect();
        assert_eq!(preview, planned, "round {round}");
        let end = through.last().copied().unwrap_or(start);
        if planned.is_empty() {
            assert!(release(&mut app, end).is_err());
            acknowledge_ui(&mut app);
            continue;
        }
        release(&mut app, end).unwrap();
        applied += 1;
        let prepared = pending(&app);
        assert_eq!(
            prepared.keys().copied().collect::<BTreeSet<_>>(),
            preview,
            "round {round}"
        );
        acknowledge_ui(&mut app);
        route(&mut app, &key(Keycode::Return)).unwrap();
        // Accept wrote exactly the prepared cells, and nothing else moved.
        for (&[x, y], &old) in &before {
            let now = i64::from(height(&app, x, y));
            match prepared.get(&[x, y]) {
                Some(&(expect, value)) => {
                    assert_eq!((i64::from(old), now), (expect, value), "({x}, {y})");
                }
                None => assert_eq!(now, i64::from(old), "({x}, {y})"),
            }
        }
        acknowledge_ui(&mut app);
    }
    assert!(applied >= 5, "{applied}");
}

#[test]
fn cells_without_a_chunk_and_already_even_are_counted() {
    // A view over chunk 0_0 and the chunk 1_0 that does not exist; (31, 2)
    // stands at 6.
    let (_temporary, mut app) = smooth_application(|root| {
        heights(root, -8, 8, |x, y| if [x, y] == [31, 2] { 6 } else { 0 });
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([28, 0, 8, 6]);
        });
    });
    drag(&mut app, [31, 2], &[]);
    assert_eq!(
        app.status_label(),
        "Smoothing 9 cells, 4 to change, 2 already even, 3 without a chunk; release to review."
    );
    release(&mut app, [31, 2]).unwrap();
    assert_eq!(height(&app, 31, 2), 1);
}

#[test]
fn a_stroke_over_even_ground_says_why_and_prepares_nothing() {
    let (_temporary, mut app) = smooth_application(|root| heights(root, -4, 4, |_, _| 2));
    drag(&mut app, [3, 3], &[]);
    let label = app.status_label();
    assert_eq!(
        label,
        "Smoothing 9 cells refused: smooth changes no cell: 9 covered cells already even, 0 in map chunks that do not exist (painting terrain there creates them)"
    );
    let error = release(&mut app, [3, 3]).unwrap_err();
    assert!(label.ends_with(&error), "{error}");
    assert!(app.session.history().pending().is_none());
}

#[test]
fn a_running_smooth_stroke_is_recorded_in_the_frame_evidence() {
    let (temporary, mut app) = smooth_application(spike);
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_slice(
            &fs::read(temporary.path().join(format!("evidence/{name}.json"))).unwrap(),
        )
        .unwrap()
    };
    drag(&mut app, [2, 2], &[]);
    app.capture("during").unwrap();
    assert_eq!(
        read("during")["sculpt_preview"],
        json!({"kernel": 1, "cells": 9, "changes": 8, "even": 1, "missing": 0,
            "first": [[1, 1], [1, 2], [1, 3], [2, 1], [2, 2], [2, 3], [3, 1], [3, 2]]})
    );
    release(&mut app, [2, 2]).unwrap();
    app.capture("after").unwrap();
    assert!(read("after").get("sculpt_preview").is_none());
}

#[test]
fn changed_cells_this_view_does_not_draw_are_counted_not_marked_and_written() {
    // Top-down, range -8 to 8: (3, 0) at i32::MAX and (4, 0) at 100000 are
    // so far off that their tiles are not drawn; each reads as 8, the
    // range's end, and comes down to 3.
    let (temporary, mut app) = smooth_application(|root| {
        heights(root, -8, 8, |x, y| match [x, y] {
            [1, 0] => 5,
            [3, 0] => i64::from(i32::MAX),
            [4, 0] => 100_000,
            _ => 0,
        });
    });
    drag(&mut app, [2, 1], &[[3, 1]]);
    let changes = engine(&app);
    assert_eq!(changes.len(), 8);
    assert_eq!(
        marked(&mut app),
        changes
            .iter()
            .copied()
            .filter(|at| ![[3, 0], [4, 0]].contains(at))
            .collect()
    );
    assert_eq!(
        app.status_label(),
        "Smoothing 12 cells, 8 to change, 2 not drawn at this view, 4 already even; release to review."
    );
    app.capture("undrawn").unwrap();
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(temporary.path().join("evidence/undrawn.json")).unwrap())
            .unwrap();
    assert_eq!(
        record["sculpt_preview"]["undrawn"],
        json!({"count": 2, "first": [[3, 0], [4, 0]]})
    );
    release(&mut app, [3, 1]).unwrap();
    assert_eq!(pending(&app)[&[3, 0]], (i64::from(i32::MAX), 3));
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert_eq!(height(&app, 3, 0), 3);
    assert_eq!(height(&app, 4, 0), 3);
}

#[test]
fn the_persons_own_smooth_between_proposal_steps_is_allowed() {
    use super::proposal::{open, prepare_and_accept, state, twin};
    use crate::prototype::proposal_view::State;
    let grant = |root: &Path| heights(root, -64, 64, |x, _| x * 4);
    let mut twin = twin(2, grant, grant, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    let app = &mut twin.app;
    route(app, &key(Keycode::U)).unwrap();
    app.tools.set_sculpt_tool(SculptTool::Smooth);
    app.tools.set_brush(Brush::default());
    acknowledge_ui(app);
    // (7, 5) at 28 in the view's corner reads columns 5 to 7 (20, 24,
    // 28) of rows 3 to 5, weight 121: (-8 × 11 - 4 × 44) / 121 = -2.2
    // below it, so 26.
    drag(app, [7, 5], &[]);
    release(app, [7, 5]).unwrap();
    assert_eq!(state(app), State::OtherPending);
    acknowledge_ui(app);
    route(app, &key(Keycode::Return)).unwrap();
    assert_eq!(height(app, 7, 5), 26);
}

#[test]
fn a_raised_cliff_smooths_over_two_strokes_and_then_rests() {
    // docs/OFFLINE-PROTOTYPE.md, Sculpt mode: the sample with the range -8
    // to 8. Raise by 8 with a 7 x 7 brush, clicked on (7, 2): columns 4 to
    // 7 of the 8 x 6 view rise to 8. Then Smooth, the same brush dragged
    // from (3, 2) to (4, 2), which covers the whole view.
    let (_temporary, mut app) = sculpt_application(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["heights"] = json!({"min": -8, "max": 8});
        });
    });
    app.tools.set_brush(Brush::default());
    for _ in 0..3 {
        route(&mut app, &key(Keycode::RightBracket)).unwrap();
    }
    assert!(sidebar_texts(&mut app).contains(&"Brush: square 7x7".to_owned()));
    route(&mut app, &key(Keycode::_8)).unwrap();
    acknowledge_ui(&mut app);
    drag(&mut app, [7, 2], &[]);
    release(&mut app, [7, 2]).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    let rows = |app: &Application| -> Vec<Vec<i32>> {
        (0..6)
            .map(|y| (0..8).map(|x| height(app, x, y)).collect())
            .collect()
    };
    assert_eq!(rows(&app), vec![vec![0, 0, 0, 0, 8, 8, 8, 8]; 6]);
    for _ in 0..3 {
        route(&mut app, &key(Keycode::F)).unwrap();
    }
    assert_eq!(app.tools.sculpt_tool(), SculptTool::Smooth);
    for (status, after) in [
        (
            "Smoothing 48 cells, 12 to change, 36 already even; release to review.",
            [0, 0, 0, 2, 6, 8, 8, 8],
        ),
        (
            "Smoothing 48 cells, 24 to change, 24 already even; release to review.",
            [0, 0, 1, 3, 5, 7, 8, 8],
        ),
    ] {
        acknowledge_ui(&mut app);
        drag(&mut app, [3, 2], &[[4, 2]]);
        assert_eq!(app.status_label(), status);
        release(&mut app, [4, 2]).unwrap();
        acknowledge_ui(&mut app);
        route(&mut app, &key(Keycode::Return)).unwrap();
        assert_eq!(rows(&app), vec![after.to_vec(); 6]);
    }
    // At rest: a third stroke changes nothing.
    acknowledge_ui(&mut app);
    drag(&mut app, [3, 2], &[[4, 2]]);
    assert_eq!(
        app.status_label(),
        "Smoothing 48 cells refused: smooth changes no cell: 48 covered cells already even, 0 in map chunks that do not exist (painting terrain there creates them)"
    );
    assert!(release(&mut app, [4, 2]).is_err());
    assert_eq!(rows(&app), vec![vec![0, 0, 1, 3, 5, 7, 8, 8]; 6]);
}
