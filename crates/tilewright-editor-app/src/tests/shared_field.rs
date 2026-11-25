// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The opt-in shared string field in the application: the switch, the
//! frame record, the reserved identities, refusals that pause rather than
//! panic, and a differential run of real SDL events against the editor's
//! own field.
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
