// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A proposal the person opened (`--proposal-fd`): its steps offered one at
//! a time through the normal Review and Accept (docs/PROPOSALS.md, "Applying
//! in the desktop").
//!
//! The person's actions all go through `Application::action`'s funnel;
//! preparing a step goes through the same funnel, and after every action the
//! [`Progress`] is settled. Nothing here dispatches Accept: a step is applied
//! only by the person's own Accept, and the Review shows the session's own
//! review of the freshly prepared change, never recorded transaction text.
//! What the panel says about a step comes from its decoded Action and from
//! the person's current world, never from the recorded transaction.
use super::{Application, Result};
use crate::workspace_ui::Control;
use tilewright_editor_document::revision_hex;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_editor_proposal::{Notice, Progress, Proposal, Settled};
use tilewright_editor_ui::{Action, PortalTarget, RefusalCode, Session, SessionPhase};
use tilewright_source::{SourceRoot, World};

/// The format of the evidence record written when a proposal is opened.
pub(super) const OPEN_FORMAT: &str = "tilewright.prototype-proposal/1";
/// The format of the evidence record written for each applied step.
pub(super) const STEP_FORMAT: &str = "tilewright.prototype-proposal-step/1";
/// The most characters of an identifier, name or value shown in a step's
/// words; longer ones are cut with an ellipsis.
pub(super) const TEXT_CHARACTERS: usize = 48;
/// The most characters of one panel row before it is laid out (the layout
/// then fits it to the sidebar's width).
pub(super) const ROW_CHARACTERS: usize = 160;
/// The most rows a step's words or the state line wrap to.
const WRAPPED_ROWS: usize = 3;
/// Panel rows: compact, in the smaller type.
pub(super) const ROW_HEIGHT: u32 = 20;
const ROW_GAP: u32 = 2;
/// The control row's height.
const BUTTON_HEIGHT: u32 = 26;
/// This editor's version, as a proposal's `editor` member records one.
pub(super) const EDITOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An open proposal and how far it is applied to this session.
pub(super) struct OpenProposal {
    pub(super) proposal: Proposal,
    /// The file name it was read as.
    pub(super) name: String,
    /// `None` when the proposal does not apply to this world.
    pub(super) progress: Option<Progress>,
    pub(super) notices: Vec<Notice>,
    /// How it was opened: begun at the base, resumed after a step, or why it
    /// does not apply.
    pub(super) opened: Opened,
    /// The last refusal or undo to show, until something else happens.
    pub(super) last: Last,
    /// The sequence number of the last step record written.
    pub(super) records: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Opened {
    Begun,
    /// Resumed with this many steps already applied (the world was at
    /// their result).
    Resumed(usize),
    /// Neither begun nor resumed: the engine's code.
    DoesNotApply(RefusalCode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Last {
    Nothing,
    /// Preparing step `step` (from 0) was refused.
    Refused {
        step: usize,
        code: RefusalCode,
        engine: Option<RefusalCode>,
    },
    /// Step `step` was accepted and undone.
    Undone {
        step: usize,
    },
}

/// What the panel shows: derived from the progress and the session each
/// time the interface is laid out, so it never goes stale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum State {
    /// The session or the rendering is blocked.
    Blocked,
    DoesNotApply,
    /// Stopped by a Cancel; `resume` is the next step a resume would take.
    Stopped {
        resume: Option<usize>,
    },
    Finished,
    /// Step `step` is pending and matches its recording.
    Pending {
        step: usize,
    },
    /// The person's own change is pending.
    OtherPending,
    Refused {
        step: usize,
        code: RefusalCode,
        engine: Option<RefusalCode>,
    },
    Undone {
        step: usize,
    },
    Ready {
        step: usize,
    },
    /// The world is not where the next step starts; `resume` is the next
    /// step a resume would take when the world is at one position.
    Moved {
        resume: Option<usize>,
    },
}

impl State {
    /// A stable key for the frame evidence.
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::DoesNotApply => "does_not_apply",
            Self::Stopped { .. } => "stopped",
            Self::Finished => "finished",
            Self::Pending { .. } => "pending",
            Self::OtherPending => "other_pending",
            Self::Refused { .. } => "refused",
            Self::Undone { .. } => "undone",
            Self::Ready { .. } => "ready",
            Self::Moved { .. } => "moved",
        }
    }
}

impl OpenProposal {
    /// Opens `proposal` on `session`: begun at its base, else resumed at the
    /// one position of its chain the world is at, else shown as not
    /// applying.
    pub(super) fn open(proposal: Proposal, name: String, session: &Session) -> Self {
        let (progress, notices, opened) = match Progress::begin(&proposal, session) {
            Ok((progress, notices)) => (Some(progress), notices, Opened::Begun),
            Err(error) if error.code() == RefusalCode::ProposalBaseStale => {
                match Progress::resume(&proposal, session) {
                    Ok((progress, notices)) => {
                        let next = progress.next();
                        (Some(progress), notices, Opened::Resumed(next))
                    }
                    Err(error) => (None, Vec::new(), Opened::DoesNotApply(error.code())),
                }
            }
            Err(error) => (None, Vec::new(), Opened::DoesNotApply(error.code())),
        };
        Self {
            proposal,
            name,
            progress,
            notices,
            opened,
            last: Last::Nothing,
            records: 0,
        }
    }

    fn steps(&self) -> usize {
        self.proposal.steps().len()
    }

    /// The positions of the proposal's chain the session's world is at: 0
    /// for its base, `j` for the result of step `j` (counting from 1).
    pub(super) fn positions(&self, session: &Session) -> Vec<usize> {
        let current = session.history().document().revision();
        std::iter::once(self.proposal.base())
            .chain(
                self.proposal
                    .steps()
                    .iter()
                    .map(tilewright_editor_proposal::ProposalStep::after),
            )
            .enumerate()
            .filter(|(_, at)| *at == current)
            .map(|(index, _)| index)
            .collect()
    }

    fn position(&self, session: &Session) -> Option<usize> {
        match self.positions(session).as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }

    /// What the panel shows now.
    pub(super) fn state(&self, session: &Session, blocked: bool) -> State {
        if blocked || session.status().phase == SessionPhase::Blocked {
            return State::Blocked;
        }
        let Some(progress) = &self.progress else {
            return State::DoesNotApply;
        };
        // Read the prepared step without settling the real progress: the
        // funnel settles that after every action.
        match progress.clone().settle(&self.proposal, session) {
            Ok(Settled::Pending { step }) => return State::Pending { step },
            Ok(Settled::OtherPending { .. }) => return State::OtherPending,
            Err(_) => {
                return State::Moved {
                    resume: self.position(session),
                };
            }
            Ok(_) => {}
        }
        let pending = session.history().pending().is_some();
        if progress.stopped() {
            return State::Stopped {
                resume: self.position(session).filter(|_| !pending),
            };
        }
        if pending {
            return State::OtherPending;
        }
        let next = progress.next();
        let expected = self.proposal.steps().get(next).map_or(
            self.proposal.result(),
            tilewright_editor_proposal::ProposalStep::before,
        );
        if session.history().document().revision() != expected {
            return match self.position(session) {
                // The person undid applied steps: the first undone one can
                // be prepared again (a Redo continues too).
                Some(position) if position < next => State::Undone { step: position },
                // Every step applied, and the person edited after them.
                None if next == self.steps() => State::Finished,
                resume => State::Moved { resume },
            };
        }
        if next == self.steps() {
            return State::Finished;
        }
        match self.last {
            Last::Refused { step, code, engine } if step == next => {
                State::Refused { step, code, engine }
            }
            Last::Undone { step } if step == next => State::Undone { step },
            _ => State::Ready { step: next },
        }
    }

    /// The panel's one control: its label, the control and whether it is
    /// enabled; `None` when there is nothing to do.
    pub(super) fn control(&self, state: State, ready: bool) -> Option<(String, Control, bool)> {
        let steps = self.steps();
        match state {
            State::Ready { step } | State::Undone { step } | State::Refused { step, .. } => Some((
                format!("Prepare step {} (G)", step + 1),
                Control::ProposalPrepare,
                ready,
            )),
            State::Moved {
                resume: Some(resume),
            } => Some((
                if resume < steps {
                    format!("Continue from step {} (G)", resume + 1)
                } else {
                    format!("Continue: all {steps} steps applied (G)")
                },
                Control::ProposalContinue,
                ready,
            )),
            State::Stopped {
                resume: Some(resume),
            } => Some((
                if resume < steps {
                    format!("Resume at step {} (G)", resume + 1)
                } else {
                    format!("Resume: all {steps} steps applied (G)")
                },
                Control::ProposalContinue,
                ready,
            )),
            _ => None,
        }
    }

    /// The state line, in words.
    pub(super) fn state_words(&self, state: State) -> String {
        let steps = self.steps();
        match state {
            State::Blocked => "This session cannot continue (blocked).".into(),
            State::DoesNotApply => "Does not apply to this world; nothing can be prepared.".into(),
            State::Stopped { resume: Some(_) } => "Stopped by Cancel. Resume continues it.".into(),
            State::Stopped { resume: None } => {
                "Stopped by Cancel; the world is not on its path.".into()
            }
            State::Finished => format!("All {steps} steps applied."),
            State::Pending { step } => format!(
                "Step {} pending: matches its recording. Accept or Cancel below.",
                step + 1
            ),
            State::OtherPending => "Your own change is pending: the proposal waits.".into(),
            State::Refused {
                code: RefusalCode::ProposalStepDiverged,
                ..
            } => format!(
                "Diverged: the step prepared another change than recorded (recorded by editor {}). Nothing changed.",
                shown(self.proposal.editor())
            ),
            State::Refused { code, engine, .. } => match engine {
                Some(engine) => format!("Refused: {code} ({engine}). Nothing changed."),
                None => format!("Refused: {code}. Nothing changed."),
            },
            State::Undone { step } => {
                format!("Step {} undone: prepare it again (or Redo).", step + 1)
            }
            State::Ready { .. } => "Ready to prepare.".into(),
            State::Moved {
                resume: Some(resume),
            } if resume < steps => format!(
                "The world moved: undo your edits or continue from step {}.",
                resume + 1
            ),
            State::Moved { resume: Some(_) } => {
                "The world moved: every step is in it; continue to finish.".into()
            }
            State::Moved { resume: None } => {
                "The world moved: undo your own edits to continue.".into()
            }
        }
    }

    /// Where the world is on the proposal's path.
    pub(super) fn where_words(&self, session: &Session) -> String {
        let steps = self.steps();
        match self.positions(session).as_slice() {
            [] => "World: not on its path".into(),
            [0] => "World: at its base".into(),
            [one] => format!("World: after step {one} of {steps}"),
            several => format!("World: at {} places on its path", several.len()),
        }
    }

    /// The step the panel offers, in words: step `step` (from 0) of the
    /// proposal, described from its Action and the person's world.
    pub(super) fn step_words(
        &self,
        step: usize,
        world: &World,
        catalog: &PreviewCatalog,
    ) -> String {
        let Some(recorded) = self.proposal.steps().get(step) else {
            return String::new();
        };
        format!(
            "Step {} of {}: {}",
            step + 1,
            self.steps(),
            action_words(recorded.action(), world, catalog)
        )
    }

    /// The panel's rows, before they are wrapped to the sidebar's width.
    pub(super) fn rows(&self, session: &Session, blocked: bool) -> Vec<String> {
        let proposal = &self.proposal;
        let state = self.state(session, blocked);
        let steps = self.steps();
        let mut rows = vec![
            format!(
                "Proposal {} by {}",
                &revision_hex(&proposal.body_sha256())[..8],
                shown(proposal.actor().as_str())
            ),
            proposal.client().map_or_else(
                || "Client label: none".into(),
                |label| format!("Client label (unverified): {}", shown(label)),
            ),
            if proposal.editor() == EDITOR_VERSION {
                format!(
                    "Recorded by editor {} (this editor too)",
                    shown(proposal.editor())
                )
            } else {
                format!(
                    "Recorded by editor {} (this editor {EDITOR_VERSION}): may diverge",
                    shown(proposal.editor())
                )
            },
        ];
        if let Opened::DoesNotApply(code) = self.opened {
            rows.push(format!("Does not apply here ({code})"));
            rows.push(format!(
                "Its base: {}",
                &revision_hex(&proposal.base())[..16]
            ));
            rows.push(format!(
                "This world: {}",
                &revision_hex(&session.history().document().revision())[..16]
            ));
        } else {
            rows.push(self.where_words(session));
        }
        let next = self.progress.as_ref().map_or(0, Progress::next);
        let current = match state {
            State::Pending { step } | State::Refused { step, .. } | State::Undone { step } => {
                Some(step)
            }
            State::Ready { step } => Some(step),
            State::Stopped {
                resume: Some(resume),
            }
            | State::Moved {
                resume: Some(resume),
            } => Some(resume),
            State::Finished | State::Blocked | State::DoesNotApply => None,
            _ => Some(next),
        };
        if let Some(step) = current.filter(|step| *step < steps) {
            // The committed world: a pending step is not yet in it.
            rows.push(self.step_words(
                step,
                session.history().document().world(),
                session.catalog(),
            ));
        }
        rows.push(self.state_words(state));
        if self.progress.is_some() {
            rows.push(format!(
                "Applied {next} of {steps}, remaining {}",
                steps.saturating_sub(next)
            ));
        }
        for notice in &self.notices {
            match notice {
                Notice::CatalogDiffers { .. } => {
                    rows.push("Catalog differs from the recording: your grants apply".into());
                }
            }
        }
        rows.into_iter()
            .map(|text| clipped(&text, ROW_CHARACTERS))
            .collect()
    }

    /// The `proposal-open.json` evidence record.
    pub(super) fn open_record(&self, session: &Session) -> serde_json::Value {
        let proposal = &self.proposal;
        let opened = match self.opened {
            Opened::Begun => serde_json::json!({"begun": true}),
            Opened::Resumed(next) => serde_json::json!({"resumed_after": next}),
            Opened::DoesNotApply(code) => serde_json::json!({
                "applies": false,
                "code": code.to_string(),
                "world": revision_hex(&session.history().document().revision()),
            }),
        };
        let notices: Vec<_> = self
            .notices
            .iter()
            .map(|notice| match notice {
                Notice::CatalogDiffers {
                    proposal,
                    session: own,
                } => serde_json::json!({"catalog_differs": {
                    "proposal": revision_hex(proposal), "session": revision_hex(own)}}),
            })
            .collect();
        serde_json::json!({
            "format": OPEN_FORMAT,
            "proposal": revision_hex(&proposal.body_sha256()),
            "file": self.name,
            "editor": shown(proposal.editor()),
            "this_editor": EDITOR_VERSION,
            "agent": shown(proposal.actor().as_str()),
            "client": proposal.client().map(shown),
            "person": shown(session.actor().as_str()),
            "base": revision_hex(&proposal.base()),
            "result": revision_hex(&proposal.result()),
            "catalog": revision_hex(&proposal.catalog()),
            "steps": proposal.steps().len(),
            "notices": notices,
            "opened": opened,
        })
    }
}

/// Reads the proposal to open through its provisioned directory, bounded
/// and checked; a refusal is a start-up error naming the proposal's stable
/// code. `native_demo` refuses a proposal in the windowed scripted run.
pub(super) fn read(
    root: Option<&(std::path::PathBuf, String)>,
    native_demo: bool,
) -> Result<Option<(Proposal, String)>> {
    let Some((root, name)) = root else {
        return Ok(None);
    };
    if native_demo {
        return Err(
            "with a proposal, the scripted run is reference evidence only: add --reference".into(),
        );
    }
    let proposal = tilewright_editor_proposal::read_proposal(&SourceRoot::new(root), name)
        .map_err(|error| format!("proposal refused: {}: {error}", error.code()))?;
    Ok(Some((proposal, name.clone())))
}

impl Application {
    /// Opens the proposal read at start-up and writes `proposal-open.json`.
    pub(super) fn open_proposal(&mut self, proposal: Option<(Proposal, String)>) -> Result<()> {
        let Some((proposal, name)) = proposal else {
            return Ok(());
        };
        let open = OpenProposal::open(proposal, name, &self.session);
        let record = serde_json::to_string_pretty(&open.open_record(&self.session))
            .map_err(|error| error.to_string())?;
        self.evidence
            .retain("proposal-open.json", record.as_bytes())?;
        self.proposal = Some(open);
        Ok(())
    }

    /// Prepares the proposal's next step through the same funnel as every
    /// other edit. The step is then pending for the person's Review; this
    /// never accepts it.
    pub(super) fn prepare_proposal_step(&mut self) -> Result<()> {
        if let Some(reason) = &self.blocked {
            return Err(format!("rendering blocked editing: {reason}"));
        }
        self.funnel(false, |app| {
            let Some(open) = app.proposal.as_mut() else {
                return Err("no proposal is open".into());
            };
            let Some(progress) = open.progress.as_mut() else {
                return Err("the proposal does not apply to this world".into());
            };
            match progress.prepare_step(&open.proposal, &mut app.session, &mut app.recovery) {
                Ok(_) => {
                    open.last = Last::Nothing;
                    Ok(())
                }
                Err(error) => {
                    // A refusal of the step itself is shown until something
                    // else happens; the others (a world that moved, a change
                    // pending) the panel reads from the session.
                    if matches!(
                        error.code(),
                        RefusalCode::ProposalStepRefused | RefusalCode::ProposalStepDiverged
                    ) {
                        open.last = Last::Refused {
                            step: error.step().unwrap_or_else(|| progress.next()),
                            code: error.code(),
                            engine: error.engine_code(),
                        };
                    }
                    Err(format!("proposal {}: {error}", error.code()))
                }
            }
        })
    }

    /// Continues the proposal from where the world is: a new [`Progress`]
    /// from `resume`, after the person's own edits, an undo of a settled
    /// step or a Cancel. It prepares nothing.
    pub(super) fn continue_proposal(&mut self) -> Result<()> {
        if let Some(reason) = &self.blocked {
            return Err(format!("rendering blocked editing: {reason}"));
        }
        let Some(open) = self.proposal.as_mut() else {
            return Err("no proposal is open".into());
        };
        if open.progress.is_none() {
            return Err("the proposal does not apply to this world".into());
        }
        let (progress, notices) = Progress::resume(&open.proposal, &self.session)
            .map_err(|error| format!("proposal {}: {error}", error.code()))?;
        open.progress = Some(progress);
        open.notices = notices;
        open.last = Last::Nothing;
        self.presented_generation = None;
        Ok(())
    }

    /// The panel's control, when it is pressed or its key is used.
    pub(super) fn proposal_control(&mut self) -> Result<()> {
        let Some(open) = &self.proposal else {
            return Ok(());
        };
        let state = open.state(&self.session, self.blocked.is_some());
        let ready = self.blocked.is_none() && self.session.can_prepare();
        match open.control(state, ready) {
            Some((_, Control::ProposalPrepare, true)) => {
                // A step the person undid after it was settled: the progress
                // is ahead of the world until it resumes where the world is.
                let behind = matches!(state, State::Undone { step }
                    if open.progress.as_ref().is_some_and(|progress| progress.next() != step));
                if behind {
                    self.continue_proposal()?;
                }
                self.prepare_proposal_step()
            }
            Some((_, Control::ProposalContinue, true)) => self.continue_proposal(),
            _ => Err("nothing of the proposal can be prepared now".into()),
        }
    }

    /// Settles the progress after an action and writes a step record when
    /// the person's Accept applied a step. `accepted` holds the number of
    /// recovery records before the action, when the action was an Accept.
    pub(super) fn settle_proposal(&mut self, accepted: Option<usize>) -> Result<()> {
        let Some(open) = self.proposal.as_mut() else {
            return Ok(());
        };
        let Some(progress) = open.progress.as_mut() else {
            return Ok(());
        };
        match progress.settle(&open.proposal, &self.session) {
            Ok(Settled::Applied { step }) => {
                open.last = Last::Nothing;
                open.records += 1;
                let sequence = open.records;
                let record = self.step_record(step, sequence, accepted);
                self.evidence.retain(
                    &format!("proposal-step-{sequence:06}.json"),
                    record.as_bytes(),
                )
            }
            Ok(Settled::NotApplied { step }) => {
                open.last = Last::Undone { step };
                Ok(())
            }
            Ok(Settled::Stopped { .. }) => {
                open.last = Last::Nothing;
                Ok(())
            }
            // Pending, the person's own change, nothing prepared, or a world
            // that moved: the panel reads these from the session.
            Ok(_) | Err(_) => Ok(()),
        }
    }

    /// The `proposal-step-<sequence>.json` record of applied step `step`.
    fn step_record(&self, step: usize, sequence: u64, accepted: Option<usize>) -> String {
        let Some(open) = &self.proposal else {
            return String::new();
        };
        let recorded = &open.proposal.steps()[step];
        let applied = self.session.applied_steps().last();
        // The record the person's Accept retained, when this Accept is
        // what applied the step; a step found applied later (after the
        // person undid an edit of their own) names none.
        let recovery = accepted
            .filter(|before| self.recovery.records().len() == before + 1)
            .and_then(|_| self.recovery.records().last())
            .map(|record| {
                serde_json::json!({"name": record.name, "sha256": revision_hex(&record.sha256)})
            });
        let record = serde_json::json!({
            "format": STEP_FORMAT,
            "sequence": sequence,
            "proposal": revision_hex(&open.proposal.body_sha256()),
            "file": open.name,
            "step": step + 1,
            "steps": open.proposal.steps().len(),
            "actor": shown(self.session.actor().as_str()),
            "recorded_transaction_sha256": revision_hex(&recorded.transaction_sha256()),
            "applied_transaction_sha256": applied.map(|applied| revision_hex(&sha256(applied.transaction))),
            "before": revision_hex(&recorded.before()),
            "after": revision_hex(&recorded.after()),
            "recovery": recovery,
        });
        serde_json::to_string_pretty(&record).unwrap_or_default()
    }

    /// The Review's status line while the pending change is the proposal's
    /// step; `None` otherwise (the person's own change keeps today's).
    pub(super) fn proposal_review_status(&self) -> Option<String> {
        let open = self.proposal.as_ref()?;
        match open.state(&self.session, self.blocked.is_some()) {
            State::Pending { step } => Some(format!(
                "Proposal step {} of {} by {}: matches its recording. Accept applies it; Cancel stops the proposal.",
                step + 1,
                open.proposal.steps().len(),
                shown(open.proposal.actor().as_str())
            )),
            _ => None,
        }
    }

    /// The proposal block at the top of the sidebar, from logical `y`;
    /// returns the next free row. Every row goes through the ordinary
    /// fit-and-clip path when the interface is laid out.
    pub(super) fn proposal_panel(
        &mut self,
        ui: &mut crate::workspace_ui::WorkspaceUi,
        mut y: u32,
    ) -> u32 {
        let Some(open) = &self.proposal else {
            return y;
        };
        let x = ui.sidebar_x();
        let width = ui.sidebar_width();
        let blocked = self.blocked.is_some();
        let rows = open.rows(&self.session, blocked);
        let state = open.state(&self.session, blocked);
        let ready = !blocked && self.session.can_prepare();
        let control = open.control(state, ready);
        for text in rows {
            for line in wrapped(&mut self.text, &text, width, ROW_HEIGHT) {
                ui.widget(
                    crate::workspace_ui::Rect {
                        x,
                        y,
                        width,
                        height: ROW_HEIGHT,
                    },
                    line,
                    None,
                    true,
                    false,
                );
                y += ROW_HEIGHT + ROW_GAP;
            }
        }
        if let Some((label, control, enabled)) = control {
            ui.widget(
                crate::workspace_ui::Rect {
                    x,
                    y,
                    width,
                    height: BUTTON_HEIGHT,
                },
                label,
                Some(control),
                enabled,
                false,
            );
            y += BUTTON_HEIGHT + ROW_GAP;
        }
        y + 6
    }

    /// The frame evidence's `proposal` member, while a proposal is open.
    pub(super) fn proposal_frame(&self) -> Option<serde_json::Value> {
        let open = self.proposal.as_ref()?;
        let state = open.state(&self.session, self.blocked.is_some());
        Some(serde_json::json!({
            "proposal": revision_hex(&open.proposal.body_sha256()),
            "steps": open.proposal.steps().len(),
            "applied": open.progress.as_ref().map(Progress::next),
            "state": state.key(),
            "pending_step": matches!(state, State::Pending { .. }),
        }))
    }

    /// The scripted reference run with a proposal: the panel when opened,
    /// then each step prepared, captured pending, accepted as the scripted
    /// demonstration accepts its own edits, and captured applied. It runs
    /// only with `--reference --demo-script`, which never opens a window.
    pub(super) fn proposal_demo(&mut self) -> Result<()> {
        self.capture("proposal-opened")?;
        loop {
            let Some(open) = &self.proposal else {
                return Ok(());
            };
            let state = open.state(&self.session, self.blocked.is_some());
            let step = match state {
                State::Ready { step } => step,
                State::Finished | State::DoesNotApply => return Ok(()),
                other => {
                    return Err(format!(
                        "the scripted proposal run stopped: {}",
                        other.key()
                    ));
                }
            };
            if let Err(error) = self.prepare_proposal_step() {
                self.capture(&format!("proposal-step-{}-refused", step + 1))?;
                return Err(error);
            }
            self.capture(&format!("proposal-step-{}-pending", step + 1))?;
            self.action(Action::Accept)?;
            self.capture(&format!("proposal-step-{}-applied", step + 1))?;
        }
    }
}

/// Up to [`WRAPPED_ROWS`] rows of `text` wrapped to `width` between
/// words (a word too wide alone is broken where it must be), the last cut
/// with an ellipsis when more remained.
fn wrapped(
    system: &mut tilewright_render_ui::TextSystem,
    text: &str,
    width: u32,
    height: u32,
) -> Vec<String> {
    let fits = |system: &mut tilewright_render_ui::TextSystem, line: &str| {
        crate::workspace_render::fit_text(system, line, width, height) == line
    };
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_inclusive(' ') {
        let candidate = format!("{line}{word}");
        if line.is_empty() || fits(system, candidate.trim_end()) {
            line = candidate;
            continue;
        }
        lines.push(std::mem::take(&mut line).trim_end().to_owned());
        word.clone_into(&mut line);
    }
    lines.push(line.trim_end().to_owned());
    // A word wider than the row alone is broken where it must be.
    let mut lines: Vec<String> = lines
        .into_iter()
        .flat_map(|line| {
            if fits(system, &line) {
                vec![line]
            } else {
                crate::workspace_render::wrap_text(system, &line, width, height)
            }
        })
        .collect();
    if lines.len() > WRAPPED_ROWS {
        let mut rest = lines.split_off(WRAPPED_ROWS - 1).join(" ");
        rest.push_str(" ...");
        // The ordinary fit path cuts it at the end when laid out.
        lines.push(rest);
    }
    lines
}

/// `text` with every character the observe crate escapes written as
/// `\u{…}` (and a backslash as `\\`), so no control, bidirectional,
/// line-separating or invisible character reaches a row; at most
/// [`TEXT_CHARACTERS`] characters of it, cut with an ellipsis.
pub(super) fn shown(text: &str) -> String {
    clipped(&escaped(text), TEXT_CHARACTERS)
}

fn escaped(text: &str) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\\' {
            output.push_str("\\\\");
        } else if tilewright_editor_observe::node::escaped(character) {
            let _ = write!(output, "\\u{{{:x}}}", u32::from(character));
        } else {
            output.push(character);
        }
    }
    output
}

/// At most `limit` characters of `text`, the last an ellipsis when cut.
pub(super) fn clipped(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.into();
    }
    let mut cut: String = text.chars().take(limit - 1).collect();
    cut.push('…');
    cut
}

fn sha256(text: &str) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(text.as_bytes()).into()
}

/// The tile `(x, y)` on `plane`/`layer`, and whether it is outside the view.
fn tile(catalog: &PreviewCatalog, plane: &str, layer: i32, [x, y]: [i64; 2]) -> String {
    if catalog.tile_in_active_view(plane, layer, x, y) {
        format!("({x}, {y})")
    } else {
        format!("({x}, {y}), outside the view")
    }
}

/// Where object `uid` is in the person's world: plane, layer and plane
/// tile, for a placed object.
fn object_place(world: &World, uid: &str) -> Option<(String, i32, Option<[i64; 2]>, String)> {
    let location = world.object_location(uid)?;
    let object = world.object(uid)?;
    let (layer, cx, cy) = location.chunk;
    let size = i64::from(world.planes().get(&location.plane)?.chunk_size);
    let at = object.at.and_then(|at| {
        Some([
            cx.checked_mul(size)?.checked_add(i64::from(at[0]))?,
            cy.checked_mul(size)?.checked_add(i64::from(at[1]))?,
        ])
    });
    Some((location.plane.clone(), layer, at, object.proto.clone()))
}

/// A step's Action in words: its kind and key arguments, and facts about
/// the person's current world where they help (where an object or anchor
/// being moved or deleted is now, and whether a tile is outside the view).
/// Every string from the Action or the world is escaped and cut.
#[allow(clippy::too_many_lines)]
pub(super) fn action_words(action: &Action, world: &World, catalog: &PreviewCatalog) -> String {
    let anchor_at = |plane: &str, anchor: &str| {
        world
            .planes()
            .get(plane)
            .and_then(|source| source.anchors.get(anchor))
            .map(|found| tile(catalog, plane, found.layer, found.at))
    };
    match action {
        Action::Move { uid, x, y, .. } => match object_place(world, uid) {
            Some((plane, layer, Some(at), _)) => format!(
                "Move {} on {}/{layer} from {} to {}",
                shown(uid),
                shown(&plane),
                tile(catalog, &plane, layer, at),
                tile(catalog, &plane, layer, [*x, *y])
            ),
            _ => format!(
                "Move {} (not placed in this world) to ({x}, {y})",
                shown(uid)
            ),
        },
        Action::SetField {
            uid, field, value, ..
        } => format!(
            "Set {} of {} to {}",
            shown(field),
            shown(uid),
            shown(&value.to_string())
        ),
        Action::Paint {
            plane,
            layer,
            grid,
            x,
            y,
            proto,
            ..
        } => format!(
            "Paint {} on {}/{layer} {} with {}",
            tile(catalog, plane, *layer, [*x, *y]),
            shown(plane),
            shown(grid),
            shown(proto)
        ),
        Action::PaintStroke {
            plane,
            layer,
            grid,
            cells,
            proto,
            ..
        } => format!(
            "Paint {} on {}/{layer} {} with {}{}",
            cells_words(cells.len()),
            shown(plane),
            shown(grid),
            shown(proto),
            outside(catalog, plane, *layer, cells)
        ),
        Action::PaintZones {
            plane,
            layer,
            cells,
            zone,
            ..
        } => format!(
            "Paint {} on {}/{layer} {}{}",
            cells_words(cells.len()),
            shown(plane),
            zone.as_ref().map_or_else(
                || "back to the default zone".to_owned(),
                |zone| format!("into zone {}", shown(zone))
            ),
            outside(catalog, plane, *layer, cells)
        ),
        Action::Remove { uid, .. } => match object_place(world, uid) {
            Some((plane, layer, Some(at), proto)) => format!(
                "Delete {} ({}) on {}/{layer} at {}",
                shown(uid),
                shown(&proto),
                shown(&plane),
                tile(catalog, &plane, layer, at)
            ),
            Some((plane, layer, None, proto)) => format!(
                "Delete {} ({}) on {}/{layer}",
                shown(uid),
                shown(&proto),
                shown(&plane)
            ),
            None => format!("Delete {} (not in this world)", shown(uid)),
        },
        Action::Place {
            plane,
            layer,
            x,
            y,
            proto,
            ..
        } => format!(
            "Place {} on {}/{layer} at {}",
            shown(proto),
            shown(plane),
            tile(catalog, plane, *layer, [*x, *y])
        ),
        Action::RenameZone {
            plane, zone, name, ..
        } => match name {
            Some(name) => format!(
                "Rename zone {} on {} to {}",
                shown(zone),
                shown(plane),
                shown(name)
            ),
            None => format!("Clear the name of zone {} on {}", shown(zone), shown(plane)),
        },
        Action::SetZoneParent {
            plane,
            zone,
            parent,
            ..
        } => match parent {
            Some(parent) => format!(
                "Put zone {} on {} under {}",
                shown(zone),
                shown(plane),
                shown(parent)
            ),
            None => format!("Make zone {} on {} top level", shown(zone), shown(plane)),
        },
        Action::CreateZone {
            plane,
            zone,
            name,
            parent,
            ..
        } => format!(
            "Create zone {} on {}{}{}",
            shown(zone),
            shown(plane),
            name.as_ref()
                .map_or_else(String::new, |name| format!(" named {}", shown(name))),
            parent.as_ref().map_or_else(
                || " at the top level".to_owned(),
                |parent| format!(" under {}", shown(parent))
            )
        ),
        Action::DeleteZone { plane, zone, .. } => {
            format!("Delete zone {} on {}", shown(zone), shown(plane))
        }
        Action::CreateAnchor {
            plane,
            anchor,
            x,
            y,
            ..
        } => format!(
            "Create anchor {} on {} at {}",
            shown(anchor),
            shown(plane),
            tile(catalog, plane, catalog.view().1, [*x, *y])
        ),
        Action::MoveAnchor {
            plane,
            anchor,
            x,
            y,
            ..
        } => {
            let layer = world
                .planes()
                .get(plane.as_str())
                .and_then(|source| source.anchors.get(anchor.as_str()))
                .map_or(catalog.view().1, |found| found.layer);
            format!(
                "Move anchor {} on {} from {} to {}",
                shown(anchor),
                shown(plane),
                anchor_at(plane, anchor).unwrap_or_else(|| "nowhere (not in this world)".into()),
                tile(catalog, plane, layer, [*x, *y])
            )
        }
        Action::DeleteAnchor { plane, anchor, .. } => match anchor_at(plane, anchor) {
            Some(at) => format!(
                "Delete anchor {} on {} at {at}",
                shown(anchor),
                shown(plane)
            ),
            None => format!(
                "Delete anchor {} on {} (not in this world)",
                shown(anchor),
                shown(plane)
            ),
        },
        Action::SetPortal { uid, target, .. } => match target {
            None => format!("Clear the portal of {}", shown(uid)),
            Some(PortalTarget::Anchor { plane, anchor }) => format!(
                "Point the portal of {} to anchor {} on {}",
                shown(uid),
                shown(anchor),
                shown(plane)
            ),
            Some(PortalTarget::Location { plane, layer, at }) => format!(
                "Point the portal of {} to ({}, {}) on {}/{layer}",
                shown(uid),
                at[0],
                at[1],
                shown(plane)
            ),
        },
        Action::Select { .. }
        | Action::ClearSelection
        | Action::Accept
        | Action::Cancel
        | Action::Undo
        | Action::Redo
        | Action::Reopen => "not an edit".into(),
    }
}

/// ", some outside the view" when any of `cells` is.
fn outside(catalog: &PreviewCatalog, plane: &str, layer: i32, cells: &[[i64; 2]]) -> &'static str {
    if cells
        .iter()
        .all(|[x, y]| catalog.tile_in_active_view(plane, layer, *x, *y))
    {
        ""
    } else {
        ", some outside the view"
    }
}

fn cells_words(count: usize) -> String {
    if count == 1 {
        "1 cell".into()
    } else {
        format!("{count} cells")
    }
}
