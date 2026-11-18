// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A zoom or pan whose view cannot be drawn is refused, and a rendering
//! block ends when the view can be drawn again. Every world here is a
//! temporary copy of the MIT neutral sample, made isometric (32 x 16
//! diamond, its 32 x 32 ground art unchanged) with steep stored heights
//! where a test needs a view that cannot be drawn: the renderer cannot bend
//! that art over them at zoom 7.45 and above.
// Zoom values are copied from the camera, never computed, so they compare exactly.
#![allow(clippy::float_cmp)]
use super::proposal::edit_json;
use super::zones::{button, key, route, zone_application};
use crate::prototype::{
    Application, PAN_REFUSED, VIEW_RESUMED, ViewNote, ZOOM_IN_REFUSED, ZOOM_OUT_REFUSED,
};
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::keyboard::Keycode;
use serde_json::json;
use std::path::{Path, PathBuf};
use tilewright_editor_ui::Action;

const MAIN: &str = "planes/main/layers/0/0_0.jsonc";
const VIEW_PAUSED: &str = "Editing paused: this view cannot be drawn.";
const SOURCES_PAUSED: &str = "Editing paused: catalog or art files changed.";

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
    // Lines are wrapped at spaces: joined with one they read whole.
    app.review_lines(620)
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The review row's widget for `control`: whether it is enabled, and its
/// centre.
fn review_button(app: &mut Application, control: &Control) -> (bool, [f32; 2]) {
    app.refresh_ui();
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control.as_ref() == Some(control))
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    let centre = [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ];
    (widget.enabled, centre)
}

/// A click on the review row's `control`, on the presented frame.
fn press(app: &mut Application, control: &Control) -> Result<(), String> {
    super::acknowledge_ui(app);
    let (enabled, at) = review_button(app, control);
    assert!(enabled, "{control:?} is disabled");
    route(app, &button(true, at))?;
    route(app, &button(false, at))
}

/// Changes the session's `preview.jsonc` on disk; returns its path and the
/// bytes the session loaded.
fn change_catalog(root: &Path) -> (PathBuf, Vec<u8>) {
    let catalog = root.join("world/preview.jsonc");
    let original = std::fs::read(&catalog).unwrap();
    std::fs::write(&catalog, [original.as_slice(), b"\n"].concat()).unwrap();
    (catalog, original)
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
            details(&mut app).contains("The view stays where it was: invalid world view"),
            "{}",
            details(&mut app)
        );
    }
    // A zoom out that draws replaces the word; zooming in is refused again.
    route(&mut app, &key(Keycode::Minus)).unwrap();
    assert!(app.view_note.is_none());
    assert_eq!(app.view.camera.zoom, good.zoom * 0.8);
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert_eq!(app.view.camera.zoom, good.zoom * 0.8 * 1.25);
    let back = app.view.camera;
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert_eq!(app.view.camera, back);
    assert_eq!(app.status_label(), ZOOM_IN_REFUSED);
    // A later refused input's notice wins over the older word, and the next
    // refused zoom is newer than that notice.
    app.notice = Some("no selectable semantic hit at this pixel".into());
    assert_eq!(app.status_label(), "Input not applied. Details below.");
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert!(app.notice.is_none());
    assert_eq!(app.status_label(), ZOOM_IN_REFUSED);
    // Editing continues: an edit is prepared and accepted at this zoom.
    paint(&mut app, 6, 4).unwrap();
    assert!(app.blocked.is_none());
    assert!(app.session.history().pending().is_some());
    super::acknowledge_ui(&mut app);
    app.action(Action::Accept).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.status_label(), "Changes saved. Ready to edit.");
    // Any control replaces the word.
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert_eq!(app.status_label(), ZOOM_IN_REFUSED);
    app.control(Control::Terrain).unwrap();
    assert!(app.view_note.is_none());
    assert!(app.blocked.is_none());
}

#[test]
fn the_view_words_fit_the_status_line_of_the_smallest_window() {
    let (_temporary, mut app) = zone_application(|_| {});
    app.ui = WorkspaceUi::new((720, 480), (720, 480));
    // The status widget holds its text as fitted to its width.
    let fits = |app: &mut Application| {
        app.refresh_ui();
        let status = app.status_label();
        assert!(
            app.ui.widgets.iter().any(|widget| widget.text == status),
            "{status} is cut at 720 x 480"
        );
    };
    for status in [ZOOM_IN_REFUSED, ZOOM_OUT_REFUSED, PAN_REFUSED] {
        app.view_note = Some(ViewNote::Refused {
            status,
            reason: "invalid world view".into(),
        });
        fits(&mut app);
    }
    app.view_note = Some(ViewNote::Resumed);
    fits(&mut app);
    app.view_note = None;
    for cause in [
        crate::prototype::BlockCause::View,
        crate::prototype::BlockCause::Sources,
        crate::prototype::BlockCause::Present,
    ] {
        app.blocked = Some(crate::prototype::Blocked::new(cause, "reason"));
        fits(&mut app);
    }
}

/// A paint of tile (6, 4), empty in the sample, prepared at zoom 5.96,
/// where it draws; then the zoom set to 8 and the frame rebuilt directly.
/// At 8 the new ground bends over the steep heights around it and cannot
/// be drawn: a render block of a pending World. (The desktop's own keys
/// cannot reach this: zooming in is refused, and an edit that cannot be
/// drawn is not left pending.)
fn blocked_with_a_pending_change() -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = zone_application(|root| steep(root, [5, 3], 512));
    app.view.camera.zoom = 5.960_464_5;
    app.rebuild().unwrap();
    paint(&mut app, 6, 4).unwrap();
    assert!(app.session.history().pending().is_some());
    app.view.camera.zoom = 8.0;
    assert_eq!(app.rebuild().unwrap_err(), "invalid world view");
    assert_eq!(
        app.blocked.as_ref().map(|blocked| blocked.reason.as_str()),
        Some("invalid world view")
    );
    assert!(!current(&app));
    assert_eq!(app.status_label(), VIEW_PAUSED);
    let lines = details(&mut app);
    assert!(
        lines.starts_with(
            "Rendering blocked: invalid world view Editing resumes once the view can be drawn: zoom out, pan or Cancel."
        ),
        "{lines}"
    );
    (temporary, app)
}

#[test]
fn every_action_but_cancel_is_refused_while_blocked() {
    let (_temporary, mut app) = blocked_with_a_pending_change();
    for action in [
        Action::Accept,
        Action::Undo,
        Action::Redo,
        Action::Reopen,
        Action::ClearSelection,
    ] {
        let name = format!("{action:?}");
        let error = app.action(action).unwrap_err();
        assert_eq!(
            error, "rendering blocked editing: invalid world view",
            "{name}"
        );
    }
    let error = paint(&mut app, 5, 0).unwrap_err();
    assert_eq!(error, "rendering blocked editing: invalid world view");
    assert!(app.prepare_proposal_step().is_err());
    assert!(app.continue_proposal().is_err());
    assert!(app.session.history().pending().is_some());
    assert!(app.blocked.is_some());
    // Accept stays disabled; Cancel is offered.
    super::acknowledge_ui(&mut app);
    assert!(!review_button(&mut app, &Control::Accept).0);
    assert!(review_button(&mut app, &Control::Cancel).0);
}

#[test]
fn while_blocked_zoom_and_pan_are_kept_and_the_first_view_that_draws_ends_the_block() {
    let (_temporary, mut app) = blocked_with_a_pending_change();
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
fn the_cancel_button_works_while_blocked_and_a_changed_catalog_needs_its_bytes_back() {
    // Not a view problem: the catalog changed on disk under the session, so
    // a zoom cannot be refused into a good view either. It blocks, and only
    // a rebuild that verifies the catalog again ends the block.
    let (temporary, mut app) = zone_application(|_| {});
    paint(&mut app, 5, 0).unwrap();
    let (catalog, original) = change_catalog(temporary.path());
    let zoom = app.view.camera.zoom;
    let error = route(&mut app, &key(Keycode::Equals)).unwrap_err();
    assert_eq!(error, "preview catalog changed");
    assert_eq!(app.view.camera.zoom, zoom);
    assert!(app.view_note.is_none());
    assert_eq!(app.status_label(), SOURCES_PAUSED);
    let lines = details(&mut app);
    assert!(
        lines.contains("The session's catalog or art files changed on disk. Editing resumes when they match what the session loaded again"),
        "{lines}"
    );
    assert!(
        lines.starts_with("Rendering blocked: preview catalog changed The session's"),
        "{lines}"
    );
    assert!(
        !lines.contains("zoom out") && !lines.contains("reopen"),
        "{lines}"
    );
    // The Cancel button drops the pending change; the block stays.
    press(&mut app, &Control::Cancel).unwrap_err();
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_some());
    // Still changed: the next zoom is kept, the block stays.
    assert!(route(&mut app, &key(Keycode::Minus)).is_err());
    assert_eq!(app.view.camera.zoom, zoom * 0.8);
    assert_eq!(app.status_label(), SOURCES_PAUSED);
    std::fs::write(&catalog, &original).unwrap();
    route(&mut app, &key(Keycode::Equals)).unwrap();
    assert!(app.blocked.is_none());
    assert_eq!(app.status_label(), VIEW_RESUMED);
    // The next rebuild does not repeat the word.
    route(&mut app, &key(Keycode::Minus)).unwrap();
    assert!(app.view_note.is_none());
}

#[test]
fn the_cancel_button_ends_a_block_of_a_pending_world_that_cannot_be_drawn() {
    let (_temporary, mut app) = blocked_with_a_pending_change();
    press(&mut app, &Control::Cancel).unwrap();
    assert!(app.session.history().pending().is_none());
    assert!(app.blocked.is_none());
    assert!(current(&app));
    assert_eq!(app.status_label(), VIEW_RESUMED);
    // Editing works again, and the word goes with the next rebuild.
    paint(&mut app, 5, 0).unwrap();
    assert!(app.view_note.is_none());
    assert_eq!(app.status_label(), "Review changes, then Accept or Cancel.");
}
