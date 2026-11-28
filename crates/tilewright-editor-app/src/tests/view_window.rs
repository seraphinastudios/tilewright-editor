// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The moving view window: a pan that takes the camera's centre past the
//! view's edge moves the view with it, and what is drawn, picked and
//! edited follows; a pending change keeps the view where it is.
use super::zones::{key, route, zone_application};
use crate::prototype::Application;
use sdl3::keyboard::Keycode;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;

fn paint(app: &mut Application, x: i64, y: i64) -> Result<(), String> {
    let expected = app.session.history().document().revision();
    app.action(Action::Paint {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
        proto: "sand".into(),
        expected,
    })
}

fn picks_column(app: &Application, column: i64) -> bool {
    app.prepared
        .cell_picks
        .values()
        .any(|key| matches!(key, SelectionKey::Cell { x, .. } if *x == column))
}

fn pan(app: &mut Application, keycode: Keycode, times: usize) {
    for _ in 0..times {
        route(app, &key(keycode)).unwrap();
    }
}

#[test]
fn a_pan_past_the_edge_moves_the_view_and_editing_follows_it() {
    let (_temporary, mut app) = zone_application(|_| {});
    assert_eq!(app.view.bounds, [0, 0, 8, 6]);
    // Outside the catalog's view: refused.
    assert!(paint(&mut app, 9, 1).is_err());
    assert!(app.session.history().pending().is_none());
    // The centre moves from (4, 3) to (7, 3), still inside: nothing moves.
    pan(&mut app, Keycode::D, 3);
    assert_eq!(app.view.bounds, [0, 0, 8, 6]);
    // To (8, 3), outside: a view of the same size around tile 8.
    pan(&mut app, Keycode::D, 1);
    assert_eq!(app.view.bounds, [4, 0, 8, 6]);
    assert_eq!(app.session.catalog().view().2, [4, 0, 8, 6]);
    assert!(picks_column(&app, 11) && !picks_column(&app, 3));
    // The tile is in the moved view: the paint is prepared for review.
    paint(&mut app, 9, 1).unwrap();
    assert!(app.session.history().pending().is_some());
    // While it is pending, the view stays and the camera pans inside it.
    let center = app.view.camera.center;
    pan(&mut app, Keycode::D, 8);
    assert_eq!(app.view.bounds, [4, 0, 8, 6]);
    assert_eq!(app.view.camera.center, [center[0] + 8.0, center[1]]);
    app.action(Action::Cancel).unwrap();
    // The next pan moves it to the camera: centre 17, the view from 13.
    pan(&mut app, Keycode::D, 1);
    assert_eq!(app.view.bounds, [13, 0, 8, 6]);
    // And back, up and to the left, past the sample's chunk [-1, 0].
    pan(&mut app, Keycode::A, 30);
    pan(&mut app, Keycode::W, 4);
    assert_eq!(app.view.bounds, [-17, -4, 8, 6]);
    assert!(picks_column(&app, -10) && !picks_column(&app, -8));
}
