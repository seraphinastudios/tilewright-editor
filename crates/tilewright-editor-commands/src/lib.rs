// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! Editor commands are world toolkit transactions (`tilewright.transaction/1`).
//! [`preview`] carries one out against an open world in memory: the toolkit
//! edits the files losslessly, validates the result and returns the sealed
//! transaction and its exact inverse. Nothing is written. Publishing, and
//! the history built on previews, come with the commands that need them.

use std::fmt;
use tilewright_edit::{FileChange, Options, Plan, Transaction};
use tilewright_editor_document::WorldDocument;
use tilewright_source::{Diagnostic, World};

/// A transaction carried out in memory against the tree of an open world.
#[derive(Debug)]
pub struct Preview {
    plan: Plan,
}

impl Preview {
    /// Every file the transaction touched or consulted, by path.
    #[must_use]
    pub fn files(&self) -> &[FileChange] {
        self.plan.files()
    }

    /// The files the transaction changes, by path.
    pub fn changed(&self) -> impl Iterator<Item = &FileChange> + '_ {
        self.plan.files().iter().filter(|file| file.is_changed())
    }

    /// The validated world after the transaction.
    #[must_use]
    pub fn world(&self) -> &World {
        self.plan.world()
    }

    /// The transaction with the digests of every file it touched or
    /// consulted, before and after.
    #[must_use]
    pub fn sealed(&self) -> &str {
        self.plan.sealed()
    }

    /// The transaction that, carried out after this one, restores every file
    /// byte for byte.
    #[must_use]
    pub fn inverse(&self) -> &str {
        self.plan.inverse()
    }
}

/// Carries out the transaction `text`, named `name` in diagnostics, on the
/// tree of `document` as it is now, without writing anything. Its
/// preconditions are its own: cell and value expectations, tests, and the
/// file digests of its `base`.
///
/// # Errors
/// Returns the transaction's problems, each at its operation, or the
/// diagnostics of the edited world.
pub fn preview(document: &WorldDocument, name: &str, text: &str) -> Result<Preview, Error> {
    let transaction = Transaction::parse(name, text).map_err(Error::Rejected)?;
    let plan = tilewright_edit::plan(document.root(), &transaction, &Options::default())
        .map_err(Error::Rejected)?;
    Ok(Preview { plan })
}

#[derive(Debug)]
pub enum Error {
    /// The transaction is malformed, a precondition failed, or the edited
    /// world does not validate: the toolkit's diagnostics.
    Rejected(Vec<Diagnostic>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(diagnostics) => {
                write!(
                    formatter,
                    "the transaction was rejected: {} problem(s)",
                    diagnostics.len()
                )?;
                if let Some(first) = diagnostics.first() {
                    write!(formatter, "; first: {first}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::{Error, preview};
    use tilewright_editor_document::WorldDocument;
    use tilewright_editor_testkit::fixture_world;

    const PAINT: &str = r#"{
  "format": "tilewright.transaction/1",
  "title": "Paint sand into the pond and a new chunk",
  "actor": "test:commands",
  "operations": [
    {"op": "terrain.set", "plane": "meadow", "layer": 0, "grid": "ground",
     "cells": [{"at": [10, 10], "value": "sand", "expect": "water"},
               {"at": [32, 0], "value": "sand", "expect": null}]}
  ]
}
"#;

    #[test]
    fn previews_a_transaction_without_writing() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let preview = preview(&document, "paint.jsonc", PAINT).unwrap();
        let changed: Vec<(&str, bool)> = preview
            .changed()
            .map(|file| (file.path.as_str(), file.before.is_none()))
            .collect();
        assert_eq!(
            changed,
            [
                ("planes/meadow/layers/0/0_0.jsonc", false),
                ("planes/meadow/layers/0/1_0.jsonc", true),
            ]
        );
        let plane = &preview.world().planes()["meadow"];
        let pond = &plane.chunks[&(0, 0, 0)].ground[0];
        assert_eq!(pond.value(10 * 32 + 10), Some("sand"));
        assert!(plane.chunks.contains_key(&(0, 1, 0)));
        assert!(preview.sealed().contains("\"base\""));
        assert!(preview.inverse().contains("document.delete"));
        // Previewing wrote nothing: the tree still has its revision.
        let reopened = WorldDocument::open(document.root()).unwrap();
        assert_eq!(reopened.revision(), document.revision());
    }

    #[test]
    fn a_failed_expectation_is_a_conflict() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let stale = PAINT.replace(r#""expect": "water""#, r#""expect": "grass""#);
        let Err(Error::Rejected(diagnostics)) = preview(&document, "paint.jsonc", &stale) else {
            panic!("a stale expectation was accepted");
        };
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "tx.conflict"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn a_malformed_transaction_is_rejected() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let Err(Error::Rejected(diagnostics)) = preview(&document, "bad.jsonc", "{\"format\": 1}")
        else {
            panic!("a malformed transaction was accepted");
        };
        assert!(!diagnostics.is_empty());
    }
}
