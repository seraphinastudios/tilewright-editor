// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A zoom or pan whose view cannot be drawn is refused, and a rendering
//! block ends when the view can be drawn again. Every world here is a
//! temporary copy of the MIT neutral sample made isometric (32 x 16 diamond,
//! its 32 x 32 ground art unchanged) with steep stored heights: the renderer
//! cannot bend that art over them at zoom 7.45 and above.
// Zoom values are copied from the camera, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::proposal::edit_json;
use super::zones::{key, route, zone_application};
use crate::prototype::{Application, VIEW_RESUMED, ViewNote, ZOOM_IN_REFUSED};
use sdl3::keyboard::Keycode;
use serde_json::json;
use std::path::Path;
use tilewright_editor_ui::Action;

const MAIN: &str = "planes/main/layers/0/0_0.jsonc";

/// Makes the copy isometric and stores heights of +-`height` in the 3 x 3
/// tiles from `at` (a pattern that bends their middle tile the most), 0
/// elsewhere.
fn steep(root: &Path, at: [usize; 2], height: i32) {
    edit_json(root, "profile/profile.jsonc", |profile| {
        profile["projection"] = json!({"kind": "isometric", "tile_width": 32, "tile_height": 16});
    });
    let pattern = [[1, 1, -1], [1, -1, -1], [-1, -1, 1]];
    let rows: Vec<Vec<i32>> = (0..32)
        .map(|y: usize| {
            (0..32)
                .map(|x: usize| {
                    if (at[0]..at[0] + 3).contains(&x) && (at[1]..at[1] + 3).contains(&y) {
                        pattern[y - at[1]][x - at[0]] * height
                    } else {
                        0
                    }
                })
                .collect()
        })
        .collect();
    edit_json(root, MAIN, |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}

fn paint(app: &mut Application, x: i64, y: i64) -> Result<(), String> {
    let expected = app.session.history().document().revision();
    app.action(Action::Paint {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
        proto: "grass".into(),
        expected,
    })
}

/// Whether the prepared frame is the current view's.
fn current(app: &Application) -> bool {
    app.prepared.frame_revision == app.view.frame_revision
}

fn details(app: &mut Application) -> String {
    app.review_lines(620).join("\n")
}

#[test]
fn a_zoom_step_that_cannot_be_drawn_is_refused_and_editing_continues() {
    let (_temporary, mut app) = zone_application(|root| steep(root, [1, 1], 512));
    // Eight steps draw: 1.25 up to 5.96.
    for _ in 0..8 {
        route(&mut app, &key(Keycode::Equals)).unwrap();
        assert!(app.blocked.is_none());
        assert!(app.view_note.is_none());
    }
    let good = app.view.camera;
    assert_eq!(good.zoom, 5.960_464_5);
    // The next step, to 7.45, cannot be drawn: refused, not blocked.
    for _ in 0..3 {
        route(&mut app, &key(Keycode::Equals)).unwrap();
        assert_eq!(app.view.camera, good);
        assert!(app.blocked.is_none());
        assert!(current(&app));
        assert_eq!(app.status_label(), ZOOM_IN_REFUSED);
        assert!(
            details(&mut app).contains("View not changed: invalid world view"),
            "{}",
            details(&mut app)
        );
    }
    // Editing continues: an edit is prepared and accepted at this zoom.
    paint(&mut app, 6, 4).unwrap();
    assert!(app.blocked.is_none());
    assert!(app.session.history().pending().is_some());
    super::acknowledge_ui(&mut app);
    app.action(Action::Accept).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.status_label(), "Changes saved. Ready to edit.");
    // Back and forth: out works and clears the word, in is refused again.
    route(&mut app, &key(Keycode::Minus)).unwrap();
    assert!(app.view_note.is_none());
    assert_eq!(app.view.camera.zoom, good.zoom * 0.8);
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert_eq!(app.view.camera.zoom, good.zoom * 0.8 * 1.25);
    let back = app.view.camera;
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert_eq!(app.view.camera, back);
    assert_eq!(app.status_label(), ZOOM_IN_REFUSED);
    // Any control replaces the word.
    app.control(crate::workspace_ui::Control::Terrain).unwrap();
    assert!(app.view_note.is_none());
    assert!(app.blocked.is_none());
}

/// A view at zoom 8 that draws until tile (6, 4), empty in the sample,
/// gets ground: then its art bends over the steep heights around it.
fn blocked_by_a_pending_edit() -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = zone_application(|root| steep(root, [5, 3], 512));
    app.view.camera.zoom = 8.0;
    app.rebuild().unwrap();
    super::acknowledge_ui(&mut app);
    let error = paint(&mut app, 6, 4).unwrap_err();
    assert_eq!(error, "invalid world view");
    assert_eq!(app.blocked.as_deref(), Some("invalid world view"));
    assert!(app.session.history().pending().is_some());
    assert!(!current(&app));
    assert_eq!(
        app.status_label(),
        "Editing blocked. Recovery details below."
    );
    let lines = details(&mut app);
    assert!(lines.starts_with("Rendering blocked: invalid world view\nEditing resumes once"));
    (temporary, app)
}

#[test]
fn cancel_ends_a_block_from_a_pending_change_that_cannot_be_drawn() {
    let (_temporary, mut app) = blocked_by_a_pending_edit();
    // Accept and further edits stay refused while blocked.
    let refused = app.action(Action::Accept).unwrap_err();
    assert_eq!(refused, "rendering blocked editing: invalid world view");
    // Cancel goes back to the committed World, which draws.
    app.action(Action::Cancel).unwrap();
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_none());
    assert!(current(&app));
    assert_eq!(app.view_note, Some(ViewNote::Resumed));
    assert_eq!(app.status_label(), VIEW_RESUMED);
    // Editing works again, and the word goes with the next rebuild.
    paint(&mut app, 5, 0).unwrap();
    assert!(app.blocked.is_none());
    assert!(app.view_note.is_none());
    assert_eq!(app.status_label(), "Review changes, then Accept or Cancel.");
}

#[test]
fn while_blocked_zoom_and_pan_are_kept_and_the_first_view_that_draws_ends_the_block() {
    let (_temporary, mut app) = blocked_by_a_pending_edit();
    // A pan is kept although its view does not draw either: no step back.
    let center = app.view.camera.center;
    assert!(route(&mut app, &key(Keycode::D)).is_err());
    assert_eq!(app.view.camera.center, [center[0] + 1.0, center[1]]);
    assert!(app.blocked.is_some());
    // Zooming in is kept too (8 stays 8, the limit); zooming out to 6.4
    // draws and ends the block.
    assert!(route(&mut app, &key(Keycode::Equals)).is_err());
    assert!(app.blocked.is_some());
    route(&mut app, &key(Keycode::Minus)).unwrap();
    assert_eq!(app.view.camera.zoom, 6.4);
    assert!(app.blocked.is_none());
    assert!(current(&app));
    assert_eq!(app.status_label(), VIEW_RESUMED);
    // The pending change can now be reviewed on its frame and accepted.
    super::acknowledge_ui(&mut app);
    app.action(Action::Accept).unwrap();
    assert!(app.session.history().pending().is_none());
    // Zooming in again is refused rather than blocking.
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert_eq!(app.view.camera.zoom, 6.4);
    assert!(app.blocked.is_none());
    assert_eq!(app.status_label(), ZOOM_IN_REFUSED);
}

#[test]
fn a_changed_catalog_blocks_until_its_bytes_are_back() {
    // Not a view problem: the catalog changed on disk under the session, so
    // a zoom cannot be refused into a good view either. It blocks, and only
    // a rebuild that verifies the catalog again ends the block.
    let (temporary, mut app) = zone_application(|_| {});
    let catalog = temporary.path().join("world/preview.jsonc");
    let original = std::fs::read(&catalog).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    std::fs::write(&catalog, &changed).unwrap();
    let zoom = app.view.camera.zoom;
    let error = route(&mut app, &key(Keycode::Equals)).unwrap_err();
    assert!(error.contains("reopen required"), "{error}");
    assert_eq!(app.view.camera.zoom, zoom);
    assert!(app.blocked.as_deref().unwrap().contains("reopen required"));
    assert!(app.view_note.is_none());
    // Still changed: the next zoom is kept but the block stays.
    assert!(route(&mut app, &key(Keycode::Minus)).is_err());
    assert_eq!(app.view.camera.zoom, zoom * 0.8);
    assert!(app.blocked.is_some());
    std::fs::write(&catalog, &original).unwrap();
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert!(app.blocked.is_none());
    assert_eq!(app.status_label(), VIEW_RESUMED);
}
