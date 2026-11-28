// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Edits refused while rendering pauses editing say so in the pause's own
//! words, not as if an edit were pending: the Delete key, a stroke's
//! release, and the anchor and portal edits.
use super::acknowledge_ui;
use super::anchors::{click_object, click_tile};
use super::sculpt_mode::{drag, heights, release, sculpt_application};
use super::zones::{key, route, zone_application};
use crate::prototype::{Application, BlockCause, Blocked};
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;

const PAUSED: &str = "rendering blocked editing: test";

fn pause(app: &mut Application) {
    app.blocked = Some(Blocked::new(BlockCause::Present, "test"));
    acknowledge_ui(app);
}

/// The Delete key on a selected object while paused, with nothing
/// pending: refused with the pause's reason, the
/// selection kept; once the pause ends the same key prepares the removal.
#[test]
fn the_delete_key_while_paused_says_why() {
    let (_temporary, mut app) = zone_application(|_| {});
    app.select_visible_object(crate::prototype::Capability::Delete)
        .unwrap();
    let selected = app.session.selection().cloned();
    assert!(selected.is_some());
    pause(&mut app);
    let error = route(&mut app, &key(Keycode::Delete)).unwrap_err();
    assert_eq!(error, PAUSED);
    assert_eq!(app.session.selection().cloned(), selected);
    assert!(app.session.history().pending().is_none());
    app.blocked = None;
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::Delete)).unwrap();
    assert!(app.session.history().pending().is_some());
}

/// A stroke released after editing paused mid-drag (a frame that could
/// not be shown) prepares nothing, in the pause's words, and ends.
#[test]
fn a_stroke_released_while_paused_says_why() {
    let (_temporary, mut app) = sculpt_application(|root| heights(root, -64, 64, |_, _| 0));
    drag(&mut app, [1, 1], &[]);
    assert!(app.tools.stroke().is_some());
    pause(&mut app);
    assert_eq!(release(&mut app, [1, 1]).unwrap_err(), PAUSED);
    assert!(app.tools.stroke().is_none());
    assert!(app.session.history().pending().is_none());
    // Once the pause ends the next stroke prepares its change.
    app.blocked = None;
    acknowledge_ui(&mut app);
    drag(&mut app, [1, 1], &[]);
    release(&mut app, [1, 1]).unwrap();
    assert!(app.session.history().pending().is_some());
}

/// Anchor Move and Delete and the portal rows while paused: each refused
/// with the pause's reason, nothing prepared; choosing an anchor stays
/// open.
#[test]
fn anchor_and_portal_edits_while_paused_say_why() {
    let (_temporary, mut app) = zone_application(|_| {});
    route(&mut app, &key(Keycode::K)).unwrap();
    app.control(Control::AnchorChoose("arrival".into()))
        .unwrap();
    click_tile(&mut app, 3, 4);
    pause(&mut app);
    for control in [Control::AnchorMove, Control::AnchorDelete] {
        assert_eq!(
            app.control(control.clone()).unwrap_err(),
            PAUSED,
            "{control:?}"
        );
    }
    app.control(Control::AnchorChoose("arrival".into()))
        .unwrap();
    assert!(app.session.history().pending().is_none());
    app.blocked = None;
    app.control(Control::Select).unwrap();
    click_object(&mut app, "gate_east");
    pause(&mut app);
    for control in [
        Control::PortalAnchors,
        Control::PortalPick,
        Control::PortalAnchor {
            plane: "main".into(),
            anchor: "arrival".into(),
        },
        Control::PortalClear,
    ] {
        assert_eq!(
            app.control(control.clone()).unwrap_err(),
            PAUSED,
            "{control:?}"
        );
        assert_eq!(app.tools.portal_step(), None, "{control:?}");
    }
    assert!(app.session.history().pending().is_none());
}
