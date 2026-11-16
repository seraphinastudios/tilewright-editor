// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

//! The MCP editing server (`docs/MCP-EDITING.md`): the sandbox runner's
//! requests offered as Model Context Protocol tools over stdio, so an MCP
//! client can edit inside one provisioned sandbox and submit a proposal.
//! `tools/provision-sandbox.py --mcp` makes the sandbox and starts
//! `tilewright-editor sandbox-mcp` in it.
//!
//! The protocol loop is the read-only server's ([`Endpoint`], revision
//! 2026-07-28 only); this crate gives it its own tools. The editing tools
//! answer through the runner's typed entry point
//! ([`Sandbox::respond`]), so each tool's structured content is exactly
//! the reply the runner's line protocol writes for the same request,
//! refusals with the runner's codes included. The observe tools are the
//! read-only server's, on the sandbox world, through an [`Observer`] that
//! is loaded again from the sandbox copy whenever its revision moved.
//!
//! No tool takes a path, a descriptor, an actor, a label or a receipt:
//! those are the operator's, given when the sandbox is provisioned. No
//! tool creates, lists or deletes a sandbox, or reaches a person's world.

mod action;
mod tools;

pub use action::action_schema;
pub use tools::{EDITING_TOOLS, INSTRUCTIONS, output_schema};

use std::path::Path;
use tilewright_editor_commands::RecoveryStore;
use tilewright_editor_mcp::{Encode, Endpoint, MAXIMUM_MESSAGE_BYTES, Observer, Tools};
use tilewright_editor_proposal::{hex, limits};
use tilewright_editor_sandbox::Sandbox;
use tilewright_jsonc::{Limits, Value};

/// Most bytes of one runner reply taken into a result. A result carries
/// it twice, as structured content and as its text copy, where each byte
/// of the reply costs at most two; a third of the read-only server's
/// result bound, less room for the members around them, keeps every
/// result within that bound.
pub const MAXIMUM_REPLY_BYTES: usize = 85 * 1024;

/// The parser's limits for one message: the read-only server's, widened
/// so that every action the proposal format records fits as its tools
/// take it (`params.arguments.action`, two levels deeper than the
/// runner's `action`), within the same 64 KiB line.
pub const MESSAGE_LIMITS: Limits = Limits {
    max_input_bytes: MAXIMUM_MESSAGE_BYTES,
    max_depth: 32,
    max_values: 3 * limits::CELLS + 10_000,
    max_array_items: limits::CELLS,
    max_object_keys: limits::OBJECT_KEYS,
    max_string_bytes: MAXIMUM_MESSAGE_BYTES,
    max_number_bytes: 128,
    max_comments: 0,
    max_comment_bytes: 0,
    max_lines: 1,
    max_line_bytes: MAXIMUM_MESSAGE_BYTES,
};

/// The editing server's tools: the sandbox, and the observe tools on its
/// world.
pub struct Editing<R> {
    sandbox: Sandbox<R>,
    observer: Observer,
    /// The sandbox revision the observer was last loaded at.
    observed: [u8; 32],
}

/// The MCP editing server over `sandbox`, whose world copy is at `world`,
/// reporting `version` in its `serverInfo` and encoding `render`'s images
/// with `encode`.
pub fn server<R: RecoveryStore>(
    mut sandbox: Sandbox<R>,
    world: &Path,
    version: &str,
    encode: Encode,
) -> Endpoint<Editing<R>> {
    sandbox.set_reply_limit(MAXIMUM_REPLY_BYTES);
    let observed = sandbox.revision();
    let editing = Editing {
        observer: Observer::open(world, encode),
        sandbox,
        observed,
    };
    Endpoint::new(
        editing,
        "Tilewright editor (sandbox editing)",
        version,
        "tilewright-editor sandbox-mcp",
        MESSAGE_LIMITS,
    )
}

impl<R: RecoveryStore> Editing<R> {
    /// The sandbox, read-only.
    pub fn sandbox(&self) -> &Sandbox<R> {
        &self.sandbox
    }

    /// The observer, loaded again first when the sandbox revision moved
    /// since it was last loaded: after an accepted step, an accepted undo
    /// or redo. A pending change is not observed: it is not published.
    fn observer(&mut self) -> &mut Observer {
        let revision = self.sandbox.revision();
        if revision != self.observed {
            self.observer.reload();
            self.observed = revision;
        }
        &mut self.observer
    }
}

impl<R: RecoveryStore> Tools for Editing<R> {
    fn instructions(&self) -> &str {
        INSTRUCTIONS
    }

    fn list(&self) -> String {
        tools::list()
    }

    fn call(&mut self, name: &str, arguments: Option<&Value>) -> Option<String> {
        match tools::Tool::named(name) {
            Some(tool) => Some(tools::call(tool, arguments, &mut self.sandbox)),
            None => self.observer().call(name, arguments),
        }
    }

    fn status(&self) -> String {
        // The actor follows the engine's rule: printable ASCII only.
        format!(
            "ready actor={} revision={}",
            self.sandbox.session().actor().as_str(),
            hex(&self.sandbox.revision())
        )
    }
}
