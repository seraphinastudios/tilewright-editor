// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A proposal's height step (`adjust_heights`): its words, which count the
//! cells the engine's own plan would change in the person's world, and its
//! map highlight, the step's cells. There is no sculpt mode in the desktop
//! yet; only recorded steps are shown.
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
