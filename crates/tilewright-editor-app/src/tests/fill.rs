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
/// The centre of the rule header row, when it is laid out within the
/// window.
fn rule_header(app: &mut Application) -> Option<[f32; 2]> {
    app.refresh_ui();
    let (_, height) = app.ui.size;
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.text.starts_with("Rule: "))?;
    (widget.bounds.y + widget.bounds.height <= height).then(|| {
        #[allow(clippy::cast_precision_loss)]
        [
            (widget.bounds.x + widget.bounds.width / 2) as f32,
            (widget.bounds.y + widget.bounds.height / 2) as f32,
        ]
    })
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
    // The current rule heads the sidebar; the rule rows follow the palette.
    let header = texts
        .iter()
        .position(|text| text == "Rule: Brush  F")
        .unwrap();
    let fill = texts
        .iter()
        .position(|text| text.starts_with("Fill: click"))
        .unwrap();
    let palette = texts
        .iter()
        .position(|text| text == "floor / sand")
        .unwrap();
    assert!(header < palette && fill > palette, "{texts:?}");
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    for (expected, name) in [
        (TerrainRule::Fill, "Fill"),
        (TerrainRule::Replace, "Replace"),
        (TerrainRule::Road, "Road"),
        (TerrainRule::Brush, "Brush"),
    ] {
        route(&mut app, &key(Keycode::F)).unwrap();
        assert_eq!(app.tools.terrain_rule(), expected);
        assert!(sidebar_texts(&mut app).contains(&format!("Rule: {name}  F")));
    }
    // Pressing the header moves to the next rule, as F does.
    let header = rule_header(&mut app).unwrap();
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, header)).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Fill);
    for _ in 0..3 {
        route(&mut app, &key(Keycode::F)).unwrap();
    }
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    // A rule row, pressed with the pointer, chooses its rule and is marked.
    app.refresh_ui();
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| {
            widget.control == Some(Control::TerrainRule(TerrainRule::Replace))
                && !widget.text.starts_with("Rule:")
        })
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
    // The Road rule uses the brush: its width and shape are the road's.
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Road);
    app.refresh_ui();
    assert!(enabled(&app, &Control::BrushSize(1)));
    route(&mut app, &key(Keycode::F)).unwrap();
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    app.refresh_ui();
    assert!(enabled(&app, &Control::BrushSize(1)));
    route(&mut app, &key(Keycode::RightBracket)).unwrap();
    assert_eq!(app.tools.brush().radius, brush.radius + 1);
}

#[test]
#[allow(clippy::too_many_lines)]
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
    assert!(app.hover.cell.is_none() && app.hover.preview.is_none());
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
            &app.hover.cell.as_ref().unwrap().1,
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
        .hover
        .preview
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
        app.hover
            .preview
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
    assert!(app.hover.cell.is_none() && app.hover.preview.is_none());
    assert!(marked(&mut app).is_empty());
    // A map cell drawn under the sidebar is under the panel, not hovered:
    // pan the view right until the east columns lie beneath it.
    app.view.camera.center[0] -= 8.0;
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    let covered = under_panel(&mut app).expect("a cell drawn under the sidebar");
    route(&mut app, &hover(covered)).unwrap();
    assert!(app.hover.cell.is_none() && app.hover.preview.is_none());
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
    assert!(app.hover.cell.is_none());
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

/// A 1280x800 Terrain-mode workspace like `brush_application`, over a world
/// prepared by `prepare`.
fn terrain_application(prepare: impl FnOnce(&std::path::Path)) -> (tempfile::TempDir, Application) {
    use crate::workspace_ui::WorkspaceUi;
    let (temporary, mut app) = super::application_with_source(prepare);
    app.ui = WorkspaceUi::new(super::BRUSH_WINDOW, super::BRUSH_WINDOW);
    app.view.camera.width = super::BRUSH_WINDOW.0;
    app.view.camera.height = super::BRUSH_WINDOW.1;
    app.rebuild().unwrap();
    app.control(Control::Terrain).unwrap();
    choose(&mut app, "sand");
    acknowledge_ui(&mut app);
    (temporary, app)
}

#[test]
fn the_rule_header_stays_on_screen_with_an_object_selected() {
    use crate::workspace_ui::WorkspaceUi;
    for size in [(1280, 800), (720, 480)] {
        let (_temporary, mut app) = super::application();
        app.ui = WorkspaceUi::new(size, size);
        app.view.camera.width = size.0;
        app.view.camera.height = size.1;
        app.rebuild().unwrap();
        app.control(Control::Terrain).unwrap();
        app.action(Action::Select {
            generation: app.session.generation(),
            key: SelectionKey::Object {
                uid: "gate_west".into(),
            },
        })
        .unwrap();
        assert!(app.session.selection().is_some());
        route(&mut app, &key(Keycode::F)).unwrap();
        assert!(rule_header(&mut app).is_some(), "{size:?}");
        assert!(sidebar_texts(&mut app).contains(&"Rule: Fill  F".to_owned()));
    }
}

#[test]
fn hovering_builds_one_mask_per_frame_changes_no_state_and_rests_while_pending() {
    let (_temporary, mut app) = brush_application();
    route(&mut app, &key(Keycode::F)).unwrap();
    let pixels: Vec<[f32; 2]> = [[0, 0], [1, 1], [2, 3], [5, 1], [7, 5], [3, 4]]
        .iter()
        .map(|&[x, y]| cell_pixel(&mut app, x, y))
        .collect();
    let selection = app.session.selection().cloned();
    let revision = app.session.history().document().revision();
    let depths = app.session.history().depths();
    let records = app.recovery.records().len();
    let generation = app.session.generation();
    let before = app.hover.masks;
    let started = std::time::Instant::now();
    let mut events = 0u32;
    for round in 0..20 {
        for (index, &[x, y]) in pixels.iter().enumerate() {
            // The cell centre and a pixel beside it: the same cell twice.
            route(&mut app, &hover([x, y])).unwrap();
            route(&mut app, &hover([x + 1.0, y])).unwrap();
            events += 2;
            if round == 0 && index == 0 {
                assert_eq!(app.hover.masks, before + 1);
            }
        }
    }
    let per_event = started.elapsed() / events;
    eprintln!(
        "hover: {events} events, {per_event:?} each, {} masks",
        app.hover.masks - before
    );
    // One mask for the frame, whatever the number of motion events.
    assert_eq!(app.hover.masks, before + 1);
    assert!(app.hover.preview.is_some());
    // Hovering changes no session state.
    assert_eq!(app.session.selection().cloned(), selection);
    assert_eq!(app.session.history().document().revision(), revision);
    assert_eq!(app.session.history().depths(), depths);
    assert_eq!(app.recovery.records().len(), records);
    assert_eq!(app.session.generation(), generation);
    assert!(app.session.history().pending().is_none());
    // A pending change: hovering builds nothing and previews nothing.
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, pixels[1])).unwrap();
    assert!(app.session.history().pending().is_some());
    let masks = app.hover.masks;
    for &at in &pixels {
        route(&mut app, &hover(at)).unwrap();
        app.refresh_ui();
    }
    assert_eq!(app.hover.masks, masks);
    assert!(app.hover.cell.is_none() && app.hover.preview.is_none());
}

#[test]
fn a_pending_changes_status_lines_outrank_the_fill() {
    // A pending fill: its review line stays through pointer motion.
    let (_temporary, mut app) = brush_application();
    route(&mut app, &key(Keycode::F)).unwrap();
    let at = cell_pixel(&mut app, 2, 3);
    let other = cell_pixel(&mut app, 5, 1);
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, at)).unwrap();
    let pending = app.status_label();
    assert_eq!(pending, "Review changes, then Accept or Cancel.");
    for point in [at, other] {
        route(&mut app, &hover(point)).unwrap();
        assert_eq!(app.status_label(), pending);
    }
    // A refused click's notice stays through pointer motion.
    route(&mut app, &key(Keycode::Escape)).unwrap();
    choose(&mut app, "grass");
    route(&mut app, &key(Keycode::F)).unwrap();
    acknowledge_ui(&mut app);
    let grass = cell_pixel(&mut app, 1, 1);
    acknowledge_ui(&mut app);
    let refused = route(&mut app, &button(true, grass)).unwrap_err();
    assert!(refused.contains("fill changes no cell"), "{refused}");
    // The native loop keeps a refusal as the notice.
    app.notice = Some(refused);
    assert_eq!(app.status_label(), "Input not applied. Details below.");
    route(&mut app, &hover(other)).unwrap();
    assert_eq!(app.status_label(), "Input not applied. Details below.");

    // A pending change whose review is cut short keeps the review-limit line.
    let (_temporary, mut app) = terrain_application(|world| {
        let schema_path = world.join("profile/schema/object.jsonc");
        let mut schema: serde_json::Value =
            serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
        schema["fields"]["label"]["max_length"] = serde_json::Value::from(80_000);
        fs::write(schema_path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
        let path = world.join("planes/main/layers/0/0_0.jsonc");
        let mut source: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        source["objects"][0]["fields"]["label"] = serde_json::Value::String("a".repeat(70_000));
        fs::write(path, serde_json::to_vec(&source).unwrap()).unwrap();
    });
    route(&mut app, &key(Keycode::F)).unwrap();
    app.action(Action::SetField {
        uid: "marker_1".into(),
        field: "label".into(),
        value: serde_json::Value::String("b".repeat(100)),
        expected: app.session.history().document().revision(),
    })
    .unwrap();
    assert!(!app.source_review().complete);
    let grass = cell_pixel(&mut app, 1, 1);
    route(&mut app, &hover(grass)).unwrap();
    assert_eq!(
        app.status_label(),
        tilewright_editor_ui::review::REVIEW_LIMIT_MESSAGE
    );
}

#[test]
fn a_proposal_step_keeps_its_status_and_the_fill_preview_wins_its_cells() {
    let mut twin = super::proposal::twin(3, |_| {}, |_| {}, None);
    super::proposal::open(&mut twin);
    let app = &mut twin.app;
    app.control(Control::Terrain).unwrap();
    choose(app, "sand");
    route(app, &key(Keycode::F)).unwrap();
    acknowledge_ui(app);
    // Step 1 moves marker_1 from (3, 2) to (4, 2). The fill from (1, 1)
    // previews the grass of columns 0 to 3, so (3, 2) shows the preview and
    // only (4, 2) the highlight.
    let grass = cell_pixel(app, 1, 1);
    route(app, &hover(grass)).unwrap();
    assert!(app.fill_changes().unwrap().contains(&[3, 2]));
    app.refresh_ui();
    let highlighted: Vec<[i64; 2]> = crate::prototype::proposal_overlay(app)
        .iter()
        .map(
            |quad| match app.prepared.cell_picks.get(&quad.semantic_id) {
                Some(SelectionKey::Cell { x, y, .. }) => [*x, *y],
                _ => panic!("a highlight copy of no cell"),
            },
        )
        .collect();
    assert!(highlighted.contains(&[4, 2]), "{highlighted:?}");
    assert!(!highlighted.contains(&[3, 2]), "{highlighted:?}");
    // The step pending: its status line stays through pointer motion.
    acknowledge_ui(app);
    route(app, &key(Keycode::G)).unwrap();
    assert!(app.session.history().pending().is_some());
    let status = app.status_label();
    assert!(
        status.starts_with("Step 1 of 3 matches its recording"),
        "{status}"
    );
    route(app, &hover(grass)).unwrap();
    assert_eq!(app.status_label(), status);
}

#[test]
fn the_preview_follows_the_terrain_and_a_cancel_without_pointer_motion() {
    let (_temporary, mut app) = brush_application();
    route(&mut app, &key(Keycode::F)).unwrap();
    let grass = cell_pixel(&mut app, 1, 1);
    route(&mut app, &hover(grass)).unwrap();
    assert_eq!(app.fill_changes().unwrap().len(), 24);
    // Grass on grass changes nothing: no preview, the refusal instead.
    choose(&mut app, "grass");
    app.refresh_ui();
    assert!(app.fill_changes().is_none());
    assert!(
        app.status_label().contains("refused"),
        "{}",
        app.status_label()
    );
    choose(&mut app, "sand");
    app.refresh_ui();
    assert_eq!(app.fill_changes().unwrap().len(), 24);
    // Click, then Cancel: the preview comes back with no new motion.
    acknowledge_ui(&mut app);
    route(&mut app, &button(true, grass)).unwrap();
    assert!(app.session.history().pending().is_some());
    app.refresh_ui();
    assert!(app.fill_changes().is_none());
    route(&mut app, &key(Keycode::Escape)).unwrap();
    assert!(app.session.history().pending().is_none());
    app.refresh_ui();
    assert_eq!(app.fill_changes().unwrap().len(), 24);
    // The rule changed with F: nothing until the pointer moves, then
    // Replace from the cell under it.
    route(&mut app, &key(Keycode::F)).unwrap();
    app.refresh_ui();
    assert!(app.fill_changes().is_none() && app.hover.cell.is_none());
    assert!(!app.status_label().contains("from ("));
    route(&mut app, &hover(grass)).unwrap();
    assert_eq!(app.fill_changes().unwrap().len(), 24);
    assert!(app.status_label().starts_with("Replace from (1, 1)"));
}

fn window(win_event: sdl3::event::WindowEvent) -> Event {
    Event::Window {
        timestamp: 0,
        window_id: 7,
        win_event,
    }
}

#[test]
fn a_mode_or_rule_change_forgets_the_hover_until_the_pointer_moves() {
    let (_temporary, mut app) = brush_application();
    route(&mut app, &key(Keycode::F)).unwrap();
    let grass = cell_pixel(&mut app, 1, 1);
    let sand = cell_pixel(&mut app, 5, 1);
    route(&mut app, &hover(grass)).unwrap();
    assert!(app.fill_changes().is_some());
    // Leave Terrain mode by key, move elsewhere, come back by key: no
    // preview of (1, 1) under a pointer that is on (5, 1).
    route(&mut app, &key(Keycode::P)).unwrap();
    assert!(
        app.hover.mask.is_none(),
        "the mask is released with the mode"
    );
    route(&mut app, &hover(sand)).unwrap();
    route(&mut app, &key(Keycode::T)).unwrap();
    app.refresh_ui();
    assert!(app.hover.cell.is_none() && app.fill_changes().is_none());
    assert!(marked(&mut app).is_empty());
    assert!(
        !app.status_label().contains("(1, 1)"),
        "{}",
        app.status_label()
    );
    route(&mut app, &hover(sand)).unwrap();
    assert!(
        app.status_label().starts_with("Fill from (5, 1)"),
        "{}",
        app.status_label()
    );
    // The same through Zone mode, whose key goes through the mode controls.
    route(&mut app, &key(Keycode::N)).unwrap();
    route(&mut app, &hover(grass)).unwrap();
    route(&mut app, &key(Keycode::T)).unwrap();
    app.refresh_ui();
    assert!(app.hover.cell.is_none() && app.fill_changes().is_none());
    assert!(
        !app.status_label().contains("(5, 1)"),
        "{}",
        app.status_label()
    );
    route(&mut app, &hover(sand)).unwrap();
    // Brush, move, F: nothing until the pointer moves; Brush releases the
    // mask.
    for _ in 0..3 {
        route(&mut app, &key(Keycode::F)).unwrap();
    }
    assert_eq!(app.tools.terrain_rule(), TerrainRule::Brush);
    route(&mut app, &hover(grass)).unwrap();
    assert!(
        app.hover.mask.is_none(),
        "the mask is released with the rule"
    );
    route(&mut app, &key(Keycode::F)).unwrap();
    app.refresh_ui();
    assert!(app.hover.cell.is_none() && app.fill_changes().is_none());
    route(&mut app, &hover(grass)).unwrap();
    assert!(app.status_label().starts_with("Fill from (1, 1)"));
}

#[test]
fn a_pointer_that_leaves_the_window_hovers_nothing_even_after_a_rebuild() {
    use sdl3::event::WindowEvent;
    for leave in [WindowEvent::MouseLeave, WindowEvent::FocusLost] {
        let (_temporary, mut app) = brush_application();
        route(&mut app, &key(Keycode::F)).unwrap();
        let grass = cell_pixel(&mut app, 1, 1);
        route(&mut app, &hover(grass)).unwrap();
        assert!(app.fill_changes().is_some());
        route(&mut app, &window(leave)).unwrap();
        assert!(app.hover.cell.is_none() && app.hover.pixel.is_none());
        assert!(app.fill_changes().is_none());
        // A keyboard rebuild (a pan) does not bring it back.
        acknowledge_ui(&mut app);
        route(&mut app, &key(Keycode::D)).unwrap();
        app.refresh_ui();
        assert!(app.hover.cell.is_none() && app.fill_changes().is_none());
        assert!(marked(&mut app).is_empty());
    }
}

#[test]
fn a_still_pointer_after_a_pan_hovers_the_cell_now_under_it() {
    let (_temporary, mut app) = brush_application();
    route(&mut app, &key(Keycode::F)).unwrap();
    let at = cell_pixel(&mut app, 3, 3);
    route(&mut app, &hover(at)).unwrap();
    let before = app.hover.cell.as_ref().map(|cell| cell.1.clone());
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::D)).unwrap();
    app.refresh_ui();
    let after = app.hover.cell.as_ref().map(|cell| cell.1.clone());
    // What a fresh exact mask says is under the same pixel now.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let truth = app
        .prepared
        .pick(app.view.frame_revision, at[0] as u32, at[1] as u32, true)
        .unwrap()
        .filter(|key| matches!(key, SelectionKey::Cell { .. }));
    assert_ne!(after, before, "the pan moved the map under the pointer");
    assert_eq!(after, truth);
}

#[test]
fn a_hover_after_a_resize_picks_from_a_new_mask() {
    use crate::workspace_ui::WorkspaceUi;
    let (_temporary, mut app) = brush_application();
    route(&mut app, &key(Keycode::F)).unwrap();
    let at = cell_pixel(&mut app, 1, 1);
    route(&mut app, &hover(at)).unwrap();
    let masks = app.hover.masks;
    let frame = app.view.frame_revision;
    app.ui = WorkspaceUi::new((720, 480), (720, 480));
    app.view.camera.width = 720;
    app.view.camera.height = 480;
    app.reflow().unwrap();
    // A reflow keeps the frame generation; only the size changed.
    assert_eq!(frame, app.view.frame_revision);
    acknowledge_ui(&mut app);
    let moved = cell_pixel(&mut app, 5, 1);
    route(&mut app, &hover(moved)).unwrap();
    assert_eq!(app.hover.masks, masks + 1);
    assert!(matches!(
        app.hover.cell.as_ref().map(|cell| &cell.1),
        Some(SelectionKey::Cell { x: 5, y: 1, .. })
    ));
}

#[test]
fn without_cell_slots_the_fill_line_says_the_preview_is_unavailable() {
    let (_temporary, mut app) = brush_application();
    assert!(app.prepared.withhold_cell_slots());
    route(&mut app, &key(Keycode::F)).unwrap();
    let grass = cell_pixel(&mut app, 1, 1);
    route(&mut app, &hover(grass)).unwrap();
    assert!(app.fill_changes().is_some());
    assert!(crate::prototype::stroke_overlay(&app).is_empty());
    assert_eq!(
        app.status_label(),
        "Fill from (1, 1) with sand: 24 cells to change; click to review. Preview unavailable for this view."
    );
}
