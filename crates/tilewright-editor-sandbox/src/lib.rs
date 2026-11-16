// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! The headless sandbox runner (`docs/SANDBOX.md`): one editing session in
//! an agent's own provisioned copy of a world, driven by typed requests,
//! one JSON object per line on standard input, each answered by one JSON
//! object per line on standard output. An agent edits, reviews and accepts
//! steps here, in its copy only, and submits them as a proposal file
//! (`docs/PROPOSALS.md`) that only a person can apply to their own world.
//!
//! `tilewright-editor sandbox`, which `tools/provision-sandbox.py` starts
//! with the sandbox's directories as open descriptors, runs it. Nothing
//! here opens a path a request names: no request carries a path. The world
//! is the provisioned copy, the recovery store and the proposal store are
//! the provisioned directories, and Accept publishes into the copy, as the
//! prototype does. The actor of every step is the operator's
//! `agent:<name>`; the client label is an untrusted label only.
//!
//! Requests are read with World's strict JSON reader under fixed limits,
//! and every reply is written by hand with the observe crate's string
//! escaping, so world text in a reply is always data inside a string.

mod reply;
mod request;

use reply::Reply;
use std::io::{self, BufRead, Write};
use std::path::Path;
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::{WorldDocument, terminal_field};
use tilewright_editor_observe::line::{Line, read_line};
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_proposal::{Proposal, ProposalStore, hex};
use tilewright_editor_recovery::{DurableRecovery, RecoveryLimits};
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session};
use tilewright_source::SourceRoot;

pub use request::{CODES, REQUESTS};

/// One request given as a value rather than a line, as the MCP editing
/// server (`docs/MCP-EDITING.md`) gives them: every request but `quit`,
/// which ends a run and has no counterpart there.
#[derive(Clone, Copy, Debug)]
pub enum Call<'a> {
    Status,
    /// An edit: its action as World's strict reader read it, in the
    /// proposal format's form, decoded exactly as an `edit` line's.
    Edit(&'a tilewright_jsonc::Value),
    Review,
    Accept,
    Cancel,
    Undo,
    Redo,
    Submit,
}

/// The reply the runner writes for a request, and whether it was done.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Answer {
    /// The reply object, as the line protocol writes it, without its
    /// newline.
    pub reply: String,
    /// Its `ok`: false for a refusal.
    pub ok: bool,
}

impl Answer {
    fn of(reply: String) -> Self {
        Self {
            // Every reply starts with its `ok` member (`Reply`).
            ok: reply.starts_with("{\"ok\":true"),
            reply,
        }
    }
}

/// Most bytes of one request line, its newline and a CR before it not
/// counted. A longer line is refused without being kept.
pub const MAXIMUM_REQUEST_BYTES: usize = 64 * 1024;

/// Most bytes of one reply line, its newline not counted. Lists that do
/// not fit are cut, and the reply says how many entries it left out.
pub const MAXIMUM_REPLY_BYTES: usize = 256 * 1024;

/// The prefix every sandbox actor has: steps made in a sandbox are an
/// agent's, never a person's.
pub const ACTOR_PREFIX: &str = "agent:";

/// What the provisioner gives a sandbox besides its directories.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    /// The untrusted client label, recorded in proposals as given and
    /// shown escaped; never an actor. At most 256 bytes.
    pub client: Option<String>,
    /// The SHA-256 of the sandbox's provisioning receipt, recorded in
    /// every proposal.
    pub receipt: [u8; 32],
    /// The editor version the binary reports for itself, recorded in
    /// every proposal.
    pub editor: String,
}

/// A sandbox: the session, its recovery store and its proposal store.
pub struct Sandbox<R> {
    session: Session,
    recovery: R,
    store: ProposalStore,
    settings: Settings,
    reply_bytes: usize,
    /// Proposal files retained (or found already retained) by this run.
    submitted: usize,
    /// Whether the recovery store refused a record in this run. The
    /// durable store keeps refusing after any failure, so no later Accept
    /// can succeed; the runner says so at once.
    recovery_failed: bool,
}

/// The message of an Accept after the recovery store refused a record.
const RECOVERY_LATCHED: &str = "the recovery store refused a record earlier in this run and refuses every later one: no accept can succeed in this sandbox; cancel, submit the steps so far and start a new sandbox";

/// Why a sandbox did not open. The text names no path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenError(pub String);

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OpenError {}

/// Refuses an actor that is not `agent:` followed by at least one
/// character; [`Actor`] has already checked the rest.
///
/// # Errors
/// Returns why the actor is not a sandbox actor.
pub fn sandbox_actor(actor: &Actor) -> Result<(), OpenError> {
    match actor.as_str().strip_prefix(ACTOR_PREFIX) {
        Some(name) if !name.is_empty() => Ok(()),
        _ => Err(OpenError(format!(
            "a sandbox actor is {ACTOR_PREFIX}<name>: sandbox steps are an agent's"
        ))),
    }
}

impl Sandbox<DurableRecovery> {
    /// Opens the provisioned sandbox: the recovery store on `recovery`
    /// (refused unless it is a private, empty, checked directory), the
    /// proposal store on `proposals` (refused unless private and checked),
    /// then the world copy at `world` with the prototype's source budget,
    /// its catalog, and a session whose every step carries `actor`.
    ///
    /// # Errors
    /// Refuses an actor without the `agent:` prefix, a client label over
    /// 256 bytes, and whatever the stores, the world or its catalog
    /// refuse.
    pub fn open(
        world: &Path,
        recovery: SourceRoot,
        proposals: SourceRoot,
        actor: Actor,
        settings: Settings,
    ) -> Result<Self, OpenError> {
        sandbox_actor(&actor)?;
        check_settings(&settings)?;
        let recovery =
            DurableRecovery::from_provisioned_session_root(recovery, RecoveryLimits::default())
                .map_err(|error| OpenError(format!("recovery store refused: {error}")))?;
        let store = ProposalStore::open(proposals).map_err(|error| {
            OpenError(format!(
                "proposal store refused ({}): {error}",
                error.code().as_str()
            ))
        })?;
        let document = WorldDocument::open_prototype(world)
            .map_err(|error| OpenError(format!("the world copy does not load: {error}")))?;
        let (catalog, resources) = loader::load(
            document.source(),
            document.world(),
            PreviewLimits::default(),
        )
        .map_err(|error| OpenError(format!("the editor catalog does not load: {error}")))?;
        let session = Session::new(History::new(document), catalog, resources, actor);
        Self::new(session, recovery, store, settings)
    }
}

impl<R: RecoveryStore> Sandbox<R> {
    /// A sandbox over an open session and stores. [`Sandbox::open`] is how
    /// the runner makes one; tests give other recovery stores.
    ///
    /// # Errors
    /// Refuses a session whose actor is not a sandbox actor and a client
    /// label over 256 bytes.
    pub fn new(
        session: Session,
        recovery: R,
        store: ProposalStore,
        settings: Settings,
    ) -> Result<Self, OpenError> {
        sandbox_actor(session.actor())?;
        check_settings(&settings)?;
        Ok(Self {
            session,
            recovery,
            store,
            settings,
            reply_bytes: MAXIMUM_REPLY_BYTES,
            submitted: 0,
            recovery_failed: false,
        })
    }

    /// Lowers the reply cap: for tests of how replies are cut, and for the
    /// MCP editing server, whose results carry each reply twice. It is
    /// never raised above [`MAXIMUM_REPLY_BYTES`].
    pub fn set_reply_limit(&mut self, bytes: usize) {
        self.reply_bytes = bytes.min(MAXIMUM_REPLY_BYTES);
    }

    /// The reply cap in force.
    pub fn reply_limit(&self) -> usize {
        self.reply_bytes
    }

    /// The session, read-only.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The sandbox world's committed revision.
    pub fn revision(&self) -> [u8; 32] {
        self.session.history().document().revision()
    }

    /// Serves requests from `input` until it ends or a `quit` request,
    /// writing each reply as one line to `output` and flushing it, and
    /// notes for people to `log`: one startup line, and one line for each
    /// request line too long to read, every one escaped.
    ///
    /// # Errors
    /// Returns an error when reading `input` or writing `output` fails; a
    /// failure to write to `log` is ignored.
    pub fn serve(
        &mut self,
        mut input: impl BufRead,
        mut output: impl Write,
        log: &mut impl Write,
    ) -> io::Result<()> {
        let _ = writeln!(
            log,
            "tilewright-editor sandbox: ready actor={} revision={}",
            terminal_field(self.session.actor().as_str()),
            hex(&self.revision())
        );
        let mut line = Vec::new();
        loop {
            let (reply, quit) = match read_line(&mut input, &mut line, MAXIMUM_REQUEST_BYTES)? {
                Line::End => return Ok(()),
                Line::TooLong => {
                    let _ = writeln!(
                        log,
                        "tilewright-editor sandbox: refused a request longer than {MAXIMUM_REQUEST_BYTES} bytes"
                    );
                    (
                        self.refusal(
                            None,
                            request::TOO_LARGE,
                            "the request is longer than the runner reads (64 KiB)",
                        ),
                        false,
                    )
                }
                Line::Message => self.handle(&line),
            };
            output.write_all(reply.as_bytes())?;
            output.write_all(b"\n")?;
            output.flush()?;
            if quit {
                return Ok(());
            }
        }
    }

    /// Answers one request given as a value: exactly the reply its line
    /// would get.
    pub fn respond(&mut self, call: Call<'_>) -> Answer {
        let request = match call {
            Call::Status => Ok(request::Request::Status),
            Call::Edit(action) => request::edit(action),
            Call::Review => Ok(request::Request::Review),
            Call::Accept => Ok(request::Request::Accept),
            Call::Cancel => Ok(request::Request::Cancel),
            Call::Undo => Ok(request::Request::Undo),
            Call::Redo => Ok(request::Request::Redo),
            Call::Submit => Ok(request::Request::Submit),
        };
        Answer::of(self.answer(request).0)
    }

    /// A refusal of `request` with the runner's `request.invalid` code and
    /// `message`, closed with the session's state: for a request whose
    /// members (an MCP call's arguments) are not the request's own.
    pub fn refuse_invalid(&self, request: &'static str, message: &str) -> Answer {
        Answer::of(self.refusal(Some(request), request::INVALID, message))
    }

    /// The reply to one request line, and whether it was `quit`.
    fn handle(&mut self, line: &[u8]) -> (String, bool) {
        self.answer(request::read(line))
    }

    /// The reply to a request that was read, and whether it was `quit`.
    fn answer(&mut self, request: Result<request::Request, request::Refused>) -> (String, bool) {
        match request {
            Err(refused) => (
                self.refusal(refused.request, refused.code, &refused.message),
                false,
            ),
            Ok(request::Request::Status) => (self.status(), false),
            Ok(request::Request::Edit(action)) => (self.dispatch("edit", action), false),
            Ok(request::Request::Review) => (self.review(), false),
            Ok(request::Request::Accept) => (self.accept(), false),
            Ok(request::Request::Cancel) => (self.dispatch("cancel", Action::Cancel), false),
            Ok(request::Request::Undo) => (self.dispatch("undo", Action::Undo), false),
            Ok(request::Request::Redo) => (self.dispatch("redo", Action::Redo), false),
            Ok(request::Request::Submit) => (self.submit(), false),
            Ok(request::Request::Quit) => (self.finish(Reply::ok("quit")), true),
        }
    }

    /// Closes `reply` with the session's state.
    fn finish(&self, reply: Reply) -> String {
        reply.finish(&self.session)
    }

    /// A refusal of a line the runner could not take as a request, or of
    /// an action not in the proposal format's form: no engine code.
    fn refusal(&self, request: Option<&str>, code: &str, message: &str) -> String {
        self.finish(Reply::refused(request, code, None, None, message))
    }

    fn status(&self) -> String {
        let mut reply = Reply::ok("status");
        reply.string("actor", self.session.actor().as_str());
        reply.optional_string("client", self.settings.client.as_deref());
        reply.string("editor", &self.settings.editor);
        reply.string("receipt", &hex(&self.settings.receipt));
        let catalog = self.session.catalog();
        reply.string("catalog", &hex(&catalog.revision()));
        let document = self.session.history().document();
        reply.boolean(
            "catalog_verified",
            loader::revalidate(
                document.source(),
                catalog,
                self.session.resources(),
                PreviewLimits::default(),
            )
            .is_ok(),
        );
        let (plane, layer, bounds) = catalog.view();
        reply.view(plane, layer, bounds);
        reply.number("proposals", self.submitted);
        reply.boolean("recovery_failed", self.recovery_failed);
        self.finish(reply)
    }

    /// Dispatches an edit, Cancel, Undo or Redo, and replies with the
    /// pending change it leaves, if any.
    fn dispatch(&mut self, request: &'static str, action: Action) -> String {
        match self.session.dispatch(action, &mut self.recovery) {
            Ok(()) => {
                let mut reply = Reply::ok(request);
                self.pending(&mut reply);
                self.finish(reply)
            }
            Err(error) => self.finish(Reply::engine_refused(request, &error)),
        }
    }

    fn review(&self) -> String {
        let mut reply = Reply::ok("review");
        self.pending(&mut reply);
        self.finish(reply)
    }

    /// Adds the pending change's summary, or `null`, within the reply cap.
    fn pending(&self, reply: &mut Reply) {
        let summary = self.session.pending_summary();
        reply.pending(summary.as_ref(), self.reply_bytes);
    }

    fn accept(&mut self) -> String {
        let (undo, _) = self.session.history().depths();
        if self.recovery_failed {
            let code = RefusalCode::RecoveryFailed.as_str();
            return self.finish(Reply::refused(
                Some("accept"),
                code,
                Some(code),
                None,
                RECOVERY_LATCHED,
            ));
        }
        if let Err(error) = self.session.dispatch(Action::Accept, &mut self.recovery) {
            self.recovery_failed |= error.code() == RefusalCode::RecoveryFailed;
            return self.finish(Reply::engine_refused("accept", &error));
        }
        let mut reply = Reply::ok("accept");
        // An accepted edit or Redo adds a step; an accepted Undo removes one.
        let step = (self.session.history().depths().0 > undo)
            .then(|| self.session.applied_steps().last())
            .flatten();
        reply.applied(step.as_ref());
        self.finish(reply)
    }

    fn submit(&mut self) -> String {
        let proposal = match Proposal::from_session(
            &self.session,
            &self.settings.editor,
            self.settings.receipt,
            self.settings.client.as_deref(),
        ) {
            Ok(proposal) => proposal,
            Err(error) => return self.finish(Reply::proposal_refused("submit", &error)),
        };
        match self.store.retain(&proposal) {
            Ok(stored) => {
                self.submitted += 1;
                let mut reply = Reply::ok("submit");
                reply.proposal(&proposal, &stored);
                self.finish(reply)
            }
            Err(error) => self.finish(Reply::proposal_refused("submit", &error)),
        }
    }
}

fn check_settings(settings: &Settings) -> Result<(), OpenError> {
    if settings
        .client
        .as_ref()
        .is_some_and(|client| client.is_empty() || client.len() > CLIENT_BYTES)
    {
        return Err(OpenError(
            "a client label is 1 to 256 bytes; omit it for none".into(),
        ));
    }
    Ok(())
}

/// Most bytes of the client label: what a proposal records.
pub const CLIENT_BYTES: usize = tilewright_editor_proposal::limits::CLIENT_BYTES;
