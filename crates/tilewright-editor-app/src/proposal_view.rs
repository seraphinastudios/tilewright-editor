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
/// The most lines a step's words wrap to; the state line wraps to two and
/// every other row is one fitted line.
pub(super) const WRAPPED_ROWS: usize = 3;
const STATE_ROWS: usize = 2;
/// Panel rows: compact, in the smaller type.
pub(super) const ROW_HEIGHT: u32 = 20;
const ROW_GAP: u32 = 2;
/// The control row's height.
const BUTTON_HEIGHT: u32 = 26;
/// The most logical pixels the whole Proposal block takes, its control and
/// the space after it included, at the minimum window; a taller window
/// gives it up to a third of the sidebar (`panel_height`). At the minimum
/// 720x480 window the sidebar runs from y=56 to 480: this leaves 184 px for
/// an open text draft (its field, hint and Review/Discard buttons, 96 px)
/// and the inspector rows below it.
pub(super) const PANEL_HEIGHT: u32 = 240;
/// The text lines that fit in [`PANEL_HEIGHT`] beside the control.
#[cfg(test)]
pub(super) const PANEL_LINES: usize = panel_lines_in(PANEL_HEIGHT);

/// The block's height for a sidebar whose rows end at logical `bottom`:
/// [`PANEL_HEIGHT`], or a third of the sidebar when that is more.
pub(super) fn panel_height(bottom: u32) -> u32 {
    PANEL_HEIGHT.max(bottom.saturating_sub(56) / 3)
}

/// The text lines that fit in a block `height` pixels tall beside the
/// control.
pub(super) const fn panel_lines_in(height: u32) -> usize {
    (height.saturating_sub(BUTTON_HEIGHT + ROW_GAP + 6) / (ROW_HEIGHT + ROW_GAP)) as usize
}
// This editor's version, as a proposal's `editor` member records one.
pub(super) use crate::build_version::EDITOR_VERSION;

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
    /// The last refusal or undo to show, until the person's next action.
    pub(super) last: Last,
    /// What the last settle read: kept so the panel never settles (or
    /// copies the prepared transaction) to lay itself out. Only the
    /// funnel changes the session, and it settles after every action.
    pub(super) reading: Reading,
    /// The sequence number of the last step record written.
    pub(super) records: u64,
    /// The map highlight of the current step's tiles, derived when the
    /// proposal's state changes (`Application::refresh_highlight`), never
    /// per frame.
    pub(super) highlight: Option<Highlight>,
}

/// The most tiles a step's highlight holds: the stroke bound, which the
/// proposal reader already applies to a stroke's cells.
pub(super) const HIGHLIGHT_TILES: usize = tilewright_editor_ui::brush::MAXIMUM_STROKE_CELLS;
/// The Proposal block's notice when the last frame left the highlight out
/// to fit the scene limits; the status line keeps what it says.
pub(super) const HIGHLIGHT_LEFT_OUT: &str = "Highlight left out: too many cells for one frame";
/// The most tiles the frame evidence lists by name.
pub(super) const FRAME_TILES: usize = 8;
/// The highlight's tint: black at 50 % over the tile, so it darkens a tile
/// where the brush preview ([`crate::workspace_render::STROKE_PREVIEW_TINT`])
/// lightens one and the zone tints colour one.
pub(super) const HIGHLIGHT_TINT: [f32; 4] = [0.0, 0.0, 0.0, 0.5];
/// The tint of a tile something leaves (`from`): the same black at 25 %.
pub(super) const HIGHLIGHT_FROM_TINT: [f32; 4] = [0.0, 0.0, 0.0, 0.25];

/// What a highlighted tile is to its step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Role {
    /// Where an object or anchor is moved from.
    From,
    /// Where it is moved to.
    To,
    /// The tile of the object a field, a deletion or a portal concerns.
    Object,
    /// A painted cell.
    Cell,
    /// Where an object is placed, or where a portal leads.
    Target,
    /// The tile of an anchor created or deleted.
    Anchor,
}

impl Role {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::From => "from",
            Self::To => "to",
            Self::Object => "object",
            Self::Cell => "cell",
            Self::Target => "target",
            Self::Anchor => "anchor",
        }
    }

    pub(super) fn tint(self) -> [f32; 4] {
        if self == Self::From {
            HIGHLIGHT_FROM_TINT
        } else {
            HIGHLIGHT_TINT
        }
    }
}

/// One tile a step concerns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Tile {
    pub(super) role: Role,
    pub(super) plane: String,
    pub(super) layer: i32,
    pub(super) at: [i64; 2],
    /// On the active view's plane and layer, inside its bounds.
    pub(super) in_view: bool,
}

/// Why a step has no tiles, or that it has some.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Concerns {
    Tiles,
    /// A zone definition change: nothing on the map.
    NoTiles,
    /// The object or anchor it names is not placed in the person's world.
    NotPlaced,
}

impl Concerns {
    fn key(self) -> &'static str {
        match self {
            Self::Tiles => "tiles",
            Self::NoTiles => "no tiles",
            Self::NotPlaced => "not placed",
        }
    }
}

/// The highlight of step `step`: its tiles, from its decoded Action and the
/// person's committed world at `revision`, and the tints of those in view.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Highlight {
    pub(super) step: usize,
    pub(super) revision: [u8; 32],
    pub(super) concerns: Concerns,
    /// Distinct tiles (plane, layer and tile), at most [`HIGHLIGHT_TILES`];
    /// a tile named twice keeps its first role.
    pub(super) tiles: Vec<Tile>,
    /// The in-view tiles' tints, by plane tile, as the overlay draws them.
    pub(super) tints: std::collections::BTreeMap<[i64; 2], [f32; 4]>,
}

impl Highlight {
    pub(super) fn of(step: usize, action: &Action, session: &Session) -> Self {
        let world = session.history().document().world();
        let catalog = session.catalog();
        let (concerns, found) = step_tiles(action, world, catalog);
        let mut seen = std::collections::BTreeSet::new();
        let mut tiles = Vec::new();
        let mut tints = std::collections::BTreeMap::new();
        // `step_tiles` names at most `HIGHLIGHT_TILES`.
        for (role, plane, layer, at) in found {
            if !seen.insert((plane, layer, at)) {
                continue;
            }
            let in_view = catalog.tile_in_active_view(plane, layer, at[0], at[1]);
            if in_view {
                tints.insert(at, role.tint());
            }
            tiles.push(Tile {
                role,
                plane: plane.to_owned(),
                layer,
                at,
                in_view,
            });
        }
        Self {
            step,
            revision: session.history().document().revision(),
            concerns,
            tiles,
            tints,
        }
    }

    /// The frame evidence's `highlight` member.
    pub(super) fn frame(&self) -> serde_json::Value {
        let in_view = self.tiles.iter().filter(|tile| tile.in_view).count();
        let first: Vec<_> = self
            .tiles
            .iter()
            .take(FRAME_TILES)
            .map(|tile| {
                serde_json::json!({
                    "role": tile.role.key(),
                    "plane": shown(&tile.plane),
                    "layer": tile.layer,
                    "x": tile.at[0],
                    "y": tile.at[1],
                    "in_view": tile.in_view,
                })
            })
            .collect();
        serde_json::json!({
            "step": self.step + 1,
            "concerns": self.concerns.key(),
            "in_view": in_view,
            "outside_view": self.tiles.len() - in_view,
            "tiles": first,
        })
    }
}

/// The step a highlight shows for `state`: the step the panel offers to
/// prepare (ready, undone or refused) or the one pending; none otherwise.
pub(super) fn highlighted_step(state: State) -> Option<usize> {
    match state {
        State::Ready { step }
        | State::Undone { step }
        | State::Refused { step, .. }
        | State::Pending { step } => Some(step),
        _ => None,
    }
}

/// A tile a step names: its role, plane, layer and plane tile.
pub(super) type Named<'a> = (Role, &'a str, i32, [i64; 2]);

/// The tiles step `action` concerns, each with its role, plane and layer,
/// from the decoded Action and the person's committed `world` (where an
/// object or anchor is now), in the order the step's words name them; at
/// most [`HIGHLIGHT_TILES`] of a stroke's cells. A step whose object or
/// anchor is not placed in `world` names none of its tiles: without the
/// object there is no plane and layer for a move's destination either.
pub(super) fn step_tiles<'a>(
    action: &'a Action,
    world: &'a World,
    catalog: &'a PreviewCatalog,
) -> (Concerns, Vec<Named<'a>>) {
    // Where object `uid` is placed, as `role`.
    let placed = |role: Role, uid: &str| {
        let plane = world.object_location(uid)?.plane.as_str();
        let found = object_place(world, uid)?;
        Some((role, plane, found.layer, found.at?))
    };
    let anchor = |role: Role, plane: &'a str, anchor: &str| {
        let found = world.planes().get(plane)?.anchors.get(anchor)?;
        Some((role, plane, found.layer, found.at))
    };
    // A move's destination, on the plane and layer of where it is now.
    let to = |from: Option<Named<'a>>, x: i64, y: i64| {
        from.map(|(_, plane, layer, _)| (Role::To, plane, layer, [x, y]))
    };
    let found: Vec<Option<Named<'a>>> = match action {
        Action::Move { uid, x, y, .. } => {
            let from = placed(Role::From, uid);
            vec![to(from, *x, *y), from]
        }
        Action::SetField { uid, .. } | Action::Remove { uid, .. } => {
            vec![placed(Role::Object, uid)]
        }
        Action::SetPortal { uid, target, .. } => vec![
            placed(Role::Object, uid),
            match target {
                None => None,
                Some(PortalTarget::Location { plane, layer, at }) => {
                    Some((Role::Target, plane.as_str(), *layer, *at))
                }
                Some(PortalTarget::Anchor { plane, anchor: id }) => anchor(Role::Target, plane, id),
            },
        ],
        Action::Paint {
            plane, layer, x, y, ..
        } => vec![Some((Role::Cell, plane.as_str(), *layer, [*x, *y]))],
        Action::PaintStroke {
            plane,
            layer,
            cells,
            ..
        }
        | Action::ScatterTerrain {
            plane,
            layer,
            cells,
            ..
        }
        | Action::PaintZones {
            plane,
            layer,
            cells,
            ..
        } => cells
            .iter()
            .take(HIGHLIGHT_TILES)
            .map(|at| Some((Role::Cell, plane.as_str(), *layer, *at)))
            .collect(),
        Action::FillTerrain { .. } => fill_tiles(action, world, catalog),
        Action::Place {
            plane, layer, x, y, ..
        } => vec![Some((Role::Target, plane.as_str(), *layer, [*x, *y]))],
        // Created on the active view's spatial layer.
        Action::CreateAnchor { plane, x, y, .. } => vec![Some((
            Role::Anchor,
            plane.as_str(),
            catalog.view().1,
            [*x, *y],
        ))],
        Action::MoveAnchor {
            plane,
            anchor: id,
            x,
            y,
            ..
        } => {
            let from = anchor(Role::From, plane, id);
            vec![to(from, *x, *y), from]
        }
        Action::DeleteAnchor {
            plane, anchor: id, ..
        } => vec![anchor(Role::Anchor, plane, id)],
        Action::RenameZone { .. }
        | Action::SetZoneParent { .. }
        | Action::CreateZone { .. }
        | Action::DeleteZone { .. }
        | Action::Select { .. }
        | Action::ClearSelection
        | Action::Accept
        | Action::Cancel
        | Action::Undo
        | Action::Redo
        | Action::Reopen => return (Concerns::NoTiles, Vec::new()),
    };
    let tiles: Vec<Named<'a>> = found.into_iter().flatten().collect();
    if tiles.is_empty() {
        (Concerns::NotPlaced, tiles)
    } else {
        (Concerns::Tiles, tiles)
    }
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
    /// Step `step` was accepted and undone before the progress settled it.
    /// Defensive: the funnel settles right after the Accept, so the desktop
    /// sees an undo of a settled step instead (`State::Undone` from the
    /// world's position).
    Undone {
        step: usize,
    },
}

/// What [`Progress::settle`] last read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Reading {
    /// Nothing prepared, or the prepared step settled.
    Idle,
    /// The prepared step is pending.
    Pending(usize),
    /// Defensive: another change pending while a step is prepared, or a
    /// prepared step whose world moved. The engine fences preparing over a
    /// pending change, so the desktop cannot reach either.
    OtherPending,
    Stale,
}

/// The kinds of panel rows, for the block's height cap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Row {
    Header,
    Label,
    Caveat,
    Editor,
    Where,
    Step,
    State,
    Counts,
    Notice,
    /// The map highlight was left out of the last frame.
    Highlight,
}

impl Row {
    /// The most lines this row may take.
    fn lines(self) -> usize {
        match self {
            Self::Step => WRAPPED_ROWS,
            Self::State => STATE_ROWS,
            _ => 1,
        }
    }
    /// The rank of its line `line` when the block is short of room: lower
    /// is kept first. While a step is refused or diverged, the catalog
    /// notice (a likely cause) outranks the counts and the words' third
    /// line.
    fn rank(self, line: usize, refused: bool) -> usize {
        match (self, line) {
            (Self::Header, _) => 0,
            (Self::Step, 0) => 1,
            // Kept with the state's first line: the map no longer shows
            // what the words name.
            (Self::State, 0) | (Self::Highlight, _) => 2,
            (Self::Label, _) => 3,
            (Self::Caveat, _) => 4,
            (Self::Editor, _) => 5,
            (Self::Where, _) => 6,
            (Self::Step, 1) => 7,
            (Self::State, _) => 8,
            (Self::Notice, _) if refused => 9,
            (Self::Step, _) => 10,
            (Self::Counts, _) => 11,
            (Self::Notice, _) => 12,
        }
    }
}

/// What the panel shows: derived, once per layout, from the reading the
/// funnel's settle stored after the last action (the only place the
/// session changes), the stored refusal and the session itself.
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
    /// The world is at several positions of a chain that revisits a
    /// revision: a resume would be ambiguous. `redo` and `undo` say whether
    /// the session has a change to redo (a step the person undid) or to
    /// undo (an edit of the person's own that brought it here).
    Several {
        redo: bool,
        undo: bool,
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
            Self::Several { .. } => "several",
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
            reading: Reading::Idle,
            records: 0,
            highlight: None,
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
        // What the funnel's last settle read: the session has not changed
        // since.
        match self.reading {
            Reading::Pending(step) => return State::Pending { step },
            Reading::OtherPending => return State::OtherPending,
            Reading::Stale => {
                return State::Moved {
                    resume: self.position(session),
                };
            }
            Reading::Idle => {}
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
            return match self.positions(session).as_slice() {
                // The person undid applied steps: the first undone one can
                // be prepared again (a Redo continues too).
                [position] if *position < next => State::Undone { step: *position },
                [position] => State::Moved {
                    resume: Some(*position),
                },
                // Every step applied, and the person edited after them.
                [] if next == self.steps() => State::Finished,
                [] => State::Moved { resume: None },
                _ => {
                    let (undo, redo) = session.history().depths();
                    State::Several {
                        redo: redo > 0,
                        undo: undo > 0,
                    }
                }
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
                "Stopped by Cancel; it cannot resume from where the world is.".into()
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
            } => {
                if self.proposal.editor() == EDITOR_VERSION {
                    "Diverged from its recording. Nothing changed.".into()
                } else {
                    "Diverged from its recording. Nothing changed. Recorded by another editor build."
                        .into()
                }
            }
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
            State::Several { redo: true, .. } => {
                "The world is at several places on its path: Redo continues the proposal.".into()
            }
            State::Several { undo: true, .. } => {
                "The world is at several places on its path: undo your last edits to continue."
                    .into()
            }
            State::Several { .. } => {
                "The world is at several places on its path: the proposal cannot continue from here."
                    .into()
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

    /// The panel's rows for `state`, before they are wrapped to the
    /// sidebar's width, each cut at `characters` (what its lines can hold).
    pub(super) fn rows(
        &self,
        session: &Session,
        state: State,
        characters: usize,
    ) -> Vec<(Row, String)> {
        let proposal = &self.proposal;
        let steps = self.steps();
        let actor = proposal.actor().as_str();
        let mut rows = vec![
            (
                Row::Header,
                format!(
                    "Proposal {}, recorded actor: {}",
                    &revision_hex(&proposal.body_sha256())[..8],
                    shown(actor)
                ),
            ),
            (
                Row::Label,
                proposal.client().map_or_else(
                    || "Client label: none".into(),
                    |label| format!("Client label (unverified): {}", shown(label)),
                ),
            ),
        ];
        // The format allows any actor; the sandbox helper records agent:.
        if !actor.starts_with("agent:") {
            rows.push((Row::Caveat, "Recorded actor is not an agent actor".into()));
        }
        rows.push((
            Row::Editor,
            if proposal.editor() == EDITOR_VERSION {
                format!("Recorded by this editor build ({EDITOR_VERSION})")
            } else {
                format!(
                    "Recorded by another editor build ({}; this is {EDITOR_VERSION}): steps may diverge",
                    shown(proposal.editor())
                )
            },
        ));
        if let Opened::DoesNotApply(code) = self.opened {
            rows.push((Row::Where, format!("Does not apply here ({code})")));
            rows.push((
                Row::Where,
                format!("Its base: {}", &revision_hex(&proposal.base())[..16]),
            ));
            rows.push((
                Row::Where,
                format!(
                    "This world: {}",
                    &revision_hex(&session.history().document().revision())[..16]
                ),
            ));
        } else {
            rows.push((Row::Where, self.where_words(session)));
        }
        let next = self.progress.as_ref().map_or(0, Progress::next);
        let current = match state {
            State::Pending { step }
            | State::Refused { step, .. }
            | State::Undone { step }
            | State::Ready { step } => Some(step),
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
            rows.push((
                Row::Step,
                self.step_words(
                    step,
                    session.history().document().world(),
                    session.catalog(),
                ),
            ));
        }
        rows.push((Row::State, self.state_words(state)));
        if self.progress.is_some() {
            rows.push((
                Row::Counts,
                format!(
                    "Applied {next} of {steps}, remaining {}",
                    steps.saturating_sub(next)
                ),
            ));
        }
        for notice in &self.notices {
            match notice {
                Notice::CatalogDiffers { .. } => {
                    rows.push((Row::Notice, "Catalog differs; your grants apply".into()));
                }
            }
        }
        rows.into_iter()
            .map(|(kind, text)| (kind, clipped(&text, characters)))
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
        self.refresh_highlight();
        Ok(())
    }

    /// Prepares the proposal's next step through the same funnel as every
    /// other edit. The step is then pending for the person's Review; this
    /// never accepts it.
    pub(super) fn prepare_proposal_step(&mut self) -> Result<()> {
        if let Some(reason) = &self.blocked {
            return Err(format!("rendering blocked editing: {reason}"));
        }
        self.funnel(false, true, |app| {
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
                    // A refusal of the step itself is shown until the
                    // person's next action; the others (a world that moved,
                    // a change pending) the panel reads from the session.
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
        open.reading = Reading::Idle;
        self.presented_generation = None;
        self.refresh_highlight();
        Ok(())
    }

    /// Derives the map highlight for the proposal's state after it changed
    /// (the funnel's settle, a continue, the opening): the tiles of the step
    /// [`highlighted_step`] names, kept while the step and the committed
    /// revision stay the same.
    pub(super) fn refresh_highlight(&mut self) {
        let Some(open) = self.proposal.as_mut() else {
            return;
        };
        let state = open.state(&self.session, self.blocked.is_some());
        let revision = self.session.history().document().revision();
        open.highlight = match highlighted_step(state) {
            Some(step)
                if open
                    .highlight
                    .as_ref()
                    .is_some_and(|kept| kept.step == step && kept.revision == revision) =>
            {
                open.highlight.take()
            }
            Some(step) => open
                .proposal
                .steps()
                .get(step)
                .map(|recorded| Highlight::of(step, recorded.action(), &self.session)),
            None => None,
        };
    }

    /// The highlight to draw and record now: the stored one, while the
    /// panel's state still names its step (a rendering failure after it was
    /// derived blocks the session and clears it).
    pub(super) fn proposal_highlight(&self) -> Option<&Highlight> {
        let highlight = self.proposal.as_ref()?.highlight.as_ref()?;
        (highlighted_step(self.proposal_state()?) == Some(highlight.step)).then_some(highlight)
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
    /// recovery records before the action, when the action was an Accept;
    /// `prepared` tells whether the action was preparing a step (any other
    /// action of the person clears a shown refusal).
    pub(super) fn settle_proposal(
        &mut self,
        accepted: Option<usize>,
        prepared: bool,
    ) -> Result<()> {
        let Some(open) = self.proposal.as_mut() else {
            return Ok(());
        };
        let Some(progress) = open.progress.as_mut() else {
            return Ok(());
        };
        if !prepared && matches!(open.last, Last::Refused { .. }) {
            open.last = Last::Nothing;
        }
        let settled = progress.settle(&open.proposal, &self.session);
        open.reading = match settled {
            Ok(Settled::Pending { step }) => Reading::Pending(step),
            Ok(Settled::OtherPending { .. }) => Reading::OtherPending,
            Err(_) => Reading::Stale,
            Ok(_) => Reading::Idle,
        };
        match settled {
            Ok(Settled::Applied { step }) => {
                open.last = Last::Nothing;
                // The sequence advances before the write, so a failed write
                // never makes a later record reuse its number.
                open.records += 1;
                let sequence = open.records;
                let record = self.step_record(step, sequence, accepted);
                let name = format!("proposal-step-{sequence:06}.json");
                // The step is applied whatever happens to its record.
                self.evidence
                    .retain(&name, record.as_bytes())
                    .map_err(|error| {
                        format!(
                            "Step {} applied; its evidence record {name} could not be written: {error}",
                            step + 1
                        )
                    })
            }
            Ok(Settled::NotApplied { step }) => {
                // Defensive: see `Last::Undone`.
                open.last = Last::Undone { step };
                Ok(())
            }
            Ok(Settled::Stopped { .. }) => {
                open.last = Last::Nothing;
                Ok(())
            }
            // Pending, nothing prepared, or (defensive) another change
            // pending or a world that moved: the panel reads these.
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
        // The record the person's Accept retained. Defensive `null`: the
        // funnel settles right after the Accept that applies a step, and
        // every such Accept retains exactly one record.
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
        match self.proposal_state()? {
            State::Pending { step } => Some(format!(
                "Step {} of {} matches its recording (recorded as {}). Accept applies it; Cancel stops the proposal.",
                step + 1,
                open.proposal.steps().len(),
                shown(open.proposal.actor().as_str())
            )),
            _ => None,
        }
    }

    /// The panel's state: the one computed for this layout, else now.
    pub(super) fn proposal_state(&self) -> Option<State> {
        self.proposal_layout.or_else(|| {
            self.proposal
                .as_ref()
                .map(|open| open.state(&self.session, self.blocked.is_some()))
        })
    }

    /// The proposal block at the top of the sidebar, from logical `y`;
    /// returns the next free row. The block never takes more than
    /// `panel_height` ([`PANEL_HEIGHT`] at the minimum window): when its
    /// rows need more lines than fit, the least important lines go first (see
    /// `Row::rank`) and a row cut short ends in "...". Every line goes
    /// through the ordinary fit-and-clip path when the interface is laid
    /// out.
    pub(super) fn proposal_panel(
        &mut self,
        ui: &mut crate::workspace_ui::WorkspaceUi,
        mut y: u32,
    ) -> u32 {
        let (Some(open), Some(state)) = (&self.proposal, self.proposal_state()) else {
            return y;
        };
        let x = ui.sidebar_x();
        let width = ui.sidebar_width();
        let characters = row_characters(&mut self.text, width);
        let mut rows = open.rows(&self.session, state, characters);
        if self.highlight_omitted && self.proposal_highlight().is_some() {
            rows.push((Row::Highlight, HIGHLIGHT_LEFT_OUT.into()));
        }
        let ready = self.blocked.is_none() && self.session.can_prepare();
        let control = open.control(state, ready);
        let budget = panel_lines_in(panel_height(ui.logical_height()));
        let refused = matches!(state, State::Refused { .. });
        for line in panel_lines(&mut self.text, &rows, width, budget, refused) {
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
        let state = self.proposal_state()?;
        Some(serde_json::json!({
            "proposal": revision_hex(&open.proposal.body_sha256()),
            "steps": open.proposal.steps().len(),
            "applied": open.progress.as_ref().map(Progress::next),
            "state": state.key(),
            "pending_step": matches!(state, State::Pending { .. }),
            "highlight": self.proposal_highlight().map(|highlight| {
                let mut frame = highlight.frame();
                frame["left_out"] = self.highlight_omitted.into();
                frame
            }),
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

/// About the most characters three lines of the sidebar can hold, from the
/// advance the layout's font gives a full stop at the row size. Narrower
/// glyphs (an "l") can fit a few more; the cut is always marked with an
/// ellipsis, and the layout cuts what does not fit.
pub(super) fn row_characters(system: &mut tilewright_render_ui::TextSystem, width: u32) -> usize {
    let size = crate::workspace_render::text_size(ROW_HEIGHT);
    let available = f64::from(width.saturating_sub(8));
    let advance = system
        .layout("..........", size, None)
        .map_or(0.0, |layout| f64::from(layout.size()[0]) / 10.0);
    if advance <= 0.0 {
        return TEXT_CHARACTERS * WRAPPED_ROWS;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let line = (available / advance).floor() as usize;
    WRAPPED_ROWS * line.max(1)
}

/// The block's lines: each row wrapped to at most its own number of lines,
/// then at most `budget` in all, keeping the best-ranked lines (`refused`:
/// a step is refused or diverged); a row that loses lines ends in "...",
/// and a row that loses all is left out.
pub(super) fn panel_lines(
    system: &mut tilewright_render_ui::TextSystem,
    rows: &[(Row, String)],
    width: u32,
    budget: usize,
    refused: bool,
) -> Vec<String> {
    let wrapped: Vec<Vec<String>> = rows
        .iter()
        .map(|(kind, text)| {
            if kind.lines() == 1 {
                vec![text.clone()]
            } else {
                wrapped(system, text, width, ROW_HEIGHT, usize::MAX)
            }
        })
        .collect();
    let mut ranked: Vec<(usize, usize, usize)> = rows
        .iter()
        .zip(&wrapped)
        .enumerate()
        .flat_map(|(index, ((kind, _), lines))| {
            (0..lines.len().min(kind.lines()))
                .map(move |line| (kind.rank(line, refused), index, line))
        })
        .collect();
    ranked.sort_unstable();
    let mut kept = vec![0; rows.len()];
    for (_, index, _) in ranked.into_iter().take(budget) {
        kept[index] += 1;
    }
    let mut lines = Vec::new();
    for (mut row, kept) in wrapped.into_iter().zip(kept) {
        if kept == 0 {
            continue;
        }
        if row.len() > kept {
            let mut rest = row.split_off(kept - 1).join(" ");
            rest.push_str(" ...");
            // The ordinary fit path cuts it at the end when laid out.
            row.push(rest);
        }
        lines.extend(row);
    }
    lines
}

/// Up to `limit` rows of `text` wrapped to `width` between words (a word
/// too wide alone is broken where it must be), the last cut with an
/// ellipsis when more remained.
fn wrapped(
    system: &mut tilewright_render_ui::TextSystem,
    text: &str,
    width: u32,
    height: u32,
    limit: usize,
) -> Vec<String> {
    let fits = |system: &mut tilewright_render_ui::TextSystem, line: &str| {
        crate::workspace_render::fit_text(system, line, width, height) == line
    };
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_inclusive(' ') {
        let candidate = format!("{line}{word}");
        // A word too wide for a line of its own stays on this one and is
        // broken where it must be below, instead of leaving a short line.
        if line.is_empty() || fits(system, candidate.trim_end()) || !fits(system, word.trim_end()) {
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
    if lines.len() > limit {
        let mut rest = lines.split_off(limit - 1).join(" ");
        rest.push_str(" ...");
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

/// Object `uid` in the person's world: its plane, layer, plane tile (for a
/// placed object), prototype and how many objects it carries (its whole
/// inventory, nested ones included).
struct Found {
    plane: String,
    layer: i32,
    at: Option<[i64; 2]>,
    proto: String,
    carried: usize,
}

fn object_place(world: &World, uid: &str) -> Option<Found> {
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
    let mut carried = 0;
    let mut pending: Vec<&tilewright_source::Object> = object.inventory.iter().collect();
    while let Some(held) = pending.pop() {
        carried += 1;
        pending.extend(&held.inventory);
    }
    Some(Found {
        plane: location.plane.clone(),
        layer,
        at,
        proto: object.proto.clone(),
        carried,
    })
}

/// A step's Action in words: what it does, to which id, and where to (the
/// facts that decide, first, so a cut row keeps them), then the rest; with
/// facts from the person's committed world where they help (where an
/// object or anchor being moved or deleted is now, what a deleted object
/// carries, and whether a tile is outside the view). Every string from the
/// Action or the world is escaped and cut.
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
            Some(Found {
                plane,
                layer,
                at: Some(at),
                ..
            }) => format!(
                "Move {} to {}, from {}, on {}/{layer}",
                shown(uid),
                tile(catalog, &plane, layer, [*x, *y]),
                tile(catalog, &plane, layer, at),
                shown(&plane)
            ),
            _ => format!(
                "Move {} to ({x}, {y}) (not placed in this world)",
                shown(uid)
            ),
        },
        Action::SetField {
            uid, field, value, ..
        } => format!(
            "Set {} on {} to {}{}",
            shown(field),
            shown(uid),
            shown(&value.to_string()),
            object_outside(world, catalog, uid)
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
            "Paint {} with {} ({} on {}/{layer})",
            tile(catalog, plane, *layer, [*x, *y]),
            shown(proto),
            shown(grid),
            shown(plane)
        ),
        Action::PaintStroke {
            plane,
            layer,
            grid,
            cells,
            proto,
            ..
        } => format!(
            "Paint {}{} with {} ({} on {}/{layer})",
            cells_words(cells.len()),
            outside(catalog, plane, *layer, cells),
            shown(proto),
            shown(grid),
            shown(plane)
        ),
        Action::FillTerrain { .. } => fill_words(action, world, catalog),
        Action::ScatterTerrain {
            plane,
            layer,
            grid,
            cells,
            palette,
            seed,
            ..
        } => {
            // How many cells the engine's own plan would change in the
            // person's world, as fill's words say; nothing when it would
            // refuse for another reason.
            let plan = tilewright_editor_ui::scatter::plan(
                world, catalog, plane, *layer, grid, cells, palette, *seed,
            );
            let effect = match plan {
                Ok(changes) => format!(", {} to change", changes.len()),
                Err(error) if error.code() == tilewright_editor_ui::RefusalCode::ChangeNone => {
                    ", none to change".to_owned()
                }
                Err(_) => String::new(),
            };
            format!(
                "Scatter palette {}{} over {}{}{effect}, seed {seed} ({} on {}/{layer})",
                shown(palette),
                if catalog.editing().palettes.contains_key(palette) {
                    ""
                } else {
                    " (not in this catalog)"
                },
                cells_words(cells.len()),
                outside(catalog, plane, *layer, cells),
                shown(grid),
                shown(plane)
            )
        }
        Action::PaintZones {
            plane,
            layer,
            cells,
            zone,
            ..
        } => format!(
            "Paint {}{} {} (on {}/{layer})",
            cells_words(cells.len()),
            outside(catalog, plane, *layer, cells),
            zone.as_ref().map_or_else(
                || "back to the default zone".to_owned(),
                |zone| format!("into zone {}", shown(zone))
            ),
            shown(plane)
        ),
        Action::Remove { uid, .. } => match object_place(world, uid) {
            Some(found) => format!(
                "Delete {}{}{} ({} on {}/{})",
                shown(uid),
                found.at.map_or_else(String::new, |at| format!(
                    " at {}",
                    tile(catalog, &found.plane, found.layer, at)
                )),
                match found.carried {
                    0 => String::new(),
                    1 => " and the 1 object it carries".into(),
                    carried => format!(" and the {carried} objects it carries"),
                },
                shown(&found.proto),
                shown(&found.plane),
                found.layer
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
            "Place {} at {} on {}/{layer}",
            shown(proto),
            tile(catalog, plane, *layer, [*x, *y]),
            shown(plane)
        ),
        Action::RenameZone {
            plane, zone, name, ..
        } => match name {
            Some(name) => format!(
                "Rename zone {} to {} on {}",
                shown(zone),
                shown(name),
                shown(plane)
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
                "Put zone {} under {} on {}",
                shown(zone),
                shown(parent),
                shown(plane)
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
            "Create zone {}{}{} on {}",
            shown(zone),
            parent.as_ref().map_or_else(
                || " at the top level".to_owned(),
                |parent| format!(" under {}", shown(parent))
            ),
            name.as_ref()
                .map_or_else(String::new, |name| format!(", named {},", shown(name))),
            shown(plane)
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
            "Create anchor {} at {} on {}",
            shown(anchor),
            tile(catalog, plane, catalog.view().1, [*x, *y]),
            shown(plane)
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
                "Move anchor {} to {}, from {}, on {}",
                shown(anchor),
                tile(catalog, plane, layer, [*x, *y]),
                anchor_at(plane, anchor).unwrap_or_else(|| "nowhere (not in this world)".into()),
                shown(plane)
            )
        }
        Action::DeleteAnchor { plane, anchor, .. } => match anchor_at(plane, anchor) {
            Some(at) => format!(
                "Delete anchor {} at {at} on {}",
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
                "Point the portal of {} to anchor {}{} on {}",
                shown(uid),
                shown(anchor),
                anchor_at(plane, anchor).map_or_else(String::new, |at| format!(" at {at}")),
                shown(plane)
            ),
            Some(PortalTarget::Location { plane, layer, at }) => format!(
                "Point the portal of {} to {} on {}/{layer}",
                shown(uid),
                tile(catalog, plane, *layer, *at),
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

/// The region fill `action` covers in `world`
/// ([`tilewright_editor_ui::fill::region`], the engine's own expansion) and
/// how many of its cells the fill would change: none when the region
/// already holds the fill's terrain, as the engine leaves out cells that
/// hold it. `None` for another action, or when the engine would not expand
/// the fill at all (outside the view, an unknown grid).
fn fill_region<'w>(
    action: &Action,
    world: &'w World,
    catalog: &PreviewCatalog,
) -> Option<(tilewright_editor_ui::fill::Region<'w>, usize)> {
    let Action::FillTerrain {
        plane,
        layer,
        grid,
        x,
        y,
        proto,
        contiguous,
        ..
    } = action
    else {
        return None;
    };
    let region = tilewright_editor_ui::fill::region(
        world,
        catalog,
        plane,
        *layer,
        grid,
        [*x, *y],
        *contiguous,
    )
    .ok()?;
    let changed = if region.value == Some(proto.as_str()) {
        0
    } else {
        region.cells.len()
    };
    Some((region, changed))
}

/// The tiles of fill `action`: its start cell, then every cell the
/// engine's fill would change in the person's world, at most
/// [`HIGHLIGHT_TILES`]; the start cell alone when it would change none
/// (already that terrain, or it starts outside the view).
fn fill_tiles<'a>(
    action: &'a Action,
    world: &'a World,
    catalog: &'a PreviewCatalog,
) -> Vec<Option<Named<'a>>> {
    let Action::FillTerrain {
        plane, layer, x, y, ..
    } = action
    else {
        return Vec::new();
    };
    let start = [*x, *y];
    let changed = match fill_region(action, world, catalog) {
        Some((region, changed)) if changed > 0 => region.cells,
        _ => Vec::new(),
    };
    std::iter::once(start)
        .chain(changed.into_iter().filter(|at| *at != start))
        .take(HIGHLIGHT_TILES)
        .map(|at| Some((Role::Cell, plane.as_str(), *layer, at)))
        .collect()
}

/// Fill `action` in words: the rule, where it starts and the terrain first,
/// then what the person's world holds there and how many cells it would
/// change.
fn fill_words(action: &Action, world: &World, catalog: &PreviewCatalog) -> String {
    let Action::FillTerrain {
        plane,
        layer,
        grid,
        x,
        y,
        proto,
        contiguous,
        ..
    } = action
    else {
        return String::new();
    };
    let start = tile(catalog, plane, *layer, [*x, *y]);
    let on = format!("({} on {}/{layer})", shown(grid), shown(plane));
    let Some((region, changed)) = fill_region(action, world, catalog) else {
        let rule = if *contiguous { "Fill" } else { "Replace" };
        return format!("{rule} from {start} with {} {on}", shown(proto));
    };
    let held = region.value.map(shown);
    let effect = if changed == 0 {
        format!("no change, already {}", shown(proto))
    } else {
        match &held {
            Some(value) if *contiguous => format!("{} of {value}", cells_words(changed)),
            None if *contiguous => format!("{} with no terrain", cells_words(changed)),
            _ => cells_words(changed),
        }
    };
    if *contiguous {
        format!("Fill from {start} with {}: {effect} {on}", shown(proto))
    } else {
        format!(
            "Replace every {} in the view with {}: {effect}, from {start} {on}",
            held.unwrap_or_else(|| "cell with no terrain".into()),
            shown(proto)
        )
    }
}

/// " (outside the view)" when object `uid` is placed on a tile outside it.
fn object_outside(world: &World, catalog: &PreviewCatalog, uid: &str) -> &'static str {
    match object_place(world, uid) {
        Some(Found {
            plane,
            layer,
            at: Some([x, y]),
            ..
        }) if !catalog.tile_in_active_view(&plane, layer, x, y) => " (outside the view)",
        _ => "",
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
