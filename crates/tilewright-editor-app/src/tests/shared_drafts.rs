// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The shared field on the coordinates, a new anchor's id and a portal's
//! typed location: the frame budget, the road fence, the pause, the portal
//! pick step and the hand test's steps.
use super::shared_string_field::{field_drawn, raster_samples, route};
use super::{acknowledge_ui, input_key};
use crate::prototype::Application;
use crate::shared_field::SHARED_FIRST_SEMANTIC;
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::keyboard::Keycode;
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;

/// The drafts typed as text and reviewed as something other than a field
/// value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Typed {
    /// "Edit position (x,y)" for `marker_1`.
    Move,
    /// "New anchor at (3, 3)" in Anchor mode.
    Anchor,
    /// "Type a location" for `gate_west`.
    Location,
}

impl Typed {
    const ALL: [Self; 3] = [Self::Move, Self::Anchor, Self::Location];

    fn control(self) -> Control {
        match self {
            Self::Move => Control::Coordinates,
            Self::Anchor => Control::AnchorCreate,
            Self::Location => Control::PortalLocation,
        }
    }

    fn mode(self) -> Control {
        match self {
            Self::Anchor => Control::Anchor,
            Self::Move | Self::Location => Control::Select,
        }
    }

    fn selection(self) -> SelectionKey {
        match self {
            Self::Move => SelectionKey::Object {
                uid: "marker_1".into(),
            },
            Self::Anchor => SelectionKey::Cell {
                plane: "main".into(),
                layer: 0,
                grid: "floor".into(),
                x: 3,
                y: 3,
            },
            Self::Location => SelectionKey::Object {
                uid: "gate_west".into(),
            },
        }
    }

    /// The hint below the draft's row, where the sidebar is wide enough.
    fn hint(self) -> &'static str {
        match self {
            Self::Move => "Ctrl+A selects / Enter reviews",
            Self::Anchor => "New anchor id at (3, 3); Enter reviews",
            Self::Location => "Portal of gate_west: [plane] layer x y",
        }
    }

    /// Text a person types into the draft: accepted, refused by each
    /// review check, and in between.
    fn texts(self) -> &'static [&'static str] {
        match self {
            Self::Move => &[
                "4,4", " 5 , 6 ", "-3,-2", "4,x", "x,4", "44", "1,2,3", "٣,٤", ",", "4", "-",
            ],
            Self::Anchor => &[
                "camp",
                "camp.b_c-1",
                "Camp",
                "-x",
                "arrival",
                "lookout",
                "a b",
                "c",
                ".",
            ],
            Self::Location => &[
                "0 5 5",
                "main 0, 5, 5",
                "0,,5,,5",
                "1 1",
                "main 0 5 5 5",
                "elsewhere 0 1 1",
                "main x 1 1",
                "0 1 99999999999999999",
                "0 -2 2",
                " ",
                "5",
            ],
        }
    }
}

/// Select `typed`'s object or tile, as a click in its mode selects it.
fn select(app: &mut Application, typed: Typed) -> Result<(), String> {
    app.action(Action::Select {
        generation: app.view.frame_revision,
        key: typed.selection(),
    })
}

/// The sample in a window of `logical` size at `scale`, with `typed` open.
fn drafted(logical: (u32, u32), scale: u32, typed: Typed) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = super::application();
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.reflow().unwrap();
    app.control(typed.mode()).unwrap();
    select(&mut app, typed).unwrap();
    app.control(typed.control()).unwrap();
    assert_eq!(open(&app), typed.name(), "{typed:?}");
    assert!(app.tools.shared_draft().is_some());
    acknowledge_ui(&mut app);
    (temporary, app)
}

impl Typed {
    fn name(self) -> &'static str {
        match self {
            Self::Move => "coordinates",
            Self::Anchor => "anchor",
            Self::Location => "location",
        }
    }
}

/// Which draft is open: one of these, another kind, or none.
fn open(app: &Application) -> &'static str {
    let tools = &app.tools;
    if tools.draft_text().is_none() {
        "none"
    } else if tools.anchor_draft().is_some() {
        "anchor"
    } else if tools.portal_draft().is_some() {
        "location"
    } else if tools.zone_draft().is_some() {
        "zone"
    } else if tools.filter_focused() {
        "filter"
    } else {
        // These runs never open an inspector field: the one other draft
        // is the coordinates.
        "coordinates"
    }
}

/// Each draft's field is compiled first and the editor's frame fitted to
/// leave its quads and raster samples free, as for the other drafts; below
/// the field alone editing pauses with the draft kept.
#[test]
fn a_typed_draft_frame_leaves_room_for_the_shared_field_and_pauses_without_it() {
    for typed in Typed::ALL {
        let (_temporary, mut app) = drafted((960, 640), 1, typed);
        let app = &mut app;
        app.tools.input("0 5 5").unwrap();
        let text = app.tools.draft_text().unwrap().to_owned();
        acknowledge_ui(app);
        let full = crate::prototype::workspace_presentation(app).unwrap();
        let field = crate::prototype::prepare_shared_field(app)
            .unwrap()
            .unwrap();
        let (quads, samples) = (field.quads(), field.raster_samples());
        let all = full.scene.quads();
        let editor: Vec<_> = all
            .iter()
            .copied()
            .filter(|quad| quad.semantic_id.0 < SHARED_FIRST_SEMANTIC)
            .collect();
        // The field's counts are exactly what it adds to the frame.
        assert_eq!(all.len() - editor.len(), quads, "{typed:?}");
        let total = raster_samples(&full.scene, all);
        assert_eq!(raster_samples(&full.scene, &editor) + samples, total);
        // Exactly enough room, or one more: drawn whole.
        for spare in [(0, 0), (1, 0), (0, 1)] {
            app.frame_limits = tilewright_scene::SceneLimits {
                maximum_quads: all.len() + spare.0,
                maximum_raster_samples: total + spare.1,
                ..tilewright_scene::SceneLimits::default()
            };
            let fitted = crate::prototype::workspace_presentation(app).unwrap();
            assert_eq!(fitted.scene.quads(), all, "{typed:?} {spare:?}");
        }
        // One quad or one sample short: the field's share is set aside
        // first, so the editor's own frame gives way within the rest, or is
        // what does not fit; the field is never drawn over the budget.
        for short in [(1, 0), (0, 1)] {
            app.frame_limits = tilewright_scene::SceneLimits {
                maximum_quads: all.len() - short.0,
                maximum_raster_samples: total - short.1,
                ..tilewright_scene::SceneLimits::default()
            };
            app.refresh_ui();
            match crate::prototype::workspace_presentation(app) {
                Ok(fitted) => {
                    assert!(field_drawn(&fitted.scene), "{typed:?} {short:?}");
                    assert!(fitted.scene.quads().len() <= all.len() - short.0);
                    assert!(raster_samples(&fitted.scene, fitted.scene.quads()) <= total - short.1);
                }
                Err(error) => assert!(
                    error.contains("LimitExceeded") && !error.starts_with("shared field"),
                    "{typed:?} {short:?}: {error}"
                ),
            }
            assert_eq!(app.tools.draft_text(), Some(text.as_str()), "kept");
            assert!(app.tools.shared_draft().is_some());
        }
        // Less than the field alone: the editor's share is nothing, so its
        // frame is refused and editing pauses with the draft kept.
        app.frame_limits = tilewright_scene::SceneLimits {
            maximum_raster_samples: samples - 1,
            ..tilewright_scene::SceneLimits::default()
        };
        let error = crate::prototype::workspace_presentation(app).err().unwrap();
        assert!(
            error.contains("LimitExceeded") && !error.starts_with("shared field"),
            "{typed:?}: {error}"
        );
        assert_eq!(app.tools.draft_text(), Some(text.as_str()), "kept");
        assert!(app.session.history().pending().is_none());
        app.blocked = Some(crate::prototype::Blocked::new(
            crate::prototype::BlockCause::Present,
            &error,
        ));
        // The first Esc discards the draft; the pause stays. A second Esc
        // ends it, and the field is drawn again.
        let mut input = crate::native_input::NativeInput::new(7);
        let mut clipboard = String::new();
        let escape = input_key(7, Keycode::Escape, false);
        route(app, &mut input, &escape, &mut clipboard).unwrap();
        assert!(app.tools.draft_text().is_none(), "the draft is discarded");
        assert!(app.blocked.is_some());
        let _ = route(app, &mut input, &escape, &mut clipboard);
        assert!(app.blocked.is_none());
        app.frame_limits = tilewright_scene::SceneLimits::default();
        select(app, typed).unwrap();
        app.control(typed.control()).unwrap();
        acknowledge_ui(app);
        let again = crate::prototype::workspace_presentation(app).unwrap();
        assert!(field_drawn(&again.scene), "{typed:?}");
    }
}

/// The road fence and the pause refuse these drafts: while a road is drawn
/// none opens, with the road's message;
/// while editing is paused none opens and an open one is not reviewed, and
/// its text is kept.
#[test]
fn the_road_fence_and_the_pause_refuse_typed_drafts() {
    use crate::prototype::ROAD_IN_PROGRESS;
    for typed in Typed::ALL {
        let (_temporary, mut app) = super::road::road_application(1, |_| {});
        super::road::click(&mut app, [0, 0]).unwrap();
        assert!(app.tools.road().is_some());
        assert_eq!(
            app.control(typed.control()).unwrap_err(),
            ROAD_IN_PROGRESS,
            "{typed:?}"
        );
        assert!(app.tools.draft_text().is_none(), "{typed:?}");
        assert_eq!(super::road::points(&app), [[0, 0]]);
        // Paused: the open draft is not reviewed and keeps its text,
        // and none opens.
        let (_temporary, mut app) = drafted((960, 640), 1, typed);
        app.tools.select_all().unwrap();
        app.tools.input(typed.texts()[0]).unwrap();
        app.blocked = Some(crate::prototype::Blocked::new(
            crate::prototype::BlockCause::Present,
            "test",
        ));
        acknowledge_ui(&mut app);
        let mut input = crate::native_input::NativeInput::new(7);
        let mut clipboard = String::new();
        app.control(Control::ApplyDraft).unwrap_err();
        route(
            &mut app,
            &mut input,
            &input_key(7, Keycode::Return, false),
            &mut clipboard,
        )
        .unwrap_or_default();
        assert_eq!(app.tools.draft_text(), Some(typed.texts()[0]));
        assert!(app.session.history().pending().is_none());
        app.control(typed.control()).unwrap_err();
        assert_eq!(app.tools.draft_text(), Some(typed.texts()[0]), "kept");
        assert!(app.tools.shared_draft().is_some());
    }
}

/// Typing a location while a portal pick step is open ends the step and
/// its prompt, as every draft does: the tile click that
/// follows does not set the portal.
#[test]
fn opening_a_typed_location_ends_the_pick_step() {
    use super::anchors::{click_tile, has};
    use super::zone_manage::press;
    use super::zones::{sidebar_texts, zone_application};
    let (_temporary, mut app) = zone_application(|_| {});
    super::anchors::click_object(&mut app, "gate_west");
    press(&mut app, &Control::PortalPick);
    assert!(app.tools.portal_picking());
    press(&mut app, &Control::PortalLocation);
    assert_eq!(app.tools.portal_draft(), Some("gate_west"));
    assert!(app.tools.shared_draft().is_some());
    assert!(!app.tools.portal_picking());
    assert_eq!(app.tools.portal_step(), None);
    let texts = sidebar_texts(&mut app);
    assert!(!has(&texts, "Click a tile in the view (Esc ends)"));
    assert!(!app.status_label().starts_with("Click a tile in the view"));
    click_tile(&mut app, 4, 3);
    assert!(app.tools.portal_draft().is_none(), "a tile click ends it");
    assert!(app.session.history().pending().is_none());
}

/// The hand test's step 15, through SDL events: its hints, refusals and
/// reviews.
#[test]
fn the_hand_test_steps_give_their_hints_refusals_and_reviews() {
    use super::zone_manage::{enter, press, review_has, type_text};
    use super::zones::{key, sidebar_texts, zone_application};
    let (_temporary, mut app) = zone_application(|_| {});
    let attempt = |app: &mut Application, text: &str| {
        app.tools.select_all().unwrap();
        type_text(app, text);
        acknowledge_ui(app);
        super::zones::route(app, &key(Keycode::Return))
    };
    // The coordinates.
    select(&mut app, Typed::Move).unwrap();
    press(&mut app, &Control::Coordinates);
    assert_eq!(app.tools.draft_text(), Some("3,2"));
    assert!(sidebar_texts(&mut app).contains(&Typed::Move.hint().to_owned()));
    assert_eq!(
        attempt(&mut app, "4,x").unwrap_err(),
        "Invalid y coordinate"
    );
    attempt(&mut app, "4,4").unwrap();
    // The review shows the position's new value.
    assert!(
        review_has(&mut app, "4,\\u{a}        4"),
        "{:?}",
        app.review_lines(900)
    );
    app.action(Action::Cancel).unwrap();
    // A new anchor's id.
    app.control(Control::Anchor).unwrap();
    select(&mut app, Typed::Anchor).unwrap();
    press(&mut app, &Control::AnchorCreate);
    assert!(sidebar_texts(&mut app).contains(&Typed::Anchor.hint().to_owned()));
    assert!(
        attempt(&mut app, "Camp")
            .unwrap_err()
            .starts_with("An anchor id is")
    );
    attempt(&mut app, "camp").unwrap();
    assert!(review_has(
        &mut app,
        "Creates anchor camp on layer 0 at (3, 3)."
    ));
    app.action(Action::Cancel).unwrap();
    // A typed location.
    app.control(Control::Select).unwrap();
    select(&mut app, Typed::Location).unwrap();
    press(&mut app, &Control::PortalLocation);
    assert!(sidebar_texts(&mut app).contains(&Typed::Location.hint().to_owned()));
    assert_eq!(
        attempt(&mut app, "1 1").unwrap_err(),
        "Type three or four values: layer x y, or plane layer x y"
    );
    attempt(&mut app, "main 0, 5, 5").unwrap();
    assert!(review_has(
        &mut app,
        "gate_west now leads to main, layer 0, (5, 5)"
    ));
    enter(&mut app);
    assert!(app.session.history().pending().is_none(), "accepted");
}
