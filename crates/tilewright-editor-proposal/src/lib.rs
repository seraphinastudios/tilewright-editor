// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! Change-set proposals (`tilewright.editor-proposal/1`): the accepted
//! steps of an editing session in a sandbox, recorded so a person can
//! apply them, one at a time, to their own world.
//!
//! - [`Proposal::from_session`] records a session's applied steps, each as
//!   its typed [`Action`](tilewright_editor_ui::Action) and the sealed
//!   transaction it was accepted as; [`Proposal::decode`] reads one back
//!   and verifies everything that can be checked without a world.
//! - [`Progress`] applies a verified proposal to a person's session step
//!   by step: each Action is dispatched again through the engine, under
//!   the person's catalog grants, and must prepare the transaction it
//!   recorded (all but its actor). The person reviews and accepts every
//!   step through the normal Review and Accept.
//! - [`ProposalStore`] writes proposal files durably into a directory the
//!   caller opened, and [`read_proposal`] reads one back, bounded.
//!
//! **What the digests protect.** The body digest and the digest of each
//! step's transaction detect corruption and accidental edits. They do not
//! prevent forgery: whoever writes a proposal, an agent running as the
//! same user among them, can write matching digests. Safety comes from
//! elsewhere: every step is dispatched again under the person's grants, a
//! proposal applies only to the exact revision it was made from (the
//! strict base), and the person reviews and accepts every step. Nothing
//! here publishes anything on its own: only the person's Accept does.
//!
//! See `docs/PROPOSALS.md` for the format.

mod action;
mod apply;
mod compare;
mod document;
mod store;

pub use action::{decode_action, encode_action};
pub use apply::{Notice, Progress, Settled};
pub use compare::{Divergence, compare_transactions};
pub use document::{FORMAT, Proposal, ProposalStep};
pub use store::{ProposalStore, StoredProposal, read_proposal};

use std::fmt;
use tilewright_editor_ui::RefusalCode;

/// Every bound of a proposal, in one place.
pub mod limits {
    /// Most steps in one proposal. History keeps up to
    /// [`tilewright_editor_commands::MAX_HISTORY`] steps; a session with
    /// more applied steps than this cannot be recorded in one proposal.
    pub const STEPS: usize = 256;
    /// Most bytes of a proposal file. History may retain up to
    /// [`tilewright_editor_commands::MAX_HISTORY_BYTES`]; a proposal is
    /// smaller.
    pub const BYTES: usize = 16 << 20;
    /// Most bytes of one step's sealed transaction.
    pub const TRANSACTION_BYTES: usize = 1 << 20;
    /// Most files one step's transaction may change: the engine's review
    /// limit, so every step can be reviewed completely.
    pub const CHANGED_FILES: usize = tilewright_editor_ui::review::MAXIMUM_REVIEW_FILES;
    /// Most bytes of a string in an action (a uid, plane, grid, prototype,
    /// field, zone, anchor or display name).
    pub const TEXT_BYTES: usize = 4096;
    /// Most cells of a stroke action: the brush's own limit.
    pub const CELLS: usize = tilewright_editor_ui::brush::MAXIMUM_STROKE_CELLS;
    /// The largest `amount` of a height adjustment either way: the
    /// engine's own bound. 0 is not an amount.
    pub const HEIGHT_AMOUNT: i32 = tilewright_editor_ui::sculpt::MAXIMUM_AMOUNT;
    /// Most bytes of a field value, encoded, and its deepest nesting: the
    /// engine's own field budget.
    pub const FIELD_VALUE_BYTES: usize = 64 << 10;
    pub const FIELD_VALUE_DEPTH: usize = 8;
    /// Most JSON values (each an entry of the engine's own count) in one
    /// field value, as the engine counts them: the value itself and every
    /// value nested in it.
    pub const FIELD_VALUE_ENTRIES: usize = 4096;
    /// Most members of one object in a field value: what the World format's
    /// reader allows, so a transaction carrying more could never have been
    /// accepted.
    pub const OBJECT_KEYS: usize = 256;
    /// Most JSON values the reader takes in one proposal (object keys are
    /// not values and are not counted).
    /// It follows from the other bounds: a step holds at most one action of
    /// [`CELLS`] cells (3 values each) or a field value of
    /// [`FIELD_VALUE_ENTRIES`] values, and fewer than 64 values more, and
    /// the document around the steps fewer than 64. No proposal within the
    /// other bounds reaches it.
    pub const VALUES: usize = STEPS * (3 * CELLS + 64) + 64;
    /// Most bytes of the untrusted client label.
    pub const CLIENT_BYTES: usize = 256;
    /// Most bytes of the editor version.
    pub const EDITOR_BYTES: usize = 64;
    /// Largest magnitude of an integer: the World format's.
    pub const INTEGER: i64 = tilewright_jsonc::MAX_SAFE_INTEGER;
}

/// A refusal of a proposal: a stable [`RefusalCode`] (one of the
/// `proposal.*` codes, or `pending.exists` and `session.blocked` from the
/// engine), the engine's own code when the engine refused a step, the
/// step it concerns, and a message written here. Messages never quote
/// world or proposal text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposalError {
    code: RefusalCode,
    engine: Option<RefusalCode>,
    step: Option<usize>,
    message: String,
}

impl ProposalError {
    pub(crate) fn new(code: RefusalCode, message: impl Into<String>) -> Self {
        Self {
            code,
            engine: None,
            step: None,
            message: message.into(),
        }
    }
    pub(crate) fn at(mut self, step: usize) -> Self {
        self.step = Some(step);
        self
    }
    pub(crate) fn engine(mut self, code: RefusalCode) -> Self {
        self.engine = Some(code);
        self
    }

    #[must_use]
    pub const fn code(&self) -> RefusalCode {
        self.code
    }
    /// The engine's code, for [`RefusalCode::ProposalStepRefused`].
    #[must_use]
    pub const fn engine_code(&self) -> Option<RefusalCode> {
        self.engine
    }
    /// The index of the step concerned, counting from 0.
    #[must_use]
    pub const fn step(&self) -> Option<usize> {
        self.step
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProposalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(step) = self.step {
            write!(f, "step {}: ", step + 1)?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProposalError {}

pub(crate) fn invalid(message: impl Into<String>) -> ProposalError {
    ProposalError::new(RefusalCode::ProposalInvalid, message)
}
pub(crate) fn budget(message: impl Into<String>) -> ProposalError {
    ProposalError::new(RefusalCode::ProposalBudget, message)
}

/// SHA-256 of `bytes`, through the World toolkit's own digest.
pub(crate) fn sha256(text: &str) -> [u8; 32] {
    tilewright_edit::digest(Some(text)).unwrap_or_default()
}

/// Lowercase hexadecimal of a digest.
#[must_use]
pub fn hex(digest: &[u8; 32]) -> String {
    tilewright_editor_document::revision_hex(digest)
}

/// The digest written as 64 lowercase hexadecimal digits, or `None`.
pub(crate) fn unhex(text: &str) -> Option<[u8; 32]> {
    let bytes = text.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let digit = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    };
    let mut digest = [0; 32];
    for (index, pair) in bytes.chunks_exact(2).enumerate() {
        digest[index] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(digest)
}

#[cfg(test)]
mod tests {
    use super::{hex, unhex};

    #[test]
    fn digests_are_exactly_64_lowercase_hexadecimal_digits() {
        let digest: [u8; 32] = std::array::from_fn(|index| u8::try_from(index * 8).unwrap());
        assert_eq!(unhex(&hex(&digest)), Some(digest));
        let text = hex(&digest);
        for other in [
            text.to_uppercase(),
            text[..62].to_owned(),
            format!("{text}0"),
            format!("{}g", &text[..63]),
            format!(" {}", &text[1..]),
        ] {
            assert_eq!(unhex(&other), None, "{other}");
        }
    }
}
