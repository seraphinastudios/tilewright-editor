// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The shared field (the default) on the coordinates, a new anchor's id and a
//! portal's typed location: differential runs of real SDL events against
//! the editor's own field, the frames each draws, the frame budget, the
//! road fence, the pause and the portal pick step.
use super::shared_string_field::{
    Edit, Seeded, banner, comparable, field_drawn, operations, preedit, raster_samples, route,
    same_frames, sidebar_hints, text,
};
use super::{acknowledge_ui, input_key, modified_input_key};
use crate::prototype::Application;
use crate::shared_field::SHARED_FIRST_SEMANTIC;
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::{
    event::Event,
    keyboard::{Keycode, Mod},
};
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_ui::Action;
use tilewright_editor_ui::field_input::FieldImplementation;
use tilewright_editor_ui::text_input::{TextCursor, TextInputSnapshot, TextSelection};
use tilewright_editor_ui::tools::{PortalStep, ToolMode};

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

    fn limit(self) -> usize {
        match self {
            Self::Move => 4096,
            Self::Anchor => 128,
            Self::Location => 128 + 3 * 21,
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

/// The sample with the text drafts edited by `implementation`, in a window
/// of `logical` size at `scale`, with `typed` open.
fn drafted(
    implementation: FieldImplementation,
    logical: (u32, u32),
    scale: u32,
    typed: Typed,
) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = super::application_using(implementation);
    let physical = (logical.0 * scale, logical.1 * scale);
    app.ui = WorkspaceUi::new(logical, physical);
    app.view.camera.width = physical.0;
    app.view.camera.height = physical.1;
    app.reflow().unwrap();
    app.control(typed.mode()).unwrap();
    select(&mut app, typed).unwrap();
    app.control(typed.control()).unwrap();
    assert_eq!(open(&app), typed.name(), "{typed:?}");
    assert_eq!(
        app.tools.shared_draft().is_some(),
        implementation == FieldImplementation::Shared
    );
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
    portal_step: Option<(String, PortalStep)>,
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
        portal_step: app
            .tools
            .portal_step()
            .map(|(uid, step)| (uid.to_owned(), step)),
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
    /// The draft's object or tile selected again and its row pressed.
    Reopen,
    /// The pending change cancelled, if there is one.
    Cancel,
}

fn step(random: &mut Seeded, typed: Typed) -> Step {
    const PIECES: [&str; 16] = [
        "a", "é", "青", "e\u{301}", "\u{301}", "😀", "עב", "٣", "0", " ", ",", "\n", "\t",
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
    let texts = typed.texts();
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
    typed: Typed,
    step: &Step,
) -> Result<(), String> {
    match step {
        Step::Event(event) => route(app, input, event, clipboard),
        Step::Fill(delta, paste) => {
            let used = app.tools.draft_text().map_or(0, str::len);
            let room = typed
                .limit()
                .saturating_sub(used)
                .saturating_add_signed(*delta);
            // ASCII digits, ending in a two-byte character when it would
            // straddle the limit.
            let mut fill = "1".repeat(room.saturating_sub(1));
            fill.push(if *delta > 0 { 'é' } else { '2' });
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
            app.control(typed.mode())?;
            select(app, typed)?;
            app.control(typed.control())
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

/// The randomised differential through real SDL events for the
/// coordinates, a new anchor's id and a portal's typed location: from each
/// draft open in four windows, both fields take the same seeded steps
/// (text, fills at the byte limit typed or pasted, compositions, keys with
/// and without Shift and Ctrl, the clipboard, Enter, presses on the draft
/// row and the map, reopening the draft, cancelling the pending change).
/// After every step, presented as the native loop presents it, both give
/// the same result and the same open draft, snapshot, composition, pending
/// change and review lines, mode, selection, portal step, unsupported-text
/// count and banner, status line, sidebar texts and clipboard.
#[test]
#[allow(clippy::too_many_lines)] // The coverage counts are listed inline.
fn both_fields_turn_the_same_random_sdl_events_into_the_same_typed_drafts_and_reviews() {
    let (mut steps, mut refusals, mut open_steps, mut reviews) = (0, 0, 0, 0);
    let mut outcomes = std::collections::BTreeSet::new();
    let mut banners = 0;
    for typed in Typed::ALL {
        for seed in [0x7e57_ed17_u64, 0x0c0f_fee5, 0x5eed_a11c] {
            for (logical, scale) in [
                ((1280, 800), 1),
                ((720, 480), 1),
                ((720, 480), 2),
                ((320, 240), 1),
            ] {
                let mut random = Seeded(seed ^ u64::from(logical.1 * scale));
                let mut runs = FieldImplementation::ALL.map(|implementation| {
                    let (temporary, app) = drafted(implementation, logical, scale, typed);
                    let input = crate::native_input::NativeInput::new(7);
                    (temporary, app, input, String::new())
                });
                for _ in 0..150 {
                    let step = step(&mut random, typed);
                    let case = format!("{typed:?} {seed:x} {logical:?}x{scale} {step:?}");
                    let before = draft_state(&mut runs[0].1);
                    let results = runs.each_mut().map(|(_, app, input, clipboard)| {
                        run_step(app, input, clipboard, typed, &step)
                    });
                    assert_eq!(results[0], results[1], "{case}");
                    refusals += usize::from(results[0].is_err());
                    if matches!(step, Step::Enter)
                        && before.open == typed.name()
                        && let Err(error) = &results[0]
                    {
                        outcomes.insert((typed.name(), error.clone()));
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
                    open_steps += usize::from(state.open == typed.name());
                    if before.pending.is_none() && state.pending.is_some() {
                        reviews += 1;
                        outcomes.insert((typed.name(), "reviewed".into()));
                    }
                    banners += usize::from(state.banner.is_some() && state.open != "none");
                    steps += 1;
                }
            }
        }
    }
    // Every draft was reviewed, and refused for each kind of reason.
    for (typed, outcome) in [
        ("coordinates", "reviewed"),
        ("coordinates", "Invalid x coordinate"),
        ("coordinates", "Invalid y coordinate"),
        ("coordinates", "Enter coordinates as x,y"),
        ("anchor", "reviewed"),
        (
            "anchor",
            "An anchor id is 1 to 128 lowercase letters, digits, '_', '.' or '-', starting with a letter or digit",
        ),
        ("anchor", "This plane already has an anchor with this id"),
        ("anchor", "Type the new anchor's id first"),
        ("location", "reviewed"),
        (
            "location",
            "Type three or four values: layer x y, or plane layer x y",
        ),
        ("location", "The world has no plane with this id"),
        ("location", "The portal already leads there"),
        (
            "location",
            "missing font glyphs: review blocked; draft preserved",
        ),
    ] {
        assert!(
            outcomes.contains(&(typed, outcome.to_owned())),
            "{typed} {outcome}: {outcomes:#?}"
        );
    }
    assert!(
        steps >= 5_400 && open_steps > 2_000 && refusals > 300 && reviews > 50 && banners > 0,
        "{steps} {open_steps} {refusals} {reviews} {banners}"
    );
}

/// Each draft's row in both fields, at the three window sizes: the same
/// row and reserved row, origin, clip, size, quad coverage, toolbar count
/// and banner, hint and sidebar, and the same pixels but the caret and
/// underline.
#[test]
#[allow(clippy::too_many_lines)] // The cases are listed inline.
fn the_shared_coordinate_anchor_and_location_fields_cover_and_paint_what_the_editors_own_do() {
    let cases: [(&str, Typed, Edit); 19] = [
        ("opened", Typed::Move, |_| {}),
        ("typed", Typed::Move, |app| {
            app.tools.select_all().unwrap();
            app.tools.input("-12, 40").unwrap();
        }),
        ("long, scrolled to the end", Typed::Move, |app| {
            app.tools.input(&format!(",{}", "4".repeat(60))).unwrap();
        }),
        ("mixed direction", Typed::Move, |app| {
            app.tools.input(" עברית ٣,٤").unwrap();
        }),
        ("preedit", Typed::Move, |app| {
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(1)), revision)
                .unwrap();
            app.tools.update_preedit("にほん", 2, 0).unwrap();
        }),
        ("selection", Typed::Move, |app| {
            app.tools.select_all().unwrap();
        }),
        ("unsupported glyph", Typed::Move, |app| {
            app.tools.input("\u{e000}").unwrap();
        }),
        ("empty", Typed::Anchor, |_| {}),
        ("typed", Typed::Anchor, |app| {
            app.tools.input("camp").unwrap();
        }),
        ("long, scrolled to the end", Typed::Anchor, |app| {
            app.tools.input(&"a".repeat(128)).unwrap();
        }),
        ("preedit", Typed::Anchor, |app| {
            app.tools.input("cap").unwrap();
            let revision = app.tools.input_revision().unwrap();
            app.tools
                .set_selection(TextSelection::caret(TextCursor::new(2)), revision)
                .unwrap();
            app.tools.update_preedit("にほん", 2, 0).unwrap();
        }),
        ("joining marks", Typed::Anchor, |app| {
            app.tools.input("e\u{301} 👨\u{200d}👩\u{200d}👧").unwrap();
        }),
        ("unsupported glyph", Typed::Anchor, |app| {
            app.tools.input("odd\u{e000}id").unwrap();
        }),
        ("empty", Typed::Location, |_| {}),
        ("typed", Typed::Location, |app| {
            app.tools.input("main 0, 5, 5").unwrap();
        }),
        ("long, scrolled to the end", Typed::Location, |app| {
            app.tools
                .input("main 0 9007199254740991 -9007199254740991 0123")
                .unwrap();
        }),
        ("mixed direction", Typed::Location, |app| {
            app.tools.input("עברית 0 5 5").unwrap();
        }),
        ("selection", Typed::Location, |app| {
            app.tools.input("0 5 5").unwrap();
            app.tools.select_all().unwrap();
        }),
        ("unsupported glyph", Typed::Location, |app| {
            app.tools.input("0 \u{e000} 5").unwrap();
        }),
    ];
    for (logical, scale) in [((1280, 800), 1), ((720, 480), 1), ((720, 480), 2)] {
        for (name, typed, edit) in cases {
            let [(_own, mut own), (_shared, mut shared)] =
                FieldImplementation::ALL.map(|implementation| {
                    let (temporary, mut app) = drafted(implementation, logical, scale, typed);
                    edit(&mut app);
                    acknowledge_ui(&mut app);
                    (temporary, app)
                });
            let case = format!("{typed:?} {name} at {logical:?}x{scale}");
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
                    typed.hint()
                };
                assert!(
                    hints[0].iter().any(|text| text == hint),
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

/// Each draft's field is compiled first and the editor's frame fitted to
/// leave its quads and raster samples free, as for the other drafts; below
/// the field alone editing pauses with the draft kept.
#[test]
fn a_typed_draft_frame_leaves_room_for_the_shared_field_and_pauses_without_it() {
    for typed in Typed::ALL {
        let (_temporary, mut app) = drafted(FieldImplementation::Shared, (960, 640), 1, typed);
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

/// The road fence and the pause refuse these drafts the same way with the
/// switch on: while a road is drawn none opens, with the road's message;
/// while editing is paused none opens and an open one is not reviewed, and
/// its text is kept.
#[test]
fn the_road_fence_and_the_pause_refuse_typed_drafts_with_either_field() {
    use crate::prototype::ROAD_IN_PROGRESS;
    let mut paused = Vec::new();
    for implementation in FieldImplementation::ALL {
        for typed in Typed::ALL {
            let (_temporary, mut app) = super::road::road_application(1, |_| {});
            if implementation == FieldImplementation::Shared {
                app.share_string_field().unwrap();
            }
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
            let (_temporary, mut app) = drafted(implementation, (960, 640), 1, typed);
            app.tools.select_all().unwrap();
            app.tools.input(typed.texts()[0]).unwrap();
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
            assert_eq!(review, "rendering blocked editing: test", "{typed:?}");
            acknowledge_ui(&mut app);
            let enter = route(
                &mut app,
                &mut input,
                &input_key(7, Keycode::Return, false),
                &mut clipboard,
            );
            assert_eq!(enter, Err(review.clone()), "{typed:?}");
            assert_eq!(app.tools.draft_text(), Some(typed.texts()[0]));
            assert!(app.session.history().pending().is_none());
            let reopen = app.control(typed.control()).unwrap_err();
            assert_eq!(app.tools.draft_text(), Some(typed.texts()[0]), "kept");
            assert_eq!(
                app.tools.shared_draft().is_some(),
                implementation == FieldImplementation::Shared
            );
            paused.push((typed, review, reopen));
        }
    }
    assert_eq!(paused[..3], paused[3..], "{paused:#?}");
}

/// Typing a location while a portal pick step is open ends the step and
/// its prompt with either field, as every draft does: the tile click that
/// follows does not set the portal.
#[test]
fn opening_a_typed_location_ends_the_pick_step_with_either_field() {
    use super::anchors::{click_tile, has};
    use super::zone_manage::press;
    use super::zones::{sidebar_texts, zone_application};
    let mut outcomes = Vec::new();
    for implementation in FieldImplementation::ALL {
        let (_temporary, mut app) = zone_application(|_| {});
        if implementation == FieldImplementation::Shared {
            app.share_string_field().unwrap();
        }
        super::anchors::click_object(&mut app, "gate_west");
        press(&mut app, &Control::PortalPick);
        assert!(app.tools.portal_picking());
        press(&mut app, &Control::PortalLocation);
        assert_eq!(app.tools.portal_draft(), Some("gate_west"));
        assert_eq!(
            app.tools.shared_draft().is_some(),
            implementation == FieldImplementation::Shared
        );
        assert!(!app.tools.portal_picking());
        assert_eq!(app.tools.portal_step(), None);
        let texts = sidebar_texts(&mut app);
        assert!(!has(&texts, "Click a tile in the view (Esc ends)"));
        assert!(!app.status_label().starts_with("Click a tile in the view"));
        click_tile(&mut app, 4, 3);
        assert!(app.tools.portal_draft().is_none(), "a tile click ends it");
        assert!(app.session.history().pending().is_none());
        outcomes.push((texts, app.status_label(), app.session.selection().cloned()));
    }
    assert_eq!(outcomes[0], outcomes[1]);
}

/// The hand test's step 15, through SDL events in both fields: the same
/// hints, refusals and reviews.
#[test]
fn the_hand_test_steps_give_the_same_hints_refusals_and_reviews_with_either_field() {
    use super::zone_manage::{enter, press, review_has, type_text};
    use super::zones::{key, sidebar_texts, zone_application};
    let mut transcripts = Vec::new();
    for implementation in FieldImplementation::ALL {
        let (_temporary, mut app) = zone_application(|_| {});
        if implementation == FieldImplementation::Shared {
            app.share_string_field().unwrap();
        }
        let mut transcript = Vec::new();
        let mut attempt = |app: &mut Application, text: &str| {
            app.tools.select_all().unwrap();
            type_text(app, text);
            acknowledge_ui(app);
            let result = super::zones::route(app, &key(Keycode::Return));
            let pending = app.session.history().pending().is_some();
            transcript.push((text.to_owned(), result.clone(), pending));
            result
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
        let moved = app.review_lines(900);
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
        let lines = app.review_lines(900);
        enter(&mut app);
        assert!(app.session.history().pending().is_none(), "accepted");
        transcripts.push((transcript, moved, lines));
    }
    assert_eq!(transcripts[0], transcripts[1]);
}
