// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Terrain mode's palette names each row by the World format's terrain
//! its value is in, with the value's thumbnail, through the application
//! object and real SDL pointer events; choosing a row still writes exactly
//! the granted value, for the brush, Fill and Replace.
use super::proposal::edit_json;
use super::zones::{button, key, route};
use super::{BRUSH_WINDOW, acknowledge_ui, application_with_source, cell_pixel, terrain_value};
use crate::prototype::Application;
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::keyboard::Keycode;
use serde_json::json;
use std::path::Path;
use tilewright_editor_ui::Action;

/// Rename the neutral sample's terrains (sand to `dune`, grass to `lawn`),
/// give `lawn` the variant `grass_edge_15`, and grant `values` on `floor`.
fn renamed(values: &'static [&'static str]) -> impl FnOnce(&Path) {
    move |root| {
        edit_json(root, "profile/transitions.jsonc", |transitions| {
            let terrains = transitions["terrains"].as_array_mut().unwrap();
            terrains[0]["id"] = json!("dune");
            terrains[1]["id"] = json!("lawn");
            terrains[1]["protos"] = json!(["grass", "grass_edge_15"]);
            terrains[1]["transitions"][0]["with"] = json!(["dune"]);
        });
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["terrain"] = json!([{"grid": "floor", "values": values}]);
            catalog["editing"]
                .as_object_mut()
                .unwrap()
                .remove("palettes");
        });
    }
}

/// The 1280x800 application on the fixture changed by `prepare`, in
/// Terrain mode with nothing chosen.
fn terrain_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    terrain_application_at(BRUSH_WINDOW, prepare)
}

fn terrain_application_at(
    size: (u32, u32),
    prepare: impl FnOnce(&Path),
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = application_with_source(prepare);
    app.ui = WorkspaceUi::new(size, size);
    app.view.camera.width = size.0;
    app.view.camera.height = size.1;
    app.rebuild().unwrap();
    app.control(Control::Terrain).unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The terrain palette rows laid out: text, the value they choose, and
/// whether they carry exactly that value's prototype thumbnail.
fn rows(app: &mut Application) -> Vec<(String, String, bool)> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .filter_map(|widget| match &widget.control {
            Some(Control::Palette { grid, prototype }) => {
                assert_eq!(grid, "floor");
                let texture = app
                    .session
                    .resources()
                    .prototype_texture(app.session.catalog(), prototype)
                    .map(|texture| (texture.id, texture.revision, texture.digest));
                let shown = widget
                    .thumbnail
                    .map(|texture| (texture.id, texture.revision, texture.digest));
                Some((
                    widget.text.clone(),
                    prototype.clone(),
                    texture.is_some() && shown == texture,
                ))
            }
            _ => None,
        })
        .collect()
}

fn texts(rows: &[(String, String, bool)]) -> Vec<&str> {
    rows.iter().map(|(text, ..)| text.as_str()).collect()
}

/// Press and release the palette row that chooses `value`.
fn press_row(app: &mut Application, value: &str) {
    let control = Control::Palette {
        grid: "floor".into(),
        prototype: value.into(),
    };
    let at = super::widget_point(app, &control);
    acknowledge_ui(app);
    route(app, &button(true, at)).unwrap();
    route(app, &button(false, at)).unwrap();
    assert_eq!(
        app.tools.terrain_choice().map(|c| c.prototype.as_str()),
        Some(value)
    );
}

/// The `cells` of the pending change's one `terrain.set`, each `[x, y]`
/// with the value it writes.
fn pending_cells(app: &Application) -> Vec<([i64; 2], String)> {
    let sealed: serde_json::Value =
        serde_json::from_str(app.session.history().pending().unwrap().sealed()).unwrap();
    assert_eq!(sealed["operations"].as_array().unwrap().len(), 1);
    assert_eq!(sealed["operations"][0]["op"], "terrain.set");
    sealed["operations"][0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            (
                [
                    cell["at"][0].as_i64().unwrap(),
                    cell["at"][1].as_i64().unwrap(),
                ],
                cell["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn the_neutral_samples_rows_are_its_terrains_with_their_thumbnails() {
    let (_temporary, mut app) = terrain_application(|_| {});
    let rows = rows(&mut app);
    // The sample's terrains share their prototypes' ids, so the labels are
    // as before; each row now carries its value's thumbnail.
    assert_eq!(texts(&rows), ["floor / grass", "floor / sand"]);
    assert!(rows.iter().all(|(.., thumbnail)| *thumbnail), "{rows:?}");
    for widget in app
        .ui
        .widgets
        .iter()
        .filter(|widget| matches!(widget.control, Some(Control::Palette { .. })))
    {
        assert_eq!(widget.layout.height, 40);
    }
}

#[test]
fn rows_name_the_terrain_a_variant_and_a_value_in_no_terrain() {
    let (_temporary, mut app) =
        terrain_application(renamed(&["sand", "grass", "grass_edge_15", "stone"]));
    let rows = rows(&mut app);
    let labels = [
        "floor / lawn (grass)",
        "floor / lawn variant: grass_edge_15",
        "floor / dune (sand)",
        "floor / stone (no terrain)",
    ];
    let full: Vec<String> = rows
        .iter()
        .map(|(_, value, _)| {
            tilewright_editor_ui::tools::terrain_label(
                app.session.world(),
                &tilewright_editor_ui::tools::TerrainChoice {
                    grid: "floor".into(),
                    prototype: value.clone(),
                },
            )
        })
        .collect();
    assert_eq!(full, labels);
    // The sidebar cuts a row that does not fit with an ellipsis, after
    // the terrain, which comes first.
    let shown = texts(&rows);
    assert_eq!([shown[0], shown[2], shown[3]], [labels[0], labels[2], labels[3]]);
    assert!(
        shown[1].starts_with("floor / lawn variant: ") && shown[1].ends_with("..."),
        "{shown:?}"
    );
    // One row per granted value, choosing exactly that value, in the
    // catalog's order.
    let values: Vec<&str> = rows.iter().map(|(_, value, _)| value.as_str()).collect();
    assert_eq!(values, ["grass", "grass_edge_15", "sand", "stone"]);
    assert!(rows.iter().all(|(.., thumbnail)| *thumbnail), "{rows:?}");
    // The scatter palette rows are gone with the catalog's palettes, and
    // the rules still follow the palette.
    app.refresh_ui();
    assert!(
        app.ui
            .widgets
            .iter()
            .any(|widget| widget.text.starts_with("Brush: drag"))
    );
}

#[test]
fn a_world_without_transitions_keeps_the_plain_value_rows() {
    let (_temporary, mut app) = terrain_application(|root| {
        edit_json(root, "profile/profile.jsonc", |profile| {
            profile.as_object_mut().unwrap().remove("transitions");
        });
    });
    let rows = rows(&mut app);
    assert_eq!(texts(&rows), ["floor / grass", "floor / sand"]);
}

#[test]
fn only_granted_values_get_rows_and_others_are_refused() {
    let (_temporary, mut app) = terrain_application(renamed(&["grass"]));
    assert_eq!(texts(&rows(&mut app)), ["floor / lawn (grass)"]);
    let catalog = app.session.catalog().clone();
    for value in ["sand", "grass_edge_15", "lawn", "dune"] {
        assert!(
            app.tools.choose_terrain(&catalog, "floor", value).is_err(),
            "{value}"
        );
    }
}

#[test]
fn a_terrain_row_paints_fills_and_replaces_with_its_granted_value() {
    let (temporary, mut app) =
        terrain_application(renamed(&["sand", "grass", "grass_edge_15", "stone"]));
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = std::fs::read(&chunk).unwrap();
    // The brush: the "dune" row writes `sand`, the value the catalog grants.
    press_row(&mut app, "sand");
    acknowledge_ui(&mut app);
    let at = cell_pixel(&mut app, 0, 0);
    let before = terrain_value(&app, 0, 0);
    assert_ne!(before.as_deref(), Some("sand"));
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    assert_eq!(pending_cells(&app), [([0, 0], "sand".to_owned())]);
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert_eq!(terrain_value(&app, 0, 0).as_deref(), Some("sand"));
    // Fill: the "lawn" row writes `grass` over the joined sand.
    press_row(&mut app, "grass");
    route(&mut app, &key(Keycode::F)).unwrap();
    acknowledge_ui(&mut app);
    let at = cell_pixel(&mut app, 0, 0);
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    let filled = pending_cells(&app);
    assert!(!filled.is_empty());
    assert!(filled.iter().all(|(_, value)| value == "grass"), "{filled:?}");
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    let step = app.session.applied_steps().last().unwrap();
    assert!(matches!(
        step.origin.unwrap().action(),
        Action::FillTerrain { proto, contiguous: true, .. } if proto == "grass"
    ));
    assert_eq!(terrain_value(&app, 0, 0).as_deref(), Some("grass"));
    // Replace: the variant row writes the variant itself.
    press_row(&mut app, "grass_edge_15");
    route(&mut app, &key(Keycode::F)).unwrap();
    acknowledge_ui(&mut app);
    let at = cell_pixel(&mut app, 0, 0);
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    let replaced = pending_cells(&app);
    assert!(
        replaced.iter().all(|(_, value)| value == "grass_edge_15"),
        "{replaced:?}"
    );
    app.action(Action::Cancel).unwrap();
    // Two undos restore the bytes the stroke and the fill changed.
    for _ in 0..2 {
        app.action(Action::Undo).unwrap();
        app.action(Action::Accept).unwrap();
    }
    assert_eq!(std::fs::read(&chunk).unwrap(), original);
}

#[test]
fn the_rows_fit_at_the_minimum_window_and_scroll_whole() {
    let (_temporary, mut app) = terrain_application_at(
        (720, 480),
        renamed(&["sand", "grass", "grass_edge_15", "stone"]),
    );
    let width = app.ui.sidebar_width();
    let mut seen = std::collections::BTreeSet::new();
    for scroll in 0..40 {
        app.sidebar_scroll = scroll;
        app.refresh_ui();
        let (_, height) = app.ui.size;
        for widget in app
            .ui
            .widgets
            .iter()
            .filter(|widget| matches!(widget.control, Some(Control::Palette { .. })))
        {
            assert_eq!(widget.layout.width, width);
            assert!(widget.bounds.y + widget.bounds.height <= height);
            assert!(widget.thumbnail.is_some());
            seen.insert(widget.text.clone());
        }
    }
    // Every row can be scrolled into view.
    assert_eq!(seen.len(), 4, "{seen:?}");
}
