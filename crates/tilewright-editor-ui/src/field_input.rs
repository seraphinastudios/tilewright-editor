// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The text a field draft edits: the editor's own [`TextInput`], or, opt-in
//! for the inspector's string fields, the shared `tilewright-ui` field.
//!
//! Both answer the same calls with the same results and the same
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
        CompositionSnapshot, DisplayMarker, TextAffinity, TextCursor, TextInput, TextInputSnapshot,
        TextRevision, TextSelection, scalar_selection_bytes,
    },
    tools::ToolError,
};

/// Which implementation edits a draft. The editor's own is the default;
/// only the inspector's string fields can use the shared one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FieldImplementation {
    #[default]
    Editor,
    Shared,
}

impl FieldImplementation {
    pub const ALL: [Self; 2] = [Self::Editor, Self::Shared];

    /// The name evidence records carry for the shared field.
    pub const SHARED_NAME: &'static str = "tilewright-ui 0.1.1";
}

/// The rejection wording of the editor's own field, for each shared refusal.
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
    /// as the editor's own field words it.
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

#[derive(Clone, Debug)]
enum Backend {
    Editor(TextInput),
    Shared(Box<SharedInput>),
}

/// One draft's text, edited by the implementation it was opened with.
#[derive(Clone, Debug)]
pub struct FieldInput(Backend);

impl PartialEq for FieldInput {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Backend::Editor(a), Backend::Editor(b)) => a == b,
            (Backend::Shared(a), Backend::Shared(b)) => {
                a.draft == b.draft && a.ui.snapshot() == b.ui.snapshot()
            }
            _ => false,
        }
    }
}

impl FieldInput {
    pub fn new(text: String, implementation: FieldImplementation) -> Result<Self, ToolError> {
        Self::with_limit(text, 0, crate::inspector::MAX_INPUT_BYTES, implementation)
    }

    pub(crate) fn with_limit(
        text: String,
        draft: u64,
        limit: usize,
        implementation: FieldImplementation,
    ) -> Result<Self, ToolError> {
        Ok(Self(match implementation {
            FieldImplementation::Editor => {
                Backend::Editor(TextInput::with_limit(text, draft, limit)?)
            }
            FieldImplementation::Shared => {
                let limit = limit.min(crate::inspector::MAX_INPUT_BYTES);
                if text.len() > limit {
                    return Err(ToolError("Input exceeds byte budget"));
                }
                Backend::Shared(Box::new(SharedInput::open(text, draft, limit)?))
            }
        }))
    }

    pub fn implementation(&self) -> FieldImplementation {
        match self.0 {
            Backend::Editor(_) => FieldImplementation::Editor,
            Backend::Shared(_) => FieldImplementation::Shared,
        }
    }

    /// The shared field's state, while it edits this draft.
    pub fn shared(&self) -> Option<&SharedInput> {
        match &self.0 {
            Backend::Shared(shared) => Some(shared),
            Backend::Editor(_) => None,
        }
    }

    pub fn text(&self) -> &str {
        match &self.0 {
            Backend::Editor(input) => input.text(),
            Backend::Shared(shared) => shared.ui.composer_text(),
        }
    }

    pub fn selection(&self) -> TextSelection {
        match &self.0 {
            Backend::Editor(input) => input.selection(),
            Backend::Shared(shared) => selection(shared.ui.composer_selection()),
        }
    }

    pub fn revision(&self) -> TextRevision {
        match &self.0 {
            Backend::Editor(input) => input.revision(),
            Backend::Shared(shared) => shared.revision(),
        }
    }

    pub fn is_composing(&self) -> bool {
        match &self.0 {
            Backend::Editor(input) => input.is_composing(),
            Backend::Shared(shared) => shared.ui.is_composing(),
        }
    }

    pub fn snapshot(&self) -> TextInputSnapshot {
        match &self.0 {
            Backend::Editor(input) => input.snapshot(),
            Backend::Shared(shared) => {
                editor_snapshot(&shared.ui.snapshot().composer, shared.draft)
            }
        }
    }

    pub fn set_selection(
        &mut self,
        selection: TextSelection,
        presented: TextRevision,
    ) -> Result<(), ToolError> {
        match &mut self.0 {
            Backend::Editor(input) => input.set_selection(selection, presented),
            Backend::Shared(shared) => {
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
        }
    }

    pub fn select_all(&mut self) -> Result<(), ToolError> {
        match &mut self.0 {
            Backend::Editor(input) => input.select_all(),
            Backend::Shared(shared) => shared.edit(TextCommand::SelectAll),
        }
    }

    pub fn input(&mut self, text: &str) -> Result<(), ToolError> {
        match &mut self.0 {
            Backend::Editor(input) => input.input(text),
            Backend::Shared(shared) => {
                let target = shared.target()?;
                shared.apply(
                    Input::TextCommit {
                        target,
                        text: text.into(),
                    },
                    |_| REFUSED,
                )
            }
        }
    }

    pub fn backspace(&mut self) -> Result<(), ToolError> {
        match &mut self.0 {
            Backend::Editor(input) => input.backspace(),
            Backend::Shared(shared) => shared.edit(TextCommand::Backspace),
        }
    }

    pub fn delete(&mut self) -> Result<(), ToolError> {
        match &mut self.0 {
            Backend::Editor(input) => input.delete(),
            Backend::Shared(shared) => shared.edit(TextCommand::Delete),
        }
    }

    pub fn update_preedit(&mut self, text: &str, start: i32, length: i32) -> Result<(), ToolError> {
        match &mut self.0 {
            Backend::Editor(input) => input.update_preedit(text, start, length),
            Backend::Shared(shared) => {
                let target = shared.target()?;
                shared.apply(
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
        }
    }

    pub fn cancel_preedit(&mut self) -> Result<(), ToolError> {
        match &mut self.0 {
            Backend::Editor(input) => input.cancel_preedit(),
            Backend::Shared(shared) => {
                let target = shared.target()?;
                shared.apply(Input::CancelComposition { target }, |_| COMPOSING)
            }
        }
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

/// The shared field's snapshot in the editor's own terms: the same
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

/// Turn each named `fn case(implementation: FieldImplementation)` into two
/// tests, `case::editor` and `case::shared`, so one test body checks both
/// fields. The app crate's tests use it too.
#[macro_export]
macro_rules! both_fields {
    ($($case:ident),* $(,)?) => {
        $(
            mod $case {
                #[test]
                fn editor() {
                    super::$case($crate::field_input::FieldImplementation::Editor);
                }
                #[test]
                fn shared() {
                    super::$case($crate::field_input::FieldImplementation::Shared);
                }
            }
        )*
    };
}
