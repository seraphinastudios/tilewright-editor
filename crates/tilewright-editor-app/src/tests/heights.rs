// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The height tint, its legend and the inspector's height line, on the CPU
//! scene path the scripted demonstration captures. Every world here is a
//! temporary copy of the MIT neutral sample with generated heights; the
//! committed sample has none.
// Colours are copied, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::zones::{key, route, sidebar_texts, zone_application};
use crate::prototype::Application;
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::heights::{self, HeightRange};
use tilewright_editor_ui::tools::ToolMode;
use tilewright_scene::{Layer, Quad};

fn edit_json(world: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = world.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// Chunk `path`'s height grid, `height(x, y)` for each of its tiles (chunk
/// coordinates).
fn heights_of(world: &Path, path: &str, height: impl Fn(i64, i64) -> i64) {
    let rows: Vec<Vec<i64>> = (0..32)
        .map(|y| (0..32).map(|x| height(x, y)).collect())
        .collect();
    edit_json(world, path, |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}

const MAIN: &str = "planes/main/layers/0/0_0.jsonc";

/// Heights `3x - y` in the sample's main chunk: -5 to 21 in its 8 x 6 view.
fn sloped(world: &Path) {
    heights_of(world, MAIN, |x, y| 3 * x - y);
}

/// The quads the height tint adds to the presented frame.
fn tint_quads(app: &mut Application) -> Vec<Quad> {
    app.refresh_ui();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    let world = app.prepared.scene.quads();
    presentation
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.layer != Layer::Interface && !world.contains(quad))
        .copied()
        .collect()
}

/// The tile each tint copy covers.
fn tinted_tiles(app: &Application, overlay: &[Quad]) -> Vec<([i64; 2], [f32; 4])> {
    overlay
        .iter()
        .map(|copy| {
            let source = app
                .prepared
                .cell_scene
                .quads()
                .iter()
                .find(|quad| quad.positions == copy.positions)
                .unwrap();
            match &app.prepared.cell_picks[&source.semantic_id] {
                SelectionKey::Cell { x, y, .. } => ([*x, *y], copy.color),
                SelectionKey::Object { .. } => unreachable!(),
            }
        })
        .collect()
}

/// How many tiles the prepared cell scene draws.
fn shown_tiles(app: &Application) -> usize {
    app.prepared
        .cell_scene
        .quads()
        .iter()
        .filter_map(|quad| app.prepared.cell_picks.get(&quad.semantic_id))
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn select(app: &mut Application, layer: i32, x: i64, y: i64) {
    let generation = app.view.frame_revision;
    app.action(tilewright_editor_ui::Action::Select {
        generation,
        key: SelectionKey::Cell {
            plane: "main".into(),
            layer,
            grid: "floor".into(),
            x,
            y,
        },
    })
    .unwrap();
}

fn height_line(app: &mut Application) -> Option<String> {
    sidebar_texts(app)
        .into_iter()
        .find(|text| text.starts_with("Height: "))
}

#[test]
fn the_tint_is_off_by_default_and_h_toggles_it_in_every_mode() {
    let (_temporary, mut app) = zone_application(sloped);
    assert!(!app.tools.heights_visible());
    // Off: the frame has the World and the interface only.
    assert!(tint_quads(&mut app).is_empty());
    let texts = sidebar_texts(&mut app);
    assert!(texts.contains(&"Show heights  H".to_owned()), "{texts:?}");
    assert!(!texts.iter().any(|text| text.starts_with("Heights")));
    route(&mut app, &key(Keycode::H)).unwrap();
    assert!(app.tools.heights_visible());
    assert_eq!(tint_quads(&mut app).len(), 48, "every tile of 8 x 6");
    let texts = sidebar_texts(&mut app);
    for line in [
        "Hide heights  H",
        "Heights -5 to 21",
        "Darkest: lowest, -5",
        "Lightest: highest, 21",
    ] {
        assert!(texts.contains(&line.to_owned()), "{line}: {texts:?}");
    }
    // The legend's swatches are the ramp's ends; the toggle row has none.
    let swatch = |app: &Application, text: &str| {
        app.ui
            .widgets
            .iter()
            .find(|widget| widget.text == text)
            .unwrap()
            .swatch
    };
    assert_eq!(
        swatch(&app, "Darkest: lowest, -5"),
        Some(heights::swatch_color(0))
    );
    assert_eq!(
        swatch(&app, "Lightest: highest, 21"),
        Some(heights::swatch_color(7))
    );
    assert_eq!(swatch(&app, "Hide heights  H"), None);
    assert_eq!(swatch(&app, "Heights -5 to 21"), None);
    // Kept across modes, Zone mode included, and the sidebar row toggles
    // it too.
    for mode in [
        Control::Terrain,
        Control::Place,
        Control::Zone,
        Control::Anchor,
        Control::Move,
    ] {
        app.control(mode).unwrap();
        assert!(app.tools.heights_visible());
        assert!(sidebar_texts(&mut app).contains(&"Hide heights  H".to_owned()));
    }
    app.control(Control::HeightOverlay).unwrap();
    assert!(!app.tools.heights_visible());
    assert!(tint_quads(&mut app).is_empty());
    assert_eq!(app.tools.mode(), ToolMode::Move);
}

#[test]
fn each_tile_takes_its_band_of_the_view_range() {
    let (_temporary, mut app) = zone_application(sloped);
    app.control(Control::HeightOverlay).unwrap();
    let overlay = tint_quads(&mut app);
    let range = HeightRange { min: -5, max: 21 };
    let tiles = tinted_tiles(&app, &overlay);
    assert_eq!(tiles.len(), 48);
    for ([x, y], color) in tiles {
        let band = range.band(i32::try_from(3 * x - y).unwrap()).unwrap();
        assert_eq!(color, heights::tint_color(band), "{x},{y}");
    }
    // Golden: the lowest tile (0, 5) is band 0, the highest (7, 0) band 7,
    // and (3, 2) at 7 is 12 of 26 up: band 3.
    let at = |app: &Application, tile: [i64; 2]| {
        tinted_tiles(app, &overlay)
            .into_iter()
            .find(|(at, _)| *at == tile)
            .unwrap()
            .1
    };
    assert_eq!(at(&app, [0, 5]), heights::tint_color(0));
    assert_eq!(at(&app, [7, 0]), heights::tint_color(7));
    assert_eq!(at(&app, [3, 2]), heights::tint_color(3));
    // Unpickable World copies in the World's draw order, like zone tints.
    for copy in &overlay {
        assert!(!copy.pickable && copy.layer == Layer::World && copy.glow.is_none());
    }
}

#[test]
fn no_tint_is_drawn_from_a_frame_that_is_not_the_views() {
    let (_temporary, mut app) = zone_application(sloped);
    app.control(Control::HeightOverlay).unwrap();
    assert_eq!(tint_quads(&mut app).len(), 48);
    let generation = app.view.frame_revision;
    app.prepared.frame_revision = generation + 1;
    assert!(tint_quads(&mut app).is_empty());
    app.prepared.frame_revision = generation;
    assert_eq!(tint_quads(&mut app).len(), 48);
}

#[test]
fn zone_tints_draw_over_height_tints() {
    let (_temporary, mut app) = zone_application(sloped);
    app.control(Control::HeightOverlay).unwrap();
    route(&mut app, &key(Keycode::O)).unwrap();
    let overlay = tint_quads(&mut app);
    // 48 height tints, 48 zone tints and the sample's badges.
    assert_eq!(overlay.len(), 48 + 48 + 2 * 3);
    let ramp: Vec<[f32; 4]> = (0..8).map(heights::tint_color).collect();
    let tinted: Vec<usize> = (0..overlay.len())
        .filter(|&index| ramp.contains(&overlay[index].color))
        .collect();
    assert_eq!(tinted.len(), 48);
    // Each tile's zone tint shares its height tint's slot and is drawn
    // after it, so it draws over it.
    for index in tinted {
        let height = overlay[index];
        let zone = (0..overlay.len())
            .find(|&other| {
                other != index
                    && overlay[other].positions == height.positions
                    && overlay[other].color[3] == tilewright_editor_ui::zones::TINT_ALPHA
            })
            .unwrap();
        assert!(zone > index);
        assert_eq!(
            (height.depth, height.painter_order),
            (overlay[zone].depth, overlay[zone].painter_order)
        );
    }
}

#[test]
fn a_flat_view_has_no_tint_and_says_so() {
    // The sample itself: every height 0.
    let (_temporary, mut app) = zone_application(|_| {});
    route(&mut app, &key(Keycode::H)).unwrap();
    assert!(tint_quads(&mut app).is_empty());
    let texts = sidebar_texts(&mut app);
    assert!(
        texts.contains(&"Heights: all 0, no tint".to_owned()),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|text| text.starts_with("Darkest")));
    // Flat at another height.
    let (_temporary, mut app) = zone_application(|world| heights_of(world, MAIN, |_, _| -7));
    route(&mut app, &key(Keycode::H)).unwrap();
    assert!(tint_quads(&mut app).is_empty());
    assert!(sidebar_texts(&mut app).contains(&"Heights: all -7, no tint".to_owned()));
}

#[test]
fn tiles_of_chunks_that_do_not_exist_are_neither_tinted_nor_counted() {
    // The view straddles chunk 0_0 (x 28 to 31) and the missing 1_0.
    let (_temporary, mut app) = zone_application(|world| {
        sloped(world);
        edit_json(world, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([28, 0, 8, 6]);
        });
    });
    route(&mut app, &key(Keycode::H)).unwrap();
    let overlay = tint_quads(&mut app);
    let tiles = tinted_tiles(&app, &overlay);
    assert_eq!(tiles.len(), 4 * 6);
    assert!(tiles.iter().all(|([x, _], _)| (28..32).contains(x)));
    // 3 x 28 - 5 to 3 x 31.
    assert!(sidebar_texts(&mut app).contains(&"Heights 79 to 93".to_owned()));
    select(&mut app, 0, 33, 1);
    assert_eq!(
        height_line(&mut app).as_deref(),
        Some("Height: none, no chunk here")
    );
    // A view with no chunk at all.
    let (_temporary, mut app) = zone_application(|world| {
        edit_json(world, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([40, 0, 8, 6]);
        });
    });
    route(&mut app, &key(Keycode::H)).unwrap();
    assert!(tint_quads(&mut app).is_empty());
    assert!(sidebar_texts(&mut app).contains(&"Heights: no tiles in view".to_owned()));
}

#[test]
fn the_inspector_shows_the_selected_cells_height() {
    let (_temporary, mut app) = zone_application(sloped);
    // Nothing selected: no height line.
    assert_eq!(height_line(&mut app), None);
    select(&mut app, 0, 3, 2);
    // Right after the cell's value, whether or not the tint shows.
    let texts = sidebar_texts(&mut app);
    let at = texts.iter().position(|text| text == "Height: 7").unwrap();
    assert!(texts[at - 1].starts_with("Value: "), "{texts:?}");
    app.control(Control::HeightOverlay).unwrap();
    assert_eq!(height_line(&mut app).as_deref(), Some("Height: 7"));
    // A chunk without a height grid reads 0.
    let (_temporary, mut app) = zone_application(|_| {});
    select(&mut app, 0, 3, 2);
    assert_eq!(height_line(&mut app).as_deref(), Some("Height: 0"));
    // With a profile layer_height, the layer's base and the sum follow.
    let (_temporary, mut app) = zone_application(|world| {
        sloped(world);
        edit_json(world, "profile/profile.jsonc", |profile| {
            profile["layer_height"] = json!(16);
        });
    });
    select(&mut app, 0, 0, 5);
    assert_eq!(
        height_line(&mut app).as_deref(),
        Some("Height: -5 + base 0 = -5")
    );
}

/// Profile `layer_height` 65,535 and a copy of the main chunk, without its
/// objects, as spatial layer -3, which the view shows; `height` sets that
/// copy's heights.
fn stacked(world: &Path, height: impl Fn(i64, i64) -> i64) {
    edit_json(world, "profile/profile.jsonc", |profile| {
        profile["layer_height"] = json!(65_535);
    });
    let below = world.join("planes/main/layers/-3");
    fs::create_dir_all(&below).unwrap();
    let mut chunk: Value = serde_json::from_slice(&fs::read(world.join(MAIN)).unwrap()).unwrap();
    chunk["layer"] = json!(-3);
    chunk.as_object_mut().unwrap().remove("objects");
    fs::write(
        below.join("0_0.jsonc"),
        serde_json::to_vec_pretty(&chunk).unwrap(),
    )
    .unwrap();
    heights_of(world, "planes/main/layers/-3/0_0.jsonc", height);
    edit_json(world, "preview.jsonc", |catalog| {
        catalog["view"]["layer"] = json!(-3);
    });
}

#[test]
fn extreme_heights_neither_overflow_nor_panic() {
    // The full 32-bit range on one layer, top-down.
    let (_temporary, mut app) = zone_application(|world| {
        heights_of(world, MAIN, |x, y| match (x, y) {
            (0, 0) => i64::from(i32::MIN),
            (1, 0) => i64::from(i32::MAX),
            _ => 0,
        });
    });
    route(&mut app, &key(Keycode::H)).unwrap();
    let overlay = tint_quads(&mut app);
    let tiles = tinted_tiles(&app, &overlay);
    // The two extreme tiles are shifted some 2^31 pixels out of the frame,
    // so the prepared cell scene has no quad for them and the tint, a copy
    // of that scene, none either; every other tile is tinted.
    assert_eq!(tiles.len(), 46);
    assert_eq!(tiles.len(), shown_tiles(&app));
    assert!(!tiles.iter().any(|(at, _)| at[1] == 0 && at[0] < 2));
    for (at, color) in tiles {
        let band = match at {
            [0, 0] => 0,
            [1, 0] => 7,
            _ => 4,
        };
        assert_eq!(color, heights::tint_color(band), "{at:?}");
    }
    // The first legend line is cut to the sidebar; the band lines are whole.
    let texts = sidebar_texts(&mut app);
    for line in [
        "Darkest: lowest, -2147483648",
        "Lightest: highest, 2147483647",
    ] {
        assert!(texts.contains(&line.to_owned()), "{line}: {texts:?}");
    }
    select(&mut app, 0, 0, 0);
    assert_eq!(
        height_line(&mut app).as_deref(),
        Some("Height: -2147483648")
    );

    // Stacked: layer -3 of 65,535 units sits at -196,605; the sum is shown
    // whole, and the tint still uses the stored heights.
    let (_temporary, mut app) = zone_application(|world| {
        stacked(world, |x, y| {
            if (x, y) == (2, 2) {
                i64::from(i32::MAX)
            } else {
                0
            }
        });
    });
    // The sidebar cuts the long line after the stored height; the whole
    // line is the inspector's.
    select(&mut app, -3, 2, 2);
    let line = height_line(&mut app).unwrap();
    assert!(line.starts_with("Height: 2147483647 + "), "{line}");
    assert_eq!(
        heights::inspector_line(app.session.world(), "main", -3, 2, 2),
        "Height: 2147483647 + base -196605 = 2147287042"
    );
    select(&mut app, -3, 1, 1);
    assert!(
        height_line(&mut app)
            .unwrap()
            .starts_with("Height: 0 + base -")
    );
    assert_eq!(
        heights::inspector_line(app.session.world(), "main", -3, 1, 1),
        "Height: 0 + base -196605 = -196605"
    );
    route(&mut app, &key(Keycode::H)).unwrap();
    assert!(sidebar_texts(&mut app).contains(&"Heights 0 to 2147483647".to_owned()));
    let overlay = tint_quads(&mut app);
    assert_eq!(tinted_tiles(&app, &overlay).len(), shown_tiles(&app));
}

#[test]
fn a_world_the_preview_refuses_is_refused_before_any_tint() {
    // Layer -3 at -196,605 with a stored i32::MIN leaves the 32 bits the
    // preview stacks in: the preview refuses the view, so there is no frame
    // to tint. The height facts themselves still compute without overflow.
    let directory = tempfile::tempdir().unwrap();
    let world = directory.path().join("world");
    super::copy_private(
        &tilewright_editor_testkit::fixture_world("prototype-neutral"),
        &world,
    );
    stacked(&world, |x, y| {
        if (x, y) == (4, 4) {
            i64::from(i32::MIN)
        } else {
            0
        }
    });
    let document = tilewright_editor_document::WorldDocument::open_prototype(&world).unwrap();
    let limits = tilewright_editor_preview::PreviewLimits::default();
    let (catalog, resources) =
        tilewright_editor_preview::loader::load(document.source(), document.world(), limits)
            .unwrap();
    let view = catalog.default_view(document.world(), 320, 240, 1);
    let refused =
        tilewright_editor_preview::prepare(document.world(), &catalog, &resources, &view, limits);
    assert!(refused.is_err());
    let (range, tints) = heights::tints(&heights::view_heights(document.world(), &catalog));
    assert_eq!(
        range,
        Some(HeightRange {
            min: i32::MIN,
            max: 0
        })
    );
    assert_eq!(tints.len(), 48);
    assert_eq!(
        heights::inspector_line(document.world(), "main", -3, 4, 4),
        "Height: -2147483648 + base -196605 = -2147680253"
    );
}
