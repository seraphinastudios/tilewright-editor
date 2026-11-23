// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use tilewright_editor_ui::{
    field_input::{FieldImplementation, FieldInput},
    inspector::MAX_INPUT_BYTES,
    text_input::{TextAffinity, TextCursor, TextSelection, scalar_selection_bytes},
};

fn select(input: &mut FieldInput, anchor: usize, focus: usize) {
    input
        .set_selection(
            TextSelection {
                anchor: TextCursor::new(anchor),
                focus: TextCursor::new(focus),
            },
            input.revision(),
        )
        .unwrap();
}

fn editing_preserves_exact_utf8_and_replaces_grapheme_selection_in_middle(
    implementation: FieldImplementation,
) {
    let original = "é e\u{301} العربية 123 עברית क्\u{200d}ष 👨‍👩‍👧‍👦 🇺🇳";
    let mut input = FieldInput::new(original.into(), implementation).unwrap();
    assert_eq!(input.text(), original);
    input.select_all().unwrap();
    assert_eq!(input.text(), original, "select-all must not delete input");
    input.input("Ae\u{301}B").unwrap();
    select(&mut input, 1, 4);
    input.input("青").unwrap();
    assert_eq!(input.text(), "A青B");
    select(&mut input, 1, 1);
    input.input("עברית").unwrap();
    assert_eq!(input.text(), "Aעברית青B");
}

fn both_deletions_remove_whole_extended_graphemes(implementation: FieldImplementation) {
    for grapheme in ["e\u{301}", "👨‍👩‍👧‍👦", "🇺🇳", "क्ष", "👍🏽", "שׁ"]
    {
        let mut input = FieldInput::new(format!("A{grapheme}B"), implementation).unwrap();
        select(&mut input, 1 + grapheme.len(), 1 + grapheme.len());
        input.backspace().unwrap();
        assert_eq!(input.text(), "AB", "backspace split {grapheme}");
        let mut input = FieldInput::new(format!("A{grapheme}B"), implementation).unwrap();
        select(&mut input, 1, 1);
        input.delete().unwrap();
        assert_eq!(input.text(), "AB", "delete split {grapheme}");
    }
}

fn selection_validates_utf8_graphemes_affinity_and_display_revision_atomically(
    implementation: FieldImplementation,
) {
    let mut input = FieldInput::new("Ae\u{301}🙂B".into(), implementation).unwrap();
    for byte in [2, 3, 5, 6, 7, 10, usize::MAX] {
        let before = input.clone();
        let cursor = TextCursor::new(byte);
        assert!(
            input
                .set_selection(TextSelection::caret(cursor), input.revision())
                .is_err()
        );
        assert_eq!(input, before);
    }
    let old = input.revision();
    let cursor = TextCursor {
        byte: 4,
        affinity: TextAffinity::Before,
    };
    input
        .set_selection(TextSelection::caret(cursor), old)
        .unwrap();
    assert_eq!(input.selection().focus, cursor);
    assert!(
        input
            .set_selection(TextSelection::caret(TextCursor::new(0)), old)
            .is_err()
    );
    // Renderer visual navigation supplies either affinity without model-side bidi.
    let selection = TextSelection {
        anchor: cursor,
        focus: TextCursor::new(8),
    };
    input.set_selection(selection, input.revision()).unwrap();
    assert_eq!(input.selection(), selection);
    input.input("中").unwrap();
    assert_eq!(input.text(), "Ae\u{301}中B");
}

fn budget_is_replacement_aware_and_rejection_preserves_composition_and_selection(
    implementation: FieldImplementation,
) {
    let mut input = FieldInput::new("a".repeat(MAX_INPUT_BYTES), implementation).unwrap();
    let before = input.clone();
    assert!(input.input("é").is_err());
    assert_eq!(input, before);
    select(&mut input, 0, 2);
    input.input("é").unwrap();
    assert_eq!(input.text().len(), MAX_INPUT_BYTES);
    select(&mut input, 0, 2);
    input.update_preedit("é", 1, 0).unwrap();
    let before = input.clone();
    assert!(input.update_preedit("青", 1, 0).is_err());
    assert!(input.input("青").is_err());
    assert!(input.input("\n").is_err());
    assert_eq!(input, before);
    input.input("é").unwrap();
    assert!(!input.is_composing());
    assert_eq!(input.text().len(), MAX_INPUT_BYTES);
}

fn ime_is_a_virtual_splice_then_one_commit_over_original_selection(
    implementation: FieldImplementation,
) {
    let mut input = FieldInput::new("AoldZ".into(), implementation).unwrap();
    select(&mut input, 4, 1);
    let selection = input.selection();
    input.update_preedit("é🙂青", 1, 1).unwrap();
    assert_eq!(input.text(), "AoldZ");
    assert_eq!(input.selection(), selection);
    let snapshot = input.snapshot();
    assert_eq!(snapshot.display_text, "Aé🙂青Z");
    let composition = snapshot.composition.as_ref().unwrap();
    assert_eq!(composition.replacement, 1..4);
    assert_eq!(composition.selection, Some(2..6));
    assert_eq!(composition.display_range, 1..10);
    assert_eq!(snapshot.display_selection.anchor.byte, 3);
    assert_eq!(snapshot.display_selection.focus.byte, 7);
    assert_eq!(
        snapshot.committed_cursor(TextCursor::new(10)).unwrap().byte,
        4
    );
    assert!(snapshot.committed_cursor(TextCursor::new(3)).is_err());
    input.update_preedit("אב", -1, -1).unwrap();
    assert_eq!(input.snapshot().composition.unwrap().replacement, 1..4);
    let before = input.clone();
    assert!(input.select_all().is_err());
    assert!(input.backspace().is_err());
    assert!(input.delete().is_err());
    assert_eq!(input, before);
    input.input("עברית").unwrap();
    assert_eq!(input.text(), "AעבריתZ");
    assert!(!input.is_composing());
    let revision = input.revision();
    input.cancel_preedit().unwrap();
    assert_eq!(
        input.revision(),
        revision,
        "cancel after commit cannot replay replacement"
    );
    input.update_preedit("مرحبا", 5, 0).unwrap();
    input.cancel_preedit().unwrap();
    assert_eq!(input.text(), "AעבריתZ");
}

fn ime_offsets_are_scalars_and_bad_updates_do_not_mutate_any_state(
    implementation: FieldImplementation,
) {
    assert_eq!(scalar_selection_bytes("é🙂青", 1, 1).unwrap(), Some(2..6));
    assert_eq!(scalar_selection_bytes("é🙂青", 3, 0).unwrap(), Some(9..9));
    assert_eq!(scalar_selection_bytes("é🙂青", -1, -1).unwrap(), None);
    let mut input = FieldInput::new("e\u{301}".into(), implementation).unwrap();
    input.update_preedit("🙂", 1, 0).unwrap();
    let before = input.clone();
    for (start, length) in [
        (2, 0),
        (0, 2),
        (-2, 0),
        (0, -2),
        (i32::MAX, i32::MAX),
        (-1, i32::MAX),
        (i32::MAX, -1),
    ] {
        assert!(input.update_preedit("🙂", start, length).is_err());
        assert_eq!(input, before);
    }
    input.update_preedit("", 0, 0).unwrap();
    assert!(!input.is_composing());
    assert_eq!(input.text(), "e\u{301}");
}

fn grapheme_joins_keep_caret_valid_without_rewriting_raw_input(
    implementation: FieldImplementation,
) {
    let mut input = FieldInput::new("eX".into(), implementation).unwrap();
    select(&mut input, 1, 1);
    input.input("\u{301}").unwrap();
    assert_eq!(input.text(), "e\u{301}X");
    assert_eq!(input.selection().focus.byte, 3);
    select(&mut input, 0, 0);
    input.update_preedit("\u{301}", 1, 0).unwrap();
    let snapshot = input.snapshot();
    assert_eq!(
        snapshot.display_selection.anchor,
        snapshot.display_selection.focus
    );
    // Format controls, joining marks and bidi marks remain exact user text.
    input.cancel_preedit().unwrap();
    input.input("\u{2067}עברית\u{2069}\u{200d}").unwrap();
    assert!(input.text().starts_with("\u{2067}עברית\u{2069}\u{200d}"));
}

fn new_line_and_paragraph_separators_are_rejected_like_controls(
    implementation: FieldImplementation,
) {
    for separator in ["\u{2028}", "\u{2029}"] {
        let mut input = FieldInput::new("AZ".into(), implementation).unwrap();
        select(&mut input, 1, 1);
        let before = input.clone();
        // Typed or pasted text arrives through `input`; IME through preedit.
        assert!(input.input(separator).is_err());
        assert!(input.input(&format!("x{separator}y")).is_err());
        assert!(input.update_preedit(separator, 1, 0).is_err());
        assert_eq!(input, before);
        input.update_preedit("é", 1, 0).unwrap();
        let composing = input.clone();
        assert!(
            input
                .update_preedit(&format!("é{separator}"), 2, 0)
                .is_err()
        );
        assert!(input.input(&format!("é{separator}")).is_err());
        assert_eq!(input, composing, "a rejected commit keeps the composition");
        input.input("é").unwrap();
        assert_eq!(input.text(), "AéZ");
    }
}

fn loaded_line_and_paragraph_separators_are_kept_until_the_user_removes_them(
    implementation: FieldImplementation,
) {
    let loaded = "A\u{2028}B\u{2029}C";
    let mut input = FieldInput::new(loaded.into(), implementation).unwrap();
    assert_eq!(input.text(), loaded);
    // Shown as markers on one line; the draft keeps the real characters.
    assert_eq!(input.snapshot().display_text, "A\u{23ce}B\u{b6}C");
    assert_eq!(input.snapshot().committed, loaded);
    // Editing elsewhere preserves the stored separators byte for byte.
    select(&mut input, 0, 0);
    input.input("<").unwrap();
    assert_eq!(input.text(), "<A\u{2028}B\u{2029}C");
    // Each separator is a whole grapheme the user can delete or replace.
    select(&mut input, 2, 2);
    input.delete().unwrap();
    assert_eq!(input.text(), "<AB\u{2029}C");
    select(&mut input, 3, 6);
    input.input(" ").unwrap();
    assert_eq!(input.text(), "<AB C");
}

// Each case runs against the editor's own field and the shared field.
tilewright_editor_ui::both_fields!(
    editing_preserves_exact_utf8_and_replaces_grapheme_selection_in_middle,
    both_deletions_remove_whole_extended_graphemes,
    selection_validates_utf8_graphemes_affinity_and_display_revision_atomically,
    budget_is_replacement_aware_and_rejection_preserves_composition_and_selection,
    ime_is_a_virtual_splice_then_one_commit_over_original_selection,
    ime_offsets_are_scalars_and_bad_updates_do_not_mutate_any_state,
    grapheme_joins_keep_caret_valid_without_rewriting_raw_input,
    new_line_and_paragraph_separators_are_rejected_like_controls,
    loaded_line_and_paragraph_separators_are_kept_until_the_user_removes_them,
);
