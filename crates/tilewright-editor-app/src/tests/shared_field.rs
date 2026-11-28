// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The opt-in shared field in the application: the switch, the frame
//! record, the reserved identities, refusals that pause rather than panic,
//! and differential runs of real SDL events against the editor's own field,
//! for the inspector's string field, both zone name drafts and the palette
//! filter.
use super::{acknowledge_ui, application_with_source_using, input_key, modified_input_key};
use crate::prototype::Application;
use crate::shared_field::{
    RESERVED_RESOURCES, SHARED_ATLAS_RESOURCE, SHARED_FIRST_SEMANTIC, SHARED_SOLID_RESOURCE,
};
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::{
    event::Event,
    keyboard::{Keycode, Mod},
};
use std::fs;
use tilewright_editor_ui::field_input::FieldImplementation;
use tilewright_editor_ui::text_input::{
    TextCursor, TextInputSnapshot, TextRevision, TextSelection,
};
use tilewright_editor_ui::tools::{ZoneChoice, ZoneDraft};

fn labelled(
    implementation: FieldImplementation,
    stored: &str,
    logical: (u32, u32),
) -> (tempfile::TempDir, Application) {
    let stored = stored.to_owned();
    let (temporary, mut app) = application_with_source_using(implementation, move |world| {
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

#[test]
fn the_switch_is_off_by_default_and_on_names_the_field_in_every_frame() {
    for implementation in FieldImplementation::ALL {
        let shared = implementation == FieldImplementation::Shared;
        let (temporary, mut app) = labelled(implementation, "Trail marker", (960, 640));
        app.capture("before").unwrap();
        app.control(Control::Field("label".into())).unwrap();
        assert_eq!(app.tools.shared_draft().is_some(), shared);
        // Other drafts keep the editor's own field either way.
        app.control(Control::Coordinates).unwrap();
        assert!(app.tools.shared_draft().is_none());
        app.control(Control::Field("label".into())).unwrap();
        app.capture("draft").unwrap();
        for name in ["before", "draft"] {
            let frame = record(&temporary, name);
            assert_eq!(
                frame
                    .get("string_field")
                    .and_then(serde_json::Value::as_str),
                shared.then_some(FieldImplementation::SHARED_NAME),
                "{name}"
            );
        }
        // The shared field is drawn with its own reserved atlas and solid
        // texture; the editor's own textures stay below them.
        acknowledge_ui(&mut app);
        let presentation = crate::prototype::workspace_presentation(&mut app).unwrap();
        let uses = |id: u64| {
            presentation
                .scene
                .quads()
                .iter()
                .any(|quad| quad.resource_id.0 == id)
        };
        assert_eq!(uses(SHARED_ATLAS_RESOURCE), shared);
        assert_eq!(uses(SHARED_SOLID_RESOURCE), shared);
        let reserved = [SHARED_ATLAS_RESOURCE, SHARED_SOLID_RESOURCE];
        for quad in presentation.scene.quads() {
            let own = quad.semantic_id.0 >= SHARED_FIRST_SEMANTIC;
            assert_eq!(reserved.contains(&quad.resource_id.0), own, "{quad:?}");
            assert!(
                own || quad.resource_id.0 < RESERVED_RESOURCES || quad.resource_id.0 == u64::MAX
            );
        }
        let geometry = app.displayed_text.as_ref().unwrap();
        assert_eq!(geometry.shared.is_some(), shared);
        assert_eq!(geometry.snapshot.committed, "Trail marker");
    }
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
    let (_temporary, mut app) = labelled(FieldImplementation::Shared, "Trail", (960, 640));
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
        .as_mut()
        .unwrap()
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
fn route(
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

fn comparable(snapshot: Option<TextInputSnapshot>) -> Option<TextInputSnapshot> {
    snapshot.map(|mut snapshot| {
        snapshot.revision = TextRevision::default();
        snapshot
    })
}

fn text(value: &str) -> Event {
    Event::TextInput {
        timestamp: 0,
        window_id: 7,
        text: value.into(),
    }
}

fn preedit(value: &str, start: i32, length: i32) -> Event {
    Event::TextEditing {
        timestamp: 0,
        window_id: 7,
        text: value.into(),
        start,
        length,
    }
}

#[test]
#[allow(clippy::too_many_lines)] // The event sequences are listed inline.
fn both_fields_turn_the_same_sdl_events_into_the_same_draft_and_review() {
    let key = |code| input_key(7, code, false);
    let shift = |code| modified_input_key(code, false, Mod::LSHIFTMOD);
    let command = |code| modified_input_key(code, false, Mod::LCTRLMOD);
    let sequences: Vec<Vec<Event>> = vec![
        vec![
            key(Keycode::Home),
            text("« "),
            key(Keycode::End),
            text(" »"),
        ],
        vec![
            key(Keycode::Left),
            key(Keycode::Left),
            key(Keycode::Backspace),
            key(Keycode::Delete),
            text("青"),
        ],
        vec![
            shift(Keycode::Left),
            shift(Keycode::Left),
            command(Keycode::C),
            key(Keycode::Home),
            command(Keycode::V),
            command(Keycode::A),
            command(Keycode::X),
            command(Keycode::V),
        ],
        vec![
            preedit("に", 1, 0),
            preedit("にほ", 2, 0),
            key(Keycode::Left),
            text("にほん"),
            key(Keycode::Home),
            preedit("x", 1, 0),
            key(Keycode::Escape),
            text("y"),
        ],
        vec![
            text("a\nb"),
            text("\u{2028}"),
            preedit("\u{2029}", 1, 0),
            text("x".repeat(5000).as_str()),
            command(Keycode::A),
            text("עברית 12"),
            key(Keycode::Home),
            key(Keycode::Right),
            shift(Keycode::End),
            key(Keycode::Backspace),
        ],
        vec![
            key(Keycode::Home),
            key(Keycode::Right),
            key(Keycode::Right),
            key(Keycode::Right),
            key(Keycode::Right),
            key(Keycode::Right),
            key(Keycode::Right),
            key(Keycode::Backspace),
            key(Keycode::Right),
            key(Keycode::Delete),
        ],
    ];
    let mut events = 0;
    let mut reviews = 0;
    for stored in [
        "Trail marker",
        "Trail\nmar\u{2028}ker\u{2029}",
        "e\u{301} 👨\u{200d}👩\u{200d}👧 \r\n\t",
        "A\n\u{301}B",
        "",
    ] {
        let mut runs = FieldImplementation::ALL.map(|implementation| {
            let (temporary, app) = labelled(implementation, stored, (960, 640));
            let input = crate::native_input::NativeInput::new(7);
            (temporary, app, input, String::new())
        });
        for sequence in &sequences {
            for (_, app, _, clipboard) in &mut runs {
                app.select_visible_object(crate::prototype::Capability::Field)
                    .unwrap();
                app.control(Control::Field("label".into())).unwrap();
                acknowledge_ui(app);
                clipboard.clear();
            }
            assert!(runs[1].1.tools.shared_draft().is_some());
            for event in sequence {
                let results = runs
                    .each_mut()
                    .map(|(_, app, input, clipboard)| route(app, input, event, clipboard));
                assert_eq!(results[0], results[1], "{stored:?} {event:?}");
                let [own, shared] = runs.each_ref().map(|run| &run.1);
                assert_eq!(
                    comparable(own.tools.draft_snapshot()),
                    comparable(shared.tools.draft_snapshot()),
                    "{stored:?} {event:?}"
                );
                assert_eq!(runs[0].3, runs[1].3, "the clipboard holds the same");
                events += 1;
            }
            // Enter reviews: the same pending source change, byte for byte.
            for run in &mut runs {
                acknowledge_ui(&mut run.1);
            }
            let results = runs.each_mut().map(|(_, app, input, clipboard)| {
                route(app, input, &key(Keycode::Return), clipboard)
            });
            assert_eq!(results[0], results[1], "{stored:?}");
            let pending = runs.each_ref().map(|run| {
                run.1.session.history().pending().map(|pending| {
                    pending
                        .changed()
                        .map(|file| (file.path.clone(), file.after.clone()))
                        .collect::<Vec<_>>()
                })
            });
            assert_eq!(pending[0], pending[1], "{stored:?}");
            reviews += usize::from(pending[0].is_some());
            for run in &mut runs {
                if run.1.session.history().pending().is_some() {
                    run.1.action(tilewright_editor_ui::Action::Cancel).unwrap();
                }
            }
        }
    }
    assert!(events > 150 && reviews > 10, "{events} {reviews}");
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

/// Every arrow press beside a stored-break marker moves the caret, in both
/// fields and both directions.
#[test]
fn the_shared_field_visits_every_position_beside_a_marker_with_no_press_that_does_nothing() {
    let stored = "Trail\nmar\u{2028}ker\u{2029}";
    let left = vec![15, 14, 13, 12, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
    let right = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 13, 14, 15, 18];
    for implementation in FieldImplementation::ALL {
        let (_temporary, mut app) = labelled(implementation, stored, (960, 640));
        app.control(Control::Field("label".into())).unwrap();
        acknowledge_ui(&mut app);
        assert_eq!(
            app.tools.shared_draft().is_some(),
            implementation == FieldImplementation::Shared
        );
        assert_eq!(
            presses(&mut app, Keycode::End, Keycode::Left, left.len()),
            left,
            "{implementation:?}"
        );
        assert_eq!(
            presses(&mut app, Keycode::Home, Keycode::Right, right.len()),
            right,
            "{implementation:?}"
        );
    }
}

#[test]
fn the_shared_field_has_no_button_of_its_own_and_its_text_takes_the_row() {
    let (_temporary, mut app) = labelled(FieldImplementation::Shared, "Trail marker", (960, 640));
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
    // and the frame is refused, as the editor's own field would be at the
    // same share; it is never drawn over the budget.
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
    app.share_string_field().unwrap();
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
fn raster_samples(scene: &tilewright_scene::SceneBundle, quads: &[tilewright_scene::Quad]) -> u64 {
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
/// 0.1.1 counts the two stops as one place, and the editor's own field
/// steps by the same rule, so every press of either field moves, to the
/// same stops.
#[test]
fn every_arrow_press_moves_the_shared_caret_where_two_stops_are_a_float_apart() {
    let stored = "\u{5d0}abb\u{5d0} ";
    let moved = |stops: &[(usize, f32)]| {
        stops
            .windows(2)
            .map(|pair| (pair[1].1 - pair[0].1).abs() > 0.5)
            .collect::<Vec<_>>()
    };
    let (_own, mut own) = labelled(FieldImplementation::Editor, stored, (960, 640));
    own.control(Control::Field("label".into())).unwrap();
    // The editor's own field steps over the pair too.
    let stops = visual_presses(&mut own, Keycode::Home, Keycode::Right, 6);
    let bytes: Vec<_> = stops.iter().map(|stop| stop.0).collect();
    assert_eq!(bytes, [8, 7, 2, 3, 4, 2, 0], "{stops:?}");
    assert!(moved(&stops).iter().all(|moved| *moved), "{stops:?}");
    let (_shared, mut shared) = labelled(FieldImplementation::Shared, stored, (960, 640));
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
    app.share_string_field().unwrap();
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

fn field_drawn(scene: &tilewright_scene::SceneBundle) -> bool {
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
    app.share_string_field().unwrap();
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
    let (_temporary, mut app) = labelled(FieldImplementation::Shared, "Trail marker", (960, 640));
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

/// `stored` in the label field, opened with `implementation` in a window
/// of `logical` size at `scale`, then `edit` applied to the draft.
fn opened(
    implementation: FieldImplementation,
    stored: &str,
    logical: (u32, u32),
    scale: u32,
    edit: Edit,
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = labelled(implementation, stored, logical);
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

/// Where a frame's quads are and their colours, in drawing order's place
/// a sorted list: the two fields' quads differ in identity, painter order
/// and atlas, not in what they cover or their colour.
fn coverage(scene: &tilewright_scene::SceneBundle) -> Vec<[u32; 12]> {
    let mut quads: Vec<_> = scene
        .quads()
        .iter()
        .map(|quad| {
            let mut key = [0; 12];
            for (slot, value) in key
                .iter_mut()
                .zip(quad.positions.as_flattened().iter().chain(&quad.color))
            {
                *slot = value.to_bits();
            }
            key
        })
        .collect();
    quads.sort_unstable();
    quads
}

/// The editor's own field row, pinned at the values it has always had, so
/// that a drift of the constants both fields now share is seen: the active
/// row fill, the text inset, the accent of the caret and selection, the
/// scroll margin, and the 4 px gap to the hint below.
#[test]
// The values are exact by construction.
#[allow(clippy::float_cmp)]
fn the_editors_own_field_row_keeps_its_look() {
    let rect = |quad: &tilewright_scene::Quad| {
        let [x, y] = quad.positions[0];
        [x, y, quad.positions[2][0] - x, quad.positions[2][1] - y]
    };
    let select = |app: &mut Application| app.tools.select_all().unwrap();
    let (_temporary, mut app) = opened(
        FieldImplementation::Editor,
        "Trail marker",
        (1280, 800),
        1,
        select,
    );
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
        FieldImplementation::Editor,
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
type Edit = fn(&mut Application);

#[test]
// The two fields must agree exactly, not within a tolerance.
#[allow(clippy::float_cmp)]
fn the_styled_shared_field_covers_what_the_editors_own_field_covers() {
    let long = "A long value past the width of the field, so it scrolls 0123";
    let cases: [(&str, &str, Edit); 7] = [
        ("empty", "", |_| {}),
        ("short", "Trail marker", |_| {}),
        ("long, scrolled to the end", long, |_| {}),
        ("mixed direction", "Trail עברית 123 marker", |_| {}),
        (
            "stored line breaks",
            "Trail\nmar\u{2028}ker\u{2029}",
            |_| {},
        ),
        ("preedit", "Trail marker", |app| {
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(6)), revision)
                .unwrap();
            app.tools.update_preedit("にほん", 2, 0).unwrap();
        }),
        ("selection", "Trail marker", |app| {
            app.tools.select_all().unwrap();
        }),
    ];
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        for (name, stored, edit) in cases {
            let [(_own, mut own), (_shared, mut shared)] = FieldImplementation::ALL
                .map(|implementation| opened(implementation, stored, logical, scale, edit));
            let case = format!("{name} at {logical:?}x{scale}");
            assert!(shared.tools.shared_draft().is_some(), "{case}");
            let frames = [&mut own, &mut shared]
                .map(|app| crate::prototype::workspace_presentation(app).unwrap());
            // The same row, so every row below stays where it was.
            let row = |app: &Application| {
                app.ui
                    .widgets
                    .iter()
                    .find(|widget| widget.control == Some(Control::EditDraft))
                    .map(|widget| widget.layout)
            };
            assert_eq!(row(&own), row(&shared), "{case}");
            assert_eq!(shared.ui.shared_field, row(&own), "{case}");
            // The same text origin, clip, size and caret.
            let [mine, theirs] = frames
                .each_ref()
                .map(|frame| frame.editable.as_ref().unwrap());
            assert_eq!(mine.origin, theirs.origin, "{case}");
            assert_eq!(mine.clip, theirs.clip, "{case}");
            assert_eq!(mine.layout.size(), theirs.layout.size(), "{case}");
            // The shared field's right padding is the editor's scroll
            // margin less the renderer's caret, so that caret is the width
            // the style counts on; a long value really scrolls.
            let cursor =
                crate::workspace_render::renderer_cursor(mine.snapshot.display_selection.focus);
            let caret = mine.layout.caret(cursor).unwrap();
            assert_eq!(caret.width, crate::workspace_render::CARET_WIDTH, "{case}");
            if stored == long {
                assert!(mine.origin[0] < mine.clip.x, "{case}: {:?}", mine.origin);
            }
            // The same quads, where they are and their colours: the row's
            // fill, the text, the selection, the caret and the underline.
            assert_eq!(
                coverage(&frames[0].scene),
                coverage(&frames[1].scene),
                "{case}"
            );
            assert!(field_drawn(&frames[1].scene), "{case}");
        }
    }
}

/// Open `draft` for meadow: its display name, or a new zone under it.
fn open_zone(app: &mut Application, draft: ZoneDraft) -> Result<(), String> {
    app.control(match draft {
        ZoneDraft::Name => Control::ZoneName,
        ZoneDraft::Create => Control::ZoneCreate(Some("meadow".into())),
    })
}

/// The sample with meadow's display name `name` (`None`: no name), with
/// `implementation`, in a window of `logical` size at `scale`, in Zone mode
/// with meadow chosen and `draft` open.
fn zoned(
    implementation: FieldImplementation,
    name: Option<&str>,
    logical: (u32, u32),
    scale: u32,
    draft: ZoneDraft,
) -> (tempfile::TempDir, Application) {
    let name = name.map(str::to_owned);
    let (temporary, mut app) = application_with_source_using(implementation, move |world| {
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
    assert_eq!(
        app.tools.shared_draft().is_some(),
        implementation == FieldImplementation::Shared
    );
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The sidebar's texts but the draft row's own (the shared field draws its
/// text itself): the hint, Review and Discard and every row below.
fn sidebar_hints(app: &mut Application) -> Vec<String> {
    app.refresh_ui();
    let x = app.ui.sidebar_x();
    app.ui
        .widgets
        .iter()
        .filter(|widget| widget.layout.x >= x && widget.control != Some(Control::EditDraft))
        .map(|widget| widget.text.clone())
        .collect()
}

/// Meadow's display names the zone tests open: plain, none, stored breaks,
/// joining marks and an emoji sequence, right-to-left, and 256 four-byte
/// characters, which fill the name draft's 1024 bytes exactly.
fn zone_names() -> [Option<String>; 6] {
    [
        Some("Quiet meadow".into()),
        None,
        Some("Stone\npath\u{2028}way\u{2029}".into()),
        Some("e\u{301} 👨\u{200d}👩\u{200d}👧".into()),
        Some("Quiet עברית 123 meadow".into()),
        Some("😀".repeat(256)),
    ]
}

#[test]
#[allow(clippy::too_many_lines)] // The event sequences are listed inline.
fn both_fields_turn_the_same_sdl_events_into_the_same_zone_drafts_and_reviews() {
    let key = |code| input_key(7, code, false);
    let shift = |code| modified_input_key(code, false, Mod::LSHIFTMOD);
    let command = |code| modified_input_key(code, false, Mod::LCTRLMOD);
    let full = "😀".repeat(256);
    let sequences: Vec<Vec<Event>> = vec![
        vec![command(Keycode::A), text("Open meadow")],
        vec![
            key(Keycode::End),
            text(" »"),
            key(Keycode::Home),
            key(Keycode::Right),
            key(Keycode::Backspace),
            key(Keycode::Delete),
        ],
        vec![
            shift(Keycode::Left),
            shift(Keycode::Left),
            command(Keycode::C),
            key(Keycode::Home),
            command(Keycode::V),
            command(Keycode::A),
            command(Keycode::X),
        ],
        vec![
            preedit("に", 1, 0),
            preedit("にほ", 2, 0),
            key(Keycode::Left),
            text("にほん"),
            key(Keycode::Home),
            preedit("x", 1, 0),
            key(Keycode::Escape),
            text("y"),
        ],
        vec![
            text("a\nb"),
            text("\u{2028}"),
            preedit("\u{2029}", 1, 0),
            text("😀".repeat(300).as_str()),
            command(Keycode::A),
            text(full.as_str()),
            text("a"),
            preedit("a", 1, 0),
            key(Keycode::Backspace),
            text("a"),
        ],
        vec![command(Keycode::A), text("meadow.bridge Old bridge")],
        vec![command(Keycode::A), text("meadow.bridge")],
        vec![command(Keycode::A), text("Bridge")],
        vec![command(Keycode::A), text("path")],
        vec![command(Keycode::A), text(" no id")],
        vec![
            command(Keycode::A),
            text(format!("{} {full}", "x".repeat(128)).as_str()),
            text("a"),
        ],
        vec![
            command(Keycode::A),
            text("grove "),
            text("é".repeat(257).as_str()),
        ],
        vec![command(Keycode::A), text("Odd \u{e000} name")],
        vec![preedit("に", 1, 0)],
        vec![text("x"), key(Keycode::Escape), text("z")],
    ];
    let (mut events, mut reviews) = (0, 0);
    let mut outcomes = std::collections::BTreeSet::new();
    for name in zone_names() {
        for draft in [ZoneDraft::Name, ZoneDraft::Create] {
            let mut runs = FieldImplementation::ALL.map(|implementation| {
                let (temporary, app) = zoned(implementation, name.as_deref(), (960, 640), 1, draft);
                let input = crate::native_input::NativeInput::new(7);
                (temporary, app, input, String::new())
            });
            for sequence in &sequences {
                for (_, app, _, clipboard) in &mut runs {
                    open_zone(app, draft).unwrap();
                    acknowledge_ui(app);
                    clipboard.clear();
                }
                assert!(runs[1].1.tools.shared_draft().is_some());
                // The same hint under the same row.
                let hints = runs.each_mut().map(|run| sidebar_hints(&mut run.1));
                assert_eq!(hints[0], hints[1], "{name:?} {draft:?}");
                let hint = match draft {
                    ZoneDraft::Name => "Name of meadow; empty = none",
                    ZoneDraft::Create => "New zone under meadow: id, name",
                };
                assert!(hints[0].iter().any(|text| text == hint), "{hints:?}");
                for event in sequence {
                    let results = runs
                        .each_mut()
                        .map(|(_, app, input, clipboard)| route(app, input, event, clipboard));
                    assert_eq!(results[0], results[1], "{name:?} {draft:?} {event:?}");
                    let [own, shared] = runs.each_ref().map(|run| &run.1);
                    assert_eq!(
                        comparable(own.tools.draft_snapshot()),
                        comparable(shared.tools.draft_snapshot()),
                        "{name:?} {draft:?} {event:?}"
                    );
                    assert_eq!(runs[0].3, runs[1].3, "the clipboard holds the same");
                    events += 1;
                }
                // The same hint again (an unsupported text says so), then
                // Enter reviews: the same refusal, or the same pending
                // source change byte for byte and the same review lines.
                let hints = runs.each_mut().map(|run| sidebar_hints(&mut run.1));
                assert_eq!(hints[0], hints[1], "{name:?} {draft:?} {sequence:?}");
                // Typing fences Return until a Return is released, as when
                // a person lets go of the keys and presses Enter; while
                // composing it stays fenced and Enter does not review.
                for run in &mut runs {
                    acknowledge_ui(&mut run.1);
                }
                let results = runs.each_mut().map(|(_, app, input, clipboard)| {
                    route(app, input, &input_key(7, Keycode::Return, true), clipboard)?;
                    route(app, input, &key(Keycode::Return), clipboard)
                });
                assert_eq!(results[0], results[1], "{name:?} {draft:?} {sequence:?}");
                let pending = runs.each_ref().map(|run| {
                    run.1.session.history().pending().map(|pending| {
                        pending
                            .changed()
                            .map(|file| (file.path.clone(), file.after.clone()))
                            .collect::<Vec<_>>()
                    })
                });
                assert_eq!(pending[0], pending[1], "{name:?} {draft:?} {sequence:?}");
                let lines = runs.each_mut().map(|run| run.1.review_lines(900));
                assert_eq!(lines[0], lines[1], "{name:?} {draft:?} {sequence:?}");
                if let Err(error) = &results[0] {
                    outcomes.insert(error.clone());
                }
                if pending[0].is_some() {
                    reviews += 1;
                    for line in &lines[0] {
                        for kind in ["Renames zone", "Removes the display name", "Creates zone"] {
                            if line.contains(kind) {
                                outcomes.insert(kind.to_owned());
                            }
                        }
                    }
                }
                for run in &mut runs {
                    if run.1.session.history().pending().is_some() {
                        run.1.action(tilewright_editor_ui::Action::Cancel).unwrap();
                    }
                }
            }
        }
    }
    // Every review outcome was reached: each change, each refusal.
    for outcome in [
        "Renames zone",
        "Removes the display name",
        "Creates zone",
        "A zone id is 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
        "This plane already has a zone with this id",
        "Type the new zone's id first",
        "A zone's display name has at most 256 characters",
        "missing font glyphs: review blocked; draft preserved",
    ] {
        assert!(outcomes.contains(outcome), "{outcome}: {outcomes:?}");
    }
    assert!(events > 600 && reviews > 60, "{events} {reviews}");
}

/// Render `presentation` with the reference renderer.
fn rendered(presentation: &crate::workspace_render::Presentation) -> Vec<u8> {
    use tilewright_render_api::{RenderRequest, Renderer};
    let viewport = presentation.scene.viewport();
    tilewright_render_testkit::ReferenceRenderer::new(tilewright_render_testkit::offscreen_target(
        viewport.width,
        viewport.height,
    ))
    .unwrap()
    .render(RenderRequest {
        scene: &presentation.scene,
        resources: presentation.resources.clone(),
    })
    .unwrap()
    .rgba8
}

/// The caret's and the preedit underline's quads, one pixel wider all
/// round: the only places the two fields may paint differently (the shared
/// field draws them over the text, the editor's own under it).
fn caret_and_underline(scene: &tilewright_scene::SceneBundle) -> Vec<[f32; 4]> {
    scene
        .quads()
        .iter()
        .filter(|quad| quad.color == crate::workspace_render::FIELD_ACCENT)
        .map(|quad| {
            let [x, y] = quad.positions[0];
            let [right, bottom] = quad.positions[2];
            [x - 1.0, y - 1.0, right + 1.0, bottom + 1.0]
        })
        .filter(|[x, y, right, bottom]| right - x <= 4.0 || bottom - y <= 4.0)
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)] // The cases are listed inline.
fn the_shared_zone_name_fields_cover_and_paint_what_the_editors_own_fields_do() {
    let long = "A long display name past the width of the field, so it scrolls 0123";
    let cases: [(&str, ZoneDraft, Option<&str>, Edit); 11] = [
        ("name", ZoneDraft::Name, Some("Quiet meadow"), |_| {}),
        ("no name", ZoneDraft::Name, None, |_| {}),
        (
            "long name, scrolled to the end",
            ZoneDraft::Name,
            Some(long),
            |_| {},
        ),
        (
            "mixed direction",
            ZoneDraft::Name,
            Some("Quiet עברית 123 meadow"),
            |_| {},
        ),
        (
            "stored line breaks",
            ZoneDraft::Name,
            Some("Stone\npath\u{2028}way\u{2029}"),
            |_| {},
        ),
        ("preedit", ZoneDraft::Name, Some("Quiet meadow"), |app| {
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(6)), revision)
                .unwrap();
            app.tools.update_preedit("にほん", 2, 0).unwrap();
        }),
        ("selection", ZoneDraft::Name, Some("Quiet meadow"), |app| {
            app.tools.select_all().unwrap();
        }),
        ("new zone, empty", ZoneDraft::Create, None, |_| {}),
        ("new zone, typed", ZoneDraft::Create, None, |app| {
            app.tools.input("meadow.bridge Old bridge").unwrap();
        }),
        // A character the font pack lacks: the toolbar's banner counts the
        // draft's mark in both fields.
        (
            "name, unsupported glyph",
            ZoneDraft::Name,
            Some("Odd \u{e000} name"),
            |_| {},
        ),
        (
            "new zone, unsupported glyph",
            ZoneDraft::Create,
            None,
            |app| {
                app.tools.input("meadow.b Odd \u{e000} name").unwrap();
            },
        ),
    ];
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        for (name, draft, stored, edit) in cases {
            let [(_own, mut own), (_shared, mut shared)] =
                FieldImplementation::ALL.map(|implementation| {
                    let (temporary, mut app) = zoned(implementation, stored, logical, scale, draft);
                    edit(&mut app);
                    acknowledge_ui(&mut app);
                    (temporary, app)
                });
            let case = format!("{name} at {logical:?}x{scale}");
            let banner = same_frames(&mut own, &mut shared, &case, stored == Some(long));
            let expected = match name {
                "name, unsupported glyph" => Some("Unsupported text: 4 marks / fields read-only"),
                "new zone, unsupported glyph" => {
                    Some("Unsupported text: 1 marks / fields read-only")
                }
                _ => None,
            };
            // At 720 px the toolbar has no room left for the banner.
            if logical.0 >= 1280 {
                assert_eq!(banner.as_deref(), expected, "{case}");
            }
        }
    }
}

/// The toolbar's unsupported-text banner, if it shows.
fn banner(app: &Application) -> Option<String> {
    app.ui
        .widgets
        .iter()
        .find(|widget| widget.text.starts_with("Unsupported text:"))
        .map(|widget| widget.text.clone())
}

/// Present both fields' frames and check that they agree: the same draft
/// row and reserved row, text origin, clip and size, the same quads where
/// they are and their colours, the same toolbar unsupported-text count and
/// banner (returned), and, rendered, the same pixels except where the caret
/// or the underline is. `scrolled`: the text must be scrolled.
// The two fields must agree exactly, not within a tolerance.
#[allow(clippy::float_cmp)]
fn same_frames(
    own: &mut Application,
    shared: &mut Application,
    case: &str,
    scrolled: bool,
) -> Option<String> {
    assert!(shared.tools.shared_draft().is_some(), "{case}");
    let frames =
        [&mut *own, &mut *shared].map(|app| crate::prototype::workspace_presentation(app).unwrap());
    let row = |app: &Application| {
        app.ui
            .widgets
            .iter()
            .find(|widget| widget.control == Some(Control::EditDraft))
            .map(|widget| widget.layout)
    };
    assert_eq!(row(own), row(shared), "{case}");
    assert_eq!(shared.ui.shared_field, row(own), "{case}");
    assert_eq!(
        row(own).map(|row| row.height),
        Some(crate::workspace_render::FIELD_ROW_HEIGHT)
    );
    assert_eq!(own.unsupported_runs, shared.unsupported_runs, "{case}");
    assert_eq!(banner(own), banner(shared), "{case}");
    let [mine, theirs] = frames
        .each_ref()
        .map(|frame| frame.editable.as_ref().unwrap());
    assert_eq!(mine.origin, theirs.origin, "{case}");
    assert_eq!(mine.clip, theirs.clip, "{case}");
    assert_eq!(mine.layout.size(), theirs.layout.size(), "{case}");
    if scrolled {
        assert!(mine.origin[0] < mine.clip.x, "{case}: {:?}", mine.origin);
    }
    assert_eq!(
        coverage(&frames[0].scene),
        coverage(&frames[1].scene),
        "{case}"
    );
    assert!(field_drawn(&frames[1].scene), "{case}");
    // Rendered, the frames are the same pixels except where the caret or
    // the underline is.
    let allowed: Vec<_> = frames
        .iter()
        .flat_map(|frame| caret_and_underline(&frame.scene))
        .collect();
    assert!(!allowed.is_empty(), "{case}");
    let width = frames[0].scene.viewport().width;
    let pixels = frames.each_ref().map(rendered);
    for (index, (a, b)) in pixels[0]
        .chunks_exact(4)
        .zip(pixels[1].chunks_exact(4))
        .enumerate()
    {
        if a == b {
            continue;
        }
        let index = u32::try_from(index).unwrap();
        #[allow(clippy::cast_precision_loss)]
        let (x, y) = ((index % width) as f32 + 0.5, (index / width) as f32 + 0.5);
        assert!(
            allowed.iter().any(|[left, top, right, bottom]| {
                (*left..=*right).contains(&x) && (*top..=*bottom).contains(&y)
            }),
            "{case}: pixel ({x}, {y}) differs: {a:?} {b:?}"
        );
    }
    banner(own)
}

/// A string field holding a character the font pack lacks: the toolbar's
/// banner counts its mark in both fields, and the frames agree.
#[test]
fn an_unsupported_glyph_in_the_string_field_counts_in_the_banner_with_either_field() {
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        let [(_own, mut own), (_shared, mut shared)] =
            FieldImplementation::ALL.map(|implementation| {
                opened(
                    implementation,
                    "Odd \u{e000} marker",
                    logical,
                    scale,
                    |_| {},
                )
            });
        let case = format!("string field at {logical:?}x{scale}");
        let banner = same_frames(&mut own, &mut shared, &case, false);
        assert!(own.unsupported_runs >= 1, "{case}");
        if logical.0 >= 1280 {
            assert_eq!(
                banner,
                Some(format!(
                    "Unsupported text: {} marks / fields read-only",
                    own.unsupported_runs
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
        let (_temporary, mut app) = zoned(
            FieldImplementation::Shared,
            Some("Quiet meadow"),
            (960, 640),
            1,
            draft,
        );
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

/// The road fence and the pause refuse a zone draft the same way with the
/// switch on: nothing opens, with the editor's own message.
#[test]
fn the_road_fence_and_the_pause_refuse_zone_drafts_with_either_field() {
    use crate::prototype::ROAD_IN_PROGRESS;
    let mut paused = Vec::new();
    for implementation in FieldImplementation::ALL {
        let (_temporary, mut app) = super::road::road_application(1, |_| {});
        if implementation == FieldImplementation::Shared {
            app.share_string_field().unwrap();
        }
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
        let (_temporary, mut app) = zoned(
            implementation,
            Some("Quiet meadow"),
            (960, 640),
            1,
            ZoneDraft::Name,
        );
        app.tools.select_all().unwrap();
        app.tools.input("Open meadow").unwrap();
        app.blocked = Some(crate::prototype::Blocked::new(
            crate::prototype::BlockCause::Present,
            "test",
        ));
        let review = app.control(Control::ApplyDraft).unwrap_err();
        assert_eq!(app.tools.draft_text(), Some("Open meadow"));
        assert!(app.session.history().pending().is_none());
        let refusals: Vec<_> = [ZoneDraft::Name, ZoneDraft::Create]
            .into_iter()
            .map(|draft| open_zone(&mut app, draft).unwrap_err())
            .collect();
        assert_eq!(app.tools.draft_text(), Some("Open meadow"), "kept");
        paused.push((review, refusals));
    }
    assert_eq!(paused[0], paused[1]);
}

/// The sample in Place mode, in a window of `logical` size at `scale`, with
/// the palette filter edited by `implementation`.
fn placed(
    implementation: FieldImplementation,
    logical: (u32, u32),
    scale: u32,
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = super::application_using(implementation);
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.reflow().unwrap();
    app.control(Control::Place).unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// Everything the filter's events can change that a person sees or that
/// later events depend on.
#[derive(Debug, PartialEq)]
struct FilterState {
    filter: String,
    focused: bool,
    composing: bool,
    snapshot: Option<TextInputSnapshot>,
    /// Each laid-out palette row: its text, whether enabled, whether chosen.
    rows: Vec<(String, bool, bool)>,
    scroll: usize,
    /// The pending change's operations ([`operations`]).
    pending: Option<String>,
    mode: tilewright_editor_ui::tools::ToolMode,
    zones: bool,
    placement: Option<String>,
    unsupported: usize,
    banner: Option<String>,
    status: String,
    /// The sidebar's other rows (the filter's own row is drawn by the
    /// field): hint, placeholder or `Filter: …`, Placing, the palette.
    sidebar: Vec<String>,
}

/// A sealed change's operations, each placed object's uid left out: a
/// placement mints a fresh random uid (and so a different result digest)
/// in every run.
fn operations(sealed: &str) -> String {
    let operations = &sealed[sealed.find("\"operations\":").unwrap()..];
    let mut out = String::new();
    let mut rest = operations;
    while let Some(at) = rest.find("\"uid\":\"") {
        let value = at + "\"uid\":\"".len();
        out.push_str(&rest[..value]);
        rest = &rest[value..];
        rest = &rest[rest.find('"').unwrap()..];
    }
    out.push_str(rest);
    out
}

/// `text` with every placed object's uid (`o.` and 26 base-32 letters,
/// fresh and random in every run) written `o.*`, elided or not.
fn without_uids(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("o.") {
        out.push_str(&rest[..at + 2]);
        rest = &rest[at + 2..];
        let uid = rest
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
            .unwrap_or(rest.len());
        if uid > 0 {
            out.push('*');
            rest = &rest[uid..];
        }
    }
    out.push_str(rest);
    out
}

fn filter_state(app: &mut Application) -> FilterState {
    app.refresh_ui();
    let rows = app
        .ui
        .widgets
        .iter()
        .filter(|widget| matches!(widget.control, Some(Control::PlacePalette(_))))
        .map(|widget| (widget.text.clone(), widget.enabled, widget.active))
        .collect();
    FilterState {
        filter: app.tools.palette_filter().to_owned(),
        focused: app.tools.filter_focused(),
        composing: app.tools.is_composing(),
        snapshot: comparable(app.tools.draft_snapshot()),
        rows,
        scroll: app.sidebar_scroll,
        pending: app
            .session
            .history()
            .pending()
            .map(|pending| operations(pending.sealed())),
        mode: app.tools.mode(),
        zones: app.tools.zones_visible(),
        placement: app.tools.placement_choice().map(str::to_owned),
        unsupported: app.unsupported_runs,
        banner: banner(app),
        status: without_uids(&app.status_label()),
        sidebar: sidebar_hints(app)
            .iter()
            .map(|text| without_uids(text))
            .collect(),
    }
}

/// One step of the randomised filter run.
#[derive(Clone, Debug)]
enum FilterStep {
    /// A key, text or composition event through the editor's routing.
    Event(Event),
    /// A Return press after its release, as a person presses Enter.
    Enter,
    /// A left press on the filter's row, `fraction` of its width in (none
    /// outside Place mode).
    ClickFilter(f32),
    /// A left press on the `index`-th laid-out palette row.
    ClickRow(usize),
    /// A left press on the pinned "Placing" row, or on the map.
    ClickPlacing,
    ClickMap,
    /// The sidebar scrolled to `offset` rows, as the mouse wheel sets it.
    Scroll(usize),
}

struct Seeded(u64);
impl Seeded {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % bound as u64).unwrap()
    }
}

fn filter_step(random: &mut Seeded) -> FilterStep {
    const TYPED: [&str; 17] = [
        "a", "St", "ONE", "r", "T", "x", "é", "青", "N", "O", "/", "lamp", "\n", "\u{2028}",
        "a\tb", "\u{e000}", "e\u{301}",
    ];
    const COMPOSED: [(&str, i32, i32); 4] = [
        ("に", 1, 0),
        ("にほ", 2, 0),
        ("x", 1, 0),
        ("\u{2029}", 1, 0),
    ];
    const KEYS: [Keycode; 17] = [
        Keycode::Home,
        Keycode::End,
        Keycode::Left,
        Keycode::Right,
        Keycode::Backspace,
        Keycode::Delete,
        Keycode::Escape,
        Keycode::N,
        Keycode::O,
        Keycode::S,
        Keycode::T,
        Keycode::P,
        Keycode::Q,
        Keycode::Z,
        Keycode::Slash,
        Keycode::KpDivide,
        Keycode::Escape,
    ];
    const SHIFTED: [Keycode; 4] = [Keycode::Left, Keycode::Right, Keycode::Home, Keycode::End];
    const COMMANDS: [Keycode; 4] = [Keycode::A, Keycode::C, Keycode::X, Keycode::V];
    #[allow(clippy::cast_precision_loss)]
    match random.below(46) {
        0..=9 => FilterStep::Event(text(TYPED[random.below(TYPED.len())])),
        10 => FilterStep::Event(text("😀".repeat(15 + random.below(3)).as_str())),
        11..=13 => {
            let (value, start, length) = COMPOSED[random.below(COMPOSED.len())];
            FilterStep::Event(preedit(value, start, length))
        }
        14..=23 => FilterStep::Event(input_key(7, KEYS[random.below(KEYS.len())], false)),
        24 | 25 => FilterStep::Event(modified_input_key(
            SHIFTED[random.below(SHIFTED.len())],
            false,
            Mod::LSHIFTMOD,
        )),
        26..=28 => FilterStep::Event(modified_input_key(
            COMMANDS[random.below(COMMANDS.len())],
            false,
            Mod::LCTRLMOD,
        )),
        29 | 30 => FilterStep::Enter,
        31..=33 => FilterStep::ClickFilter(random.below(100) as f32 / 100.0),
        34 => FilterStep::ClickRow(random.below(4)),
        35 => FilterStep::ClickPlacing,
        36 => FilterStep::ClickMap,
        37 | 38 => FilterStep::Scroll(random.below(7)),
        // `/` again, often: P, T, N and Z leave the filter or Place mode.
        39..=42 => FilterStep::Event(input_key(7, Keycode::Slash, false)),
        _ => FilterStep::Event(input_key(7, Keycode::P, false)),
    }
}

/// A left press at logical `at`, through the native loop's routing.
fn press_at(app: &mut Application, at: [f32; 2]) -> Result<(), String> {
    let logical = app.ui.logical_size();
    let physical = app.ui.size;
    crate::prototype::native_input_event(
        app,
        &super::zones::button(true, at),
        logical,
        physical,
        false,
        false,
    )
    .unwrap()
}

/// A logical point `fraction` of the way across the first widget `find`
/// keeps, half way down it.
#[allow(clippy::cast_precision_loss)]
fn widget_at(
    app: &mut Application,
    find: impl Fn(&crate::workspace_ui::Widget) -> bool,
    fraction: f32,
) -> Option<[f32; 2]> {
    app.refresh_ui();
    let widget = app.ui.widgets.iter().find(|widget| find(widget))?;
    let layout = widget.layout;
    Some([
        layout.x as f32 + 1.0 + (layout.width.saturating_sub(2)) as f32 * fraction,
        layout.y as f32 + layout.height as f32 / 2.0,
    ])
}

fn run_step(
    app: &mut Application,
    input: &mut crate::native_input::NativeInput,
    clipboard: &mut String,
    step: &FilterStep,
) -> Result<(), String> {
    match step {
        FilterStep::Event(event) => route(app, input, event, clipboard),
        FilterStep::Enter => {
            route(app, input, &input_key(7, Keycode::Return, true), clipboard)?;
            route(app, input, &input_key(7, Keycode::Return, false), clipboard)
        }
        FilterStep::ClickFilter(fraction) => {
            let at = widget_at(
                app,
                |widget| {
                    matches!(
                        widget.control,
                        Some(Control::PaletteFilter | Control::EditDraft)
                    )
                },
                *fraction,
            );
            // Outside Place mode (T, N or Z left it) there is no filter.
            at.map_or(Ok(()), |at| press_at(app, at))
        }
        FilterStep::ClickRow(index) => {
            app.refresh_ui();
            let row = app
                .ui
                .widgets
                .iter()
                .filter(|widget| matches!(widget.control, Some(Control::PlacePalette(_))))
                .nth(*index)
                .map(|widget| widget.control.clone());
            let at =
                row.and_then(|control| widget_at(app, |widget| widget.control == control, 0.5));
            at.map_or(Ok(()), |at| press_at(app, at))
        }
        FilterStep::ClickPlacing => {
            let at = widget_at(app, |widget| widget.text.starts_with("Placing"), 0.5);
            at.map_or(Ok(()), |at| press_at(app, at))
        }
        FilterStep::ClickMap => {
            let (width, height) = app.ui.logical_size();
            #[allow(clippy::cast_precision_loss)]
            press_at(app, [width as f32 / 4.0, height as f32 / 2.0])
        }
        FilterStep::Scroll(offset) => {
            app.sidebar_scroll = *offset;
            Ok(())
        }
    }
}

/// The randomised differential for the palette filter through real SDL
/// events: from Place mode in four windows, both fields take the same
/// seeded steps (text, compositions, keys with and without Shift and Ctrl,
/// the clipboard, Enter, `/`, N, O, presses on the filter, a palette row, the
/// "Placing" row and the map, and the sidebar scrolled). After every step,
/// presented as the native loop presents it, both give the same result and
/// the same filter, focus, composition, snapshot, palette rows, sidebar
/// scroll, pending change, mode, overlay, choice, unsupported-text count and
/// banner, status line, sidebar texts and clipboard.
#[test]
#[allow(clippy::too_many_lines)] // The coverage counts are listed inline.
fn both_fields_turn_the_same_random_sdl_events_into_the_same_filter_rows_and_scroll() {
    let (mut steps, mut refusals, mut focused, mut pulled_back) = (0, 0, 0, 0);
    let (mut released_composing, mut cleared, mut placed_changes) = (0, 0, 0);
    let mut kept = std::collections::BTreeSet::new();
    let mut banners = 0;
    for seed in [
        0x5f11_7e25_u64,
        0x0dd1_7e57,
        0x2c0f_fee5,
        0x7a1e_5eed,
        0x51de_ba12,
        0x0b5e_55ed,
    ] {
        for (logical, scale) in [
            ((1280, 800), 1),
            ((720, 480), 1),
            ((720, 480), 2),
            ((320, 240), 1),
        ] {
            let mut random = Seeded(seed ^ u64::from(logical.1 * scale));
            let mut runs = FieldImplementation::ALL.map(|implementation| {
                let (temporary, app) = placed(implementation, logical, scale);
                let input = crate::native_input::NativeInput::new(7);
                (temporary, app, input, String::new())
            });
            for _ in 0..200 {
                let step = filter_step(&mut random);
                let case = format!("{seed:x} {logical:?}x{scale} {step:?}");
                let before = filter_state(&mut runs[0].1);
                let results = runs
                    .each_mut()
                    .map(|(_, app, input, clipboard)| run_step(app, input, clipboard, &step));
                assert_eq!(results[0], results[1], "{case}");
                refusals += usize::from(results[0].is_err());
                // Presented, as the native loop presents after each event.
                for run in &mut runs {
                    acknowledge_ui(&mut run.1);
                }
                let states = runs.each_mut().map(|run| filter_state(&mut run.1));
                assert_eq!(states[0], states[1], "{case}");
                assert_eq!(runs[0].3, runs[1].3, "{case}: the clipboard holds the same");
                // The shared run really edits with the shared field.
                assert_eq!(runs[1].1.tools.shared_draft().is_some(), states[1].focused);
                assert!(runs[0].1.tools.shared_draft().is_none());
                let state = &states[0];
                focused += usize::from(state.focused);
                kept.insert(state.rows.len());
                pulled_back +=
                    usize::from(state.scroll < before.scroll && state.filter != before.filter);
                released_composing += usize::from(before.composing && !state.focused);
                cleared += usize::from(!before.filter.is_empty() && state.filter.is_empty());
                placed_changes += usize::from(before.pending.is_none() && state.pending.is_some());
                banners += usize::from(state.banner.is_some() && state.focused);
                steps += 1;
            }
        }
    }
    // Every number of laid-out rows, none to all six, was reached.
    assert_eq!(kept, (0..=6).collect(), "{kept:?}");
    assert!(
        steps >= 4_800 && refusals > 200 && focused > 2_000,
        "{steps} {refusals} {focused}"
    );
    assert!(
        pulled_back > 0 && released_composing > 0 && cleared > 20,
        "{pulled_back} {released_composing} {cleared}"
    );
    assert!(
        placed_changes > 0 && banners > 0,
        "{placed_changes} {banners}"
    );
}

/// Place mode with the filter focused and `edit` applied to it.
fn filtering(
    implementation: FieldImplementation,
    logical: (u32, u32),
    scale: u32,
    edit: Edit,
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = placed(implementation, logical, scale);
    app.control(Control::PaletteFilter).unwrap();
    assert!(app.tools.filter_focused());
    assert_eq!(
        app.tools.shared_draft().is_some(),
        implementation == FieldImplementation::Shared
    );
    edit(&mut app);
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// The focused filter's row in both fields, at the three window sizes: the
/// same row and reserved row, origin, clip, size, quad coverage, toolbar
/// count and banner, and the same pixels but the caret and underline.
#[test]
#[allow(clippy::too_many_lines)] // The cases are listed inline.
fn the_shared_palette_filter_covers_and_paints_what_the_editors_own_filter_does() {
    let cases: [(&str, Edit); 9] = [
        ("empty", |_| {}),
        ("one row kept", |app| app.tools.input("lamp").unwrap()),
        ("no row kept", |app| app.tools.input("zz").unwrap()),
        ("long, scrolled to the end", |app| {
            app.tools
                .input("A long filter past the width of its row, so it scrolls 0123456")
                .unwrap();
        }),
        ("mixed direction", |app| {
            app.tools.input("עברית 12 lamp").unwrap();
        }),
        ("preedit", |app| {
            app.tools.input("lap").unwrap();
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(2)), revision)
                .unwrap();
            app.tools.update_preedit("にほん", 2, 0).unwrap();
        }),
        ("selection", |app| {
            app.tools.input("ate").unwrap();
            app.tools.select_all().unwrap();
        }),
        ("joining marks", |app| {
            app.tools.input("e\u{301} 👨\u{200d}👩\u{200d}👧").unwrap();
        }),
        // A character the font pack lacks: the toolbar's banner counts the
        // filter's mark in both fields.
        ("unsupported glyph", |app| {
            app.tools.input("Odd \u{e000} filter").unwrap();
        }),
    ];
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        for (name, edit) in cases {
            let [(_own, mut own), (_shared, mut shared)] = FieldImplementation::ALL
                .map(|implementation| filtering(implementation, logical, scale, edit));
            let case = format!("{name} at {logical:?}x{scale}");
            let banner = same_frames(
                &mut own,
                &mut shared,
                &case,
                name == "long, scrolled to the end",
            );
            // The same hint, palette rows and every other sidebar text.
            assert_eq!(
                sidebar_hints(&mut own),
                sidebar_hints(&mut shared),
                "{case}"
            );
            assert!(
                sidebar_hints(&mut own)
                    .iter()
                    .any(|text| text == "Enter keeps the filter / Esc clears it"),
                "{case}"
            );
            // The filter row sits at the top of the sidebar, above "Placing".
            let placing = own
                .ui
                .widgets
                .iter()
                .find(|widget| widget.text.starts_with("Placing"))
                .unwrap()
                .layout;
            assert!(shared.ui.shared_field.unwrap().y < placing.y, "{case}");
            if logical.0 >= 1280 {
                assert_eq!(
                    banner.as_deref(),
                    (name == "unsupported glyph")
                        .then_some("Unsupported text: 1 marks / fields read-only"),
                    "{case}"
                );
            }
        }
    }
}

/// The filter's field is compiled first and the editor's frame fitted to
/// leave its quads and raster samples free, as for the drafts; below the
/// field alone editing pauses with the filter kept.
#[test]
fn a_filter_frame_leaves_room_for_the_shared_field_and_pauses_without_it() {
    let (_temporary, mut app) = filtering(FieldImplementation::Shared, (960, 640), 1, |app| {
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

/// One paused step's result, then the filter's text, focus and composition,
/// whether the shared field edits it, and whether nothing is pending.
type PausedStep = (Result<(), String>, String, bool, bool, bool, bool);

/// The road fence and the pause treat the filter the same with the switch
/// on: while a road is drawn the filter cannot be focused; while editing is
/// paused it is focused, edited, released and cleared as without the
/// shared field, and never makes a change.
#[test]
fn the_road_fence_and_the_pause_treat_the_filter_the_same_with_either_field() {
    use crate::prototype::ROAD_IN_PROGRESS;
    let mut outcomes = Vec::new();
    for implementation in FieldImplementation::ALL {
        let (_temporary, mut app) = super::road::road_application(1, |_| {});
        if implementation == FieldImplementation::Shared {
            app.share_string_field().unwrap();
        }
        super::road::click(&mut app, [0, 0]).unwrap();
        assert!(app.tools.road().is_some());
        assert_eq!(
            app.control(Control::PaletteFilter).unwrap_err(),
            ROAD_IN_PROGRESS
        );
        let mut input = crate::native_input::NativeInput::new(7);
        let mut clipboard = String::new();
        for key in [Keycode::Slash, Keycode::KpDivide] {
            route(
                &mut app,
                &mut input,
                &input_key(7, key, false),
                &mut clipboard,
            )
            .unwrap();
        }
        assert!(!app.tools.filter_focused());
        assert!(app.tools.draft_text().is_none());
        assert_eq!(super::road::points(&app), [[0, 0]]);
        // Paused: the filter is still local view state.
        let (_temporary, mut app) = filtering(implementation, (960, 640), 1, |app| {
            app.tools.input("la").unwrap();
        });
        app.blocked = Some(crate::prototype::Blocked::new(
            crate::prototype::BlockCause::Present,
            "test",
        ));
        let mut input = crate::native_input::NativeInput::new(7);
        let mut clipboard = String::new();
        let mut steps = Vec::new();
        for event in [
            text("m"),
            input_key(7, Keycode::Return, true),
            input_key(7, Keycode::Return, false),
            input_key(7, Keycode::Slash, false),
            text("p"),
            preedit("x", 1, 0),
            input_key(7, Keycode::Escape, false),
            input_key(7, Keycode::Escape, false),
        ] {
            let result = route(&mut app, &mut input, &event, &mut clipboard);
            steps.push((
                result,
                app.tools.palette_filter().to_owned(),
                app.tools.filter_focused(),
                app.tools.is_composing(),
                app.tools.shared_draft().is_some(),
                app.session.history().pending().is_none(),
            ));
            acknowledge_ui(&mut app);
        }
        outcomes.push(steps);
    }
    // The same results, text and focus; only which field edits differs.
    let strip = |steps: &[PausedStep]| {
        steps
            .iter()
            .map(|(result, text, focused, composing, _, nothing)| {
                (result.clone(), text.clone(), *focused, *composing, *nothing)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&outcomes[0]), strip(&outcomes[1]), "{outcomes:#?}");
    for (step, (_, _, focused, _, shared, _)) in outcomes[1].iter().enumerate() {
        assert_eq!(shared, focused, "step {step}");
    }
    // Typing, Enter, `/`, a composition and Esc work as when not paused.
    let expected = [
        ("lam", true, false),
        ("lam", true, false),
        ("lam", false, false),
        ("lam", true, false),
        ("lamp", true, false),
        ("lamp", true, true),
        ("lamp", true, false),
        ("", false, false),
    ];
    for (step, ((result, text, focused, composing, _, nothing), expected)) in
        outcomes[0].iter().zip(expected).enumerate()
    {
        assert_eq!(result, &Ok(()), "step {step}");
        assert_eq!(
            (text.as_str(), *focused, *composing),
            expected,
            "step {step}"
        );
        assert!(nothing, "step {step}: the filter never makes a change");
    }
}
