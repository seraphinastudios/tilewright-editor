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

Decision (the format and its checks, applying a proposal and the store, as
library code):

- A new crate, `tilewright-editor-proposal`, holds the format, its checks,
  applying and the store: applying needs `Session` and the store needs the
  World toolkit's confined file access, as the recovery crate does, while
  the ui crate stays free of file access. The
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
- Steps applied by a person carry the person's actor. The sealed
  transaction holds the actor only as its top-level `actor` member (its
  `base` and `result` are digests of file bytes), so a step prepared again
  must equal the recorded one in every other member, compared in the
  strict reader's canonical form and in order.
- A differing catalog revision is allowed and reported: every step is
  checked against the person's own grants anyway.
- A refused step (stale base, diverged, refused by the engine) leaves
  nothing pending and can be tried again; only the person's Cancel stops a
  proposal; a stale outcome found when a step is settled does not stop it
  either.
- A proposal applied part-way can be resumed from the one position of its
  chain the world is at (2026-11-15, coordinator decision, easy to
  revisit); `begin` stays strict-base. Resuming widens nothing, because
  every remaining step is dispatched again and reviewed.
- The store writes `proposal-<body digest>.json` with the recovery store's
  durable sequence. It never writes to the final name and refuses a
  different file there; same bytes are success. With World 0.2.0 a
  same-user process that ignores the lock can race the absence checks
  (docs/PROPOSALS.md says exactly where).
- The proposal codes join the engine's `RefusalCode` list.

The digests detect corruption, not forgery by a same-user agent; safety
comes from dispatching again under the person's grants, the strict base
and the person's review of every step. [docs/PROPOSALS.md](../docs/PROPOSALS.md)
describes the format. MCP editing tools and the desktop's proposal review
are later work, as are rebasing a proposal onto a newer world and
reviewing it as one change.

## Addendum: sandboxes and the headless runner (PR 6b, 2026-11-15)

The provisioning helper's sandbox mode and the headless runner
([docs/SANDBOX.md](../docs/SANDBOX.md)) implement owner decisions 3 and 4.
Decisions made with them (coordinator and PR worker, easy to revisit):

- The sandbox mode is a sibling entry point, `tools/provision-sandbox.py`,
  that loads the prototype helper's directory, mount, identity and copy
  code from beside it. `tools/provision-prototype.py` is unchanged, so the
  prototype mode's behaviour and output are byte for byte what they were.
- Each start makes a new child `sandbox-<UTC time>-<random>` of a fixed
  parent, so one launch configuration can be started again and again;
  sandboxes are never reused or deleted. A sandbox has `world/`,
  `recovery/` and `proposals/`, and no `evidence/`: the runner writes no
  evidence. The helper prints nothing on standard output and execs the
  runner, so the runner's streams are the client's.
- A sandbox actor must be `agent:` followed by a name; the helper and the
  runner both refuse anything else, so a sandbox step can never carry a
  person's or the prototype's actor. The client label is optional,
  untrusted, 1 to 256 bytes.
- The receipt digest reaches the runner as an argument from the trusted
  helper, which hashes the exact bytes it wrote; the runner holds no
  descriptor for the receipt.
- The runner is its own crate, `tilewright-editor-sandbox`, which names
  the session and the proposal crate; the read-only observe and MCP crates
  cannot name it. The request reader names no path type
  (`tools/check-architecture.sh`). Its line protocol is plain JSON lines,
  not MCP; PR 6c wraps it. An edit's action is the proposal format's
  Action form, so there is one wire form for Actions.
- Undo and redo in the runner mirror the engine: each prepares a reviewed
  change that `accept` publishes or `cancel` drops.
- After a submit the session goes on. A later submit records every applied
  step from the sandbox's start, a new proposal with a new digest; the
  same steps give the same file again, written once.
- Replies are capped at 256 KiB; lists that do not fit are cut from the
  end and counted, with a `cut` flag. Accept is always gated on the
  engine's own review.
- The bounded line reader moves from the MCP crate to the observe crate
  (`line`), unchanged, so both servers read lines the same way.
