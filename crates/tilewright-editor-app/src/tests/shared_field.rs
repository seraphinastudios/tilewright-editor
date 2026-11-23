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
        .attach(&mut presentation)
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

/// The caret after each of `presses` Left presses from End.
fn lefts_from_end(app: &mut Application, presses: usize) -> Vec<usize> {
    let mut input = crate::native_input::NativeInput::new(7);
    let mut clipboard = String::new();
    route(
        app,
        &mut input,
        &input_key(7, Keycode::End, false),
        &mut clipboard,
    )
    .unwrap();
    (0..presses)
        .map(|_| {
            route(
                app,
                &mut input,
                &input_key(7, Keycode::Left, false),
                &mut clipboard,
            )
            .unwrap();
            app.tools.draft_snapshot().unwrap().selection.focus.byte
        })
        .collect()
}

/// A known limitation of the shared field, pinned so that moving to a
/// release with the fix has to update it on purpose. At its 20 px text the
/// renderer gives committed position 9 of this value two caret stops a
/// float apart (x 97.700005 and 97.7), and `step_caret` in
/// tilewright-ui-render 0.1.0 treats the affinity-only step between them
/// as a stop: one Left press there does nothing. The editor's own field,
/// at 18 px, is not affected. To be fixed in tilewright-ui-render 0.1.1.
#[test]
fn known_limitation_one_left_press_next_to_a_marker_does_nothing_in_the_shared_field() {
    let stored = "Trail\nmar\u{2028}ker\u{2029}";
    for (implementation, expected) in [
        (FieldImplementation::Editor, vec![15, 14, 13, 12, 9, 8, 7]),
        (FieldImplementation::Shared, vec![15, 14, 13, 12, 9, 9, 8]),
    ] {
        let (_temporary, mut app) = labelled(implementation, stored, (960, 640));
        app.control(Control::Field("label".into())).unwrap();
        acknowledge_ui(&mut app);
        assert_eq!(lefts_from_end(&mut app, 7), expected, "{implementation:?}");
    }
}

/// The pending change, file by file.
fn pending(app: &Application) -> Option<Vec<(String, Option<String>)>> {
    app.session.history().pending().map(|pending| {
        pending
            .changed()
            .map(|file| (file.path.clone(), file.after.clone()))
            .collect()
    })
}

#[test]
fn the_shared_apply_button_reviews_exactly_as_review_does() {
    // A label to review, then one with a character the font pack lacks,
    // which Review refuses.
    for typed in ["Applied", "a\u{10ffff}"] {
        let [(_one, mut clicked), (_two, mut reviewed)] = [0, 1].map(|_| {
            let (temporary, mut app) =
                labelled(FieldImplementation::Shared, "Trail marker", (960, 640));
            app.control(Control::Field("label".into())).unwrap();
            app.tools.select_all().unwrap();
            app.tools.input(typed).unwrap();
            acknowledge_ui(&mut app);
            (temporary, app)
        });
        let submit = clicked
            .displayed_text
            .as_ref()
            .and_then(|geometry| geometry.shared.as_ref())
            .and_then(|shared| shared.submit)
            .unwrap();
        let centre = [
            submit.x + submit.width / 2.0,
            submit.y + submit.height / 2.0,
        ];
        let by_click =
            crate::prototype::native_pointer(&mut clicked, centre, (960, 640), (320, 240));
        let by_review = reviewed.control(Control::ApplyDraft);
        assert_eq!(by_click, by_review, "{typed:?}");
        assert_eq!(pending(&clicked), pending(&reviewed), "{typed:?}");
        assert_eq!(pending(&clicked).is_some(), typed == "Applied");
        if typed != "Applied" {
            assert!(by_click.unwrap_err().contains("glyph"));
            assert_eq!(clicked.tools.draft_text(), Some(typed), "the draft is kept");
        }
    }
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
        Err(error) => assert!(error.contains("LimitExceeded"), "{error}"),
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
