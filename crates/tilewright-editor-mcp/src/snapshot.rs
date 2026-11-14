// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The world as the server last loaded it. Every result is computed from
//! one snapshot and names its revision; `refresh` replaces it.

use crate::budget::fingerprint;
use std::path::Path;
use tilewright_editor_document::revision_hex;
use tilewright_editor_observe::lint::{self, Finding};
use tilewright_editor_observe::world::{Catalog, Observed};
use tilewright_source::Diagnostic;

/// One load of the world.
pub struct Snapshot {
    /// What cursors are bound to and what `refresh` compares: for a world
    /// that loads, its revision and a fingerprint of everything else the
    /// reports read (the catalog's own revision, or why it does not load,
    /// and the editing budget's verdict); for one that does not, `x` and a
    /// fingerprint of its diagnostics.
    pub binding: String,
    pub state: State,
}

pub enum State {
    Valid(Box<Valid>),
    /// The world does not load: World gives no revision for it.
    Invalid(Vec<Diagnostic>),
}

pub struct Valid {
    pub observed: Observed,
    /// The lowercase hex World revision the reports name.
    pub revision: String,
    /// `lint`'s findings, computed once per load: they prepare the
    /// catalog's view, which is the most work a report does.
    pub findings: Vec<Finding>,
}

impl Snapshot {
    /// Loads the world at `root` as `lint` does: with the editing entry
    /// point's budget first, then World's default bounds if that refuses
    /// it, so one load serves every tool. Loading only reads.
    pub fn load(root: &Path) -> Self {
        match Observed::open_for_lint(root) {
            Ok(observed) => {
                let revision = revision_hex(&observed.document.revision());
                let catalog = match &observed.catalog {
                    Catalog::Absent => vec![b"absent".to_vec()],
                    Catalog::Invalid(problem) => {
                        vec![b"invalid".to_vec(), problem.as_bytes().to_vec()]
                    }
                    Catalog::Loaded(loaded) => {
                        vec![b"loaded".to_vec(), loaded.0.revision().to_vec()]
                    }
                };
                let budget = observed.over_editor_budget.as_ref().map_or_else(
                    || vec![b"fits".to_vec()],
                    |codes| codes.iter().map(|code| code.as_bytes().to_vec()).collect(),
                );
                let parts: Vec<&[u8]> = catalog.iter().chain(&budget).map(Vec::as_slice).collect();
                let findings = lint::findings(&observed);
                Self {
                    binding: format!("{revision}{:016x}", fingerprint(&parts)),
                    state: State::Valid(Box::new(Valid {
                        observed,
                        revision,
                        findings,
                    })),
                }
            }
            Err(diagnostics) => {
                let mut parts: Vec<Vec<u8>> = Vec::new();
                for diagnostic in &diagnostics {
                    parts.push(diagnostic.path.as_bytes().to_vec());
                    parts.push(
                        format!(
                            "{} {} {} {} {}",
                            diagnostic.start,
                            diagnostic.end,
                            diagnostic.line,
                            diagnostic.column,
                            diagnostic.code
                        )
                        .into_bytes(),
                    );
                    parts.push(diagnostic.message.as_bytes().to_vec());
                }
                let parts: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
                Self {
                    binding: format!("x{:016x}", fingerprint(&parts)),
                    state: State::Invalid(diagnostics),
                }
            }
        }
    }

    /// A snapshot that holds nothing, to stand in while the world loads
    /// again, so that two worlds are never held at once.
    pub fn empty() -> Self {
        Self {
            binding: String::new(),
            state: State::Invalid(Vec::new()),
        }
    }

    /// The revision, for a world that loads.
    pub fn revision(&self) -> Option<&str> {
        match &self.state {
            State::Valid(valid) => Some(&valid.revision),
            State::Invalid(_) => None,
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        match &self.state {
            State::Valid(_) => &[],
            State::Invalid(diagnostics) => diagnostics,
        }
    }
}
