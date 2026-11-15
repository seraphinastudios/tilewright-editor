// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The proposal document: building it from a session, the one form it is
//! encoding, and reading it back with every check that needs no world.
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session, uid::uid_material};
use tilewright_jsonc as jsonc;

use crate::{
    ProposalError, action::decode_action, action::encode_action, budget, hex, invalid, limits,
    sha256, unhex,
};

/// The format tag of a proposal document.
pub const FORMAT: &str = "tilewright.editor-proposal/1";

/// One applied step of a proposal.
#[derive(Clone, Debug, PartialEq)]
pub struct ProposalStep {
    action: Action,
    placed: Option<String>,
    transaction: String,
    transaction_sha256: [u8; 32],
    before: [u8; 32],
    after: [u8; 32],
}

impl ProposalStep {
    /// The edit action that prepared the step.
    pub fn action(&self) -> &Action {
        &self.action
    }
    /// For a placement, the uid it assigned.
    pub fn placed(&self) -> Option<&str> {
        self.placed.as_deref()
    }
    /// The sealed transaction the step was accepted as, with the actor of
    /// the session that accepted it.
    pub fn transaction(&self) -> &str {
        &self.transaction
    }
    pub fn transaction_sha256(&self) -> [u8; 32] {
        self.transaction_sha256
    }
    /// The World revision the step starts from.
    pub fn before(&self) -> [u8; 32] {
        self.before
    }
    /// The World revision it ends at.
    pub fn after(&self) -> [u8; 32] {
        self.after
    }
}

/// A verified proposal. There is no other way to make one than
/// [`Proposal::from_session`] and [`Proposal::decode`], and both verify
/// it, so every `Proposal` has passed every check that needs no world.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    editor: String,
    base: [u8; 32],
    result: [u8; 32],
    catalog: [u8; 32],
    actor: Actor,
    client: Option<String>,
    receipt: [u8; 32],
    steps: Vec<ProposalStep>,
    body_sha256: [u8; 32],
    bytes: String,
}

impl Proposal {
    /// Records the applied steps of `session` (its undo stack: undone
    /// steps are not in it) as a proposal. `editor` is the version the
    /// calling application was built as (1 to [`limits::EDITOR_BYTES`]
    /// printable ASCII characters, no space); `receipt` the digest of the
    /// sandbox's provisioning receipt; `client` an untrusted label, such
    /// as the name an MCP client gave itself.
    ///
    /// # Errors
    /// Refuses with `pending.exists` while a change is pending, with
    /// `session.blocked` in a blocked session, with `proposal.empty` when
    /// no step is applied, with `proposal.invalid` when a step was not
    /// prepared by this session or the editor version is not valid, with
    /// `proposal.chain` when the steps do not link to the session's
    /// revision, and with `proposal.budget` over a bound.
    pub fn from_session(
        session: &Session,
        editor: &str,
        receipt: [u8; 32],
        client: Option<&str>,
    ) -> Result<Self, ProposalError> {
        if session.history().needs_reload() {
            return Err(ProposalError::new(
                RefusalCode::SessionBlocked,
                "the session is blocked; record nothing from it",
            ));
        }
        if session.history().pending().is_some() {
            return Err(ProposalError::new(
                RefusalCode::PendingExists,
                "a change is pending: Accept or Cancel it before submitting",
            ));
        }
        let applied: Vec<_> = session.applied_steps().collect();
        let (Some(first), Some(last)) = (applied.first(), applied.last()) else {
            return Err(ProposalError::new(
                RefusalCode::ProposalEmpty,
                "no step is applied: a proposal needs at least one",
            ));
        };
        if applied.len() > limits::STEPS {
            return Err(budget(format!(
                "a proposal holds at most {} steps",
                limits::STEPS
            )));
        }
        // Kept although History makes it unreachable: a proposal must end
        // where the session is, whatever History does in future.
        if last.after != session.history().document().revision() {
            return Err(ProposalError::new(
                RefusalCode::ProposalChain,
                "the last applied step does not end at the session's revision",
            ));
        }
        let mut steps = Vec::with_capacity(applied.len());
        for (index, step) in applied.iter().enumerate() {
            let origin = step
                .origin
                .ok_or_else(|| invalid("the step was not prepared by this session").at(index))?;
            steps.push(ProposalStep {
                action: origin.action().clone(),
                placed: origin.placed().map(str::to_owned),
                transaction: step.transaction.to_owned(),
                transaction_sha256: sha256(step.transaction),
                before: step.before,
                after: step.after,
            });
        }
        let body = encode_body(&Fields {
            editor: editor_version(editor)?,
            base: first.before,
            result: last.after,
            catalog: session.catalog().revision(),
            actor: session.actor(),
            client,
            receipt,
            steps: &steps,
        })?;
        let bytes = frame(&body, &sha256(&body))?;
        // What is recorded is exactly what reading it back accepts.
        Self::decode(bytes.as_bytes())
    }

    /// Reads and verifies a proposal document: strict JSON with exactly the
    /// documented members, types and bounds, the body digest and every
    /// step's transaction
    /// digest, the chain of revisions from base through every step to
    /// result, and each step's agreement with its action and the actor.
    ///
    /// # Errors
    /// Refuses with `proposal.budget` over a bound (a document larger than
    /// [`limits::BYTES`] before reading it), `proposal.invalid` for a
    /// malformed document, `proposal.empty` for one
    /// without steps, `proposal.digest` for a digest that does not match,
    /// and `proposal.chain` for revisions that do not link.
    #[allow(clippy::too_many_lines)]
    pub fn decode(bytes: &[u8]) -> Result<Self, ProposalError> {
        if bytes.len() > limits::BYTES {
            return Err(budget("the proposal is larger than 16 MiB"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("a proposal is UTF-8 text"))?;
        let document = jsonc::Document::parse(text, &reader_limits()).map_err(|error| {
            use jsonc::ErrorCode::{
                ArrayLimit, DepthLimit, InputTooLarge, ObjectLimit, StringTooLong, ValueLimit,
            };
            let refuse = match error.code {
                ArrayLimit | DepthLimit | InputTooLarge | ObjectLimit | StringTooLong
                | ValueLimit => budget,
                _ => invalid,
            };
            if error.code == ValueLimit {
                return budget(format!(
                    "the proposal has more than {} JSON values",
                    limits::VALUES
                ));
            }
            refuse(format!(
                "the proposal is not strict JSON within its bounds ({})",
                error.code.as_str()
            ))
        })?;
        let root = document.root();
        if root.get("format").and_then(jsonc::Value::as_str) != Some(FORMAT) {
            return Err(invalid(format!("the format is not {FORMAT}")));
        }
        let members = exactly(root, &["format", "body", "body_sha256"])?;
        let body = members.get("body");
        let recorded_body = digest(members.get("body_sha256"), "body_sha256")?;
        let body = exactly(
            body,
            &[
                "editor", "base", "result", "catalog", "actor", "client", "receipt", "steps",
            ],
        )?;
        let editor = editor_version(body.text("editor", limits::EDITOR_BYTES)?)?;
        let actor =
            Actor::new(body.text("actor", tilewright_editor_ui::Actor::MAXIMUM_CHARACTERS)?)
                .map_err(|_| invalid("the actor is not a valid actor"))?;
        let client = match body.get("client") {
            jsonc::Value::Null { .. } => None,
            _ => Some(body.text("client", limits::CLIENT_BYTES)?.to_owned()),
        };
        let items = body
            .get("steps")
            .as_array()
            .ok_or_else(|| invalid("steps is an array"))?;
        if items.is_empty() {
            return Err(ProposalError::new(
                RefusalCode::ProposalEmpty,
                "the proposal has no steps",
            ));
        }
        if items.len() > limits::STEPS {
            return Err(budget(format!(
                "a proposal holds at most {} steps",
                limits::STEPS
            )));
        }
        let steps = items
            .iter()
            .enumerate()
            .map(|(index, item)| step(item).map_err(|error| error.at(index)))
            .collect::<Result<Vec<_>, _>>()?;
        let fields = Fields {
            editor,
            base: digest(body.get("base"), "base")?,
            result: digest(body.get("result"), "result")?,
            catalog: digest(body.get("catalog"), "catalog")?,
            actor: &actor,
            client: client.as_deref(),
            receipt: digest(body.get("receipt"), "receipt")?,
            steps: &steps,
        };
        // The digest covers the body's exact bytes, as the reader found
        // them; any layout the strict reader accepts is read.
        let body_sha256 = sha256(document.text(members.get("body").span()));
        if body_sha256 != recorded_body {
            return Err(ProposalError::new(
                RefusalCode::ProposalDigest,
                "the body digest does not match the body",
            ));
        }
        for (index, step) in steps.iter().enumerate() {
            if sha256(&step.transaction) != step.transaction_sha256 {
                return Err(ProposalError::new(
                    RefusalCode::ProposalDigest,
                    "the transaction digest does not match the transaction",
                )
                .at(index));
            }
        }
        verify(&fields)?;
        Ok(Self {
            editor: editor.to_owned(),
            base: fields.base,
            result: fields.result,
            catalog: fields.catalog,
            receipt: fields.receipt,
            client,
            actor,
            steps,
            body_sha256,
            bytes: text.to_owned(),
        })
    }

    /// The bytes of the document: as read, or as written by
    /// [`Proposal::from_session`].
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_bytes()
    }
    /// The file name a store keeps it under: `proposal-<body digest>.json`.
    pub fn file_name(&self) -> String {
        format!("proposal-{}.json", hex(&self.body_sha256))
    }
    pub fn body_sha256(&self) -> [u8; 32] {
        self.body_sha256
    }
    /// The version of the editor that wrote it, as recorded.
    pub fn editor(&self) -> &str {
        &self.editor
    }
    /// The World revision the first step starts from.
    pub fn base(&self) -> [u8; 32] {
        self.base
    }
    /// The World revision the last step ends at.
    pub fn result(&self) -> [u8; 32] {
        self.result
    }
    /// The revision of the editor catalog the steps were made under.
    pub fn catalog(&self) -> [u8; 32] {
        self.catalog
    }
    /// The actor of every recorded step.
    pub fn actor(&self) -> &Actor {
        &self.actor
    }
    /// The untrusted client label, unescaped. Use
    /// [`Proposal::client_display`] to show it.
    pub fn client(&self) -> Option<&str> {
        self.client.as_deref()
    }
    /// The client label with every character that would change how a
    /// terminal or line shows text escaped.
    pub fn client_display(&self) -> Option<String> {
        self.client
            .as_deref()
            .map(tilewright_editor_document::terminal_field)
    }
    /// The digest of the sandbox's provisioning receipt, as recorded.
    pub fn receipt(&self) -> [u8; 32] {
        self.receipt
    }
    pub fn steps(&self) -> &[ProposalStep] {
        &self.steps
    }
}

/// Everything recorded, borrowed, for encoding and verifying.
struct Fields<'a> {
    editor: &'a str,
    base: [u8; 32],
    result: [u8; 32],
    catalog: [u8; 32],
    actor: &'a Actor,
    client: Option<&'a str>,
    receipt: [u8; 32],
    steps: &'a [ProposalStep],
}

/// The form the writer emits for the body: minified JSON, members in
/// sorted order, `serde_json`'s escapes and number spellings. Readers do
/// not require it.
fn encode_body(fields: &Fields<'_>) -> Result<String, ProposalError> {
    if fields
        .client
        .is_some_and(|client| client.len() > limits::CLIENT_BYTES)
    {
        return Err(budget("the client label is too long"));
    }
    let mut steps = Vec::with_capacity(fields.steps.len());
    let mut total = 0usize;
    for (index, step) in fields.steps.iter().enumerate() {
        if step.transaction.len() > limits::TRANSACTION_BYTES {
            return Err(budget("the step's transaction is larger than 1 MiB").at(index));
        }
        total = total.saturating_add(step.transaction.len());
        if total > limits::BYTES {
            return Err(budget("the proposal is larger than 16 MiB"));
        }
        steps.push(json!({
            "action": encode_action(&step.action).map_err(|error| error.at(index))?,
            "placed": step.placed,
            "transaction": step.transaction,
            "transaction_sha256": hex(&step.transaction_sha256),
            "before": hex(&step.before),
            "after": hex(&step.after),
        }));
    }
    Ok(json!({
        "editor": fields.editor,
        "base": hex(&fields.base),
        "result": hex(&fields.result),
        "catalog": hex(&fields.catalog),
        "actor": fields.actor.as_str(),
        "client": fields.client,
        "receipt": hex(&fields.receipt),
        "steps": steps,
    })
    .to_string())
}

/// The document around an encoded body: its members in sorted order, no
/// whitespace and no trailing newline.
fn frame(body: &str, digest: &[u8; 32]) -> Result<String, ProposalError> {
    let text = format!(
        "{{\"body\":{body},\"body_sha256\":{},\"format\":{}}}",
        Value::from(hex(digest)),
        Value::from(FORMAT),
    );
    if text.len() > limits::BYTES {
        return Err(budget("the proposal is larger than 16 MiB"));
    }
    Ok(text)
}

/// The strict reader's limits for a proposal: its size and the deepest
/// nesting a field value can reach inside it.
/// Every cap follows from a documented bound, so no proposal within the
/// bounds meets a reader default first.
fn reader_limits() -> jsonc::Limits {
    jsonc::Limits {
        max_input_bytes: limits::BYTES,
        // The document, body, steps, step and action levels, then a field
        // value nested at most its own bound deep below its member.
        max_depth: 8 + limits::FIELD_VALUE_DEPTH,
        max_values: limits::VALUES,
        max_array_items: limits::CELLS
            .max(limits::STEPS)
            .max(limits::FIELD_VALUE_ENTRIES),
        max_object_keys: limits::OBJECT_KEYS,
        max_string_bytes: limits::TRANSACTION_BYTES,
        max_comments: 0,
        max_lines: limits::BYTES,
        max_line_bytes: limits::BYTES,
        ..jsonc::Limits::default()
    }
}

/// A valid editor version: 1 to [`limits::EDITOR_BYTES`] printable ASCII
/// characters, no space.
fn editor_version(editor: &str) -> Result<&str, ProposalError> {
    if editor.len() > limits::EDITOR_BYTES {
        return Err(budget("the editor version is too long"));
    }
    if editor.is_empty() || !editor.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(invalid(
            "the editor version is printable ASCII without spaces",
        ));
    }
    Ok(editor)
}

fn step(value: &jsonc::Value) -> Result<ProposalStep, ProposalError> {
    let members = exactly(
        value,
        &[
            "action",
            "placed",
            "transaction",
            "transaction_sha256",
            "before",
            "after",
        ],
    )?;
    let placed = match members.get("placed") {
        jsonc::Value::Null { .. } => None,
        _ => Some(members.text("placed", limits::TEXT_BYTES)?.to_owned()),
    };
    Ok(ProposalStep {
        action: decode_action(members.get("action"))?,
        placed,
        transaction: members
            .text("transaction", limits::TRANSACTION_BYTES)?
            .to_owned(),
        transaction_sha256: digest(members.get("transaction_sha256"), "transaction_sha256")?,
        before: digest(members.get("before"), "before")?,
        after: digest(members.get("after"), "after")?,
    })
}

/// Every check of a recorded proposal that needs no world.
fn verify(fields: &Fields<'_>) -> Result<(), ProposalError> {
    let chain = |message: &str| ProposalError::new(RefusalCode::ProposalChain, message);
    let mut revision = fields.base;
    let mut placed = BTreeSet::new();
    for (index, step) in fields.steps.iter().enumerate() {
        if step.before != revision {
            return Err(chain("the step does not start where the previous one ended").at(index));
        }
        if step.after == step.before {
            return Err(chain("the step does not change the revision").at(index));
        }
        if expected(&step.action) != step.before {
            return Err(chain("the action names another revision than the step's").at(index));
        }
        verify_transaction(step, fields.actor).map_err(|error| error.at(index))?;
        if let Some(uid) = &step.placed
            && !placed.insert(uid.as_str())
        {
            return Err(invalid("two steps place the same uid").at(index));
        }
        revision = step.after;
    }
    if revision != fields.result {
        return Err(chain("the last step does not end at the result"));
    }
    Ok(())
}

/// A step's transaction is a sealed World transaction by `actor` that
/// changes between 1 and [`limits::CHANGED_FILES`] files, and a placement
/// inserts one object with the uid it records and no inventory.
fn verify_transaction(step: &ProposalStep, actor: &Actor) -> Result<(), ProposalError> {
    let transaction =
        tilewright_edit::Transaction::parse("proposal step", step.transaction.as_str())
            .map_err(|_| invalid("the transaction is not a World transaction"))?;
    let (Some(base), Some(result)) = (transaction.base(), transaction.result()) else {
        return Err(invalid("the transaction is not sealed"));
    };
    let changed = base
        .keys()
        .chain(result.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| base.get(*path) != result.get(*path))
        .count();
    if changed == 0 {
        return Err(invalid("the transaction changes no file"));
    }
    if changed > limits::CHANGED_FILES {
        return Err(budget(
            "the transaction changes more files than one review shows",
        ));
    }
    // World read it; the strict reader reads it again for its members.
    let document = jsonc::Document::parse(step.transaction.as_str(), &jsonc::Limits::default())
        .map_err(|_| invalid("the transaction is not a World transaction"))?;
    let root = document.root();
    if root.get("actor").and_then(jsonc::Value::as_str) != Some(actor.as_str()) {
        return Err(invalid("the transaction's actor is not the proposal's"));
    }
    let inserted = root
        .get("operations")
        .and_then(jsonc::Value::as_array)
        .and_then(|operations| match operations {
            [operation]
                if operation.get("op").and_then(jsonc::Value::as_str) == Some("object.insert") =>
            {
                operation.get("object")
            }
            _ => None,
        });
    let insert = inserted
        .and_then(|object| object.get("uid"))
        .and_then(jsonc::Value::as_str);
    match (&step.action, step.placed.as_deref()) {
        (Action::Place { .. }, Some(uid)) => {
            if uid_material(uid).is_none() {
                return Err(invalid("the placed uid is not a generated uid"));
            }
            if insert != Some(uid) {
                return Err(invalid(
                    "the placement's transaction does not insert its uid",
                ));
            }
            // The engine's Place writes uid, prototype, layer and fields,
            // never an inventory, whose objects would carry uids of their
            // own.
            if inserted.is_some_and(|object| object.get("inventory").is_some()) {
                return Err(invalid("a placement inserts no inventory"));
            }
        }
        (Action::Place { .. }, None) => {
            return Err(invalid("a placement records the uid it assigned"));
        }
        (_, Some(_)) => return Err(invalid("only a placement records a uid")),
        (_, None) => {}
    }
    Ok(())
}

/// The revision an edit action was made against.
fn expected(action: &Action) -> [u8; 32] {
    match action {
        Action::Move { expected, .. }
        | Action::SetField { expected, .. }
        | Action::Paint { expected, .. }
        | Action::PaintStroke { expected, .. }
        | Action::PaintZones { expected, .. }
        | Action::Remove { expected, .. }
        | Action::Place { expected, .. }
        | Action::RenameZone { expected, .. }
        | Action::SetZoneParent { expected, .. }
        | Action::CreateZone { expected, .. }
        | Action::DeleteZone { expected, .. }
        | Action::CreateAnchor { expected, .. }
        | Action::MoveAnchor { expected, .. }
        | Action::DeleteAnchor { expected, .. }
        | Action::SetPortal { expected, .. } => *expected,
        // Decoding yields edit actions only.
        Action::Select { .. }
        | Action::ClearSelection
        | Action::Accept
        | Action::Cancel
        | Action::Undo
        | Action::Redo
        | Action::Reopen => [0; 32],
    }
}

/// The members of an object, checked to be exactly `keys`.
struct Members<'v> {
    members: &'v [jsonc::Member],
}
fn exactly<'v>(value: &'v jsonc::Value, keys: &[&str]) -> Result<Members<'v>, ProposalError> {
    let members = value
        .as_object()
        .ok_or_else(|| invalid("expected an object"))?;
    // The reader refuses duplicate keys.
    if members.len() != keys.len()
        || !members
            .iter()
            .all(|member| keys.contains(&member.key.as_str()))
    {
        return Err(invalid("an object has exactly its documented members"));
    }
    Ok(Members { members })
}
impl<'v> Members<'v> {
    fn get(&self, key: &str) -> &'v jsonc::Value {
        &self
            .members
            .iter()
            .find(|member| member.key == key)
            .expect("members were checked")
            .value
    }
    fn text(&self, key: &str, limit: usize) -> Result<&'v str, ProposalError> {
        let text = self
            .get(key)
            .as_str()
            .ok_or_else(|| invalid(format!("{key} is a string")))?;
        if text.len() > limit {
            return Err(budget(format!("{key} is too long")));
        }
        Ok(text)
    }
}
fn digest(value: &jsonc::Value, key: &str) -> Result<[u8; 32], ProposalError> {
    value
        .as_str()
        .and_then(unhex)
        .ok_or_else(|| invalid(format!("{key} is 64 lowercase hexadecimal digits")))
}
