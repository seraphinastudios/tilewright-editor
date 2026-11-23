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
use tilewright_editor_ui::text_input::{TextInputSnapshot, TextRevision};

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
/// fields and both directions. At its 20 px text the renderer gives
/// committed position 9 of this value two caret stops a float apart (x
/// 97.700005 and 97.7); tilewright-ui-render 0.1.0 took the step between
/// them for a move, so one Left press there did nothing. 0.1.1 counts them
/// as one place.
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
            height: 52.0,
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
    use crate::prototype::proposal_view::{HIGHLIGHT_FROM_TINT, HIGHLIGHT_TINT};
    let mut twin = super::proposal::twin(1, |_| {}, |_| {}, None);
    super::proposal::open(&mut twin);
    let app = &mut twin.app;
    app.share_string_field().unwrap();
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(app);
    let highlight = |scene: &tilewright_scene::SceneBundle| {
        scene
            .quads()
            .iter()
            .filter(|quad| {
                !quad.pickable
                    && (quad.color == HIGHLIGHT_TINT || quad.color == HIGHLIGHT_FROM_TINT)
            })
            .count()
    };
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

#[test]
fn the_composed_frame_keeps_to_the_editors_own_raster_sample_limit() {
    let (_temporary, mut app) = labelled(FieldImplementation::Shared, "Trail marker", (960, 640));
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(&mut app);
    let full = crate::prototype::workspace_presentation(&mut app).unwrap();
    let quads = full.scene.quads();
    let total = raster_samples(&full.scene, quads);
    let editor: Vec<_> = quads
        .iter()
        .copied()
        .filter(|quad| quad.semantic_id.0 < SHARED_FIRST_SEMANTIC)
        .collect();
    let own = raster_samples(&full.scene, &editor);
    assert!(editor.len() < quads.len() && own < total, "{own} {total}");
    // Exactly enough: drawn whole.
    app.frame_limits.maximum_raster_samples = total;
    let fitted = crate::prototype::workspace_presentation(&mut app).unwrap();
    assert_eq!(fitted.scene.quads(), quads);
    // One sample less: the editor's own frame still fits, but the field
    // would take it over the editor's limit, so it is refused and editing
    // pauses with the draft kept. The crate reports no sample count, so
    // the field's samples are not set aside beforehand.
    app.frame_limits.maximum_raster_samples = total - 1;
    let error = crate::prototype::workspace_presentation(&mut app)
        .err()
        .unwrap();
    assert!(
        error.starts_with("shared field") && error.contains("LimitExceeded"),
        "{error}"
    );
    assert_eq!(app.tools.draft_text(), Some("Trail marker"));
    assert!(app.session.history().pending().is_none());
    // The native loop pauses on that refusal. The first Esc discards the
    // draft, and the next frame draws, but the pause stays; a second Esc
    // ends it (as a zoom or a pan would).
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
    crate::prototype::workspace_presentation(&mut app).unwrap();
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
    assert!(
        again
            .scene
            .quads()
            .iter()
            .any(|quad| quad.semantic_id.0 >= SHARED_FIRST_SEMANTIC)
    );
}
