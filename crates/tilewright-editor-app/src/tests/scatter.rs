// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Terrain mode's scatter palettes through real SDL pointer events, on the
//! CPU scene path: the palette rows (only with `editing.palettes`), a fresh
//! seed per stroke from the session's random source, a live preview that
//! is exactly the release's change, and a proposal step's words and map
//! highlight.
use super::proposal::{edit_json, person_application};
use super::zones::sidebar_texts;
use super::{BRUSH_WINDOW, acknowledge_ui, application_with_source, cell_pixel, terrain_value};
use crate::prototype::Application;
use crate::prototype::proposal_view::{Highlight, action_words};
use crate::workspace_ui::{Control, WorkspaceUi};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::brush::{Brush, BrushShape, StrokeTarget};
use tilewright_editor_ui::uid::{SeededUidSource, UidSource};

fn speckle() -> Value {
    json!([{"name": "speckle", "grid": "floor", "entries": [
        {"value": "grass", "weight": 3}, {"value": "sand", "weight": 1}]}])
}
fn with_speckle(root: &Path) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["palettes"] = speckle();
    });
}
/// Remove the neutral sample's own `editing.palettes`.
fn without_palettes(root: &Path) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]
            .as_object_mut()
            .unwrap()
            .remove("palettes");
    });
}
/// The 1280x800 brush application on a catalog with `speckle`, in Terrain
/// mode, its session's random source seeded with `seed`.
fn scatter_application(seed: u64) -> (tempfile::TempDir, Application) {
    terrain_application(seed, with_speckle)
}
/// The 1280x800 application on the fixture changed by `prepare`, in
/// Terrain mode, its session's random source seeded with `seed`.
fn terrain_application(seed: u64, prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = application_with_source(prepare);
    app.ui = WorkspaceUi::new(BRUSH_WINDOW, BRUSH_WINDOW);
    app.view.camera.width = BRUSH_WINDOW.0;
    app.view.camera.height = BRUSH_WINDOW.1;
    app.session
        .set_uid_source(Box::new(SeededUidSource::new(seed)));
    app.rebuild().unwrap();
    app.control(Control::Terrain).unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}
/// The first seed `SeededUidSource::new(seed)` gives, as the session draws
/// it: the first four bytes of its first 16, big-endian.
fn first_seed(seed: u64) -> u32 {
    let mut bytes = [0; 16];
    SeededUidSource::new(seed).fill(&mut bytes).unwrap();
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
/// The cells the stroke preview marks, read back through the cell picks.
fn marked(app: &mut Application) -> Vec<[i64; 2]> {
    app.refresh_ui();
    let mut cells: Vec<[i64; 2]> = crate::prototype::stroke_overlay(app)
        .iter()
        .map(
            |quad| match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a preview copy of no cell"),
            },
        )
        .collect();
    cells.sort_unstable();
    cells.dedup();
    cells
}

#[test]
fn the_palette_rows_appear_only_with_editing_palettes() {
    let (_plain, mut plain) = terrain_application(1, without_palettes);
    let texts = sidebar_texts(&mut plain);
    // The rules are laid out, so palette rows would be too.
    assert!(
        texts.iter().any(|text| text.starts_with("Brush: drag")),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("Scatter")),
        "{texts:?}"
    );
    let (_temporary, mut app) = scatter_application(1);
    let texts = sidebar_texts(&mut app);
    let header = texts.iter().position(|text| text == "Scatter palettes");
    let row = texts
        .iter()
        .position(|text| text == "floor / speckle: grass 3, sand 1");
    assert!(
        header.is_some() && row == header.map(|at| at + 1),
        "{texts:?}"
    );
    // The rows follow the terrain palette and come before the rules.
    let terrain = texts
        .iter()
        .position(|text| text == "floor / sand")
        .unwrap();
    let rules = texts
        .iter()
        .position(|text| text.starts_with("Brush: drag"))
        .unwrap();
    assert!(terrain < header.unwrap() && row.unwrap() < rules);
    // Choosing the palette replaces the terrain choice, and back.
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    assert_eq!(app.tools.scatter_choice(), Some("speckle"));
    assert!(app.tools.terrain_choice().is_none());
    assert!(
        app.control(Control::ScatterPalette("nothing".into()))
            .is_err()
    );
    assert_eq!(app.tools.scatter_choice(), Some("speckle"));
}

#[test]
fn a_scatter_drag_previews_exactly_the_change_its_release_prepares() {
    let (temporary, mut app) = scatter_application(21);
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    app.tools.set_brush(Brush {
        shape: BrushShape::Square,
        radius: 1,
    });
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 2, 2);
    let end = cell_pixel(&mut app, 5, 2);
    crate::prototype::native_pointer(&mut app, start, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    let seed = first_seed(21);
    assert_eq!(
        app.tools.stroke().unwrap().target(),
        &StrokeTarget::Scatter {
            palette: "speckle".into(),
            seed
        }
    );
    crate::prototype::native_stroke_motion(&mut app, end, true, BRUSH_WINDOW, BRUSH_WINDOW)
        .unwrap();
    let covered: Vec<[i64; 2]> = app.tools.stroke().unwrap().cells().collect();
    assert_eq!(covered.len(), 18);
    let preview = marked(&mut app);
    let label = app.status_label();
    assert!(
        label.starts_with(&format!(
            "Painting 18 cells from palette speckle (seed {seed}), {} to change; release to review.",
            preview.len()
        )),
        "{label}"
    );
    // The preview is the engine's own answer for the stroke.
    let mut engine = app
        .session
        .stroke_changes(app.tools.stroke().unwrap())
        .unwrap();
    engine.sort_unstable();
    assert_eq!(preview, engine);
    assert!(!preview.is_empty());
    let committed: Vec<Option<String>> = covered
        .iter()
        .map(|&[x, y]| terrain_value(&app, x, y))
        .collect();
    crate::prototype::native_stroke_release(&mut app, end, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert!(app.session.history().pending().is_some());
    assert_eq!(fs::read(&chunk).unwrap(), original);
    // Exactly the previewed cells changed, each to its drawn value.
    let palette = &app.session.catalog().editing().palettes["speckle"];
    for (&[x, y], before) in covered.iter().zip(&committed) {
        let drawn = tilewright_editor_ui::scatter::value(palette, seed, 0, [x, y]).unwrap();
        let now = terrain_value(&app, x, y);
        assert_eq!(now.as_deref(), Some(drawn), "({x}, {y})");
        assert_eq!(
            preview.contains(&[x, y]),
            before.as_deref() != Some(drawn),
            "({x}, {y})"
        );
    }
    app.action(Action::Accept).unwrap();
    assert_eq!(app.recovery.records().len(), 1);
}

#[test]
fn every_stroke_draws_a_fresh_seed_and_other_strokes_draw_none() {
    let (_temporary, mut app) = scatter_application(5);
    let mut source = SeededUidSource::new(5);
    let mut next = || {
        let mut bytes = [0; 16];
        source.fill(&mut bytes).unwrap();
        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    };
    // A terrain stroke draws nothing from the source.
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    acknowledge_ui(&mut app);
    let at = cell_pixel(&mut app, 1, 1);
    crate::prototype::native_pointer(&mut app, at, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    crate::prototype::native_stroke_release(&mut app, at, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    app.action(Action::Cancel).unwrap();
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    let mut seeds = Vec::new();
    for x in [0, 1, 2] {
        acknowledge_ui(&mut app);
        let at = cell_pixel(&mut app, x, 0);
        crate::prototype::native_pointer(&mut app, at, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
        let Some(StrokeTarget::Scatter { seed, .. }) =
            app.tools.stroke().map(|stroke| stroke.target().clone())
        else {
            panic!("not a scatter stroke");
        };
        seeds.push(seed);
        app.tools.cancel_stroke();
        app.stroke_mask = None;
    }
    assert_eq!(seeds, [next(), next(), next()]);
}

#[test]
fn a_scatter_step_is_said_and_highlighted_by_its_cells() {
    let (_person, app) = person_application(with_speckle);
    let expected = super::proposal::revision(&app.session);
    let scatter = |palette: &str, cells: Vec<[i64; 2]>| Action::ScatterTerrain {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        cells,
        palette: palette.into(),
        seed: 1234,
        expected,
    };
    let words = |action: &Action| action_words(action, app.session.world(), app.session.catalog());
    assert_eq!(
        words(&scatter("speckle", vec![[0, 0], [0, 1], [3, 2]])),
        "Scatter palette speckle over 3 cells, none to change, seed 1234 (floor on main/0)"
    );
    // Seed 1234 draws sand at (1, 0) and (1, 3), which hold grass.
    assert_eq!(
        words(&scatter("speckle", vec![[0, 0], [1, 0], [1, 3]])),
        "Scatter palette speckle over 3 cells, 2 to change, seed 1234 (floor on main/0)"
    );
    assert_eq!(
        words(&scatter("dunes", vec![[1, 1], [900, 1]])),
        "Scatter palette dunes (not in this catalog) over 2 cells, some outside the view, seed 1234 (floor on main/0)"
    );
    let highlight = Highlight::of(0, &scatter("speckle", vec![[0, 0], [3, 2]]), &app.session);
    let tiles: Vec<(&str, [i64; 2])> = highlight
        .tiles
        .iter()
        .map(|tile| (tile.role.key(), tile.at))
        .collect();
    assert_eq!(tiles, [("cell", [0, 0]), ("cell", [3, 2])]);
}

#[test]
fn choosing_a_palette_of_another_grid_samples_that_grid() {
    // `edge` grants the sample's own values too, with a palette of its own.
    let (_temporary, mut app) = terrain_application(1, |root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["terrain"]
                .as_array_mut()
                .unwrap()
                .push(json!({"grid": "edge", "values": ["grass", "sand"]}));
            let mut palettes = speckle();
            palettes
                .as_array_mut()
                .unwrap()
                .push(json!({"name": "rim", "grid": "edge",
                "entries": [{"value": "sand", "weight": 1}]}));
            catalog["editing"]["palettes"] = palettes;
        });
    });
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    assert_eq!(app.view.grid.as_deref(), Some("floor"));
    app.control(Control::ScatterPalette("rim".into())).unwrap();
    assert_eq!(app.view.grid.as_deref(), Some("edge"));
    assert_eq!(app.tools.scatter_choice(), Some("rim"));
    // The press samples the edge grid, so the stroke scatters there.
    acknowledge_ui(&mut app);
    let at = cell_pixel(&mut app, 1, 1);
    crate::prototype::native_pointer(&mut app, at, BRUSH_WINDOW, BRUSH_WINDOW).unwrap();
    assert_eq!(app.tools.stroke().unwrap().grid(), "edge");
    app.tools.cancel_stroke();
    app.stroke_mask = None;
    app.control(Control::ScatterPalette("speckle".into()))
        .unwrap();
    assert_eq!(app.view.grid.as_deref(), Some("floor"));
}

#[test]
fn the_samples_own_palette_loads_and_its_scripted_scatter_is_fixed() {
    let (temporary, mut app) = terrain_application(crate::prototype::DEMO_UID_SEED, |_| {});
    // The neutral sample names one palette, so a scatter needs no catalog
    // editing.
    let palettes = &app.session.catalog().editing().palettes;
    assert_eq!(palettes.len(), 1);
    assert_eq!(palettes["speckle"].grid, "floor");
    assert_eq!(
        palettes["speckle"].entries,
        [("grass".to_owned(), 3), ("sand".to_owned(), 1)].into()
    );
    let texts = sidebar_texts(&mut app);
    assert!(
        texts
            .iter()
            .any(|text| text == "floor / speckle: grass 3, sand 1"),
        "{texts:?}"
    );
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    let (choice, brush) = (app.tools.terrain_choice().cloned(), app.tools.brush());
    let cells: Vec<(i64, i64)> = (0..6).flat_map(|y| (0..8).map(move |x| (x, y))).collect();
    let before: Vec<Option<String>> = cells
        .iter()
        .map(|&(x, y)| terrain_value(&app, x, y))
        .collect();
    app.scatter_demo().unwrap();
    // One reviewed change, accepted; the brush and terrain choice are back.
    assert_eq!(app.session.history().depths(), (1, 0));
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.tools.terrain_choice().cloned(), choice);
    assert_eq!(app.tools.brush(), brush);
    assert_eq!(app.tools.scatter_choice(), None);
    // The stroke drew the seeded source's first seed, and every cell it
    // changed holds the value the frozen draw gives it for that seed.
    let seed = first_seed(crate::prototype::DEMO_UID_SEED);
    assert_eq!(seed, 2_457_608_480);
    let palette = &app.session.catalog().editing().palettes["speckle"];
    let mut changed = 0;
    for (&(x, y), was) in cells.iter().zip(&before) {
        let now = terrain_value(&app, x, y);
        if now != *was {
            changed += 1;
            let drawn = tilewright_editor_ui::scatter::value(palette, seed, 0, [x, y]).unwrap();
            assert_eq!(now.as_deref(), Some(drawn), "({x}, {y})");
        }
    }
    assert!(changed > 0);
    let rows: Vec<String> = (0..6)
        .map(|y| {
            (0..8)
                .map(|x| match terrain_value(&app, x, y).as_deref() {
                    Some("grass") => 'g',
                    Some("sand") => 's',
                    _ => '.',
                })
                .collect()
        })
        .collect();
    assert_eq!(
        rows,
        [
            "gssgssss", "ssggssss", "ssgsssss", "ggsgssss", "gsggss.s", "gssgssss"
        ]
    );
    for (phase, pending) in [
        ("scatter-preview", false),
        ("scatter", true),
        ("scattered", false),
    ] {
        let record: Value = serde_json::from_slice(
            &fs::read(temporary.path().join(format!("evidence/{phase}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(record["pending"], pending, "{phase}");
    }
}

#[test]
fn without_palettes_the_scripted_scatter_step_is_skipped() {
    let (temporary, mut app) =
        terrain_application(crate::prototype::DEMO_UID_SEED, without_palettes);
    assert!(app.session.catalog().editing().palettes.is_empty());
    let world = app.session.world().source_digest();
    // Step 15 is skipped, not an error, and the script goes on to its end.
    assert!(!app.demo_step(15).unwrap());
    assert_eq!(app.session.history().depths(), (0, 0));
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.world().source_digest(), world);
    let evidence: Vec<String> = fs::read_dir(temporary.path().join("evidence"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("scatter"))
        .collect();
    assert!(evidence.is_empty(), "{evidence:?}");
    assert!(app.demo_step(16).unwrap());
}
