// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The shared field on the inspector's boolean, integer, number and
//! reference fields, in a copy of the sample that grants them (the hand
//! test's lines): the frame budget, the road fence, the pause and the hand
//! test's steps.
use super::shared_string_field::{field_drawn, raster_samples, route};
use super::{acknowledge_ui, application_with_source, input_key, modified_input_key};
use crate::prototype::Application;
use crate::shared_field::SHARED_FIRST_SEMANTIC;
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::keyboard::{Keycode, Mod};
use std::{fs, path::Path};
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;

/// The lines the hand test (the docs' step 16) adds to a copy of the
/// sample, word for word: a boolean, an integer, a number and a reference
/// field in the object schema, right after `"fields": {`, ...
const SCHEMA_LINES: &str = r#"
    "flag": {"type": "boolean"},
    "count": {"type": "integer", "min": -2, "max": 7},
    "weight": {"type": "number", "min": -1, "max": 2},
    "kind": {"type": "reference", "target": "proto"},"#;

/// ... and each granted on `marker_1`, right after `"fields": [` under
/// `"editing"` in `preview.jsonc`.
const GRANT_LINES: &str = r#"
      {"uid": "marker_1", "field": "flag"},
      {"uid": "marker_1", "field": "count"},
      {"uid": "marker_1", "field": "weight"},
      {"uid": "marker_1", "field": "kind"},"#;

/// Add the hand test's lines to the sample copy at `root`, as a person
/// adds them; `marker_1`'s values for them stay absent.
pub(super) fn typed_world(root: &Path) {
    typed_world_with(root, false);
}

/// [`typed_world`], with `marker_1` holding [`Scalar::stored`] values when
/// `stored`.
fn typed_world_with(root: &Path, stored: bool) {
    // Right after the first `after` in the file, which is past `"editing"`
    // in the catalog.
    let insert = |name: &str, after: &str, lines: &str| {
        let path = root.join(name);
        let mut text = fs::read_to_string(&path).unwrap();
        let at = text.find(after).unwrap() + after.len();
        assert!(name != "preview.jsonc" || text.find("\"editing\"").unwrap() < at);
        text.insert_str(at, lines);
        fs::write(path, text).unwrap();
    };
    insert("profile/schema/object.jsonc", "\"fields\": {", SCHEMA_LINES);
    insert("preview.jsonc", "\"fields\": [", GRANT_LINES);
    if stored {
        let path = root.join("planes/main/layers/0/0_0.jsonc");
        let mut chunk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let fields = &mut chunk["objects"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|object| object["uid"] == "marker_1")
            .unwrap()["fields"];
        for scalar in Scalar::ALL {
            fields[scalar.name()] = serde_json::from_str(scalar.stored()).unwrap();
        }
        fs::write(path, serde_json::to_vec_pretty(&chunk).unwrap()).unwrap();
    }
}

/// The inspector's typed fields of [`SCHEMA_LINES`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scalar {
    Flag,
    Count,
    Weight,
    Kind,
}

impl Scalar {
    const ALL: [Self; 4] = [Self::Flag, Self::Count, Self::Weight, Self::Kind];

    fn name(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Count => "count",
            Self::Weight => "weight",
            Self::Kind => "kind",
        }
    }

    /// The value `marker_1` holds in [`typed_world_with`], as JSON.
    fn stored(self) -> &'static str {
        match self {
            Self::Flag => "true",
            Self::Count => "3",
            Self::Weight => "0.5",
            Self::Kind => "\"marker\"",
        }
    }

    fn control(self) -> Control {
        Control::Field(self.name().into())
    }

    /// The row below the field's own in the inspector.
    fn description(self) -> &'static str {
        match self {
            Self::Flag => "Boolean: true or false",
            Self::Count => "Integer (-2 to 7)",
            Self::Weight => "Number (-1 to 2)",
            Self::Kind => "Reference: proto",
        }
    }

    /// Text a person types into the draft: accepted, refused by each
    /// review check, at and past the schema's range, and in between.
    fn texts(self) -> &'static [&'static str] {
        match self {
            Self::Flag => &["true", "false", "True", "1", "yes", " true", ""],
            Self::Count => &[
                "7",
                "-2",
                "8",
                "-3",
                "0",
                "+3",
                "3.0",
                "٣",
                "9223372036854775808",
                " 3",
            ],
            Self::Weight => &[
                "2", "-1", "1.5", "2.01", "-1.01", "NaN", "inf", "1e0", ".5", "1,5", "٣",
            ],
            Self::Kind => &["marker", "stone", "grass", "Marker", "unknown", "lamp "],
        }
    }
}

fn select(app: &mut Application) -> Result<(), String> {
    app.action(Action::Select {
        generation: app.view.frame_revision,
        key: SelectionKey::Object {
            uid: "marker_1".into(),
        },
    })
}

/// The typed copy in a window of `logical` size at `scale`, with `scalar` open.
fn drafted(logical: (u32, u32), scale: u32, scalar: Scalar) -> (tempfile::TempDir, Application) {
    drafted_with(logical, scale, scalar, false)
}

/// [`drafted`], with `marker_1` holding its stored values when `stored`.
fn drafted_with(
    logical: (u32, u32),
    scale: u32,
    scalar: Scalar,
    stored: bool,
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = application_with_source(|root| typed_world_with(root, stored));
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.reflow().unwrap();
    app.control(Control::Select).unwrap();
    select(&mut app).unwrap();
    app.control(scalar.control()).unwrap();
    assert_eq!(open(&app), "field", "{scalar:?}");
    let opened = if stored {
        scalar.stored().trim_matches('"')
    } else {
        ""
    };
    assert_eq!(app.tools.draft_text(), Some(opened), "{scalar:?}");
    assert!(app.tools.shared_draft().is_some());
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// Which draft is open: an inspector field, another kind, or none.
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
        // These runs never open the coordinates: the one other draft is
        // an inspector field.
        "field"
    }
}

/// Each field's draft is compiled first and the editor's frame fitted to
/// leave its quads and raster samples free, as for the other drafts; below
/// the field alone editing pauses with the draft kept.
#[test]
fn a_typed_field_frame_leaves_room_for_the_shared_field_and_pauses_without_it() {
    for scalar in Scalar::ALL {
        let (_temporary, mut app) = drafted((960, 640), 1, scalar);
        let app = &mut app;
        app.tools.input(scalar.texts()[0]).unwrap();
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
        assert_eq!(all.len() - editor.len(), quads, "{scalar:?}");
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
            assert_eq!(fitted.scene.quads(), all, "{scalar:?} {spare:?}");
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
                    assert!(field_drawn(&fitted.scene), "{scalar:?} {short:?}");
                    assert!(fitted.scene.quads().len() <= all.len() - short.0);
                    assert!(raster_samples(&fitted.scene, fitted.scene.quads()) <= total - short.1);
                }
                Err(error) => assert!(
                    error.contains("LimitExceeded") && !error.starts_with("shared field"),
                    "{scalar:?} {short:?}: {error}"
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
            "{scalar:?}: {error}"
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
        select(app).unwrap();
        app.control(scalar.control()).unwrap();
        acknowledge_ui(app);
        let again = crate::prototype::workspace_presentation(app).unwrap();
        assert!(field_drawn(&again.scene), "{scalar:?}");
    }
}

/// The road fence and the pause refuse these fields: while a road is drawn
/// none opens, with the road's message;
/// while editing is paused none opens and an open one is not reviewed, and
/// its text is kept.
#[test]
fn the_road_fence_and_the_pause_refuse_typed_fields() {
    use crate::prototype::ROAD_IN_PROGRESS;
    for scalar in Scalar::ALL {
        let (_temporary, mut app) = super::road::road_application(1, typed_world);
        super::road::click(&mut app, [0, 0]).unwrap();
        assert!(app.tools.road().is_some());
        assert_eq!(
            app.control(scalar.control()).unwrap_err(),
            ROAD_IN_PROGRESS,
            "{scalar:?}"
        );
        assert!(app.tools.draft_text().is_none(), "{scalar:?}");
        assert_eq!(super::road::points(&app), [[0, 0]]);
        // Paused: the open draft is not reviewed and keeps its text,
        // and none opens.
        let (_temporary, mut app) = drafted((960, 640), 1, scalar);
        app.tools.input(scalar.texts()[0]).unwrap();
        app.blocked = Some(crate::prototype::Blocked::new(
            crate::prototype::BlockCause::Present,
            "test",
        ));
        acknowledge_ui(&mut app);
        let mut input = crate::native_input::NativeInput::new(7);
        let mut clipboard = String::new();
        // Review and Enter are refused in the pause's own words: no
        // edit is pending.
        let review = app.control(Control::ApplyDraft).unwrap_err();
        assert_eq!(review, "rendering blocked editing: test", "{scalar:?}");
        acknowledge_ui(&mut app);
        let enter = route(
            &mut app,
            &mut input,
            &input_key(7, Keycode::Return, false),
            &mut clipboard,
        );
        assert_eq!(enter, Err(review.clone()), "{scalar:?}");
        assert_eq!(app.tools.draft_text(), Some(scalar.texts()[0]));
        assert!(app.session.history().pending().is_none());
        let reopen = app.control(scalar.control()).unwrap_err();
        assert_eq!(reopen, review, "{scalar:?}");
        assert_eq!(app.tools.draft_text(), Some(scalar.texts()[0]), "kept");
        assert!(app.tools.shared_draft().is_some());
    }
}

/// The hand test's step 12 on the typed copy, through SDL events: each row
/// opens from the inspector with its description and hint, refuses in its
/// type's words, reviews its `SetField`, is accepted, and Undo brings the absent value back.
#[test]
#[allow(clippy::too_many_lines)] // The hand test's steps are listed inline.
fn the_hand_test_typed_fields_give_their_hints_refusals_and_reviews() {
    use super::zone_manage::{all_texts, enter, press, type_text};
    use super::zones::{key, zone_application};
    let (temporary, mut app) = zone_application(typed_world);
    // The session's own copy of the world, the one file the editor
    // writes (the person's copy is never written).
    let chunk = temporary
        .path()
        .join("world/planes/main/layers/0/0_0.jsonc");
    let original = fs::read(&chunk).unwrap();
    select(&mut app).unwrap();
    let texts = all_texts(&mut app);
    for scalar in Scalar::ALL {
        assert!(
            texts.contains(&format!("{} = <absent>", scalar.name())),
            "{scalar:?}: {texts:?}"
        );
    }
    let prompt = "Optional: enter a value to add";
    assert!(texts.iter().filter(|text| *text == prompt).count() >= 4);
    assert!(texts.contains(&"Choices: true, false".to_owned()));
    assert!(texts.contains(&"Choices: bush, crate, gate, grass,...".to_owned()));
    // The rows follow the order of the lines in the schema.
    let rows: Vec<_> = texts
        .iter()
        .filter(|text| text.contains(" = "))
        .take(6)
        .collect();
    assert_eq!(
        rows,
        [
            "flag = <absent>",
            "count = <absent>",
            "weight = <absent>",
            "kind = <absent>",
            "label = Trail marker",
            "settings = Grouped values"
        ],
        "{texts:?}"
    );
    for (scalar, refused, refusal, accepted, stored) in [
        (Scalar::Flag, "yes", "Enter true or false", "true", "true"),
        (
            Scalar::Count,
            "8",
            "Integer is outside schema range",
            "-2",
            "-2",
        ),
        (
            Scalar::Weight,
            "inf",
            "Number is non-finite or outside schema range",
            "1.5",
            "1.5",
        ),
        (
            Scalar::Kind,
            "unknown",
            "Reference does not name an authoritative definition",
            "stone",
            "\"stone\"",
        ),
    ] {
        press(&mut app, &scalar.control());
        assert_eq!(app.tools.draft_text(), Some(""), "{scalar:?}");
        assert!(app.tools.shared_draft().is_some());
        // Type into the draft as it is; for the second attempt, select
        // all with Ctrl+A first, as the step says.
        let attempt = |app: &mut Application, text: &str, select: bool| {
            if select {
                acknowledge_ui(app);
                super::zones::route(app, &modified_input_key(Keycode::A, false, Mod::LCTRLMOD))
                    .unwrap();
                let snapshot = app.tools.draft_snapshot().unwrap();
                assert_eq!(snapshot.selection.anchor.byte, 0, "{text}");
                assert_eq!(snapshot.selection.focus.byte, snapshot.committed.len());
            }
            type_text(app, text);
            acknowledge_ui(app);
            let result = super::zones::route(app, &key(Keycode::Return));
            let pending = app.session.history().pending().is_some();
            (text.to_owned(), result, pending, app.review_lines(900))
        };
        let refused = attempt(&mut app, refused, false);
        assert_eq!(refused.1, Err(refusal.to_owned()), "{scalar:?}");
        assert!(!refused.2, "{scalar:?}");
        let review = attempt(&mut app, accepted, true);
        assert!(review.1.is_ok() && review.2, "{scalar:?}: {review:?}");
        // The review's added line names the field and its new value.
        let added = format!("\"{}\": {stored}", scalar.name());
        assert!(
            review
                .3
                .iter()
                .any(|line| line.starts_with("+ ") && line.contains(&added)),
            "{scalar:?}: {added} {:?}",
            review.3
        );
        enter(&mut app);
        assert!(app.session.history().pending().is_none(), "accepted");
        let saved = String::from_utf8(fs::read(&chunk).unwrap()).unwrap();
        assert!(
            saved.contains(&format!("\"{}\": {stored}", scalar.name())),
            "{scalar:?}"
        );
        let texts = all_texts(&mut app);
        let shown = format!("{} = {}", scalar.name(), stored.trim_matches('"'));
        assert!(texts.contains(&shown), "{shown}: {texts:?}");
        assert!(
            texts.contains(&scalar.description().to_owned()),
            "{scalar:?}"
        );
    }
    // Undo and Accept (Z, then Enter), four times, bring the absent
    // values back byte for byte. A second Z before the Enter is refused:
    // each Undo is a pending change to accept first.
    for round in 0..Scalar::ALL.len() {
        acknowledge_ui(&mut app);
        super::zones::route(&mut app, &key(Keycode::Z)).unwrap();
        assert!(app.session.history().pending().is_some(), "{round}");
        if round == 0 {
            acknowledge_ui(&mut app);
            assert!(super::zones::route(&mut app, &key(Keycode::Z)).is_err());
        }
        enter(&mut app);
        assert!(app.session.history().pending().is_none(), "{round}");
    }
    assert_eq!(fs::read(&chunk).unwrap(), original);
}
