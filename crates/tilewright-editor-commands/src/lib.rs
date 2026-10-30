// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! Semantic edits, reviewable previews and bounded undo/redo over World plans.
//! Preparing a step writes nothing. [`History::commit`] publishes only after
//! its inverse has been durably retained by the caller's [`RecoveryStore`].
//! All source access, validation and publication belongs to the World toolkit.

use std::fmt;
use tilewright_edit::{
    FileChange, Options, Plan, PublishError, PublishOptions, Published, Transaction,
};
use tilewright_editor_document::WorldDocument;
use tilewright_source::{Diagnostic, Overlay, World};

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
    let plan = tilewright_edit::plan_in(
        document.source(),
        &transaction,
        &Options {
            load: document.load_options(),
            rebase: false,
        },
    )
    .map_err(Error::Rejected)?;
    // The plan must start at the world the author actually inspected. Merely
    // trusting an optional transaction base would accept stale UI commands.
    let before: Overlay = plan
        .files()
        .iter()
        .map(|file| (file.path.clone(), file.before.clone()))
        .collect();
    let baseline = tilewright_source::load_edited_in(document.source(), &document.load_options(), &before)
        .map_err(Error::Rejected)?;
    if baseline.source_digest() != document.revision() {
        return Err(Error::StaleRevision);
    }
    plan.check_undo().map_err(Error::Rejected)?;
    Ok(Preview { plan })
}

/// History is bounded both by steps and by the UTF-8 recovery records retained.
pub const MAX_HISTORY: usize = 1_024;
pub const MAX_HISTORY_BYTES: usize = 64 << 20;

/// Durable recovery belongs to the application, outside authored source.
///
/// Implementations must retain the exact inverse using toolkit-owned storage,
/// atomically and durably, before returning `Ok(())`. They must refuse an
/// existing destination and keep the record even if publication fails. A
/// memory-only acknowledgement is suitable only for deterministic tests.
/// The editor never calls this for an unchanged plan.
pub trait RecoveryStore {
    fn retain(&mut self, inverse: &str) -> Result<(), String>;
}

#[derive(Clone, Debug)]
struct Entry {
    forward: String,
    inverse: String,
    before: [u8; 32],
    after: [u8; 32],
}

#[derive(Clone, Copy, Debug)]
enum Direction {
    Edit,
    Undo,
    Redo,
}

#[derive(Debug)]
struct Pending {
    preview: Preview,
    direction: Direction,
}

/// One opened world, its reviewed pending step, and successful command history.
///
/// Each commit publishes one World transaction. Undo and redo are new reviewed
/// transactions, with the same recovery, validation and publication contract.
/// Any publication error requires opening a new session after inspecting the
/// recovery record: a failed rollback can leave a partially changed tree.
#[derive(Debug)]
pub struct History {
    document: WorldDocument,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    pending: Option<Pending>,
    needs_reload: bool,
}

impl History {
    #[must_use]
    pub fn new(document: WorldDocument) -> Self {
        Self {
            document,
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
            needs_reload: false,
        }
    }

    #[must_use]
    pub fn document(&self) -> &WorldDocument {
        &self.document
    }

    #[must_use]
    pub fn depths(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    /// Memory charged to retained forward and inverse transaction records.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(&self.redo)
            .map(|entry| entry.forward.len() + entry.inverse.len())
            .sum()
    }

    #[must_use]
    pub const fn needs_reload(&self) -> bool {
        self.needs_reload
    }

    #[must_use]
    pub fn pending(&self) -> Option<&Preview> {
        self.pending.as_ref().map(|step| &step.preview)
    }

    /// Discard an uncommitted preview; published history is unaffected.
    pub fn cancel(&mut self) {
        if !self.needs_reload {
            self.pending = None;
        }
    }

    /// Prepare a semantic transaction, using the revision observed by its author.
    /// A failed preparation clears any earlier preview so it cannot be saved by mistake.
    pub fn prepare(
        &mut self,
        expected: [u8; 32],
        name: &str,
        text: &str,
    ) -> Result<&Preview, Error> {
        self.prepare_step(expected, name, text, Direction::Edit)
    }

    pub fn prepare_undo(&mut self) -> Result<&Preview, Error> {
        self.check_available()?;
        self.pending = None;
        let entry = self.undo.last().ok_or(Error::NothingToUndo)?.clone();
        self.prepare_step(entry.after, "editor:undo", &entry.inverse, Direction::Undo)
    }

    pub fn prepare_redo(&mut self) -> Result<&Preview, Error> {
        self.check_available()?;
        self.pending = None;
        let entry = self.redo.last().ok_or(Error::NothingToRedo)?.clone();
        self.prepare_step(entry.before, "editor:redo", &entry.forward, Direction::Redo)
    }

    fn check_available(&self) -> Result<(), Error> {
        if self.needs_reload {
            Err(Error::NeedsReload)
        } else {
            Ok(())
        }
    }

    fn prepare_step(
        &mut self,
        expected: [u8; 32],
        name: &str,
        text: &str,
        direction: Direction,
    ) -> Result<&Preview, Error> {
        self.check_available()?;
        self.pending = None;
        if self.document.revision() != expected {
            return Err(Error::StaleRevision);
        }
        let preview = preview(&self.document, name, text)?;
        if matches!(direction, Direction::Edit) && preview.changed().next().is_some() {
            // Retain the original sealed forward transaction as well as its
            // inverse. Moving an entry between stacks never grows its budget;
            // asymmetric create/delete operations cannot strand an undo.
            let bytes: usize = self
                .undo
                .iter()
                .map(|entry| entry.forward.len() + entry.inverse.len())
                .sum();
            if self.undo.len() >= MAX_HISTORY
                || bytes
                    .saturating_add(preview.inverse().len())
                    .saturating_add(preview.sealed().len())
                    > MAX_HISTORY_BYTES
            {
                return Err(Error::HistoryFull);
            }
        }
        self.pending = Some(Pending { preview, direction });
        Ok(&self
            .pending
            .as_ref()
            .expect("pending was just installed")
            .preview)
    }

    /// Publish the exact pending plan after durable recovery. Default options
    /// refuse platforms/filesystems whose access protections World cannot check;
    /// callers must obtain explicit acceptance before opting into unchecked access.
    ///
    /// Errors keep the pending inverse and history available for inspection.
    /// A publication error always blocks further steps in this session. Its
    /// `applied`, `durable` and `cleaned` fields must be shown to the author.
    pub fn commit(
        &mut self,
        recovery: &mut impl RecoveryStore,
        options: &PublishOptions,
    ) -> Result<Published, Error> {
        self.commit_with(recovery, |plan| plan.publish_with(options))
    }

    fn commit_with(
        &mut self,
        recovery: &mut impl RecoveryStore,
        publish: impl FnOnce(&Plan) -> Result<Published, PublishError>,
    ) -> Result<Published, Error> {
        self.check_available()?;
        let pending = self.pending.as_ref().ok_or(Error::NothingPrepared)?;
        let current = self.document.reload().map_err(Error::Document)?;
        if current.revision() != self.document.revision() {
            return Err(Error::StaleRevision);
        }
        if pending.preview.changed().next().is_none() {
            self.pending = None;
            return Ok(Published { durable: true });
        }
        recovery
            .retain(pending.preview.inverse())
            .map_err(Error::Recovery)?;
        let outcome = match publish(&pending.preview.plan) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.needs_reload = true;
                return Err(Error::Publish(error));
            }
        };
        // Even successful publication may not make directory entries durable.
        // Keep its recovery record and refuse to silently continue a session
        // whose persistence guarantee has not been established.
        if !outcome.durable {
            self.needs_reload = true;
            return Err(Error::Publish(PublishError {
                applied: true,
                durable: false,
                cleaned: true,
                problems: Vec::new(),
            }));
        }
        // World permits concurrent valid changes to unconsulted files. Never
        // silently adopt the cached preview when the actual published world
        // differs, or a concurrent write prevents it from reopening.
        let reopened = match self.document.reload() {
            Ok(reopened) if reopened.revision() == pending.preview.world().source_digest() => {
                reopened
            }
            result => {
                self.needs_reload = true;
                return Err(Error::PublishedStateChanged {
                    published: outcome,
                    reopen_error: result.err(),
                });
            }
        };
        let pending = self.pending.take().expect("published pending step");
        match pending.direction {
            Direction::Edit => {
                self.undo.push(Entry {
                    forward: pending.preview.sealed().to_owned(),
                    inverse: pending.preview.inverse().to_owned(),
                    before: self.document.revision(),
                    after: reopened.revision(),
                });
                self.redo.clear();
            }
            Direction::Undo => {
                self.redo
                    .push(self.undo.pop().expect("prepared undo entry"));
            }
            Direction::Redo => {
                self.undo
                    .push(self.redo.pop().expect("prepared redo entry"));
            }
        }
        self.document = reopened;
        Ok(outcome)
    }
}

#[derive(Debug)]
pub enum Error {
    Rejected(Vec<Diagnostic>),
    Document(tilewright_editor_document::Error),
    StaleRevision,
    HistoryFull,
    NothingToUndo,
    NothingToRedo,
    NothingPrepared,
    NeedsReload,
    Recovery(String),
    Publish(PublishError),
    /// Publication succeeded, but the reopened world differs from the reviewed
    /// result or no longer validates. Changes were applied; recovery is required.
    PublishedStateChanged {
        published: Published,
        reopen_error: Option<tilewright_editor_document::Error>,
    },
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
            Self::Document(error) => error.fmt(formatter),
            Self::Publish(error) => error.fmt(formatter),
            Self::PublishedStateChanged { published, .. } => write!(
                formatter,
                "the transaction was applied (durable={}), but the source changed while publishing or reopening failed; inspect recovery and reopen",
                published.durable
            ),
            other => write!(formatter, "editor command error: {other:?}"),
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

    #[derive(Default)]
    struct Recovery(Vec<String>);
    impl super::RecoveryStore for Recovery {
        fn retain(&mut self, inverse: &str) -> Result<(), String> {
            self.0.push(inverse.to_owned());
            Ok(())
        }
    }

    #[test]
    fn all_uncertain_publish_outcomes_preserve_recovery_and_block_the_session() {
        for (applied, durable, cleaned) in [
            (false, true, true),
            (false, false, false),
            (true, false, true),
            (true, true, false),
        ] {
            let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
            let revision = document.revision();
            let mut history = super::History::new(document);
            history.prepare(revision, "paint", PAINT).unwrap();
            let inverse = history.pending().unwrap().inverse().to_owned();
            let mut recovery = Recovery::default();
            let result = history.commit_with(&mut recovery, |_| {
                Err(tilewright_edit::PublishError {
                    applied,
                    durable,
                    cleaned,
                    problems: Vec::new(),
                })
            });
            let Err(Error::Publish(error)) = result else {
                panic!("fault was accepted")
            };
            assert_eq!(
                (error.applied, error.durable, error.cleaned),
                (applied, durable, cleaned)
            );
            assert_eq!(recovery.0, [inverse]);
            assert_eq!(history.depths(), (0, 0));
            assert!(history.needs_reload());
            assert!(history.pending().is_some());
            assert!(matches!(
                history.prepare(revision, "paint", PAINT),
                Err(Error::NeedsReload)
            ));
        }
    }

    #[test]
    fn success_without_directory_durability_requires_recovery_too() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let mut history = super::History::new(document);
        history
            .prepare(history.document().revision(), "paint", PAINT)
            .unwrap();
        let result = history.commit_with(&mut Recovery::default(), |_| {
            Ok(tilewright_edit::Published { durable: false })
        });
        assert!(matches!(result, Err(Error::Publish(error)) if error.applied && !error.durable));
        assert!(history.needs_reload());
        assert_eq!(history.depths(), (0, 0));
    }

    #[test]
    fn history_limits_refuse_before_source_or_recovery_changes() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let mut history = super::History::new(document);
        history.undo = vec![
            super::Entry {
                forward: String::new(),
                inverse: String::new(),
                before: history.document().revision(),
                after: history.document().revision()
            };
            super::MAX_HISTORY
        ];
        assert!(matches!(
            history.prepare(history.document().revision(), "paint", PAINT),
            Err(Error::HistoryFull)
        ));
        assert!(history.pending().is_none());
        history.undo.truncate(1);
        history.undo[0].forward = " ".repeat(super::MAX_HISTORY_BYTES);
        assert!(matches!(
            history.prepare(history.document().revision(), "paint", PAINT),
            Err(Error::HistoryFull)
        ));
        assert!(history.pending().is_none());
    }

    #[test]
    fn admission_reserves_the_forward_record_not_just_the_small_inverse() {
        let document = WorldDocument::open(&fixture_world("meadow")).unwrap();
        let mut history = super::History::new(document);
        let text = format!(
            r#"{{"format":"tilewright.transaction/1","operations":[{{"op":"document.create","path":"defs/proto/new.jsonc","text":"{{\"format\":\"tilewright.def/1\",\"type\":\"proto\",\"id\":\"new\",\"fields\":{{}}}}{}"}}]}}"#,
            " ".repeat(8_192)
        );
        let preview = preview(history.document(), "create", &text).unwrap();
        assert!(preview.sealed().len() > preview.inverse().len() * 5);
        history.undo.push(super::Entry {
            forward: " ".repeat(super::MAX_HISTORY_BYTES - preview.inverse().len() - 1),
            inverse: String::new(),
            before: history.document().revision(),
            after: history.document().revision(),
        });
        assert!(matches!(
            history.prepare(history.document().revision(), "create", &text),
            Err(Error::HistoryFull)
        ));
    }

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
