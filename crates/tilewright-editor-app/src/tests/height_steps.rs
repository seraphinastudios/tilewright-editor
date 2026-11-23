// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A proposal's height steps (`adjust_heights`, `flatten_heights`,
//! `smooth_heights`): their
//! words, which count the cells the engine's own plan would change in the
//! person's world, their map highlight, the step's cells, and the advice
//! when their result cannot be drawn.
use super::proposal::{edit_json, person_application};
use crate::prototype::proposal_view::{Highlight, action_words};
use serde_json::json;
use std::path::Path;
use tilewright_editor_ui::Action;

/// The range -4 to 4, and height 4 at (0, 0) and (1, 0); 0 elsewhere.
fn hills(root: &Path) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = json!({"min": -4, "max": 4});
    });
    let rows: Vec<Vec<i32>> = (0..32)
        .map(|y| {
            (0..32)
                .map(|x| if y == 0 && x < 2 { 4 } else { 0 })
                .collect()
        })
        .collect();
    edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}

#[test]
fn a_height_step_is_said_and_highlighted_by_its_cells() {
    let (_person, app) = person_application(hills);
    let expected = super::proposal::revision(&app.session);
    let adjust = |cells: Vec<[i64; 2]>, amount: i32| Action::AdjustHeights {
        plane: "main".into(),
        layer: 0,
        cells,
        amount,
        expected,
    };
    let words = |action: &Action| action_words(action, app.session.world(), app.session.catalog());
    assert_eq!(
        words(&adjust(vec![[0, 0], [1, 0], [2, 0]], 8)),
        "Raise 3 cells by 8, 1 to change (on main/0)"
    );
    assert_eq!(
        words(&adjust(vec![[0, 0], [1, 0]], 1)),
        "Raise 2 cells by 1, none to change (on main/0)"
    );
    assert_eq!(
        words(&adjust(vec![[0, 0], [1, 0], [2, 0]], -3)),
        "Lower 3 cells by 3, 3 to change (on main/0)"
    );
    assert_eq!(
        words(&adjust(vec![[0, 0]], 1)),
        "Raise 1 cell by 1, none to change (on main/0)"
    );
    // The engine would refuse: no count.
    assert_eq!(
        words(&adjust(vec![[1, 1], [900, 1]], -2)),
        "Lower 2 cells by 2, some outside the view (on main/0)"
    );
    let highlight = Highlight::of(0, &adjust(vec![[0, 0], [3, 2]], 2), &app.session);
    let tiles: Vec<(&str, [i64; 2])> = highlight
        .tiles
        .iter()
        .map(|tile| (tile.role.key(), tile.at))
        .collect();
    assert_eq!(tiles, [("cell", [0, 0]), ("cell", [3, 2])]);
}

#[test]
fn a_height_step_under_a_catalog_without_the_range_says_so() {
    let (_person, app) = person_application(|_| {});
    let expected = super::proposal::revision(&app.session);
    let action = Action::AdjustHeights {
        plane: "main".into(),
        layer: 0,
        cells: vec![[0, 0], [1, 0]],
        amount: 8,
        expected,
    };
    assert_eq!(
        action_words(&action, app.session.world(), app.session.catalog()),
        "Raise 2 cells by 8, not granted by this catalog (on main/0)"
    );
}

#[test]
fn a_flatten_step_is_said_and_highlighted_by_its_cells() {
    let (_person, app) = person_application(hills);
    let expected = super::proposal::revision(&app.session);
    let flatten = |cells: Vec<[i64; 2]>, target: i32| Action::FlattenHeights {
        plane: "main".into(),
        layer: 0,
        cells,
        target,
        expected,
    };
    let words = |action: &Action| action_words(action, app.session.world(), app.session.catalog());
    // (0, 0) and (1, 0) are at 4, the rest at 0.
    assert_eq!(
        words(&flatten(vec![[0, 0], [1, 0], [2, 0]], 4)),
        "Flatten 3 cells to 4, 1 to change (on main/0)"
    );
    assert_eq!(
        words(&flatten(vec![[0, 0], [1, 0], [2, 0]], -2)),
        "Flatten 3 cells to -2, 3 to change (on main/0)"
    );
    assert_eq!(
        words(&flatten(vec![[2, 0]], 0)),
        "Flatten 1 cell to 0, none to change (on main/0)"
    );
    // Outside the range -4 to 4: this catalog does not grant the target.
    assert_eq!(
        words(&flatten(vec![[2, 0]], 5)),
        "Flatten 1 cell to 5, not granted by this catalog (on main/0)"
    );
    // The engine would refuse for another reason: no count.
    assert_eq!(
        words(&flatten(vec![[1, 1], [900, 1]], 2)),
        "Flatten 2 cells to 2, some outside the view (on main/0)"
    );
    let highlight = Highlight::of(0, &flatten(vec![[0, 0], [3, 2]], 2), &app.session);
    let tiles: Vec<(&str, [i64; 2])> = highlight
        .tiles
        .iter()
        .map(|tile| (tile.role.key(), tile.at))
        .collect();
    assert_eq!(tiles, [("cell", [0, 0]), ("cell", [3, 2])]);
    // Without the range at all.
    let (_plain, app) = person_application(|_| {});
    assert_eq!(
        action_words(
            &flatten(vec![[0, 0], [1, 0]], 0),
            app.session.world(),
            app.session.catalog()
        ),
        "Flatten 2 cells to 0, not granted by this catalog (on main/0)"
    );
}

#[test]
fn a_smooth_step_is_said_and_highlighted_by_its_cells() {
    let (_person, app) = person_application(hills);
    let expected = super::proposal::revision(&app.session);
    let smooth = |cells: Vec<[i64; 2]>| Action::SmoothHeights {
        plane: "main".into(),
        layer: 0,
        cells,
        kernel: 1,
        expected,
    };
    let words = |action: &Action| action_words(action, app.session.world(), app.session.catalog());
    // (0, 0) and (1, 0) at 4 come down to 3 and 2, and (2, 0) beside
    // them goes up to 1; (5, 5) among zeros is already even.
    assert_eq!(
        words(&smooth(vec![[0, 0], [1, 0], [2, 0]])),
        "Smooth 3 cells, 3 to change (on main/0)"
    );
    assert_eq!(
        words(&smooth(vec![[5, 5]])),
        "Smooth 1 cell, none to change (on main/0)"
    );
    assert_eq!(
        words(&smooth(vec![[1, 1], [900, 1]])),
        "Smooth 2 cells, some outside the view (on main/0)"
    );
    let highlight = Highlight::of(0, &smooth(vec![[0, 0], [3, 2]]), &app.session);
    let tiles: Vec<(&str, [i64; 2])> = highlight
        .tiles
        .iter()
        .map(|tile| (tile.role.key(), tile.at))
        .collect();
    assert_eq!(tiles, [("cell", [0, 0]), ("cell", [3, 2])]);
    let (_plain, app) = person_application(|_| {});
    assert_eq!(
        action_words(
            &smooth(vec![[0, 0], [1, 0]]),
            app.session.world(),
            app.session.catalog()
        ),
        "Smooth 2 cells, not granted by this catalog (on main/0)"
    );
}

#[test]
fn the_advice_for_an_undrawable_smooth_is_to_zoom_out() {
    use crate::prototype::{undrawable_advice, undrawable_step_advice};
    let smooth = Action::SmoothHeights {
        plane: "main".into(),
        layer: 0,
        cells: vec![[0, 0]],
        kernel: 1,
        expected: [0; 32],
    };
    // A person's own smooth has no amount to lessen; a proposal step's
    // cells are the agent's.
    assert_eq!(undrawable_advice(Some(&smooth)), "Zoom out.");
    assert_eq!(
        undrawable_step_advice(Some(&smooth)),
        "Zoom out and prepare the step again."
    );
}

/// The neutral sample made isometric (32 × 16 diamond, its 32 × 32 art
/// unchanged), with the range -512 to 512 and, around tile (2, 2), the
/// reviewer's worst pattern for tall art half built: the "low" tiles at
/// -512, the "high" ones still at 0.
fn steep(root: &Path) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = json!({"min": -512, "max": 512});
    });
    edit_json(root, "profile/profile.jsonc", |profile| {
        profile["projection"] = json!({"kind": "isometric", "tile_width": 32, "tile_height": 16});
    });
    let rows: Vec<Vec<i32>> = (0..32_usize)
        .map(|y| {
            (0..32_usize)
                .map(|x| {
                    let low =
                        (1..4).contains(&x) && (1..4).contains(&y) && STEEP_HIGH[y - 1][x - 1] == 0;
                    if low { -512 } else { 0 }
                })
                .collect()
        })
        .collect();
    edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}
/// 1 where the pattern is high (rows are y = 1..3, columns x = 1..3).
const STEEP_HIGH: [[u8; 3]; 3] = [[1, 1, 0], [1, 0, 0], [0, 0, 1]];

#[test]
fn a_change_that_cannot_be_drawn_at_this_zoom_is_cancelled_not_blocking() {
    let (_person, mut app) = person_application(steep);
    let high: Vec<[i64; 2]> = (1..4_i64)
        .flat_map(|x| (1..4_i64).map(move |y| [x, y]))
        .filter(|&[x, y]| {
            STEEP_HIGH[usize::try_from(y - 1).unwrap()][usize::try_from(x - 1).unwrap()] == 1
        })
        .collect();
    let raise = |app: &crate::prototype::Application| Action::AdjustHeights {
        plane: "main".into(),
        layer: 0,
        cells: high.clone(),
        amount: 512,
        expected: super::proposal::revision(&app.session),
    };
    // Drawable at zoom 8 before the change.
    app.view.camera.zoom = 8.0;
    app.rebuild().unwrap();
    let error = app.action(raise(&app)).unwrap_err();
    // The neutral first sentence, the height advice, the renderer's reason.
    let notice = format!(
        "{} Zoom out, or raise or lower by less. Renderer: invalid world view",
        crate::prototype::UNDRAWABLE_CHANGE
    );
    assert_eq!(error, notice);
    assert_eq!(app.notice.as_deref(), Some(notice.as_str()));
    // The refusal is what the status line and details show, not the
    // "view can be drawn again" note of a pause ending.
    assert!(app.view_note.is_none());
    assert_eq!(app.status_label(), "Input not applied. Details below.");
    // Rows may break inside a word: compare without white space.
    let bare = |text: &str| text.split_whitespace().collect::<String>();
    let details = bare(&app.review_lines(620).join(" "));
    assert!(details.contains(&bare(&notice)), "{details}");
    assert!(!details.contains(&bare("can be drawn again")), "{details}");
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_none());
    // Zoomed out, the same change is drawn, pending for review.
    app.view.camera.zoom = 1.0;
    app.rebuild().unwrap();
    app.action(raise(&app)).unwrap();
    assert!(app.session.history().pending().is_some());
    assert!(app.blocked.is_none());
}

#[test]
fn a_flatten_that_cannot_be_drawn_at_this_zoom_says_to_flatten_nearer() {
    let (_person, mut app) = person_application(steep);
    // The high tiles of the half-built pattern flattened to 512, beside
    // the low ones at -512.
    let high: Vec<[i64; 2]> = (1..4_i64)
        .flat_map(|x| (1..4_i64).map(move |y| [x, y]))
        .filter(|&[x, y]| {
            STEEP_HIGH[usize::try_from(y - 1).unwrap()][usize::try_from(x - 1).unwrap()] == 1
        })
        .collect();
    let flatten = |app: &crate::prototype::Application| Action::FlattenHeights {
        plane: "main".into(),
        layer: 0,
        cells: high.clone(),
        target: 512,
        expected: super::proposal::revision(&app.session),
    };
    app.view.camera.zoom = 8.0;
    app.rebuild().unwrap();
    let error = app.action(flatten(&app)).unwrap_err();
    let notice = format!(
        "{} Zoom out, or flatten to a height nearer the tiles around it. Renderer: invalid world view",
        crate::prototype::UNDRAWABLE_CHANGE
    );
    assert_eq!(error, notice);
    assert_eq!(app.notice.as_deref(), Some(notice.as_str()));
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_none());
    app.view.camera.zoom = 1.0;
    app.rebuild().unwrap();
    app.action(flatten(&app)).unwrap();
    assert!(app.session.history().pending().is_some());
}

#[test]
fn another_change_that_cannot_be_drawn_gets_no_height_advice() {
    // Grass painted onto the empty tile (6, 4), whose ground then bends
    // over the reviewer's ±512 pattern: a Paint, not a height change.
    let (_person, mut app) = person_application(|root| {
        edit_json(root, "profile/profile.jsonc", |profile| {
            profile["projection"] =
                json!({"kind": "isometric", "tile_width": 32, "tile_height": 16});
        });
        let pattern = [[1, 1, -1], [1, -1, -1], [-1, -1, 1]];
        let rows: Vec<Vec<i32>> = (0..32_usize)
            .map(|y| {
                (0..32_usize)
                    .map(|x| {
                        if (5..8).contains(&x) && (3..6).contains(&y) {
                            pattern[y - 3][x - 5] * 512
                        } else {
                            0
                        }
                    })
                    .collect()
            })
            .collect();
        edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
            chunk["terrain"]["height"] = json!({ "rows": rows });
        });
    });
    app.view.camera.zoom = 8.0;
    app.rebuild().unwrap();
    let paint = Action::Paint {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x: 6,
        y: 4,
        proto: "grass".into(),
        expected: super::proposal::revision(&app.session),
    };
    let error = app.action(paint).unwrap_err();
    let notice = format!(
        "{} Zoom out or pan and try again. Renderer: invalid world view",
        crate::prototype::UNDRAWABLE_CHANGE
    );
    assert_eq!(error, notice);
    assert_eq!(app.notice.as_deref(), Some(notice.as_str()));
    assert_eq!(app.status_label(), "Input not applied. Details below.");
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_none());
}

/// A proposal of one `action(revision)` recorded on a copy of the steep
/// world, prepared in the person's steep copy at zoom 8: cancelled with
/// advice the person can follow, which for a height step is only to zoom
/// out (its amount or target is the agent's).
fn an_undrawable_height_step_says_only_to_zoom_out(action: impl FnOnce([u8; 32]) -> Action) {
    use super::proposal::{AGENT, RECEIPT, accept, open_session, revision, scratch};
    let agent = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent.path().join("world");
    super::copy_private(
        &tilewright_editor_testkit::fixture_world("prototype-neutral"),
        &world,
    );
    steep(&world);
    let mut session = open_session(&world, AGENT);
    let step = action(revision(&session));
    accept(&mut session, step);
    let proposal = tilewright_editor_proposal::Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
    let (_person, mut app) = person_application(steep);
    app.view.camera.zoom = 8.0;
    app.rebuild().unwrap();
    let name = proposal.file_name();
    app.open_proposal(Some((proposal, name))).unwrap();
    super::acknowledge_ui(&mut app);
    let error = app.prepare_proposal_step().unwrap_err();
    assert_eq!(
        error,
        format!(
            "{} Zoom out and prepare the step again. Renderer: invalid world view",
            crate::prototype::UNDRAWABLE_CHANGE
        )
    );
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_none());
}

/// The high tiles of the half-built steep pattern.
fn steep_high() -> Vec<[i64; 2]> {
    (1..4_i64)
        .flat_map(|x| (1..4_i64).map(move |y| [x, y]))
        .filter(|&[x, y]| {
            STEEP_HIGH[usize::try_from(y - 1).unwrap()][usize::try_from(x - 1).unwrap()] == 1
        })
        .collect()
}

#[test]
fn an_undrawable_raise_step_says_only_to_zoom_out() {
    an_undrawable_height_step_says_only_to_zoom_out(|expected| Action::AdjustHeights {
        plane: "main".into(),
        layer: 0,
        cells: steep_high(),
        amount: 512,
        expected,
    });
}

#[test]
fn an_undrawable_flatten_step_says_only_to_zoom_out() {
    an_undrawable_height_step_says_only_to_zoom_out(|expected| Action::FlattenHeights {
        plane: "main".into(),
        layer: 0,
        cells: steep_high(),
        target: 512,
        expected,
    });
}
