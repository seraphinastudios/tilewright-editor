// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Applying a verified proposal to a person's session, one step at a time.
//!
//! [`Progress`] is plain data: the next step and whether one is prepared.
//! A front end (the desktop, a headless runner) drives it:
//!
//! 1. [`Progress::begin`] checks the strict base: the session is at the
//!    proposal's base revision with nothing pending. [`Progress::resume`]
//!    continues a proposal applied part-way, from the one position of its
//!    chain the session is at.
//! 2. [`Progress::prepare_step`] dispatches the next step's Action through
//!    [`Session::dispatch`], under the person's catalog grants and with the
//!    person's actor (a placement with the uid it recorded), and requires
//!    the transaction it prepares to equal the recorded one in everything
//!    but the actor ([`compare_transactions`]). The step is then pending
//!    for the person's normal Review and Accept.
//! 3. The person accepts or cancels it through the session, as any change.
//!    [`Progress::settle`] (called by `prepare_step` too) then reads the
//!    outcome: applied when the session is at the step's result revision
//!    with exactly the prepared transaction on top of its undo stack; not
//!    applied, retryable, when it is back at the step's base with exactly
//!    that transaction on top of its redo stack (an Accept then Undo);
//!    stopped when it is back at the base otherwise (a Cancel); refused,
//!    retryable, otherwise.
//!
//! Applied steps stay applied and are normal undo entries and recovery
//! records. A refusal never leaves anything pending that this prepared,
//! and only a Cancel stops a proposal.
use tilewright_editor_commands::RecoveryStore;
use tilewright_editor_ui::{Action, AppliedStep, RefusalCode, Session};

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
    /// The session is back at the step's base without the step on top of
    /// its redo stack: the person cancelled it. The proposal is stopped.
    Stopped { step: usize },
    /// The session is back at the step's base with exactly the prepared
    /// transaction on top of its redo stack: the person accepted and undid
    /// it. Not applied; it can be prepared again (or redone, after which
    /// `next()` is behind the world until [`Progress::resume`]).
    NotApplied { step: usize },
    /// A change is pending that is not the prepared step: the person's
    /// own. The step is not pending; settle again once that change is
    /// accepted or cancelled.
    OtherPending { step: usize },
}

/// The prepared step: its index and the exact transaction it prepared.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Prepared {
    step: usize,
    sealed: String,
}

/// How far a proposal has been applied to one session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Progress {
    /// The body digest of the proposal this applies.
    proposal: [u8; 32],
    next: usize,
    prepared: Option<Prepared>,
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
        Ok(Self::at(proposal, session, 0))
    }

    /// Continues applying `proposal` to `session` after an interruption: a
    /// lost `Progress`, a closed editor, a stop. The session must be at
    /// exactly one position of the proposal's chain (its base or a step's
    /// result revision) with nothing pending; the next step is the one
    /// after that position. Every remaining step is still dispatched under
    /// the person's grants and reviewed, so resuming widens nothing. Steps
    /// before it count as applied only because the world is at their
    /// result revision: nothing records who applied them.
    ///
    /// # Errors
    /// Refuses as [`Progress::begin`] does when the session cannot prepare,
    /// with `proposal.base_stale` when its revision is at no position of
    /// the chain, and with `proposal.resume_ambiguous` when it is at more
    /// than one (a chain that returns to a revision it passed).
    pub fn resume(
        proposal: &Proposal,
        session: &Session,
    ) -> Result<(Self, Vec<Notice>), ProposalError> {
        ready(session)?;
        let current = revision(session);
        let positions: Vec<usize> = std::iter::once(proposal.base())
            .chain(proposal.steps().iter().map(crate::ProposalStep::after))
            .enumerate()
            .filter(|(_, at)| *at == current)
            .map(|(index, _)| index)
            .collect();
        match positions.as_slice() {
            [next] => Ok(Self::at(proposal, session, *next)),
            [] => Err(stale("the world is at no revision of the proposal's chain")),
            _ => Err(ProposalError::new(
                RefusalCode::ProposalResumeAmbiguous,
                format!(
                    "the world's revision is at {} positions of the proposal's chain",
                    positions.len()
                ),
            )),
        }
    }

    fn at(proposal: &Proposal, session: &Session, next: usize) -> (Self, Vec<Notice>) {
        let mut notices = Vec::new();
        let catalog = session.catalog().revision();
        if catalog != proposal.catalog() {
            notices.push(Notice::CatalogDiffers {
                proposal: proposal.catalog(),
                session: catalog,
            });
        }
        (
            Self {
                proposal: proposal.body_sha256(),
                next,
                prepared: None,
                stopped: false,
            },
            notices,
        )
    }

    /// The body digest of the proposal this applies, for the front end to
    /// record beside each applied step (the desktop's evidence does, PR
    /// 6d).
    pub fn proposal(&self) -> [u8; 32] {
        self.proposal
    }

    /// The index of the next step to prepare: how many steps were settled
    /// as applied. A settled step the person later undoes leaves this ahead
    /// of the world; preparing is then refused with `proposal.base_stale`
    /// until a Redo (or [`Progress::resume`]).
    pub fn next(&self) -> usize {
        self.next
    }
    /// The step prepared for review and not yet settled, if any.
    pub fn prepared(&self) -> Option<usize> {
        self.prepared.as_ref().map(|prepared| prepared.step)
    }
    /// Whether the person stopped the proposal by cancelling a step.
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
        match self.settle(proposal, session)? {
            Settled::Stopped { .. } => return Err(closed("the proposal was stopped")),
            Settled::Pending { step } => {
                return Err(ProposalError::new(
                    RefusalCode::PendingExists,
                    "the prepared step is pending: Accept or Cancel it first",
                )
                .at(step));
            }
            Settled::OtherPending { .. }
            | Settled::Idle
            | Settled::Applied { .. }
            | Settled::NotApplied { .. } => {}
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
            // The engine leaves nothing pending when it refuses; should that
            // ever change, cancel what this dispatch left (nothing was
            // pending before it). Defensive: no engine path reaches it today.
            let left = session.history().pending().is_some();
            if left {
                let _ = session.dispatch(Action::Cancel, &mut NoRecovery);
            }
            return Err(ProposalError::new(
                RefusalCode::ProposalStepRefused,
                if left {
                    format!(
                        "the engine refused the step ({}) and its change was cancelled",
                        error.code()
                    )
                } else {
                    format!("the engine refused the step ({})", error.code())
                },
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
        self.prepared = Some(Prepared {
            step: index,
            sealed: prepared,
        });
        Ok(index)
    }

    /// Reads the outcome of the prepared step from the session.
    ///
    /// # Errors
    /// Refuses with `proposal.base_stale` when the session is neither at
    /// the step's result with exactly the prepared transaction on top of
    /// its undo stack nor back at the step's base (for example the person
    /// accepted the step and then an edit of their own). Nothing is
    /// stopped: once the session is at the step's result with the step on
    /// top again (the person undid their edit), `settle` reports it
    /// applied.
    ///
    /// A `Progress` belongs to one session: it reads that session's undo
    /// and redo stacks. After the session is reopened (a new session has
    /// empty stacks), make a new `Progress` with [`Progress::resume`]. An
    /// old one would read an accepted step as `proposal.base_stale` for
    /// good, and a lost pending step as `Stopped`.
    pub fn settle(
        &mut self,
        proposal: &Proposal,
        session: &Session,
    ) -> Result<Settled, ProposalError> {
        self.check(proposal)?;
        let Some(prepared) = &self.prepared else {
            return Ok(Settled::Idle);
        };
        let index = prepared.step;
        if let Some(pending) = session.history().pending() {
            return Ok(if pending.sealed() == prepared.sealed {
                Settled::Pending { step: index }
            } else {
                Settled::OtherPending { step: index }
            });
        }
        let step = &proposal.steps()[index];
        let current = revision(session);
        let applied = on_top(session.applied_steps().last(), step, &prepared.sealed);
        if current == step.after() && applied {
            self.prepared = None;
            self.next = index + 1;
            Ok(Settled::Applied { step: index })
        } else if current == step.before() {
            // An Accept then Undo leaves this very change on top of the redo
            // stack; anything else at the base is a Cancel.
            let cancelled = !on_top(session.undone_step(), step, &prepared.sealed);
            self.prepared = None;
            if cancelled {
                self.stopped = true;
                Ok(Settled::Stopped { step: index })
            } else {
                Ok(Settled::NotApplied { step: index })
            }
        } else {
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

/// Whether `applied`, the top of the undo (or redo) stack, is the very
/// change this prepared for `step`, as the person accepted it.
fn on_top(applied: Option<AppliedStep<'_>>, step: &crate::ProposalStep, sealed: &str) -> bool {
    applied.is_some_and(|applied| {
        applied.before == step.before()
            && applied.after == step.after()
            && applied.transaction == sealed
    })
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

#[cfg(test)]
mod tests;
