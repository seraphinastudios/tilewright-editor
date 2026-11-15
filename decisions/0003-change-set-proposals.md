# ADR 0003: change sets as proposals applied step by step

Status: accepted.

Issue #57 lets agents edit worlds. The owner decided on 2026-11-14 that an
agent edits and accepts only in its own provisioned copy (a sandbox) and
submits a proposal; only a person applies a proposal to their world,
through Review and Accept, and no agent tool publishes to the project. On
2026-11-15 the owner decided how:

1. A proposal step holds the typed Action and its sealed transaction.
   Applying a proposal dispatches each Action again under the person's
   grants and requires the result to equal the recorded transaction. A
   proposal applies only to the exact revision it was made from (strict
   base).
2. The person reviews it step by step through the normal Review and Accept.
3. Sandboxes are made by the provisioning helper, one per run, with a
   `proposals/` directory; no agent tool makes sandboxes; the person points
   the desktop at the proposal file; nothing is deleted automatically.
4. The agent's actor comes from the operator's flag; the client's
   self-reported name is an untrusted label; steps a person applies carry
   the person's actor, and the evidence maps them to the proposal.

Decision (the first part: the format and its checks, as library code):

- A new crate, `tilewright-editor-proposal`, holds the format and its
  checks, and will hold applying and the store: applying needs `Session`
  and the store needs the World toolkit's confined file access, as the
  recovery crate does, while the ui crate stays free of file access. The
  read-only observe and MCP crates still name neither sessions nor
  proposals. The crate uses only dependencies the workspace already has;
  SHA-256 comes from the World toolkit's own digest.
- History gains a read-only `History::applied` (its undo stack). The
  Session keeps each step's origin (its Action, actor and placed uid)
  beside History's undo and redo stacks and moves it exactly as History
  moves the entry, so an undone step leaves the list and a redone one
  returns with its Action. Nothing published or recorded changes.
- `Session::dispatch_placing` prepares a placement with a given uid, for
  one dispatch, so a recorded placement can be prepared again exactly.
- Steps applied by a person will carry the person's actor. The sealed
  transaction holds the actor only as its top-level `actor` member (its
  `base` and `result` are digests of file bytes), so a step prepared again
  can equal the recorded one in every other member.
- The proposal codes join the engine's `RefusalCode` list.

The digests detect corruption, not forgery by a same-user agent; safety
comes from dispatching again under the person's grants, the strict base
and the person's review of every step. [docs/PROPOSALS.md](../docs/PROPOSALS.md)
describes the format. Applying a proposal and the proposal store, the
provisioner's sandbox mode and a headless runner, MCP editing tools and the
desktop's proposal review are later work, as are rebasing a proposal onto a
newer world and reviewing it as one change.
