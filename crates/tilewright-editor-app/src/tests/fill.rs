// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Terrain mode's Fill and Replace rules through real SDL pointer and key
//! events, on the CPU scene path the scripted demonstration captures: the
//! rule rows and key, the hover preview, and the one pending change a click
//! prepares.
// Colours are copied, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::zones::{button, key, route, sidebar_texts};
use super::{acknowledge_ui, brush_application, cell_pixel};
use crate::prototype::Application;
use crate::workspace_ui::Control;
use sdl3::{event::Event, keyboard::Keycode};
use std::fs;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::tools::TerrainRule;
use tilewright_editor_ui::{Action, fill};

/// Pointer motion with no button held, as SDL reports a hover.
fn hover(at: [f32; 2]) -> Event {
    Event::MouseMotion {
        timestamp: 0,
        window_id: 7,
        which: 0,
        mousestate: sdl3::mouse::MouseState::from_sdl_state(0),
        x: at[0],
        y: at[1],
        xrel: 0.0,
        yrel: 0.0,
    }
}
/// The cells the fill preview marks, read back through the prepared cell
/// picks; every copy is unpickable and in the stroke preview's tint, and
/// the composed frame holds exactly these copies.
fn marked(app: &mut Application) -> Vec<[i64; 2]> {
    app.refresh_ui();
    let overlay = crate::prototype::stroke_overlay(app);
    let mut cells: Vec<[i64; 2]> = overlay
        .iter()
        .map(|quad| {
            assert!(!quad.pickable);
            assert_eq!(quad.color, crate::workspace_render::STROKE_PREVIEW_TINT);
            match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a preview copy of no cell"),
            }
        })
        .collect();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    let presented = presentation
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.color == crate::workspace_render::STROKE_PREVIEW_TINT && !quad.pickable)
        .count();
    assert_eq!(presented, overlay.len());
    cells.sort_unstable();
    cells.dedup();
    assert_eq!(
        cells.len(),
        overlay.len(),
        "one copy per cell on this flat fixture"
    );
    cells
}
/// A pixel of the exact cell mask that a panel covers, if any.
fn under_panel(app: &mut Application) -> Option<[f32; 2]> {
    use crate::workspace_ui::Hit;
    app.refresh_ui();
    let frame = app.frame(true).unwrap();
    let cells: std::collections::BTreeSet<u32> =
        app.prepared.cell_picks.keys().map(|id| id.0).collect();
    let width = usize::try_from(frame.width).unwrap();
    let index = frame
        .semantic_ids
        .iter()
        .enumerate()
        .position(|(index, sample)| {
            let px = u32::try_from(index % width).unwrap();
            let py = u32::try_from(index / width).unwrap();
            cells.contains(sample) && !matches!(app.ui.hit(px, py), Hit::World(..))
        })?;
    #[allow(clippy::cast_precision_loss)]
    Some([(index % width) as f32 + 0.5, (index / width) as f32 + 0.5])
}
/// The region the engine expands, read from the committed World directly.
fn region(app: &Application, at: [i64; 2], contiguous: bool) -> Vec<[i64; 2]> {
    fill::region(
        app.session.history().document().world(),
        app.session.catalog(),
        "main",
        0,
        "floor",
        at,
        contiguous,
    )
    .unwrap()
    .cells
}
fn choose(app: &mut Application, prototype: &str) {
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: prototype.into(),
    })
    .unwrap();
}
fn terrain_value(app: &Application, x: i64, y: i64) -> Option<String> {
    let chunk = &app.session.world().planes()["main"].chunks[&(0, 0, 0)];
    let local = usize::try_from(y * 32 + x).unwrap();
    chunk.ground[0].value(local).map(str::to_owned)
}

#[test]
fn f_and_the_rule_rows_choose_the_brush_fill_or_replace() {
    let (_temporary, mut app) = brush_application();
    let texts = sidebar_texts(&mut app);
    let rule = texts.iter().position(|text| text == "Rule  F").unwrap();
    // After the palette, so every row above keeps its place.
    let palette = texts
        .iter()
        .position(|text| text == "floor / sand")
        .unwrap();
    assert!(rule > palette, "{texts:?}");
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    for expected in [TerrainRule::Fill, TerrainRule::Replace, TerrainRule::Brush] {
        route(&mut app, &key(Keycode::F)).unwrap();
        assert_eq!(app.tools.terrain_rule(), expected);
    }
    // A rule row, pressed with the pointer, chooses its rule and is marked.
    app.refresh_ui();
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::TerrainRule(TerrainRule::Replace)))
        .cloned()
        .unwrap();
    assert!(!widget.active);
    #[allow(clippy::cast_precision_loss)]
    let at = [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ];
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, at)).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Replace);
    app.refresh_ui();
    let active: Vec<_> = app
        .ui
        .widgets
        .iter()
        .filter(|widget| matches!(widget.control, Some(Control::TerrainRule(_))) && widget.active)
        .map(|widget| widget.control.clone())
        .collect();
    assert_eq!(active, [Some(Control::TerrainRule(TerrainRule::Replace))]);
    // The brush keys and rows do nothing while the rule clicks.
    let brush = app.tools.brush();
    route(&mut app, &key(Keycode::RightBracket)).unwrap();
    route(&mut app, &key(Keycode::B)).unwrap();
    assert_eq!(app.tools.brush(), brush);
    let enabled = |app: &Application, control: &Control| {
        app.ui
            .widgets
            .iter()
            .find(|widget| widget.control.as_ref() == Some(control))
            .unwrap()
            .enabled
    };
    app.refresh_ui();
    assert!(!enabled(&app, &Control::BrushSize(1)));
    assert!(!enabled(&app, &Control::BrushShape));
    // F means nothing outside Terrain mode.
    app.control(Control::Select).unwrap();
    route(&mut app, &key(Keycode::F)).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Replace);
    app.control(Control::Terrain).unwrap();
    route(&mut app, &key(Keycode::F)).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    app.refresh_ui();
    assert!(enabled(&app, &Control::BrushSize(1)));
    route(&mut app, &key(Keycode::RightBracket)).unwrap();
    assert_eq!(app.tools.brush().radius, brush.radius + 1);
}

#[test]
fn hovering_previews_the_engines_cells_once_and_prepares_nothing() {
    let (temporary, mut app) = brush_application();
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let grass = cell_pixel(&mut app, 1, 1);
    let sand = cell_pixel(&mut app, 5, 1);
    // With the brush rule, a hover is nothing.
    route(&mut app, &hover(grass)).unwrap();
    assert!(app.hover.is_none() && app.fill_preview.is_none());
    assert!(marked(&mut app).is_empty());

    route(&mut app, &key(Keycode::F)).unwrap();
    let scenes = app.prepared_scenes;
    route(&mut app, &hover(grass)).unwrap();
    let expected = region(&app, [1, 1], true);
    assert_eq!(expected.len(), 24, "the grass west of x = 4");
    let action = app
        .tools
        .fill_action(
            app.session.catalog(),
            &app.hover.as_ref().unwrap().1,
            app.session.history().document().revision(),
            false,
        )
        .unwrap();
    assert_eq!(app.session.fill_changes(&action).unwrap(), expected);
    assert_eq!(app.fill_changes().unwrap(), expected.as_slice());
    assert_eq!(marked(&mut app), expected);
    assert_eq!(
        app.status_label(),
        "Fill from (1, 1) with sand: 24 cells to change; click to review."
    );
    // Computed once: redrawing and hovering the same cell keep it.
    let kept = app
        .fill_preview
        .as_ref()
        .unwrap()
        .changes
        .as_ref()
        .unwrap()
        .as_ptr();
    acknowledge_ui(&mut app);
    route(&mut app, &hover(grass)).unwrap();
    app.refresh_ui();
    assert_eq!(
        app.fill_preview
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .as_ptr(),
        kept
    );
    // Nothing is prepared, written or rebuilt by hovering.
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.prepared_scenes, scenes);
    assert_eq!(fs::read(&chunk).unwrap(), original);

    // Replace with grass from a sand cell: every sand cell of the view.
    choose(&mut app, "grass");
    route(&mut app, &key(Keycode::F)).unwrap();
    route(&mut app, &hover(sand)).unwrap();
    let expected = region(&app, [5, 1], false);
    assert_eq!(expected.len(), 23);
    assert_eq!(marked(&mut app), expected);
    assert!(
        app.status_label()
            .starts_with("Replace from (5, 1) with grass: 23 cells")
    );
    app.capture("replace-preview").unwrap();
    let record: serde_json::Value = serde_json::from_slice(
        &fs::read(temporary.path().join("evidence/replace-preview.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        record["fill_preview"],
        serde_json::json!({"rule": "replace", "at": [5, 1], "proto": "grass", "cells": 23,
            "first": expected[..8]})
    );
    // Off the map, over a panel: no hover and no preview.
    let x = app.ui.sidebar_x();
    #[allow(clippy::cast_precision_loss)]
    route(&mut app, &hover([x as f32 + 4.0, 200.0])).unwrap();
    assert!(app.hover.is_none() && app.fill_preview.is_none());
    assert!(marked(&mut app).is_empty());
    // A map cell drawn under the sidebar is under the panel, not hovered:
    // pan the view right until the east columns lie beneath it.
    app.view.camera.center[0] -= 8.0;
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    let covered = under_panel(&mut app).expect("a cell drawn under the sidebar");
    route(&mut app, &hover(covered)).unwrap();
    assert!(app.hover.is_none() && app.fill_preview.is_none());
    app.view.camera.center[0] += 8.0;
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    // A hover that would change nothing shows the engine's refusal.
    route(&mut app, &hover(grass)).unwrap();
    assert!(app.fill_changes().is_none());
    assert!(marked(&mut app).is_empty());
    assert!(
        app.status_label()
            .starts_with("Replace from (1, 1) refused: fill changes no cell"),
        "{}",
        app.status_label()
    );
    assert!(app.session.history().pending().is_none());
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

#[test]
fn a_click_prepares_the_previewed_cells_as_one_reviewed_change() {
    let (temporary, mut app) = brush_application();
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let at = cell_pixel(&mut app, 2, 3);
    route(&mut app, &key(Keycode::F)).unwrap();
    route(&mut app, &hover(at)).unwrap();
    let previewed = app.fill_changes().unwrap().to_vec();
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, at)).unwrap();
    route(&mut app, &button(false, at)).unwrap();
    let pending = app.session.history().pending().unwrap();
    let text = pending.sealed().to_owned();
    assert_eq!(pending.changed().count(), 1);
    assert_eq!(pending.sealed().matches(r#""op":"terrain.set""#).count(), 1);
    let sealed: serde_json::Value = serde_json::from_str(pending.sealed()).unwrap();
    let prepared: Vec<[i64; 2]> = sealed["operations"][0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            [
                cell["at"][0].as_i64().unwrap(),
                cell["at"][1].as_i64().unwrap(),
            ]
        })
        .collect();
    assert_eq!(prepared, previewed);
    assert!(app.tools.stroke().is_none());
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(app.recovery.records().is_empty());
    // While it is pending there is no preview, and another click is fenced.
    assert!(marked(&mut app).is_empty());
    acknowledge_ui(&mut app);
    let other = cell_pixel(&mut app, 5, 1);
    route(&mut app, &hover(other)).unwrap();
    assert!(app.fill_changes().is_none());
    let fenced = route(&mut app, &button(true, other)).unwrap_err();
    assert!(fenced.contains("pending"), "{fenced}");
    assert_eq!(app.session.history().pending().unwrap().sealed(), text);
    // Accept publishes the one change with one recovery record.
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Return)).unwrap();
    assert_eq!(app.recovery.records().len(), 1);
    for &[x, y] in &previewed {
        assert_eq!(terrain_value(&app, x, y).as_deref(), Some("sand"));
    }
    assert_eq!(app.session.history().depths(), (1, 0));
    let step = app.session.applied_steps().last().unwrap();
    assert!(matches!(
        step.origin.unwrap().action(),
        Action::FillTerrain {
            x: 2,
            y: 3,
            contiguous: true,
            ..
        }
    ));
    // One undo restores the bytes.
    app.action(Action::Undo).unwrap();
    app.action(Action::Accept).unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
}

#[test]
fn the_scripted_fill_demonstration_changes_nothing_and_restores_the_brush() {
    let (temporary, mut app) = brush_application();
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    let choice = app.tools.terrain_choice().cloned();
    app.fill_demo().unwrap();
    assert_eq!(fs::read(&chunk).unwrap(), original);
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.session.history().depths(), (0, 0));
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    assert_eq!(app.tools.terrain_choice().cloned(), choice);
    assert!(app.hover.is_none());
    let read = |name: &str| -> serde_json::Value {
        serde_json::from_slice(&fs::read(temporary.path().join("evidence").join(name)).unwrap())
            .unwrap()
    };
    assert_eq!(read("fill-preview.json")["fill_preview"]["rule"], "fill");
    assert_eq!(read("fill-preview.json")["fill_preview"]["cells"], 24);
    assert_eq!(
        read("replace-preview.json")["fill_preview"]["rule"],
        "replace"
    );
    assert_eq!(read("replace-preview.json")["fill_preview"]["cells"], 23);
    assert_eq!(read("replace-pending.json")["pending"], true);
    assert!(read("replace-pending.json").get("fill_preview").is_none());
}
