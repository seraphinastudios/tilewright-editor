// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The text a field draft edits: every text draft (the inspector's string,
//! boolean, integer, number and reference fields, the zone name drafts, the
//! palette filter, the Move coordinates, a new anchor's id and a portal's
//! typed location) is edited by the shared `tilewright-ui` field.
//!
//! It answers the editor's calls in the editor's terms (its
//! [`TextInputSnapshot`], selections and revisions) and with the editor's
//! [`ToolError`] wording, so every caller (native events, the clipboard,
//! review) stays one path. The shared field is a `tilewright_ui::Ui` in its
//! single-field composition, bound to the stored value and focused for its
//! whole life; only its composer is used here. Drawing it is the host's job
//! ([`FieldInput::shared`] lends the state to the app's compiler).
use tilewright_ui::{
    COMPOSER, Composition, Input, Limits, TextCommand, TextTarget, Ui, UiError, UiSnapshot,
};

use crate::{
    text_input::{
        CompositionSnapshot, DisplayMarker, TextAffinity, TextCursor, TextInputSnapshot,
        TextRevision, TextSelection, scalar_selection_bytes,
    },
    tools::ToolError,
};

/// The editor's rejection wording, for each shared refusal.
const REFUSED: ToolError =
    ToolError("Input exceeds byte budget or contains control characters or line breaks");
const COMPOSING: ToolError = ToolError("Finish or cancel composition first");

#[derive(Clone, Debug)]
pub struct SharedInput {
    ui: Ui,
    draft: u64,
}

impl SharedInput {
    fn open(text: String, draft: u64, limit: usize) -> Result<Self, ToolError> {
        let limits = Limits {
            composer_bytes: limit,
            ..Limits::default()
        };
        let mut ui =
            Ui::with_composition(limits, Composition::SingleField).map_err(shared_error)?;
        ui.bind_field(text).map_err(|error| match error {
            UiError::LimitExceeded => ToolError("Input exceeds byte budget"),
            other => shared_error(other),
        })?;
        ui.apply(Input::Focus(Some(COMPOSER)))
            .map_err(shared_error)?;
        Ok(Self { ui, draft })
    }

    /// The shared state, for the host's compiler.
    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    pub fn snapshot(&self) -> UiSnapshot {
        self.ui.snapshot()
    }

    fn target(&self) -> Result<TextTarget, ToolError> {
        self.ui.text_target().ok_or(ToolError("No input draft"))
    }

    /// Read without a snapshot, as are the text, selection and composing
    /// state ([`Ui::composer_text`] and its siblings).
    fn revision(&self) -> TextRevision {
        TextRevision {
            draft: self.draft,
            revision: self.ui.composer_revision().get(),
        }
    }

    /// Apply one composer input; a refusal changes nothing and is worded
    /// in the editor's words.
    fn apply(
        &mut self,
        input: Input,
        invalid: impl FnOnce(bool) -> ToolError,
    ) -> Result<(), ToolError> {
        let composing = self.ui.is_composing();
        self.ui.apply(input).map(drop).map_err(|error| match error {
            UiError::InvalidInput => invalid(composing),
            other => shared_error(other),
        })
    }

    fn edit(&mut self, command: TextCommand) -> Result<(), ToolError> {
        let target = self.target()?;
        self.apply(Input::Edit { target, command }, |_| COMPOSING)
    }
}

fn shared_error(error: UiError) -> ToolError {
    match error {
        UiError::LimitExceeded | UiError::RefusedText => REFUSED,
        UiError::Stale => ToolError("Input presentation is stale"),
        UiError::Exhausted => ToolError("Input revision exhausted"),
        _ => ToolError("The shared field refused the input"),
    }
}

/// One draft's text, edited by the shared field.
#[derive(Clone, Debug)]
pub struct FieldInput(Box<SharedInput>);

impl PartialEq for FieldInput {
    fn eq(&self, other: &Self) -> bool {
        self.0.draft == other.0.draft && self.0.ui.snapshot() == other.0.ui.snapshot()
    }
}

impl FieldInput {
    pub fn new(text: String) -> Result<Self, ToolError> {
        Self::with_limit(text, 0, crate::inspector::MAX_INPUT_BYTES)
    }

    pub(crate) fn with_limit(text: String, draft: u64, limit: usize) -> Result<Self, ToolError> {
        let limit = limit.min(crate::inspector::MAX_INPUT_BYTES);
        // Existing source can contain controls or line separators. Opening it
        // stays lossless; only new input follows the single-line policy.
        if text.len() > limit {
            return Err(ToolError("Input exceeds byte budget"));
        }
        Ok(Self(Box::new(SharedInput::open(text, draft, limit)?)))
    }

    /// The shared field's state, for the host's compiler.
    pub fn shared(&self) -> &SharedInput {
        &self.0
    }

    pub fn text(&self) -> &str {
        self.0.ui.composer_text()
    }

    pub fn selection(&self) -> TextSelection {
        selection(self.0.ui.composer_selection())
    }

    pub fn revision(&self) -> TextRevision {
        self.0.revision()
    }

    pub fn is_composing(&self) -> bool {
        self.0.ui.is_composing()
    }

    pub fn snapshot(&self) -> TextInputSnapshot {
        editor_snapshot(&self.0.ui.snapshot().composer, self.0.draft)
    }

    pub fn set_selection(
        &mut self,
        selection: TextSelection,
        presented: TextRevision,
    ) -> Result<(), ToolError> {
        let shared = &mut self.0;
        let mut target = shared.target()?;
        if presented.draft != shared.draft {
            return Err(ToolError("Input presentation is stale"));
        }
        // The shared field checks the presented revision itself.
        target.revision = tilewright_ui::TextRevision::new(presented.revision)
            .map_err(|_| ToolError("Input presentation is stale"))?;
        shared.apply(
            Input::Selection {
                target,
                selection: shared_selection(selection),
            },
            |composing| {
                if composing {
                    COMPOSING
                } else {
                    ToolError("Cursor is not an extended grapheme boundary")
                }
            },
        )
    }

    pub fn select_all(&mut self) -> Result<(), ToolError> {
        self.0.edit(TextCommand::SelectAll)
    }

    pub fn input(&mut self, text: &str) -> Result<(), ToolError> {
        let target = self.0.target()?;
        self.0.apply(
            Input::TextCommit {
                target,
                text: text.into(),
            },
            |_| REFUSED,
        )
    }

    pub fn backspace(&mut self) -> Result<(), ToolError> {
        self.0.edit(TextCommand::Backspace)
    }

    pub fn delete(&mut self) -> Result<(), ToolError> {
        self.0.edit(TextCommand::Delete)
    }

    pub fn update_preedit(&mut self, text: &str, start: i32, length: i32) -> Result<(), ToolError> {
        let target = self.0.target()?;
        self.0.apply(
            Input::Preedit {
                target,
                text: text.into(),
                start,
                length,
            },
            // The offsets were refused: word it as the editor does.
            |_| {
                scalar_selection_bytes(text, start, length)
                    .err()
                    .unwrap_or(ToolError("Invalid composition selection"))
            },
        )
    }

    pub fn cancel_preedit(&mut self) -> Result<(), ToolError> {
        let target = self.0.target()?;
        self.0
            .apply(Input::CancelComposition { target }, |_| COMPOSING)
    }
}

fn affinity(value: tilewright_ui::TextAffinity) -> TextAffinity {
    match value {
        tilewright_ui::TextAffinity::Before => TextAffinity::Before,
        tilewright_ui::TextAffinity::After => TextAffinity::After,
    }
}

pub fn cursor(value: tilewright_ui::TextCursor) -> TextCursor {
    TextCursor {
        byte: value.byte,
        affinity: affinity(value.affinity),
    }
}

pub fn shared_cursor(value: TextCursor) -> tilewright_ui::TextCursor {
    tilewright_ui::TextCursor {
        byte: value.byte,
        affinity: match value.affinity {
            TextAffinity::Before => tilewright_ui::TextAffinity::Before,
            TextAffinity::After => tilewright_ui::TextAffinity::After,
        },
    }
}

fn selection(value: tilewright_ui::TextSelection) -> TextSelection {
    TextSelection {
        anchor: cursor(value.anchor),
        focus: cursor(value.focus),
    }
}

fn shared_selection(value: TextSelection) -> tilewright_ui::TextSelection {
    tilewright_ui::TextSelection {
        anchor: shared_cursor(value.anchor),
        focus: shared_cursor(value.focus),
    }
}

/// The shared field's snapshot in the editor's terms: the same
/// committed and display text, markers, selections and composition.
pub fn editor_snapshot(
    snapshot: &tilewright_ui::TextInputSnapshot,
    draft: u64,
) -> TextInputSnapshot {
    TextInputSnapshot {
        committed: snapshot.committed.clone(),
        display_text: snapshot.display_text.clone(),
        selection: selection(snapshot.selection),
        display_selection: selection(snapshot.display_selection),
        composition: snapshot
            .composition
            .as_ref()
            .map(|composition| CompositionSnapshot {
                text: composition.text.clone(),
                replacement: composition.replacement.clone(),
                selection: composition.selection.clone(),
                display_range: composition.display_range.clone(),
            }),
        markers: snapshot
            .markers
            .iter()
            .map(|marker| DisplayMarker {
                source: marker.source.clone(),
                display: marker.display.clone(),
                glyph: marker.glyph.clone(),
            })
            .collect(),
        revision: TextRevision {
            draft,
            revision: snapshot.revision.get(),
        },
    }
}
