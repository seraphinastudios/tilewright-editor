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
    renamed_as("lawn", &["grass", "grass_edge_15"], values)
}

/// [`renamed`] with the grass terrain called `lawn` and given `protos`.
fn renamed_as(
    lawn: &'static str,
    protos: &'static [&'static str],
    values: &'static [&'static str],
) -> impl FnOnce(&Path) {
    move |root| {
        edit_json(root, "profile/transitions.jsonc", |transitions| {
            let terrains = transitions["terrains"].as_array_mut().unwrap();
            terrains[0]["id"] = json!("dune");
            terrains[1]["id"] = json!(lawn);
            terrains[1]["protos"] = json!(protos);
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
            Some(Control::Palette { prototype, .. }) => {
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
    // A row too wide for the sidebar drops its grid first and keeps the
    // value it writes whole.
    let shown = texts(&rows);
    assert_eq!(
        [shown[0], shown[2], shown[3]],
        [labels[0], labels[2], labels[3]]
    );
    assert_eq!(shown[1], "lawn variant: grass_edge_15");
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
    assert!(
        filled.iter().all(|(_, value)| value == "grass"),
        "{filled:?}"
    );
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

/// Every terrain palette row's text across every scroll position, by the
/// value it chooses.
fn shown_by_value(app: &mut Application) -> std::collections::BTreeMap<String, String> {
    let mut shown = std::collections::BTreeMap::new();
    for scroll in 0..60 {
        app.sidebar_scroll = scroll;
        for (text, value, _) in rows(app) {
            let old = shown.insert(value, text.clone());
            assert!(
                old.as_ref().is_none_or(|old| *old == text),
                "{old:?} {text}"
            );
        }
    }
    shown
}

/// Whether `text` shows `value` whole, or its head and tail around an
/// ellipsis in the middle.
fn shows(text: &str, value: &str) -> bool {
    if text.contains(value) {
        return true;
    }
    let Some((before, tail)) = text.rsplit_once("...") else {
        return false;
    };
    !tail.is_empty()
        && value.ends_with(tail)
        && (1..=value.len() - tail.len()).any(|head| before.ends_with(&value[..head]))
}

#[test]
fn rows_of_one_terrain_stay_distinguishable_by_the_value_they_write() {
    const LONG: &str = "a-rather-long-terrain-id-for-the-sample-lawn-grass";
    assert_eq!(LONG.len(), 50);
    for lawn in ["lawn", LONG] {
        for size in [BRUSH_WINDOW, (720, 480)] {
            let (_temporary, mut app) = terrain_application_at(
                size,
                renamed_as(
                    lawn,
                    &["grass", "grass_edge_14", "grass_edge_15"],
                    &["grass", "grass_edge_14", "grass_edge_15", "sand"],
                ),
            );
            let shown = shown_by_value(&mut app);
            let values: Vec<&str> = shown.keys().map(String::as_str).collect();
            assert_eq!(values, ["grass", "grass_edge_14", "grass_edge_15", "sand"]);
            let texts: std::collections::BTreeSet<&String> = shown.values().collect();
            assert_eq!(texts.len(), shown.len(), "{lawn} {size:?}: {shown:?}");
            for (value, text) in &shown {
                assert!(shows(text, value), "{lawn} {size:?}: {text} for {value}");
            }
            if lawn == "lawn" {
                // A short terrain id stays named at both sizes.
                for value in ["grass_edge_14", "grass_edge_15"] {
                    assert_eq!(shown[value], format!("lawn variant: {value}"), "{size:?}");
                }
            }
        }
    }
}

#[test]
fn only_the_terrain_layer_names_terrains_even_when_it_is_not_the_first_grid() {
    let (_temporary, mut app) = terrain_application(|root| {
        renamed(&["grass", "sand"])(root);
        edit_json(root, "profile/transitions.jsonc", |transitions| {
            transitions["layer"] = json!("edge");
            transitions["terrains"][1]["transitions"][0]["layer"] = json!("floor");
        });
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["terrain"] = json!([
                {"grid": "floor", "values": ["grass", "sand"]},
                {"grid": "edge", "values": ["grass", "sand"]},
            ]);
        });
    });
    let full: Vec<String> = tilewright_editor_ui::tools::terrain_palette(app.session.catalog())
        .iter()
        .map(|choice| tilewright_editor_ui::tools::terrain_label(app.session.world(), choice))
        .collect();
    assert_eq!(
        full,
        [
            "edge / lawn (grass)",
            "edge / dune (sand)",
            "floor / grass",
            "floor / sand",
        ]
    );
    let shown: Vec<String> = shown_by_value_and_grid(&mut app);
    assert_eq!(shown, full);
}

/// Every terrain palette row's text across every scroll position, in
/// palette order.
fn shown_by_value_and_grid(app: &mut Application) -> Vec<String> {
    let mut shown: Vec<(String, String, String)> = Vec::new();
    for scroll in 0..60 {
        app.sidebar_scroll = scroll;
        app.refresh_ui();
        for widget in &app.ui.widgets {
            if let Some(Control::Palette { grid, prototype }) = &widget.control {
                let row = (grid.clone(), prototype.clone(), widget.text.clone());
                if !shown.contains(&row) {
                    shown.push(row);
                }
            }
        }
    }
    shown.sort();
    shown.into_iter().map(|(.., text)| text).collect()
}
