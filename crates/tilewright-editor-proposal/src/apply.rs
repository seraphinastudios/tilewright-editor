// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Applying a verified proposal to a person's session, one step at a time.
//!
//! [`Progress`] is plain data: the next step and whether one is prepared.
//! A front end (the desktop, a headless runner) drives it:
//!
//! 1. [`Progress::begin`] checks the strict base: the session is at the
//!    proposal's base revision with nothing pending.
//! 2. [`Progress::prepare_step`] dispatches the next step's Action through
//!    [`Session::dispatch`], under the person's catalog grants and with the
//!    person's actor (a placement with the uid it recorded), and requires
//!    the transaction it prepares to equal the recorded one in everything
//!    but the actor ([`compare_transactions`]). The step is then pending
//!    for the person's normal Review and Accept.
//! 3. The person accepts or cancels it through the session, as any change.
//!    [`Progress::settle`] (called by `prepare_step` too) then reads the
//!    outcome: accepted when the session is at the step's result revision
//!    with this step on top of its undo stack, stopped when it is back at
//!    the step's base (Cancel), and refused otherwise.
//!
//! Applied steps stay applied and are normal undo entries and recovery
//! records. A refusal never leaves anything pending that this prepared.
use tilewright_editor_commands::RecoveryStore;
use tilewright_editor_ui::{Action, RefusalCode, Session};

use crate::{Proposal, ProposalError, compare::compare_transactions, hex};

/// Something a person should know before applying a proposal; it refuses
/// nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    /// The person's editor catalog differs from the one the steps were
    /// made under. Every step is still dispatched under the person's own
    /// grants, so a step the person's catalog does not grant is refused.
    CatalogDiffers {
        proposal: [u8; 32],
        session: [u8; 32],
    },
}

/// What [`Progress::settle`] found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Settled {
    /// No step is prepared.
    Idle,
    /// The prepared step is still pending review.
    Pending { step: usize },
    /// The person accepted the step.
    Applied { step: usize },
    /// The person cancelled the step: the proposal is stopped.
    Stopped { step: usize },
}

/// How far a proposal has been applied to one session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Progress {
    /// The body digest of the proposal this applies.
    proposal: [u8; 32],
    next: usize,
    prepared: Option<usize>,
    stopped: bool,
}

impl Progress {
    /// Begins applying `proposal` to `session`.
    ///
    /// # Errors
    /// Refuses with `session.blocked` or `pending.exists` when the session
    /// cannot prepare a change, and with `proposal.base_stale` unless the
    /// session is at the proposal's base revision (another world, a world
    /// that moved on, or a proposal already applied).
    pub fn begin(
        proposal: &Proposal,
        session: &Session,
    ) -> Result<(Self, Vec<Notice>), ProposalError> {
        ready(session)?;
        if revision(session) != proposal.base() {
            return Err(stale("the world is not at the proposal's base revision"));
        }
        let mut notices = Vec::new();
        let catalog = session.catalog().revision();
        if catalog != proposal.catalog() {
            notices.push(Notice::CatalogDiffers {
                proposal: proposal.catalog(),
                session: catalog,
            });
        }
        Ok((
            Self {
                proposal: proposal.body_sha256(),
                next: 0,
                prepared: None,
                stopped: false,
            },
            notices,
        ))
    }

    /// The index of the next step to prepare, which is also how many
    /// steps are applied.
    pub fn next(&self) -> usize {
        self.next
    }
    /// The step prepared and pending review, if any.
    pub fn prepared(&self) -> Option<usize> {
        self.prepared
    }
    /// Whether the person stopped the proposal by cancelling a step, or a
    /// step's outcome could not be read.
    pub fn stopped(&self) -> bool {
        self.stopped
    }
    /// Whether every step is applied.
    pub fn finished(&self, proposal: &Proposal) -> bool {
        self.next == proposal.steps().len()
    }

    /// Prepares the next step for review; returns its index. Settles a
    /// step prepared earlier first.
    ///
    /// # Errors
    /// Refuses with `proposal.closed` when stopped or finished,
    /// `pending.exists` or `session.blocked` when the session cannot
    /// prepare, `proposal.base_stale` unless the session is at the step's
    /// base revision, `proposal.step_refused` (with the engine's code)
    /// when the engine refuses the Action, and `proposal.step_diverged`
    /// when it prepares another transaction than the recorded one, which
    /// is cancelled. After a refusal nothing this prepared is pending and
    /// the step can be tried again.
    pub fn prepare_step(
        &mut self,
        proposal: &Proposal,
        session: &mut Session,
        recovery: &mut impl RecoveryStore,
    ) -> Result<usize, ProposalError> {
        self.check(proposal)?;
        if let Settled::Stopped { .. } = self.settle(proposal, session)? {
            return Err(closed("the proposal was stopped"));
        }
        if self.stopped {
            return Err(closed("the proposal was stopped"));
        }
        let index = self.next;
        let Some(step) = proposal.steps().get(index) else {
            return Err(closed("every step of the proposal is applied"));
        };
        ready(session).map_err(|error| error.at(index))?;
        if revision(session) != step.before() {
            return Err(stale("the world is not at the revision this step starts from").at(index));
        }
        let action = step.action().clone();
        let dispatched = match (step.placed(), &action) {
            (Some(uid), Action::Place { .. }) => session.dispatch_placing(action, uid, recovery),
            _ => session.dispatch(action, recovery),
        };
        if let Err(error) = dispatched {
            // A refused preparation leaves nothing pending.
            debug_assert!(session.history().pending().is_none());
            return Err(ProposalError::new(
                RefusalCode::ProposalStepRefused,
                format!("the engine refused the step ({})", error.code()),
            )
            .engine(error.code())
            .at(index));
        }
        let prepared = session
            .history()
            .pending()
            .map(|pending| pending.sealed().to_owned())
            .unwrap_or_default();
        if let Err(divergence) = compare_transactions(step.transaction(), &prepared) {
            // Only ever cancel the change this dispatch prepared.
            let _ = session.dispatch(Action::Cancel, &mut NoRecovery);
            return Err(ProposalError::new(
                RefusalCode::ProposalStepDiverged,
                format!("the step prepared another transaction than it recorded and was cancelled: {divergence}"),
            )
            .at(index));
        }
        self.prepared = Some(index);
        Ok(index)
    }

    /// Reads the outcome of the prepared step from the session.
    ///
    /// # Errors
    /// Refuses with `proposal.base_stale`, and stops the proposal, when the
    /// session is neither at the step's result with the step on top of its
    /// undo stack nor back at its base with nothing pending.
    pub fn settle(
        &mut self,
        proposal: &Proposal,
        session: &Session,
    ) -> Result<Settled, ProposalError> {
        self.check(proposal)?;
        let Some(index) = self.prepared else {
            return Ok(Settled::Idle);
        };
        if session.history().pending().is_some() {
            return Ok(Settled::Pending { step: index });
        }
        self.prepared = None;
        let step = &proposal.steps()[index];
        let current = revision(session);
        let on_top = session.applied_steps().last().is_some_and(|applied| {
            applied.before == step.before()
                && applied.after == step.after()
                && compare_transactions(step.transaction(), applied.transaction).is_ok()
        });
        if current == step.after() && on_top {
            self.next = index + 1;
            Ok(Settled::Applied { step: index })
        } else if current == step.before() {
            self.stopped = true;
            Ok(Settled::Stopped { step: index })
        } else {
            self.stopped = true;
            Err(stale(format!(
                "after the step the world is at revision {} rather than its result",
                hex(&current)
            ))
            .at(index))
        }
    }

    fn check(&self, proposal: &Proposal) -> Result<(), ProposalError> {
        if self.proposal != proposal.body_sha256() {
            return Err(crate::invalid("this progress belongs to another proposal"));
        }
        Ok(())
    }
}

fn ready(session: &Session) -> Result<(), ProposalError> {
    if session.can_prepare() {
        Ok(())
    } else if session.history().pending().is_some() {
        Err(ProposalError::new(
            RefusalCode::PendingExists,
            "a change is pending: Accept or Cancel it first",
        ))
    } else {
        Err(ProposalError::new(
            RefusalCode::SessionBlocked,
            "the session cannot prepare a change",
        ))
    }
}
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}
fn stale(message: impl Into<String>) -> ProposalError {
    ProposalError::new(RefusalCode::ProposalBaseStale, message)
}
fn closed(message: &str) -> ProposalError {
    ProposalError::new(RefusalCode::ProposalClosed, message)
}

/// Cancel retains nothing; a store that refuses makes that certain.
struct NoRecovery;
impl RecoveryStore for NoRecovery {
    fn retain(&mut self, _: &str) -> Result<(), String> {
        Err("cancelling retains nothing".into())
    }
}
