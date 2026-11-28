// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The shared field in the application: the frame record, the reserved
//! identities, refusals that pause rather than panic, the field row's look,
//! and caret steps, for the inspector's string field, both zone name drafts
//! and the palette filter (the coordinates, anchor ids and typed portal
//! locations are in `shared_drafts.rs`).
use super::{acknowledge_ui, application_with_source, input_key};
use crate::prototype::Application;
use crate::shared_field::{
    RESERVED_RESOURCES, SHARED_ATLAS_RESOURCE, SHARED_FIRST_SEMANTIC, SHARED_SOLID_RESOURCE,
};
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::{event::Event, keyboard::Keycode};
use std::fs;
use tilewright_editor_ui::tools::{ZoneChoice, ZoneDraft};

fn labelled(stored: &str, logical: (u32, u32)) -> (tempfile::TempDir, Application) {
    let stored = stored.to_owned();
    let (temporary, mut app) = application_with_source(move |world| {
        let path = world.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        chunk["objects"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|object| object["uid"] == "marker_1")
            .unwrap()["fields"]["label"] = serde_json::json!(stored);
        fs::write(path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
    });
    app.ui = WorkspaceUi::new(logical, (320, 240));
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    (temporary, app)
}

fn record(temporary: &tempfile::TempDir, name: &str) -> serde_json::Value {
    serde_json::from_slice(
        &fs::read(temporary.path().join(format!("evidence/{name}.json"))).unwrap(),
    )
    .unwrap()
}

/// The shared field edits the drafts and is drawn with its own reserved
/// atlas and solid texture; no frame record names it any more.
#[test]
fn the_shared_field_draws_with_its_reserved_resources_and_frames_do_not_name_it() {
    let (temporary, mut app) = labelled("Trail marker", (960, 640));
    app.capture("before").unwrap();
    app.control(Control::Field("label".into())).unwrap();
    assert!(app.tools.shared_draft().is_some());
    // The coordinates too.
    app.control(Control::Coordinates).unwrap();
    assert!(app.tools.shared_draft().is_some());
    app.control(Control::Field("label".into())).unwrap();
    app.capture("draft").unwrap();
    for name in ["before", "draft"] {
        let frame = record(&temporary, name);
        assert!(frame.get("string_field").is_none(), "{name}");
    }
    // The editor's own textures stay below the shared field's.
    acknowledge_ui(&mut app);
    let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    let uses = |id: u64| {
        presentation
            .scene
            .quads()
            .iter()
            .any(|quad| quad.resource_id.0 == id)
    };
    assert!(uses(SHARED_ATLAS_RESOURCE));
    assert!(uses(SHARED_SOLID_RESOURCE));
    let reserved = [SHARED_ATLAS_RESOURCE, SHARED_SOLID_RESOURCE];
    for quad in presentation.scene.quads() {
        let own = quad.semantic_id.0 >= SHARED_FIRST_SEMANTIC;
        assert_eq!(reserved.contains(&quad.resource_id.0), own, "{quad:?}");
        assert!(own || quad.resource_id.0 < RESERVED_RESOURCES || quad.resource_id.0 == u64::MAX);
    }
    let geometry = app.displayed_text.as_ref().unwrap();
    assert_eq!(geometry.snapshot.committed, "Trail marker");
}

#[test]
fn no_resource_id_the_editor_derives_reaches_the_reserved_ones() {
    // World resource IDs are catalog positions plus one; a catalog holds
    // at most this many values, so at most this many resources. The
    // editor's own UI textures take the largest ID plus one and two.
    let catalog =
        u64::try_from(tilewright_editor_preview::PreviewLimits::default().maximum_catalog_values)
            .unwrap();
    let derived = catalog + 2;
    for reserved in [SHARED_SOLID_RESOURCE, SHARED_ATLAS_RESOURCE] {
        assert!(derived < RESERVED_RESOURCES && RESERVED_RESOURCES <= reserved);
        // Clear of the cell reference art's sentinel.
        assert_ne!(reserved, u64::MAX);
    }
}

#[test]
fn a_frame_using_a_reserved_identity_or_another_text_system_is_refused_not_drawn() {
    let (_temporary, mut app) = labelled("Trail", (960, 640));
    app.control(Control::Field("label".into())).unwrap();
    app.refresh_ui();
    let mut presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    // A host quad on the shared atlas's identity: refused at composition.
    let mut quads = presentation.scene.quads().to_vec();
    let mut stolen = quads[0];
    stolen.semantic_id = tilewright_scene::SemanticId(u32::MAX);
    stolen.resource_id = tilewright_scene::ResourceId(SHARED_ATLAS_RESOURCE);
    quads.push(stolen);
    presentation.scene = tilewright_scene::SceneBundle::from_quads(
        presentation.scene.revision(),
        presentation.scene.clock_millis(),
        presentation.scene.viewport(),
        presentation.scene.clear_color(),
        quads,
        tilewright_scene::SceneLimits::default(),
    )
    .unwrap();
    let shared = app.tools.shared_draft().unwrap().clone();
    let draft = app.tools.input_revision().unwrap().draft;
    let error = app
        .shared_field
        .prepare(&mut app.text, &shared, draft, &app.ui)
        .unwrap()
        .unwrap()
        .attach(&mut presentation, tilewright_scene::SceneLimits::default())
        .unwrap_err();
    assert!(error.contains("reserved"), "{error}");
    // Grow the atlas on the lent system, then lend another one: its atlas
    // revisions start again, so the compiler refuses it.
    for text in ["Trail abc", "Trail 0123456789", "Trail ÀÉÎÕÜ ąęł"] {
        app.tools.select_all().unwrap();
        app.tools.input(text).unwrap();
        crate::prototype::workspace_presentation(&mut app).unwrap();
    }
    app.text = crate::font_pack::test_system();
    let error = crate::prototype::workspace_presentation(&mut app)
        .err()
        .unwrap();
    assert!(error.contains("text system changed"), "{error}");
    // The draft is untouched and nothing was prepared.
    assert_eq!(app.tools.draft_text(), Some("Trail ÀÉÎÕÜ ąęł"));
    assert!(app.session.history().pending().is_none());
}

/// One SDL event through the editor's own routing: clipboard, text, keys.
pub(super) fn route(
    app: &mut Application,
    input: &mut crate::native_input::NativeInput,
    event: &Event,
    clipboard: &mut String,
) -> Result<(), String> {
    input.begin_batch(
        matches!(event, Event::TextInput { .. } | Event::TextEditing { .. }),
        app.tools.input_revision().map(|revision| revision.draft),
    );
    let paste = clipboard.clone();
    crate::prototype::native_clipboard_event(
        app,
        event,
        true,
        || Ok(paste),
        |text| {
            text.clone_into(clipboard);
            Ok(())
        },
    )
    .or_else(|| crate::prototype::native_text_event(app, event, input))
    .unwrap_or_else(|| match event {
        Event::KeyDown {
            keycode: Some(key), ..
        } => crate::prototype::native_key(app, *key),
        _ => Ok(()),
    })
}

/// The caret after each of `presses` presses of `arrow`, from `from`.
fn presses(app: &mut Application, from: Keycode, arrow: Keycode, presses: usize) -> Vec<usize> {
    let mut input = crate::native_input::NativeInput::new(7);
    let mut clipboard = String::new();
    route(app, &mut input, &input_key(7, from, false), &mut clipboard).unwrap();
    (0..presses)
        .map(|_| {
            route(app, &mut input, &input_key(7, arrow, false), &mut clipboard).unwrap();
            app.tools.draft_snapshot().unwrap().selection.focus.byte
        })
        .collect()
}

/// Every arrow press beside a stored-break marker moves the caret, either
/// way.
#[test]
fn the_shared_field_visits_every_position_beside_a_marker_with_no_press_that_does_nothing() {
    let stored = "Trail\nmar\u{2028}ker\u{2029}";
    let left = vec![15, 14, 13, 12, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
    let right = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 13, 14, 15, 18];
    let (_temporary, mut app) = labelled(stored, (960, 640));
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    assert!(app.tools.shared_draft().is_some());
    assert_eq!(
        presses(&mut app, Keycode::End, Keycode::Left, left.len()),
        left
    );
    assert_eq!(
        presses(&mut app, Keycode::Home, Keycode::Right, right.len()),
        right
    );
}

#[test]
fn the_shared_field_has_no_button_of_its_own_and_its_text_takes_the_row() {
    let (_temporary, mut app) = labelled("Trail marker", (960, 640));
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    let row = app.ui.shared_field.unwrap();
    // The shared layout has no submit control: neither its hit region nor
    // its paint.
    let shared = app.tools.shared_draft().unwrap().snapshot();
    let layout = tilewright_ui::layout_in_with(
        &shared,
        tilewright_ui::Viewport::new(960.0, 640.0, 1.0).unwrap(),
        tilewright_ui::Rect {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 30.0,
        },
        tilewright_ui::Rect {
            x: 0.0,
            y: 0.0,
            width: 960.0,
            height: 640.0,
        },
        &crate::shared_field::layout_options(),
    )
    .unwrap();
    assert!(
        layout
            .hits
            .iter()
            .all(|hit| hit.widget.id != tilewright_ui::SUBMIT)
    );
    // The drawn text area runs on past where the 80 px Apply button, 8 px
    // inside the row's right edge, used to start.
    let clip = presentation.editable.unwrap().clip;
    #[allow(clippy::cast_precision_loss)]
    let button = (row.x + row.width) as f32 - 8.0 - 80.0;
    assert!(clip.x + clip.width > button + 40.0, "{clip:?} in {row:?}");
    // A press where the button was edits the text; it does not review.
    let at = [button + 40.0, clip.y + clip.height / 2.0];
    crate::prototype::native_pointer(&mut app, at, (960, 640), (320, 240)).unwrap();
    assert!(app.session.history().pending().is_none());
    assert_eq!(app.tools.draft_text(), Some("Trail marker"));
    let caret = app.tools.draft_snapshot().unwrap().selection.focus.byte;
    assert_eq!(caret, "Trail marker".len());
}

#[test]
fn the_frame_leaves_room_for_the_shared_field_before_the_highlight() {
    let mut twin = highlighted();
    let app = &mut twin.app;
    let full = crate::prototype::workspace_presentation(app).unwrap();
    let shared = full
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.semantic_id.0 >= SHARED_FIRST_SEMANTIC)
        .count();
    assert!(highlight(&full.scene) > 0 && shared > 0);
    // The editor fits its own frame to the budget less the field's quads,
    // so the composed frame never exceeds the budget. With exactly room
    // for everything it is drawn whole.
    let total = full.scene.quads().len();
    app.frame_limits.maximum_quads = total;
    let fitted = crate::prototype::workspace_presentation(app).unwrap();
    assert_eq!(fitted.scene.quads().len(), total);
    assert!(!app.highlight_omitted);
    // One quad less: the editor's frame must give way within its share.
    // Here the "Highlight left out" note costs more quads than the two
    // highlighted tiles, so leaving the highlight out does not fit either
    // and the frame is refused; it is never drawn over the budget.
    app.frame_limits.maximum_quads = total - 1;
    app.refresh_ui();
    match crate::prototype::workspace_presentation(app) {
        Ok(presentation) => assert!(presentation.scene.quads().len() < total),
        // The editor's own frame is what does not fit, not the field: its
        // quads were set aside.
        Err(error) => assert!(
            error.contains("LimitExceeded") && !error.starts_with("shared field"),
            "{error}"
        ),
    }
    assert!(app.tools.shared_draft().is_some(), "the draft is kept");
    // The editor's share alone, with the field compiled first, is what was
    // fitted: the field's quads are reserved, not added on top.
    app.frame_limits = tilewright_scene::SceneLimits::default();
    app.highlight_omitted = false;
    app.refresh_ui();
    let again = crate::prototype::workspace_presentation(app).unwrap();
    assert_eq!(again.scene.quads().len(), total);
    assert_eq!(highlight(&again.scene), highlight(&full.scene));
    assert!(shared > 0);
}

#[test]
fn the_field_follows_its_row_when_the_frame_is_laid_out_again() {
    let mut twin = super::proposal::twin(1, |_| {}, |_| {}, None);
    super::proposal::open(&mut twin);
    let app = &mut twin.app;
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    // Laid out as if the highlight had been left out, its note above the
    // field; the frame then has room for the highlight, so it is laid out
    // again without the note and the field's row moves up.
    app.highlight_omitted = true;
    app.refresh_ui();
    let before = app.ui.shared_field.unwrap();
    let presentation = crate::prototype::workspace_presentation(app).unwrap();
    assert!(!app.highlight_omitted);
    let row = app.ui.shared_field.unwrap();
    assert!(row.y < before.y, "the row moved: {before:?} -> {row:?}");
    let clip = presentation.editable.unwrap().clip;
    #[allow(clippy::cast_precision_loss)]
    let (top, bottom) = (row.y as f32, (row.y + row.height) as f32);
    assert!(
        clip.y >= top && clip.y + clip.height <= bottom,
        "the field is drawn in its new row {row:?}, not at {clip:?}"
    );
}

/// The raster samples drawing `quads` takes, found with the scene
/// library's own check.
pub(super) fn raster_samples(
    scene: &tilewright_scene::SceneBundle,
    quads: &[tilewright_scene::Quad],
) -> u64 {
    let fits = |samples| {
        tilewright_scene::SceneBundle::from_quads(
            scene.revision(),
            scene.clock_millis(),
            scene.viewport(),
            scene.clear_color(),
            quads.iter().copied(),
            tilewright_scene::SceneLimits {
                maximum_raster_samples: samples,
                ..tilewright_scene::SceneLimits::default()
            },
        )
        .is_ok()
    };
    let (mut low, mut high) = (
        0,
        tilewright_scene::SceneLimits::default().maximum_raster_samples,
    );
    assert!(fits(high));
    while low < high {
        let middle = low + (high - low) / 2;
        if fits(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

/// The caret's position and its x in the drawn field, after each of
/// `presses` presses of `arrow` from `from`, then from where it ends.
fn visual_presses(
    app: &mut Application,
    from: Keycode,
    arrow: Keycode,
    presses: usize,
) -> Vec<(usize, f32)> {
    let mut input = crate::native_input::NativeInput::new(7);
    let mut clipboard = String::new();
    acknowledge_ui(app);
    route(app, &mut input, &input_key(7, from, false), &mut clipboard).unwrap();
    let mut stops = Vec::new();
    for press in 0..=presses {
        if press > 0 {
            route(app, &mut input, &input_key(7, arrow, false), &mut clipboard).unwrap();
        }
        acknowledge_ui(app);
        let geometry = app.displayed_text.as_ref().unwrap();
        let focus = geometry.snapshot.display_selection.focus;
        let caret = geometry
            .layout
            .caret(crate::workspace_render::renderer_cursor(focus))
            .unwrap();
        let byte = app.tools.draft_snapshot().unwrap().selection.focus.byte;
        stops.push((byte, geometry.origin[0] + caret.x));
    }
    stops
}

/// At the 18 px field text the renderer gives display position 7 of this
/// value two caret stops a float apart (x 4.68 after the letters and
/// 4.6800003 before the Hebrew letter). tilewright-ui-render 0.1.0 took the
/// step between them for a move, so one press moved nothing on screen;
/// 0.1.1 counts the two stops as one place, so every press moves.
#[test]
fn every_arrow_press_moves_the_shared_caret_where_two_stops_are_a_float_apart() {
    let stored = "\u{5d0}abb\u{5d0} ";
    let moved = |stops: &[(usize, f32)]| {
        stops
            .windows(2)
            .map(|pair| (pair[1].1 - pair[0].1).abs() > 0.5)
            .collect::<Vec<_>>()
    };
    let (_shared, mut shared) = labelled(stored, (960, 640));
    shared.control(Control::Field("label".into())).unwrap();
    for (from, arrow, expected) in [
        (Keycode::Home, Keycode::Right, [8, 7, 2, 3, 4, 2, 0]),
        (Keycode::End, Keycode::Left, [0, 5, 4, 3, 5, 7, 8]),
    ] {
        let stops = visual_presses(&mut shared, from, arrow, 6);
        let bytes: Vec<_> = stops.iter().map(|stop| stop.0).collect();
        assert_eq!(bytes, expected, "{arrow:?}");
        assert!(
            moved(&stops).iter().all(|moved| *moved),
            "{arrow:?}: {stops:?}"
        );
    }
}

/// A marker's guard beside right-to-left text. In
/// `RLM กำ b U+0600 U+2028 U+0300 가` End then Left cycled 12, 10, 9, 12 for
/// ever in the shared field before tilewright-ui-render 0.1.3. It now walks
/// these stops, each press moving the drawn caret the pressed way, until
/// the caret stays at the end.
#[test]
fn the_shared_field_does_not_cycle_beside_a_guard_in_right_to_left_text() {
    let stored = "\u{200f}\u{e01}\u{e33}b\u{600}\u{2028}\u{300}\u{ac00}";
    for (from, arrow, walk) in [
        (Keycode::End, Keycode::Left, &[12, 10, 9, 20, 17][..]),
        (Keycode::Home, Keycode::Right, &[17, 15, 3, 9, 10, 0]),
    ] {
        let (_temporary, mut app) = labelled(stored, (960, 640));
        app.control(Control::Field("label".into())).unwrap();
        // One press more than the walk: it stays where the walk ends.
        let stops = visual_presses(&mut app, from, arrow, walk.len());
        let context = format!("{arrow:?}: {stops:?}");
        let bytes: Vec<_> = stops.iter().map(|stop| stop.0).collect();
        assert_eq!(bytes[..walk.len()], walk[..], "{context}");
        assert_eq!(stops[walk.len()], stops[walk.len() - 1], "{context}");
        let pressed_way = |pair: &[(usize, f32)]| {
            if arrow == Keycode::Left {
                pair[1].1 < pair[0].1
            } else {
                pair[1].1 > pair[0].1
            }
        };
        assert!(stops[..walk.len()].windows(2).all(pressed_way), "{context}");
    }
}

/// The proposal twin with the shared field open on a marker's label, its
/// highlight on screen.
fn highlighted() -> super::proposal::Twin {
    highlighted_at(1.0)
}

/// [`highlighted`] with the camera at `zoom`.
fn highlighted_at(zoom: f32) -> super::proposal::Twin {
    let mut twin = super::proposal::twin(1, |_| {}, |_| {}, None);
    super::proposal::open(&mut twin);
    let app = &mut twin.app;
    app.view.camera.zoom = zoom;
    app.rebuild().unwrap();
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(app);
    twin
}

fn highlight(scene: &tilewright_scene::SceneBundle) -> usize {
    use crate::prototype::proposal_view::{HIGHLIGHT_FROM_TINT, HIGHLIGHT_TINT};
    scene
        .quads()
        .iter()
        .filter(|quad| {
            !quad.pickable && (quad.color == HIGHLIGHT_TINT || quad.color == HIGHLIGHT_FROM_TINT)
        })
        .count()
}

pub(super) fn field_drawn(scene: &tilewright_scene::SceneBundle) -> bool {
    scene
        .quads()
        .iter()
        .any(|quad| quad.semantic_id.0 >= SHARED_FIRST_SEMANTIC)
}

#[test]
fn the_frame_leaves_room_for_the_shared_fields_raster_samples_before_the_highlight() {
    // Zoomed in, the highlighted tiles cost more samples than the note that
    // reports them left out, by less than the field's samples.
    let mut twin = highlighted_at(2.0);
    let app = &mut twin.app;
    let full = crate::prototype::workspace_presentation(app).unwrap();
    let field = crate::prototype::prepare_shared_field(app)
        .unwrap()
        .unwrap();
    let samples = field.raster_samples();
    let quads = full.scene.quads();
    let editor: Vec<_> = quads
        .iter()
        .copied()
        .filter(|quad| quad.semantic_id.0 < SHARED_FIRST_SEMANTIC)
        .collect();
    let own = raster_samples(&full.scene, &editor);
    // The field's count is exactly what it adds, by the scene library's
    // own check, so the boundary is computed, not searched for.
    let total = own + samples;
    assert_eq!(raster_samples(&full.scene, quads), total);
    assert!(samples > 0 && highlight(&full.scene) > 0);
    // Exactly enough: drawn whole.
    app.frame_limits.maximum_raster_samples = total;
    let fitted = crate::prototype::workspace_presentation(app).unwrap();
    assert_eq!(fitted.scene.quads(), quads);
    assert!(!app.highlight_omitted);
    // One sample less: the field's samples are set aside first, so the
    // editor's frame gives way within its share (the highlight is left
    // out) and the field is still drawn; nothing pauses.
    app.frame_limits.maximum_raster_samples = total - 1;
    let fitted = crate::prototype::workspace_presentation(app).unwrap();
    assert!(app.highlight_omitted);
    assert_eq!(highlight(&fitted.scene), 0);
    assert!(field_drawn(&fitted.scene));
    assert!(raster_samples(&fitted.scene, fitted.scene.quads()) < total);
    assert!(app.tools.shared_draft().is_some(), "the draft is kept");
}

/// When the frame is laid out again (the "Highlight left out" note comes or
/// goes), the field is prepared again and its own samples set aside anew.
/// In a 1280×300 window the note pushes the field's row off the sidebar:
/// the first pass, laid out with the note, has no field and reserves
/// nothing; the frame then has room for the highlight, the note goes and
/// the row is shown, so the second pass must reserve the field's samples.
#[test]
fn a_frame_laid_out_again_reserves_the_fields_samples_again() {
    let mut twin = super::proposal::twin(1, |_| {}, |_| {}, None);
    super::proposal::open(&mut twin);
    let app = &mut twin.app;
    app.ui = WorkspaceUi::new((1280, 300), (1280, 300));
    app.view.camera.width = 1280;
    app.view.camera.height = 300;
    app.rebuild().unwrap();
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(app);
    let full = crate::prototype::workspace_presentation(app).unwrap();
    assert!(field_drawn(&full.scene) && highlight(&full.scene) > 0);
    app.highlight_omitted = true;
    app.refresh_ui();
    assert!(app.ui.shared_field.is_none(), "the note hides the row");
    // One sample short of the whole frame, the first pass laid out with
    // the note.
    let total = raster_samples(&full.scene, full.scene.quads());
    app.frame_limits.maximum_raster_samples = total - 1;
    let fitted = crate::prototype::workspace_presentation(app).unwrap();
    assert!(app.highlight_omitted);
    assert_eq!(highlight(&fitted.scene), 0);
    assert!(field_drawn(&fitted.scene));
    assert!(raster_samples(&fitted.scene, fitted.scene.quads()) < total);
}

#[test]
fn composing_the_field_checks_the_editors_own_limits_as_a_backstop() {
    let mut twin = highlighted();
    let app = &mut twin.app;
    let world = crate::workspace_render::compose_with_overlay(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
        &[],
        tilewright_scene::SceneLimits::default(),
    )
    .unwrap();
    let own = raster_samples(&world.scene, world.scene.quads());
    let attach = |app: &mut Application, samples: u64| {
        let field = crate::prototype::prepare_shared_field(app)
            .unwrap()
            .unwrap();
        let limits = tilewright_scene::SceneLimits {
            maximum_raster_samples: own + field.raster_samples() - samples,
            ..tilewright_scene::SceneLimits::default()
        };
        let mut presentation = crate::workspace_render::Presentation {
            scene: world.scene.clone(),
            resources: world.resources.clone(),
            unsupported_runs: 0,
            editable: None,
        };
        field
            .attach(&mut presentation, limits)
            .map(|()| presentation)
    };
    assert!(field_drawn(&attach(app, 0).unwrap().scene));
    let error = attach(app, 1).err().unwrap();
    assert!(
        error.starts_with("shared field") && error.contains("LimitExceeded"),
        "{error}"
    );
}

#[test]
fn a_frame_with_no_room_pauses_with_the_draft_kept() {
    let (_temporary, mut app) = labelled("Trail marker", (960, 640));
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let field = crate::prototype::prepare_shared_field(&mut app)
        .unwrap()
        .unwrap();
    // Less than the field alone: the editor's share is nothing, so its own
    // frame is what does not fit; the field is never drawn over the limit.
    app.frame_limits.maximum_raster_samples = field.raster_samples() - 1;
    let error = crate::prototype::workspace_presentation(&mut app)
        .err()
        .unwrap();
    assert!(
        error.contains("LimitExceeded") && !error.starts_with("shared field"),
        "{error}"
    );
    assert_eq!(app.tools.draft_text(), Some("Trail marker"));
    assert!(app.session.history().pending().is_none());
    // The native loop pauses on that refusal. The first Esc discards the
    // draft, but the pause stays (the frame still does not fit); a second
    // Esc ends it (as a zoom or a pan would).
    app.blocked = Some(crate::prototype::Blocked::new(
        crate::prototype::BlockCause::Present,
        &error,
    ));
    let paused = "Editing paused: the frame could not be shown.";
    let mut input = crate::native_input::NativeInput::new(7);
    let mut clipboard = String::new();
    let escape = input_key(7, Keycode::Escape, false);
    route(&mut app, &mut input, &escape, &mut clipboard).unwrap();
    assert!(app.tools.shared_draft().is_none(), "the draft is discarded");
    assert_eq!(app.status_label(), paused);
    let _ = route(&mut app, &mut input, &escape, &mut clipboard);
    assert!(app.blocked.is_none());
    assert_ne!(app.status_label(), paused);
    // Back within the limit, the field is drawn again.
    app.frame_limits = tilewright_scene::SceneLimits::default();
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let again = crate::prototype::workspace_presentation(&mut app).unwrap();
    assert!(field_drawn(&again.scene));
}

/// `stored` in the label field, opened in a window of `logical` size at `scale`, then `edit` applied to the draft.
fn opened(
    stored: &str,
    logical: (u32, u32),
    scale: u32,
    edit: Edit,
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = labelled(stored, logical);
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.reflow().unwrap();
    app.control(Control::Field("label".into())).unwrap();
    edit(&mut app);
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The field row, pinned at the values the editor's own field had before
/// the shared field replaced it (the shared field is styled from them), so
/// that a drift of the constants is seen: the active row fill, the text
/// inset, the accent of the caret and selection, the scroll margin, and the
/// 4 px gap to the hint below.
#[test]
// The values are exact by construction.
#[allow(clippy::float_cmp)]
fn the_field_row_keeps_its_look() {
    let rect = |quad: &tilewright_scene::Quad| {
        let [x, y] = quad.positions[0];
        [x, y, quad.positions[2][0] - x, quad.positions[2][1] - y]
    };
    let select = |app: &mut Application| app.tools.select_all().unwrap();
    let (_temporary, mut app) = opened("Trail marker", (1280, 800), 1, select);
    let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
    let row = app
        .ui
        .widgets
        .iter()
        .position(|widget| widget.control == Some(Control::EditDraft))
        .unwrap();
    let layout = app.ui.widgets[row].layout;
    assert_eq!(
        (layout.x, layout.y, layout.width, layout.height),
        (970, 56, 300, 30)
    );
    assert_eq!(app.ui.widgets[row + 1].layout.y, 56 + 30 + 4, "the hint");
    let quads = presentation.scene.quads();
    // The row's fill: the active row colour over the whole row.
    assert!(
        quads
            .iter()
            .any(|quad| quad.color == [0.12, 0.32, 0.47, 1.0]
                && rect(quad) == [970.0, 56.0, 300.0, 30.0])
    );
    let geometry = presentation.editable.unwrap();
    assert_eq!(geometry.origin, [974.0, 57.0]);
    // The selection and the caret, in the accent.
    let accent: Vec<_> = quads
        .iter()
        .filter(|quad| quad.color == [0.28, 0.48, 0.66, 1.0])
        .map(rect)
        .collect();
    assert_eq!(accent.len(), 2, "{accent:?}");
    assert!(accent.iter().any(|caret| caret[2] == 1.0), "{accent:?}");
    // A long value scrolls once its caret passes the width less 12.
    let (_temporary, mut app) = opened(
        "A long value past the width of the field, so it scrolls 0123",
        (1280, 800),
        1,
        |_| {},
    );
    let geometry = crate::prototype::workspace_presentation(&mut app)
        .unwrap()
        .editable
        .unwrap();
    let caret = geometry
        .layout
        .caret(crate::workspace_render::renderer_cursor(
            geometry.snapshot.display_selection.focus,
        ))
        .unwrap();
    assert!(caret.x > 300.0);
    assert_eq!(geometry.origin[0], 974.0 - (caret.x - (300.0 - 12.0)));
}

/// A change to the draft after it opens.
pub(super) type Edit = fn(&mut Application);

/// Open `draft` for meadow: its display name, or a new zone under it.
fn open_zone(app: &mut Application, draft: ZoneDraft) -> Result<(), String> {
    app.control(match draft {
        ZoneDraft::Name => Control::ZoneName,
        ZoneDraft::Create => Control::ZoneCreate(Some("meadow".into())),
    })
}

/// The sample with meadow's display name `name` (`None`: no name), in a
/// window of `logical` size at `scale`, in Zone mode
/// with meadow chosen and `draft` open.
fn zoned(
    name: Option<&str>,
    logical: (u32, u32),
    scale: u32,
    draft: ZoneDraft,
) -> (tempfile::TempDir, Application) {
    let name = name.map(str::to_owned);
    let (temporary, mut app) = application_with_source(move |world| {
        let path = world.join("planes/main/zones.jsonc");
        let mut zones: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let meadow = zones["zones"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|zone| zone["id"] == "meadow")
            .unwrap()
            .as_object_mut()
            .unwrap();
        match name {
            Some(name) => meadow.insert("name".into(), name.into()),
            None => meadow.remove("name"),
        };
        fs::write(path, serde_json::to_vec_pretty(&zones).unwrap()).unwrap();
    });
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.reflow().unwrap();
    app.control(Control::Zone).unwrap();
    app.control(Control::ZonePalette(ZoneChoice::Zone("meadow".into())))
        .unwrap();
    open_zone(&mut app, draft).unwrap();
    assert_eq!(app.tools.zone_draft(), Some(draft));
    assert!(app.tools.shared_draft().is_some());
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The toolbar's unsupported-text banner, if it shows.
pub(super) fn banner(app: &Application) -> Option<String> {
    app.ui
        .widgets
        .iter()
        .find(|widget| widget.text.starts_with("Unsupported text:"))
        .map(|widget| widget.text.clone())
}

/// A string field holding a character the font pack lacks: the toolbar's
/// banner counts its mark.
#[test]
fn an_unsupported_glyph_in_the_string_field_counts_in_the_banner() {
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        let (_temporary, mut app) = opened("Odd \u{e000} marker", logical, scale, |_| {});
        let case = format!("string field at {logical:?}x{scale}");
        let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
        assert!(field_drawn(&presentation.scene), "{case}");
        let banner = banner(&app);
        assert!(app.unsupported_runs >= 1, "{case}");
        if logical.0 >= 1280 {
            assert_eq!(
                banner,
                Some(format!(
                    "Unsupported text: {} marks / fields read-only",
                    app.unsupported_runs
                )),
                "{case}"
            );
        }
    }
}

/// A zone draft's field is compiled first and the editor's frame fitted to
/// leave its quads and raster samples free, as for the string field.
#[test]
fn a_zone_draft_frame_leaves_room_for_the_shared_field_and_pauses_without_it() {
    for draft in [ZoneDraft::Name, ZoneDraft::Create] {
        let (_temporary, mut app) = zoned(Some("Quiet meadow"), (960, 640), 1, draft);
        let app = &mut app;
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
        assert_eq!(all.len() - editor.len(), quads, "{draft:?}");
        let total = raster_samples(&full.scene, all);
        assert_eq!(raster_samples(&full.scene, &editor) + samples, total);
        // Exactly enough room: drawn whole.
        app.frame_limits.maximum_quads = all.len();
        app.frame_limits.maximum_raster_samples = total;
        let fitted = crate::prototype::workspace_presentation(app).unwrap();
        assert_eq!(fitted.scene.quads(), all, "{draft:?}");
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
                    assert!(field_drawn(&fitted.scene), "{draft:?} {short:?}");
                    assert!(fitted.scene.quads().len() <= all.len() - short.0);
                    assert!(raster_samples(&fitted.scene, fitted.scene.quads()) <= total - short.1);
                }
                Err(error) => assert!(
                    error.contains("LimitExceeded") && !error.starts_with("shared field"),
                    "{draft:?} {short:?}: {error}"
                ),
            }
            assert!(app.tools.shared_draft().is_some(), "the draft is kept");
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
            "{draft:?}: {error}"
        );
        assert!(app.tools.shared_draft().is_some(), "the draft is kept");
        app.blocked = Some(crate::prototype::Blocked::new(
            crate::prototype::BlockCause::Present,
            &error,
        ));
        let mut input = crate::native_input::NativeInput::new(7);
        let mut clipboard = String::new();
        let escape = input_key(7, Keycode::Escape, false);
        route(app, &mut input, &escape, &mut clipboard).unwrap();
        assert!(app.tools.draft_text().is_none(), "the draft is discarded");
        let _ = route(app, &mut input, &escape, &mut clipboard);
        assert!(app.blocked.is_none());
        app.frame_limits = tilewright_scene::SceneLimits::default();
        open_zone(app, draft).unwrap();
        acknowledge_ui(app);
        let again = crate::prototype::workspace_presentation(app).unwrap();
        assert!(field_drawn(&again.scene));
    }
}

/// The road fence and the pause refuse a zone draft: nothing opens, with
/// the editor's own message.
#[test]
fn the_road_fence_and_the_pause_refuse_zone_drafts() {
    use crate::prototype::ROAD_IN_PROGRESS;
    let (_temporary, mut app) = super::road::road_application(1, |_| {});
    let world = app.session.world().clone();
    app.tools
        .choose_zone(
            &world,
            app.session.catalog(),
            ZoneChoice::Zone("meadow".into()),
        )
        .unwrap();
    super::road::click(&mut app, [0, 0]).unwrap();
    assert!(app.tools.road().is_some());
    for control in [
        Control::ZoneName,
        Control::ZoneCreate(None),
        Control::ZoneCreate(Some("meadow".into())),
    ] {
        assert_eq!(
            app.control(control.clone()).unwrap_err(),
            ROAD_IN_PROGRESS,
            "{control:?}"
        );
        assert!(app.tools.draft_text().is_none(), "{control:?}");
        assert_eq!(super::road::points(&app), [[0, 0]]);
    }
    // Paused: no zone draft opens, and an open one cannot be reviewed.
    let (_temporary, mut app) = zoned(Some("Quiet meadow"), (960, 640), 1, ZoneDraft::Name);
    app.tools.select_all().unwrap();
    app.tools.input("Open meadow").unwrap();
    app.blocked = Some(crate::prototype::Blocked::new(
        crate::prototype::BlockCause::Present,
        "test",
    ));
    app.control(Control::ApplyDraft).unwrap_err();
    assert_eq!(app.tools.draft_text(), Some("Open meadow"));
    assert!(app.session.history().pending().is_none());
    for draft in [ZoneDraft::Name, ZoneDraft::Create] {
        open_zone(&mut app, draft).unwrap_err();
    }
    assert_eq!(app.tools.draft_text(), Some("Open meadow"), "kept");
}

/// The sample in Place mode, in a window of `logical` size at `scale`.
fn placed(logical: (u32, u32), scale: u32) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = super::application();
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.reflow().unwrap();
    app.control(Control::Place).unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// Place mode with the filter focused and `edit` applied to it.
fn filtering(logical: (u32, u32), scale: u32, edit: Edit) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = placed(logical, scale);
    app.control(Control::PaletteFilter).unwrap();
    assert!(app.tools.filter_focused());
    assert!(app.tools.shared_draft().is_some());
    edit(&mut app);
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The filter's field is compiled first and the editor's frame fitted to
/// leave its quads and raster samples free, as for the drafts; below the
/// field alone editing pauses with the filter kept.
#[test]
fn a_filter_frame_leaves_room_for_the_shared_field_and_pauses_without_it() {
    let (_temporary, mut app) = filtering((960, 640), 1, |app| {
        app.tools.input("la").unwrap();
    });
    let app = &mut app;
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
    assert_eq!(all.len() - editor.len(), quads);
    let total = raster_samples(&full.scene, all);
    assert_eq!(raster_samples(&full.scene, &editor) + samples, total);
    // Exactly enough room: drawn whole.
    app.frame_limits.maximum_quads = all.len();
    app.frame_limits.maximum_raster_samples = total;
    let fitted = crate::prototype::workspace_presentation(app).unwrap();
    assert_eq!(fitted.scene.quads(), all);
    // One more quad or sample than needed: drawn whole too.
    for spare in [(1, 0), (0, 1)] {
        app.frame_limits = tilewright_scene::SceneLimits {
            maximum_quads: all.len() + spare.0,
            maximum_raster_samples: total + spare.1,
            ..tilewright_scene::SceneLimits::default()
        };
        let fitted = crate::prototype::workspace_presentation(app).unwrap();
        assert_eq!(fitted.scene.quads(), all, "{spare:?}");
    }
    // One quad or one sample short: the field's share is set aside first,
    // so the editor's own frame gives way within the rest, or is what does
    // not fit; the field is never drawn over the budget.
    for short in [(1, 0), (0, 1)] {
        app.frame_limits = tilewright_scene::SceneLimits {
            maximum_quads: all.len() - short.0,
            maximum_raster_samples: total - short.1,
            ..tilewright_scene::SceneLimits::default()
        };
        app.refresh_ui();
        match crate::prototype::workspace_presentation(app) {
            Ok(fitted) => {
                assert!(field_drawn(&fitted.scene), "{short:?}");
                assert!(fitted.scene.quads().len() <= all.len() - short.0);
                assert!(raster_samples(&fitted.scene, fitted.scene.quads()) <= total - short.1);
            }
            Err(error) => assert!(
                error.contains("LimitExceeded") && !error.starts_with("shared field"),
                "{short:?}: {error}"
            ),
        }
        assert!(app.tools.shared_draft().is_some(), "the filter keeps focus");
        assert_eq!(app.tools.palette_filter(), "la");
    }
    // Less than the field alone: the editor's share is nothing, so its
    // frame is refused and editing pauses with the filter kept.
    app.frame_limits = tilewright_scene::SceneLimits {
        maximum_raster_samples: samples - 1,
        ..tilewright_scene::SceneLimits::default()
    };
    let error = crate::prototype::workspace_presentation(app).err().unwrap();
    assert!(
        error.contains("LimitExceeded") && !error.starts_with("shared field"),
        "{error}"
    );
    assert_eq!(app.tools.palette_filter(), "la", "the filter is kept");
    assert!(app.tools.shared_draft().is_some());
    app.blocked = Some(crate::prototype::Blocked::new(
        crate::prototype::BlockCause::Present,
        &error,
    ));
    // The first Esc clears and releases the filter; the pause stays. A
    // second Esc ends it, and the field is drawn again.
    let mut input = crate::native_input::NativeInput::new(7);
    let mut clipboard = String::new();
    let escape = input_key(7, Keycode::Escape, false);
    route(app, &mut input, &escape, &mut clipboard).unwrap();
    assert_eq!(
        (app.tools.palette_filter(), app.tools.filter_focused()),
        ("", false)
    );
    assert!(app.blocked.is_some());
    let _ = route(app, &mut input, &escape, &mut clipboard);
    assert!(app.blocked.is_none());
    app.frame_limits = tilewright_scene::SceneLimits::default();
    app.control(Control::PaletteFilter).unwrap();
    acknowledge_ui(app);
    let again = crate::prototype::workspace_presentation(app).unwrap();
    assert!(field_drawn(&again.scene));
}
