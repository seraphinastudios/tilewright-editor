// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The shared field (the default) on the inspector's boolean, integer, number and
//! reference fields, in a copy of the sample that grants them (the hand
//! test's lines): differential runs of real SDL events against the editor's
//! own field, the frames each draws, the frame budget, the road fence, the
//! pause and the hand test's steps.
use super::shared_string_field::{
    Edit, Seeded, banner, comparable, field_drawn, operations, preedit, raster_samples, route,
    same_frames, sidebar_hints, text,
};
use super::{acknowledge_ui, application_with_source_using, input_key, modified_input_key};
use crate::prototype::Application;
use crate::shared_field::SHARED_FIRST_SEMANTIC;
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::{
    event::Event,
    keyboard::{Keycode, Mod},
};
use std::{fs, path::Path};
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::field_input::FieldImplementation;
use tilewright_editor_ui::text_input::{TextCursor, TextInputSnapshot, TextSelection};
use tilewright_editor_ui::tools::ToolMode;

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

/// The inspector's field drafts are 4,096 bytes at most.
const LIMIT: usize = tilewright_editor_ui::inspector::MAX_INPUT_BYTES;

fn select(app: &mut Application) -> Result<(), String> {
    app.action(Action::Select {
        generation: app.view.frame_revision,
        key: SelectionKey::Object {
            uid: "marker_1".into(),
        },
    })
}

/// The typed copy with the text drafts edited by `implementation`, in a
/// window of `logical` size at `scale`, with `scalar` open.
fn drafted(
    implementation: FieldImplementation,
    logical: (u32, u32),
    scale: u32,
    scalar: Scalar,
) -> (tempfile::TempDir, Application) {
    drafted_with(implementation, logical, scale, scalar, false)
}

/// [`drafted`], with `marker_1` holding its stored values when `stored`.
fn drafted_with(
    implementation: FieldImplementation,
    logical: (u32, u32),
    scale: u32,
    scalar: Scalar,
    stored: bool,
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) =
        application_with_source_using(implementation, |root| typed_world_with(root, stored));
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
    assert_eq!(
        app.tools.shared_draft().is_some(),
        implementation == FieldImplementation::Shared
    );
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

/// Everything these drafts' events can change that a person sees or that
/// later events depend on.
#[derive(Debug, PartialEq)]
struct DraftState {
    open: &'static str,
    composing: bool,
    snapshot: Option<TextInputSnapshot>,
    /// The pending change's operations, and its review lines.
    pending: Option<(String, Vec<String>)>,
    mode: ToolMode,
    selection: Option<SelectionKey>,
    unsupported: usize,
    banner: Option<String>,
    status: String,
    /// The sidebar's texts but the draft row's own: the hint, Review and
    /// Discard, the inspector rows and the rest.
    sidebar: Vec<String>,
}

fn draft_state(app: &mut Application) -> DraftState {
    let pending = app
        .session
        .history()
        .pending()
        .map(|pending| operations(pending.sealed()));
    let pending = pending.map(|operations| (operations, app.review_lines(900)));
    DraftState {
        open: open(app),
        composing: app.tools.is_composing(),
        snapshot: comparable(app.tools.draft_snapshot()),
        pending,
        mode: app.tools.mode(),
        selection: app.session.selection().cloned(),
        unsupported: app.unsupported_runs,
        banner: banner(app),
        status: app.status_label(),
        sidebar: sidebar_hints(app),
    }
}

/// One step of the randomised run.
#[derive(Clone, Debug)]
enum Step {
    /// A key, text or composition event through the editor's routing.
    Event(Event),
    /// Text filling the draft to its byte limit plus `delta` bytes, typed
    /// or pasted, its last character straddling the limit when it does not
    /// fit.
    Fill(isize, bool),
    /// A Return press after its release, as a person presses Enter.
    Enter,
    /// A left press and release on the draft's row, `fraction` of its
    /// width in (nothing while no draft is open).
    ClickDraft(f32),
    /// A left press and release on the map, at `fraction` of its width and
    /// height.
    ClickMap(f32, f32),
    /// The marker selected again and the field's row pressed.
    Reopen,
    /// The pending change cancelled, if there is one.
    Cancel,
}

fn step(random: &mut Seeded, scalar: Scalar) -> Step {
    const PIECES: [&str; 16] = [
        "a", "é", "青", "e\u{301}", "\u{301}", "😀", "עב", "٣", "0", " ", ".", "\n", "\t",
        "\u{2028}", "\u{e000}", "x\u{1b}y",
    ];
    const COMPOSED: [(&str, i32, i32); 5] = [
        ("に", 1, 0),
        ("にほ", 2, 0),
        ("5", 1, 0),
        ("\u{2029}", 1, 0),
        ("", 0, 0),
    ];
    const KEYS: [Keycode; 16] = [
        Keycode::Home,
        Keycode::End,
        Keycode::Left,
        Keycode::Right,
        Keycode::Backspace,
        Keycode::Delete,
        Keycode::Escape,
        Keycode::N,
        Keycode::K,
        Keycode::O,
        Keycode::M,
        Keycode::S,
        Keycode::Slash,
        Keycode::Up,
        Keycode::Down,
        Keycode::Tab,
    ];
    const SHIFTED: [Keycode; 4] = [Keycode::Left, Keycode::Right, Keycode::Home, Keycode::End];
    const COMMANDS: [Keycode; 4] = [Keycode::A, Keycode::C, Keycode::X, Keycode::V];
    let texts = scalar.texts();
    #[allow(clippy::cast_precision_loss)]
    match random.below(48) {
        0..=7 => Step::Event(text(texts[random.below(texts.len())])),
        8..=12 => Step::Event(text(PIECES[random.below(PIECES.len())])),
        13 | 14 => Step::Fill(
            isize::try_from(random.below(7)).unwrap() - 3,
            random.below(2) == 0,
        ),
        15..=17 => {
            let (value, start, length) = COMPOSED[random.below(COMPOSED.len())];
            Step::Event(preedit(value, start, length))
        }
        18..=25 => Step::Event(input_key(7, KEYS[random.below(KEYS.len())], false)),
        26 | 27 => Step::Event(modified_input_key(
            SHIFTED[random.below(SHIFTED.len())],
            false,
            Mod::LSHIFTMOD,
        )),
        28..=31 => Step::Event(modified_input_key(
            COMMANDS[random.below(COMMANDS.len())],
            false,
            Mod::LCTRLMOD,
        )),
        32..=36 => Step::Enter,
        37..=39 => Step::ClickDraft(random.below(100) as f32 / 100.0),
        40 => Step::ClickMap(
            random.below(100) as f32 / 100.0,
            random.below(100) as f32 / 100.0,
        ),
        41..=45 => Step::Reopen,
        _ => Step::Cancel,
    }
}

/// A left press and release at logical `at`, through the native loop's
/// routing.
fn click_at(app: &mut Application, at: [f32; 2]) -> Result<(), String> {
    let logical = app.ui.logical_size();
    let physical = app.ui.size;
    let mut result = Ok(());
    for down in [true, false] {
        let event = super::zones::button(down, at);
        if let Some(outcome) =
            crate::prototype::native_input_event(app, &event, logical, physical, false, false)
        {
            if down {
                result = outcome;
            } else {
                outcome?;
            }
        }
    }
    result
}

fn run_step(
    app: &mut Application,
    input: &mut crate::native_input::NativeInput,
    clipboard: &mut String,
    scalar: Scalar,
    step: &Step,
) -> Result<(), String> {
    match step {
        Step::Event(event) => route(app, input, event, clipboard),
        Step::Fill(delta, paste) => {
            let used = app.tools.draft_text().map_or(0, str::len);
            let room = LIMIT.saturating_sub(used).saturating_add_signed(*delta);
            // ASCII digits, ending in a two-byte character when it would
            // straddle the limit.
            let mut fill = "0".repeat(room.saturating_sub(1));
            fill.push(if *delta > 0 { 'é' } else { '1' });
            if *paste {
                fill.clone_into(clipboard);
                route(
                    app,
                    input,
                    &modified_input_key(Keycode::V, false, Mod::LCTRLMOD),
                    clipboard,
                )
            } else {
                route(app, input, &text(&fill), clipboard)
            }
        }
        Step::Enter => {
            route(app, input, &input_key(7, Keycode::Return, true), clipboard)?;
            route(app, input, &input_key(7, Keycode::Return, false), clipboard)
        }
        Step::ClickDraft(fraction) => {
            app.refresh_ui();
            let row = app
                .ui
                .widgets
                .iter()
                .find(|widget| widget.control == Some(Control::EditDraft))
                .map(|widget| widget.layout);
            #[allow(clippy::cast_precision_loss)]
            row.map_or(Ok(()), |row| {
                click_at(
                    app,
                    [
                        row.x as f32 + 1.0 + row.width.saturating_sub(2) as f32 * fraction,
                        row.y as f32 + row.height as f32 / 2.0,
                    ],
                )
            })
        }
        Step::ClickMap(across, down) => {
            let (_, height) = app.ui.logical_size();
            #[allow(clippy::cast_precision_loss)]
            let at = [
                app.ui.sidebar_x() as f32 * across,
                (height as f32 - 60.0) * down + 50.0,
            ];
            click_at(app, at)
        }
        Step::Reopen => {
            app.control(Control::Select)?;
            select(app)?;
            app.control(scalar.control())
        }
        Step::Cancel => {
            if app.session.history().pending().is_some() {
                app.action(Action::Cancel)
            } else {
                Ok(())
            }
        }
    }
}

/// The randomised differential through real SDL events for the inspector's
/// boolean, integer, number and reference fields: from each field open in
/// four windows, both fields take the same seeded steps (text, fills at
/// the byte limit typed or pasted, compositions, keys with and without
/// Shift and Ctrl, the clipboard, Enter, presses on the draft row and the
/// map, reopening the field, cancelling the pending change). After every
/// step, presented as the native loop presents it, both give the same
/// result and the same open draft, snapshot, composition, pending change
/// and review lines, mode, selection, unsupported-text count and banner,
/// status line, sidebar texts and clipboard.
#[test]
#[allow(clippy::too_many_lines)] // The coverage counts are listed inline.
fn both_fields_turn_the_same_random_sdl_events_into_the_same_typed_field_drafts_and_reviews() {
    let (mut steps, mut refusals, mut open_steps, mut reviews) = (0, 0, 0, 0);
    let mut outcomes = std::collections::BTreeSet::new();
    let mut banners = 0;
    for scalar in Scalar::ALL {
        for seed in [0x7e57_ed17_u64, 0x0c0f_fee5, 0x5eed_a11c] {
            for (logical, scale) in [
                ((1280, 800), 1),
                ((720, 480), 1),
                ((720, 480), 2),
                ((320, 240), 1),
            ] {
                let mut random = Seeded(seed ^ u64::from(logical.1 * scale));
                let mut runs = FieldImplementation::ALL.map(|implementation| {
                    let (temporary, app) = drafted(implementation, logical, scale, scalar);
                    let input = crate::native_input::NativeInput::new(7);
                    (temporary, app, input, String::new())
                });
                for _ in 0..150 {
                    let step = step(&mut random, scalar);
                    let case = format!("{scalar:?} {seed:x} {logical:?}x{scale} {step:?}");
                    let before = draft_state(&mut runs[0].1);
                    let results = runs.each_mut().map(|(_, app, input, clipboard)| {
                        run_step(app, input, clipboard, scalar, &step)
                    });
                    assert_eq!(results[0], results[1], "{case}");
                    refusals += usize::from(results[0].is_err());
                    if matches!(step, Step::Enter)
                        && before.open == "field"
                        && let Err(error) = &results[0]
                    {
                        outcomes.insert((scalar.name(), error.clone()));
                    }
                    // Presented, as the native loop presents after each event.
                    for run in &mut runs {
                        acknowledge_ui(&mut run.1);
                    }
                    let states = runs.each_mut().map(|run| draft_state(&mut run.1));
                    assert_eq!(states[0], states[1], "{case}");
                    assert_eq!(runs[0].3, runs[1].3, "{case}: the clipboard holds the same");
                    // The shared run really edits every open draft with the
                    // shared field, the own run never.
                    assert_eq!(
                        runs[1].1.tools.shared_draft().is_some(),
                        states[1].open != "none",
                        "{case}"
                    );
                    assert!(runs[0].1.tools.shared_draft().is_none());
                    let state = &states[0];
                    open_steps += usize::from(state.open == "field");
                    if before.pending.is_none() && state.pending.is_some() {
                        reviews += 1;
                        outcomes.insert((scalar.name(), "reviewed".into()));
                    }
                    banners += usize::from(state.banner.is_some() && state.open != "none");
                    steps += 1;
                }
            }
        }
    }
    // Every field was reviewed, and refused for each kind of reason in its
    // own type's words.
    for (scalar, outcome) in [
        ("flag", "reviewed"),
        ("flag", "Enter true or false"),
        ("count", "reviewed"),
        ("count", "Enter an integer"),
        ("count", "Integer is outside schema range"),
        ("weight", "reviewed"),
        ("weight", "Enter a finite number"),
        ("weight", "Number is non-finite or outside schema range"),
        ("kind", "reviewed"),
        (
            "kind",
            "Reference does not name an authoritative definition",
        ),
        (
            "kind",
            "missing font glyphs: review blocked; draft preserved",
        ),
    ] {
        assert!(
            outcomes.contains(&(scalar, outcome.to_owned())),
            "{scalar} {outcome}: {outcomes:#?}"
        );
    }
    assert!(
        steps >= 7_200 && open_steps > 3_000 && refusals > 400 && reviews > 80 && banners > 0,
        "{steps} {open_steps} {refusals} {reviews} {banners}"
    );
}

/// Each field's row in both fields, at the three window sizes: the same
/// row and reserved row, origin, clip, size, quad coverage, toolbar count
/// and banner, hint and sidebar, and the same pixels but the caret and
/// underline.
#[test]
#[allow(clippy::too_many_lines)] // The cases are listed inline.
fn the_shared_typed_inspector_fields_cover_and_paint_what_the_editors_own_do() {
    let cases: [(&str, Scalar, Edit); 20] = [
        ("opened", Scalar::Flag, |_| {}),
        ("typed", Scalar::Flag, |app| {
            app.tools.input("false").unwrap();
        }),
        ("selection", Scalar::Flag, |app| {
            app.tools.input("true").unwrap();
            app.tools.select_all().unwrap();
        }),
        ("preedit", Scalar::Flag, |app| {
            app.tools.input("tre").unwrap();
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(2)), revision)
                .unwrap();
            app.tools.update_preedit("にほん", 2, 0).unwrap();
        }),
        ("unsupported glyph", Scalar::Flag, |app| {
            app.tools.input("tr\u{e000}ue").unwrap();
        }),
        ("opened", Scalar::Count, |_| {}),
        ("typed", Scalar::Count, |app| {
            app.tools.input("-2").unwrap();
        }),
        ("long, scrolled to the end", Scalar::Count, |app| {
            app.tools.input(&"7".repeat(90)).unwrap();
        }),
        ("mixed direction", Scalar::Count, |app| {
            app.tools.input("עב ٣ 3").unwrap();
        }),
        ("unsupported glyph", Scalar::Count, |app| {
            app.tools.input("\u{e000}").unwrap();
        }),
        ("opened", Scalar::Weight, |_| {}),
        ("typed", Scalar::Weight, |app| {
            app.tools.input("1.5").unwrap();
        }),
        ("long, scrolled to the end", Scalar::Weight, |app| {
            app.tools
                .input(&format!("0.{}", "1234567890".repeat(9)))
                .unwrap();
        }),
        ("preedit", Scalar::Weight, |app| {
            app.tools.input("1.5").unwrap();
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(1)), revision)
                .unwrap();
            app.tools.update_preedit("にほん", 2, 0).unwrap();
        }),
        ("selection", Scalar::Weight, |app| {
            app.tools.input("-1").unwrap();
            app.tools.select_all().unwrap();
        }),
        ("opened", Scalar::Kind, |_| {}),
        ("typed", Scalar::Kind, |app| {
            app.tools.input("marker").unwrap();
        }),
        ("long, scrolled to the end", Scalar::Kind, |app| {
            app.tools.input(&"marker_".repeat(14)).unwrap();
        }),
        ("joining marks", Scalar::Kind, |app| {
            app.tools.input("e\u{301} 👨\u{200d}👩\u{200d}👧").unwrap();
        }),
        ("unsupported glyph", Scalar::Kind, |app| {
            app.tools.input("odd\u{e000}kind").unwrap();
        }),
    ];
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        // Each field opened from a stored value too, which shows its type
        // in the row below instead of the absent field's prompt.
        let opened = cases
            .iter()
            .filter(|(name, ..)| *name == "opened")
            .map(|(_, scalar, edit)| ("opened, stored", *scalar, *edit));
        for (name, scalar, edit) in cases.into_iter().chain(opened) {
            let stored = name == "opened, stored";
            let [(_own, mut own), (_shared, mut shared)] =
                FieldImplementation::ALL.map(|implementation| {
                    let (temporary, mut app) =
                        drafted_with(implementation, logical, scale, scalar, stored);
                    edit(&mut app);
                    acknowledge_ui(&mut app);
                    (temporary, app)
                });
            let case = format!("{scalar:?} {name} at {logical:?}x{scale}");
            let banner = same_frames(
                &mut own,
                &mut shared,
                &case,
                name == "long, scrolled to the end",
            );
            let hints = [&mut own, &mut shared].map(sidebar_hints);
            assert_eq!(hints[0], hints[1], "{case}");
            if logical.0 >= 1280 {
                let hint = if name == "unsupported glyph" {
                    "Unsupported text; cannot review"
                } else {
                    "Ctrl+A selects / Enter reviews"
                };
                assert!(
                    hints[0].iter().any(|text| text == hint),
                    "{case}: {hints:?}"
                );
                let description = if stored {
                    scalar.description()
                } else {
                    "Optional: enter a value to add"
                };
                assert!(
                    hints[0].iter().any(|text| text == description),
                    "{case}: {hints:?}"
                );
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

/// Each field's draft is compiled first and the editor's frame fitted to
/// leave its quads and raster samples free, as for the other drafts; below
/// the field alone editing pauses with the draft kept.
#[test]
fn a_typed_field_frame_leaves_room_for_the_shared_field_and_pauses_without_it() {
    for scalar in Scalar::ALL {
        let (_temporary, mut app) = drafted(FieldImplementation::Shared, (960, 640), 1, scalar);
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

/// The road fence and the pause refuse these fields the same way with the
/// switch on: while a road is drawn none opens, with the road's message;
/// while editing is paused none opens and an open one is not reviewed, and
/// its text is kept.
#[test]
fn the_road_fence_and_the_pause_refuse_typed_fields_with_either_field() {
    use crate::prototype::ROAD_IN_PROGRESS;
    let mut paused = Vec::new();
    for implementation in FieldImplementation::ALL {
        for scalar in Scalar::ALL {
            let (_temporary, mut app) = super::road::road_application(1, typed_world);
            if implementation == FieldImplementation::Shared {
                app.share_string_field().unwrap();
            }
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
            let (_temporary, mut app) = drafted(implementation, (960, 640), 1, scalar);
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
            assert_eq!(app.tools.draft_text(), Some(scalar.texts()[0]), "kept");
            assert_eq!(
                app.tools.shared_draft().is_some(),
                implementation == FieldImplementation::Shared
            );
            paused.push((scalar, review, reopen));
        }
    }
    assert_eq!(paused[..4], paused[4..], "{paused:#?}");
}

/// The hand test's step 12 on the typed copy, through SDL events in both
/// fields: each row opens from the inspector with the same description and
/// hint, refuses in its type's words, reviews the same `SetField` with the
/// same review lines, is accepted, and Undo brings the absent value back.
#[test]
#[allow(clippy::too_many_lines)] // The hand test's steps are listed inline.
fn the_hand_test_typed_fields_give_the_same_hints_refusals_and_reviews_with_either_field() {
    use super::zone_manage::{all_texts, enter, press, type_text};
    use super::zones::{key, zone_application};
    let mut transcripts = Vec::new();
    for implementation in FieldImplementation::ALL {
        let (temporary, mut app) = zone_application(typed_world);
        if implementation == FieldImplementation::Shared {
            app.share_string_field().unwrap();
        }
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
        let mut transcript = vec![texts];
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
            assert_eq!(
                app.tools.shared_draft().is_some(),
                implementation == FieldImplementation::Shared
            );
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
            let refusal = refused;
            enter(&mut app);
            assert!(app.session.history().pending().is_none(), "accepted");
            let saved = String::from_utf8(fs::read(&chunk).unwrap()).unwrap();
            assert!(
                saved.contains(&format!("\"{}\": {stored}", scalar.name())),
                "{scalar:?}"
            );
            transcript.push(vec![format!("{refusal:?}"), format!("{review:?}")]);
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
        transcripts.push(transcript);
    }
    assert_eq!(transcripts[0], transcripts[1]);
}
