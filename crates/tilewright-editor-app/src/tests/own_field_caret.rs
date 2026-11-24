// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The editor's own field: every arrow press that can move the caret moves
//! it on screen. Two caret stops at one drawn place (a float apart), or a
//! marker's guard, are not stops of their own (`workspace_render::step_caret`).
use super::{acknowledge_ui, application_with_source_using, input_key};
use crate::prototype::Application;
use crate::workspace_render::{model_cursor, renderer_cursor, step_caret, text_size};
use crate::workspace_ui::{Control, WorkspaceUi};
use sdl3::keyboard::Keycode;
use std::fs;
use tilewright_editor_ui::field_input::FieldImplementation;
use tilewright_editor_ui::text_input::{
    TextAffinity, TextCursor, TextInput, TextInputSnapshot, TextSelection,
};
use tilewright_render_ui::{TextSystem, UnicodeLayout, VisualMotion};

/// Two drawn carets at the same place: within 1/64 px on each axis.
fn same_place(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() <= 1.0 / 64.0 && (a[1] - b[1]).abs() <= 1.0 / 64.0
}

/// The own field's draft for `stored` as `marker_1`'s label, caret at
/// committed `start` (affinity After).
fn draft(stored: &str, start: usize) -> (tempfile::TempDir, Application) {
    let stored = stored.to_owned();
    let (temporary, mut app) =
        application_with_source_using(FieldImplementation::Editor, move |world| {
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
    app.ui = WorkspaceUi::new((960, 640), (320, 240));
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    let revision = app.tools.input_revision().unwrap();
    app.tools
        .set_selection(TextSelection::caret(TextCursor::new(start)), revision)
        .unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}

/// Where the field draws its caret now, from the geometry it presented.
fn drawn(app: &mut Application) -> [f32; 2] {
    acknowledge_ui(app);
    let geometry = app.displayed_text.as_ref().unwrap();
    let caret = geometry
        .layout
        .caret(renderer_cursor(geometry.snapshot.display_selection.focus))
        .unwrap();
    [caret.x, caret.y]
}

/// Presses `key` through the real SDL routing.
fn press(app: &mut Application, input: &mut crate::native_input::NativeInput, key: Keycode) {
    input.begin_batch(false, app.tools.input_revision().map(|r| r.draft));
    match crate::prototype::native_text_event(app, &input_key(7, key, false), input) {
        Some(result) => result.unwrap(),
        None => crate::prototype::native_key(app, key).unwrap(),
    }
}

/// The measured stalls: from `start`, after `before` presses, `key` must
/// move the drawn caret.
#[test]
fn an_arrow_that_used_to_do_nothing_now_moves_the_caret() {
    let cases: [(&str, usize, &[Keycode], Keycode); 5] = [
        // Two stops a float apart at a change of direction (x 4.6800003 and
        // 4.68): Home, Right, Right.
        (
            "\u{5d0}abb\u{5d0} ",
            0,
            &[Keycode::Home, Keycode::Right],
            Keycode::Right,
        ),
        // U+0600 (Prepend) before a stored break.
        (
            "\u{600}\u{2028}\u{5d0}\u{93f}a\u{10ffff}",
            2,
            &[],
            Keycode::Left,
        ),
        // An emoji ZWJ pair beside spaces and right-to-left text.
        (
            "\u{1f468}\u{200d}\u{1f469}  \u{5d1}",
            11,
            &[],
            Keycode::Left,
        ),
        // ... and beside a break.
        (
            "\u{5d1}\u{2028}\u{1f468}\u{200d}\u{1f469}\u{10ffff}\u{5d1}",
            16,
            &[],
            Keycode::Right,
        ),
        // Line breaks only.
        ("\r\n\r\n\u{2029} \u{2029}\r\n", 11, &[], Keycode::Right),
    ];
    let mut failures = Vec::new();
    for (stored, start, before, key) in cases {
        let (_temporary, mut app) = draft(stored, start);
        let mut input = crate::native_input::NativeInput::new(7);
        for key in before {
            press(&mut app, &mut input, *key);
        }
        let (focus, at) = (
            app.tools.draft_snapshot().unwrap().selection.focus,
            drawn(&mut app),
        );
        press(&mut app, &mut input, key);
        let after = app.tools.draft_snapshot().unwrap().selection.focus;
        eprintln!("{stored:?} {key:?} from {focus:?} to {after:?}");
        if same_place(at, drawn(&mut app)) {
            failures.push(format!(
                "{stored:?} {key:?} from {focus:?} to {after:?} left the drawn caret in place"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A small deterministic generator (xorshift64*), so the sweep needs no
/// dependency and every seed replays exactly.
struct Rng(u64);
impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33;
        usize::try_from(value).unwrap() % bound
    }
}

/// Mixed-direction text, stored breaks and controls, combining and prepended
/// marks, emoji sequences and spaces.
const PIECES: &[&str] = &[
    "a",
    "b",
    "Trail",
    " ",
    "  ",
    "1",
    "23",
    "\u{5d0}",
    "\u{5d1}",
    "\u{5e2}\u{5d1}",
    "\u{627}",
    "\n",
    "\r\n",
    "\u{2028}",
    "\u{2029}",
    "\t",
    "\u{1}",
    "\u{600}",
    "\u{301}",
    "\u{93f}",
    "\u{1f468}\u{200d}\u{1f469}",
    "\u{10ffff}",
    ".",
];

/// The drawn caret of committed `cursor`, as the field draws it once the
/// selection is set: its display focus in `layout`.
fn drawn_at(input: &TextInput, layout: &UnicodeLayout, cursor: TextCursor) -> [f32; 2] {
    let mut input = input.clone();
    input
        .set_selection(TextSelection::caret(cursor), input.revision())
        .unwrap();
    let caret = layout
        .caret(renderer_cursor(input.snapshot().display_selection.focus))
        .unwrap();
    [caret.x, caret.y]
}

/// The press before this rule: one raw layout step, mapped to committed text.
fn single_step(
    snapshot: &TextInputSnapshot,
    layout: &UnicodeLayout,
    motion: VisualMotion,
) -> TextCursor {
    let next = layout.move_visual(renderer_cursor(snapshot.display_selection.focus), motion);
    snapshot.committed_cursor(model_cursor(next)).unwrap()
}

/// What the sweep saw.
#[derive(Default)]
struct Tally {
    /// Presses where the single step left the caret in place on screen and
    /// the press now moves it.
    moved_on: usize,
    /// Presses to another committed position that the layout draws at the
    /// same place. The shared field's `step_caret` stops there too: at a
    /// direction boundary two different positions can share one place.
    same_place_stops: usize,
    failures: Vec<String>,
}

/// Checks every Left, Right, Home and End press from every committed caret
/// position of `value`, both affinities.
fn sweep_value(system: &mut TextSystem, value: &str, tally: &mut Tally) {
    let input = TextInput::new(value.to_owned()).unwrap();
    let display = input.snapshot().display_text;
    let layout = system.layout(&display, text_size(30), None).unwrap();
    let mut cursors = Vec::new();
    for byte in (0..=value.len()).filter(|byte| value.is_char_boundary(*byte)) {
        for affinity in [TextAffinity::Before, TextAffinity::After] {
            let cursor = TextCursor { byte, affinity };
            if input
                .clone()
                .set_selection(TextSelection::caret(cursor), input.revision())
                .is_ok()
            {
                cursors.push(cursor);
            }
        }
    }
    let places: Vec<_> = cursors
        .iter()
        .map(|cursor| drawn_at(&input, &layout, *cursor))
        .collect();
    let leftmost = places.iter().map(|place| place[0]).fold(f32::MAX, f32::min);
    let rightmost = places.iter().map(|place| place[0]).fold(f32::MIN, f32::max);
    for (cursor, at) in cursors.iter().zip(&places) {
        let mut input = input.clone();
        input
            .set_selection(TextSelection::caret(*cursor), input.revision())
            .unwrap();
        let snapshot = input.snapshot();
        // Home and End take the single step, as before.
        for motion in [VisualMotion::Home, VisualMotion::End] {
            let pressed = snapshot
                .committed_cursor(model_cursor(
                    step_caret(&snapshot, &layout, motion).unwrap(),
                ))
                .unwrap();
            let old = single_step(&snapshot, &layout, motion);
            if pressed != old {
                tally.failures.push(format!(
                    "{value:?} {motion:?} from {cursor:?}: went to {old:?}, now {pressed:?}"
                ));
            }
        }
        for motion in [VisualMotion::Left, VisualMotion::Right] {
            let pressed = snapshot
                .committed_cursor(model_cursor(
                    step_caret(&snapshot, &layout, motion).unwrap(),
                ))
                .unwrap();
            let old = single_step(&snapshot, &layout, motion);
            let context = format!("{value:?} {motion:?} from {cursor:?}");
            let moved = !same_place(*at, drawn_at(&input, &layout, pressed));
            if !same_place(*at, drawn_at(&input, &layout, old)) {
                // The single step moved the caret on screen: nothing changes.
                if pressed != old {
                    tally
                        .failures
                        .push(format!("{context}: moved to {old:?}, now {pressed:?}"));
                }
            } else if pressed == *cursor {
                // Nothing moves: the caret is at the end in that direction.
                let end = if motion == VisualMotion::Left {
                    leftmost
                } else {
                    rightmost
                };
                if (at[0] - end).abs() > 1.0 / 64.0 {
                    tally.failures.push(format!(
                        "{context}: stays at x {} (ends {leftmost}, {rightmost})",
                        at[0]
                    ));
                }
            } else if moved {
                tally.moved_on += 1;
            } else if pressed.byte == cursor.byte {
                // An affinity alone, or a guard: never a stop at one place.
                tally
                    .failures
                    .push(format!("{context}: to {pressed:?} at the same place"));
            } else {
                tally.same_place_stops += 1;
            }
        }
    }
}

/// The sweep over generated values, several seeds. Set `SWEEP_SEEDS` (comma
/// separated) to replay others.
#[test]
fn every_arrow_press_that_changes_the_position_moves_the_drawn_caret() {
    let seeds: Vec<u64> = std::env::var("SWEEP_SEEDS").map_or_else(
        |_| vec![1, 0x5eed, 99_540_633, 777_001, 424_242],
        |seeds| {
            seeds
                .split(',')
                .map(|seed| seed.trim().parse().unwrap())
                .collect()
        },
    );
    let mut system = crate::font_pack::test_system();
    let mut tally = Tally::default();
    for seed in seeds {
        let mut rng = Rng(seed | 1);
        for _ in 0..60 {
            let length = 1 + rng.below(7);
            let value: String = (0..length)
                .map(|_| PIECES[rng.below(PIECES.len())])
                .collect();
            sweep_value(&mut system, &value, &mut tally);
        }
    }
    // The measured values, too.
    for value in [
        "\u{5d0}abb\u{5d0} ",
        "\u{600}\u{2028}\u{5d0}\u{93f}a\u{10ffff}",
        "\u{1f468}\u{200d}\u{1f469}  \u{5d1}",
        "\u{5d1}\u{2028}\u{1f468}\u{200d}\u{1f469}\u{10ffff}\u{5d1}",
        "\r\n\r\n\u{2029} \u{2029}\r\n",
        "Trail\nmar\u{2028}ker\u{2029}",
        "Trail \u{5e2}\u{5d1}\u{5e8}\u{5d9}\u{5ea} 123 marker",
    ] {
        sweep_value(&mut system, value, &mut tally);
    }
    eprintln!(
        "own field sweep: {} presses now move on, {} stop at another position drawn at the same place",
        tally.moved_on, tally.same_place_stops
    );
    for failure in &tally.failures {
        eprintln!("FAIL {failure}");
    }
    assert!(
        tally.failures.is_empty(),
        "{} presses: {}",
        tally.failures.len(),
        tally.failures[0]
    );
    assert!(
        tally.moved_on > 0,
        "the sweep no longer reaches a case the rule fixes"
    );
}
