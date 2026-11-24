// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Grade road through real SDL pointer and key events, on the CPU scene
//! path: after a road's change is accepted, L or E (or the Terrain
//! sidebar's rows) prepares the existing `flatten_heights` or
//! `smooth_heights` over exactly the cells the road's change recorded, for
//! a second Review and a second Undo step; what drops the remembered road,
//! why the rows are disabled, and the road fence. Every world is a
//! temporary copy of the MIT neutral sample with a height grant; the
//! committed sample has none.
use super::acknowledge_ui;
use super::road::{Lines, click, points, press, road_application, with_speckle};
use super::sculpt_mode::{height, heights, steep_at, wide_isometric};
use super::zones::{button, route};
use crate::prototype::{Application, GRADE_NO_ROAD, ROAD_IN_PROGRESS};
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tilewright_editor_ui::Action;
use tilewright_editor_ui::brush::{Brush, BrushShape};
use tilewright_editor_ui::road::{GRADE_UNGRANTED, RoadGrade};
use tilewright_editor_ui::tools::{TerrainRule, ToolMode};

/// The range -8 to 8 over uneven heights (-2 to 2), so a flatten and a
/// smooth of most roads change something.
fn bumpy(root: &Path) {
    heights(root, -8, 8, |x, y| (x * 7 + y * 3).rem_euclid(5) - 2);
}

/// Every file of the temporary World, by path.
fn world_files(temporary: &tempfile::TempDir) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let root = temporary.path().join("world");
    let mut files = BTreeMap::new();
    walk(&root, &root, &mut files);
    files
}

/// The action of the last applied step.
fn last_step(app: &Application) -> Action {
    app.session
        .applied_steps()
        .last()
        .and_then(|step| step.origin)
        .map(|origin| origin.action().clone())
        .unwrap()
}

/// Click `points`, Enter to review and Enter to accept: the road's
/// recorded paint or scatter action.
fn accept_road(app: &mut Application, points: &[[i64; 2]]) -> Action {
    for &at in points {
        click(app, at).unwrap();
    }
    press(app, Keycode::Return).unwrap();
    assert!(app.session.history().pending().is_some());
    press(app, Keycode::Return).unwrap();
    assert!(app.session.history().pending().is_none());
    last_step(app)
}

fn recorded_cells(action: &Action) -> Vec<[i64; 2]> {
    match action {
        Action::PaintStroke { cells, .. } | Action::ScatterTerrain { cells, .. } => cells.clone(),
        other => panic!("not a road's action: {other:?}"),
    }
}

/// The road's grade rows, scrolled into view: the line above them and
/// whether each of Flatten and Smooth is enabled.
fn grade_rows(app: &mut Application) -> (String, bool, bool) {
    app.sidebar_scroll = usize::MAX;
    app.refresh_ui();
    let widgets = &app.ui.widgets;
    let at = widgets
        .iter()
        .position(|widget| widget.control == Some(Control::GradeRoad(RoadGrade::Flatten)))
        .expect("the Flatten row is laid out at the end of the sidebar");
    assert_eq!(widgets[at].text, "Grade road: Flatten  L");
    assert_eq!(
        widgets[at + 1].control,
        Some(Control::GradeRoad(RoadGrade::Smooth))
    );
    assert_eq!(widgets[at + 1].text, "Grade road: Smooth  E");
    let rows = (
        widgets[at - 1].text.clone(),
        widgets[at].enabled,
        widgets[at + 1].enabled,
    );
    app.sidebar_scroll = 0;
    app.refresh_ui();
    rows
}

fn remembered(app: &Application) -> Option<Vec<[i64; 2]>> {
    app.graded_road().map(|road| road.cells().to_vec())
}

const GRADE_KEYS: [(Keycode, RoadGrade); 2] = [
    (Keycode::L, RoadGrade::Flatten),
    (Keycode::E, RoadGrade::Smooth),
];

/// For random roads (shapes, widths, square and disc, some clipped by the
/// view's edge), painting and scattering, top-down or isometric: the grade
/// covers exactly the road's recorded cells; it is the very action a
/// direct `flatten_heights` (to the committed height at the first point)
/// or `smooth_heights` over those cells is, byte for byte in proposal
/// form, and writes the same World; two Undo steps put the World back
/// byte for byte.
#[allow(clippy::too_many_lines)]
fn grades_exactly_the_roads_cells(prepare: fn(&Path)) {
    let (temporary, mut app) = road_application(5, |root| {
        bumpy(root);
        with_speckle(root);
        prepare(root);
    });
    let mut lines = Lines(0x9ade);
    let mut graded = BTreeMap::new();
    let mut clipped = 0;
    for case in 0..14 {
        if case % 3 == 2 {
            app.control(Control::ScatterPalette("speckle".into()))
                .unwrap();
        } else {
            app.control(Control::Palette {
                grid: "floor".into(),
                prototype: "sand".into(),
            })
            .unwrap();
        }
        let brush = Brush {
            shape: if lines.next(2) == 0 {
                BrushShape::Square
            } else {
                BrushShape::Circle
            },
            radius: u16::try_from(lines.next(4)).unwrap(),
        };
        app.tools.set_brush(brush);
        let mut clicked = Vec::new();
        while clicked.len() < usize::try_from(2 + lines.next(3)).unwrap() {
            let at = [lines.next(8), lines.next(6)];
            if clicked.last() != Some(&at) {
                clicked.push(at);
            }
        }
        let original = world_files(&temporary);
        // A road that changes nothing is not accepted; draw the next.
        for &at in &clicked {
            click(&mut app, at).unwrap();
        }
        if press(&mut app, Keycode::Return).is_err() {
            press(&mut app, Keycode::Escape).unwrap();
            continue;
        }
        press(&mut app, Keycode::Return).unwrap();
        let road = last_step(&app);
        let cells = recorded_cells(&road);
        // Clipped: the brush reaches past the view's edge somewhere.
        let radius = i64::from(brush.radius);
        if clicked
            .iter()
            .any(|&[x, y]| x - radius < 0 || y - radius < 0 || x + radius > 7 || y + radius > 5)
        {
            clipped += 1;
        }
        assert_eq!(
            remembered(&app).as_deref(),
            Some(cells.as_slice()),
            "case {case}"
        );
        let built = world_files(&temporary);
        let (keycode, grade) = GRADE_KEYS[case % 2];
        let revision = app.session.history().document().revision();
        let direct = match grade {
            RoadGrade::Flatten => Action::FlattenHeights {
                plane: "main".into(),
                layer: 0,
                cells: cells.clone(),
                target: height(&app, clicked[0][0], clicked[0][1]),
                expected: revision,
            },
            RoadGrade::Smooth => Action::SmoothHeights {
                plane: "main".into(),
                layer: 0,
                cells: cells.clone(),
                kernel: 1,
                expected: revision,
            },
        };
        let (label, flatten, smooth) = grade_rows(&mut app);
        assert_eq!(
            label,
            format!(
                "Road to grade: {} cells from ({}, {})",
                cells.len(),
                clicked[0][0],
                clicked[0][1]
            )
        );
        assert!(flatten && smooth);
        if let Err(refused) = press(&mut app, keycode) {
            // Only the engine's own "nothing to change" refuses here.
            assert!(
                refused.contains("changes no cell"),
                "case {case}: {refused}"
            );
            assert!(app.session.history().pending().is_none());
            assert_eq!(remembered(&app).as_deref(), Some(cells.as_slice()));
            press(&mut app, Keycode::Z).unwrap();
            press(&mut app, Keycode::Return).unwrap();
            assert_eq!(world_files(&temporary), original, "case {case}");
            continue;
        }
        assert!(app.session.history().pending().is_some());
        assert_eq!(
            app.roads.pending().map(|road| road.cells().to_vec()),
            Some(cells.clone())
        );
        press(&mut app, Keycode::Return).unwrap();
        *graded.entry(grade).or_insert(0) += 1;
        let step = last_step(&app);
        assert_eq!(step, direct, "case {case}");
        assert_eq!(
            tilewright_editor_proposal::encode_action(&step)
                .unwrap()
                .to_string(),
            tilewright_editor_proposal::encode_action(&direct)
                .unwrap()
                .to_string()
        );
        let written = world_files(&temporary);
        assert_ne!(written, built);
        // Grading keeps the road: its cells are still the road's.
        assert_eq!(remembered(&app).as_deref(), Some(cells.as_slice()));
        // Two Undo steps: the grade, then the road.
        press(&mut app, Keycode::Z).unwrap();
        press(&mut app, Keycode::Return).unwrap();
        assert_eq!(world_files(&temporary), built, "case {case}");
        assert!(remembered(&app).is_none());
        press(&mut app, Keycode::Z).unwrap();
        press(&mut app, Keycode::Return).unwrap();
        assert_eq!(world_files(&temporary), original, "case {case}");
        // The same two edits dispatched directly write the same World.
        app.action(road.clone()).unwrap();
        acknowledge_ui(&mut app);
        press(&mut app, Keycode::Return).unwrap();
        assert_eq!(world_files(&temporary), built, "case {case}");
        app.action(direct).unwrap();
        acknowledge_ui(&mut app);
        press(&mut app, Keycode::Return).unwrap();
        assert_eq!(world_files(&temporary), written, "case {case}");
        for _ in 0..2 {
            press(&mut app, Keycode::Z).unwrap();
            press(&mut app, Keycode::Return).unwrap();
        }
        assert_eq!(world_files(&temporary), original, "case {case}");
        assert!(app.tools.road().is_none());
    }
    assert!(
        graded.values().all(|&count| count >= 3) && graded.len() == 2,
        "{graded:?}"
    );
    assert!(clipped >= 3, "{clipped} clipped roads");
}

#[test]
fn a_top_down_grade_covers_exactly_the_roads_cells() {
    grades_exactly_the_roads_cells(|_| {});
}

#[test]
fn an_isometric_grade_covers_exactly_the_roads_cells() {
    grades_exactly_the_roads_cells(wide_isometric);
}

#[test]
fn the_flatten_target_is_the_first_points_committed_height() {
    let (_temporary, mut app) = road_application(1, bumpy);
    app.tools.set_brush(Brush::default());
    // (6, 2), the first point, has height (42 + 6) mod 5 - 2 = 1; the
    // last point (0, 4), first in the cells' order, has 0.
    let road = accept_road(&mut app, &[[6, 2], [0, 4]]);
    assert_eq!(height(&app, 6, 2), 1);
    press(&mut app, Keycode::L).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    let Action::FlattenHeights { target, cells, .. } = last_step(&app) else {
        panic!("a flatten");
    };
    assert_eq!(target, 1);
    assert_eq!(cells, recorded_cells(&road));
    for &[x, y] in &cells {
        assert_eq!(height(&app, x, y), 1);
    }
    // A first point outside the granted range refuses the flatten in
    // words about the road; smooth still works.
    let (_temporary, mut app) = road_application(1, |root| {
        heights(root, -8, 8, |x, y| if [x, y] == [1, 1] { 20 } else { 0 });
    });
    accept_road(&mut app, &[[1, 1], [4, 1]]);
    let refused = press(&mut app, Keycode::L).unwrap_err();
    assert_eq!(
        refused,
        "Grade road refused: the road's first point (1, 1) has height 20, outside the granted range -8 to 8; a flatten target must lie inside it."
    );
    assert!(app.session.history().pending().is_none());
    press(&mut app, Keycode::E).unwrap();
    assert!(app.session.history().pending().is_some());
}

#[test]
fn the_rows_and_keys_say_why_grading_is_unavailable() {
    // Without the height grant (the sample as committed).
    let (_temporary, mut app) = road_application(1, |_| {});
    accept_road(&mut app, &[[0, 1], [3, 1]]);
    assert_eq!(
        grade_rows(&mut app),
        ("Grading needs editing.heights".into(), false, false)
    );
    for (keycode, grade) in GRADE_KEYS {
        assert_eq!(press(&mut app, keycode).unwrap_err(), GRADE_UNGRANTED);
        assert_eq!(
            app.control(Control::GradeRoad(grade)).unwrap_err(),
            GRADE_UNGRANTED
        );
    }
    assert!(app.session.history().pending().is_none());
    // With it, before any road.
    let (_temporary, mut app) = road_application(1, bumpy);
    assert_eq!(
        grade_rows(&mut app),
        ("Grading: accept a road first".into(), false, false)
    );
    assert_eq!(press(&mut app, Keycode::L).unwrap_err(), GRADE_NO_ROAD);
    // A road drawn but pending is not yet a road to grade.
    click(&mut app, [0, 1]).unwrap();
    click(&mut app, [3, 1]).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(press(&mut app, Keycode::E).unwrap_err(), GRADE_NO_ROAD);
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(
        grade_rows(&mut app),
        ("Road to grade: 4 cells from (0, 1)".into(), true, true)
    );
    // A pending change (the grade's own here).
    press(&mut app, Keycode::L).unwrap();
    assert_eq!(
        grade_rows(&mut app),
        ("Grading: a change is pending".into(), false, false)
    );
    assert_eq!(
        press(&mut app, Keycode::E).unwrap_err(),
        "Accept or cancel the pending edit first"
    );
    // A press on a disabled row does nothing.
    app.sidebar_scroll = usize::MAX;
    app.refresh_ui();
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::GradeRoad(RoadGrade::Smooth)))
        .cloned()
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    let at = [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ];
    acknowledge_ui(&mut app);
    let pending = app.session.history().pending().map(|p| p.changed().count());
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    assert_eq!(
        app.session.history().pending().map(|p| p.changed().count()),
        pending
    );
    app.sidebar_scroll = 0;
    press(&mut app, Keycode::Escape).unwrap();
    // A road being drawn: the fence lays them out disabled and refuses
    // them, keys and controls alike, and the road stays.
    click(&mut app, [5, 4]).unwrap();
    click(&mut app, [6, 4]).unwrap();
    let (_, flatten, smooth) = grade_rows(&mut app);
    assert!(!flatten && !smooth);
    for (keycode, grade) in GRADE_KEYS {
        assert_eq!(press(&mut app, keycode).unwrap_err(), ROAD_IN_PROGRESS);
        assert_eq!(
            app.control(Control::GradeRoad(grade)).unwrap_err(),
            ROAD_IN_PROGRESS
        );
    }
    assert_eq!(points(&app), [[5, 4], [6, 4]]);
    assert!(app.session.history().pending().is_none());
    // Dropping that road keeps the remembered one.
    press(&mut app, Keycode::Escape).unwrap();
    assert_eq!(grade_rows(&mut app).0, "Road to grade: 4 cells from (0, 1)");
}

#[test]
fn grading_is_refused_in_the_pauses_words() {
    // Tall tiles at (1, 1) to (3, 3); the road runs along row 5.
    let (_temporary, mut app) = road_application(1, |root| steep_at(root, [1, 1]));
    accept_road(&mut app, &[[0, 5], [7, 5]]);
    app.view.camera.zoom = 8.0;
    assert_eq!(app.rebuild().unwrap_err(), "invalid world view");
    acknowledge_ui(&mut app);
    assert_eq!(
        grade_rows(&mut app),
        ("Grading: editing is paused".into(), false, false)
    );
    assert_eq!(
        press(&mut app, Keycode::L).unwrap_err(),
        "rendering blocked editing: invalid world view"
    );
    assert!(app.session.history().pending().is_none());
    app.view.camera.zoom = 1.0;
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    press(&mut app, Keycode::E).unwrap();
    assert!(app.session.history().pending().is_some());
}

#[test]
fn whatever_makes_the_cells_stale_drops_the_road_and_nothing_else_does() {
    let (_temporary, mut app) = road_application(1, bumpy);
    app.tools.set_brush(Brush::default());
    let first = [[0, 1], [3, 1]];
    let cells = recorded_cells(&accept_road(&mut app, &first));
    // Leaving Terrain mode and coming back keeps it; L and E do nothing
    // outside Terrain mode.
    press(&mut app, Keycode::U).unwrap();
    assert_eq!(app.tools.mode(), ToolMode::Sculpt);
    press(&mut app, Keycode::L).unwrap();
    press(&mut app, Keycode::E).unwrap();
    assert!(app.session.history().pending().is_none());
    press(&mut app, Keycode::T).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Road);
    assert_eq!(remembered(&app), Some(cells.clone()));
    // Pans and zooms keep it.
    press(&mut app, Keycode::D).unwrap();
    press(&mut app, Keycode::Minus).unwrap();
    assert_eq!(remembered(&app), Some(cells.clone()));
    // A grade cancelled, and one accepted, keep it.
    press(&mut app, Keycode::E).unwrap();
    press(&mut app, Keycode::Escape).unwrap();
    assert_eq!(remembered(&app), Some(cells.clone()));
    press(&mut app, Keycode::E).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(remembered(&app), Some(cells.clone()));
    // An Undo prepared and cancelled keeps it; accepted it drops it, and
    // a Redo does not bring it back.
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Escape).unwrap();
    assert_eq!(remembered(&app), Some(cells.clone()));
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert!(remembered(&app).is_none());
    press(&mut app, Keycode::Y).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert!(remembered(&app).is_none());
    assert_eq!(press(&mut app, Keycode::E).unwrap_err(), GRADE_NO_ROAD);
    // Undo of the road itself.
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    let cells = recorded_cells(&accept_road(&mut app, &first));
    assert_eq!(remembered(&app), Some(cells.clone()));
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert!(remembered(&app).is_none());
    // Another accepted edit: a brush stroke.
    accept_road(&mut app, &first);
    press(&mut app, Keycode::F).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "grass".into(),
    })
    .unwrap();
    acknowledge_ui(&mut app);
    let pixel = super::cell_pixel(&mut app, 6, 4);
    route(&mut app, &button(true, pixel)).unwrap();
    route(&mut app, &button(false, pixel)).unwrap();
    assert!(app.session.history().pending().is_some());
    // While it is pending the road is still remembered (Cancel would
    // keep it); its Accept drops it.
    assert!(remembered(&app).is_some());
    press(&mut app, Keycode::Return).unwrap();
    assert!(remembered(&app).is_none());
    // Reopen.
    for _ in 0..3 {
        press(&mut app, Keycode::F).unwrap();
    }
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Road);
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    accept_road(&mut app, &[[0, 3], [4, 3]]);
    assert!(remembered(&app).is_some());
    press(&mut app, Keycode::R).unwrap();
    assert!(remembered(&app).is_none());
    assert_eq!(press(&mut app, Keycode::L).unwrap_err(), GRADE_NO_ROAD);
}

#[test]
fn the_guides_hand_test_reads_as_written() {
    // docs/OFFLINE-PROTOTYPE.md, "Grading a road": the sample with the
    // range -8 to 8 and the heights x - y.
    let (_temporary, mut app) = road_application(1, |root| heights(root, -8, 8, |x, y| x - y));
    press(&mut app, Keycode::RightBracket).unwrap();
    for at in [[0, 1], [5, 1]] {
        click(&mut app, at).unwrap();
    }
    press(&mut app, Keycode::Return).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(
        grade_rows(&mut app).0,
        "Road to grade: 21 cells from (0, 1)"
    );
    press(&mut app, Keycode::L).unwrap();
    assert!(app.session.history().pending().is_some());
    press(&mut app, Keycode::Return).unwrap();
    for x in 0..7 {
        for y in 0..3 {
            assert_eq!(height(&app, x, y), -1, "({x}, {y})");
        }
    }
    assert_eq!(height(&app, 6, 2), -1);
    assert_eq!(height(&app, 7, 2), 5);
    // Undo twice: the heights, then the road.
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(height(&app, 2, 1), 1);
    assert_eq!(super::terrain_value(&app, 2, 1).as_deref(), Some("sand"));
    press(&mut app, Keycode::Z).unwrap();
    press(&mut app, Keycode::Return).unwrap();
    assert_eq!(super::terrain_value(&app, 2, 1).as_deref(), Some("grass"));
    assert_eq!(grade_rows(&mut app).0, "Grading: accept a road first");
}
