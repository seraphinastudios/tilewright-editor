# Agent sandboxes

An agent edits a world only in its own sandbox: a fresh private copy that
the provisioning helper makes for one run. There it prepares, reviews and
accepts steps through the same editing engine the desktop uses, and submits
them as a proposal file. Only a person applies a proposal to their own
world, one step at a time, through the normal Review and Accept
([docs/PROPOSALS.md](PROPOSALS.md), [ADR 0003](../decisions/0003-change-set-proposals.md)).
Nothing in the runner's protocol lets an agent reach the person's world:
the runner opens nothing but the directories it is given, and Accept
publishes into the sandbox's own copy.

That confines what the runner's requests can do. It is not an
operating-system sandbox: a process running as the same user with a shell
or file tools can read and write the person's world, the sandbox's
receipt, `recovery/` and `proposals/` directly, and start a runner of its
own. Give such an agent only the runner, or its MCP tools
([docs/MCP-EDITING.md](MCP-EDITING.md)). Safety
against anything written outside the protocol rests on the person's review
when applying: every step is dispatched again under the person's grants,
on the exact base, and reviewed and accepted by the person
([what the digests protect](PROPOSALS.md#what-the-digests-protect)).

A sandbox serves one run of the runner. Once a step was accepted it cannot
be started again, so whatever was not submitted when the run ends is lost:
an agent submits after each batch of steps worth keeping (a later submit
records every step from the start again).

This page describes `tools/provision-sandbox.py`, which makes a sandbox and
starts the runner in it, and the runner's line protocol
(`tilewright-editor sandbox`, crate `tilewright-editor-sandbox`). Both are
Linux only, as editing is. With `--mcp` the helper starts the MCP editing
server instead, which offers the same requests as MCP tools
([docs/MCP-EDITING.md](MCP-EDITING.md)). A person applies a submitted
proposal in the desktop
([applying in the desktop](PROPOSALS.md#applying-in-the-desktop)).

The owner decided (2026-11-14 and 2026-11-15): an agent edits and accepts
only in its own provisioned sandbox and submits a proposal; no agent tool
makes sandboxes or touches the person's world; sandboxes come from the
provisioning helper, one per run, with a `proposals/` directory, and are
never deleted automatically; the agent's actor comes from the operator's
flag, and the name a client gives itself is only an untrusted label.

## Starting a sandbox

```sh
python3 tools/provision-sandbox.py \
  --world crates/tilewright-editor-testkit/fixtures/prototype-neutral \
  --destination-parent /absolute/private/sandboxes \
  --actor agent:example --client-label "example client" \
  --editor target/debug/tilewright-editor
```

The command is fixed, so an operator can give it to a client as the command
that starts its server: every start makes a new sandbox.

| Option | |
|---|---|
| `--destination-parent DIR` | An existing directory on persistent local storage. Each start makes a new child of it. |
| `--world DIR` | The world to copy, as data. It is only read. |
| `--actor agent:<name>` | The actor of every step: `agent:` and a name, 1 to 256 printable ASCII characters in all, not ending with a space (the engine's actor rule, with the prefix required, so a sandbox step can never pass for a person's). |
| `--client-label TEXT` | Optional. An untrusted label, 1 to 256 bytes of UTF-8, recorded in proposals as given and shown escaped. It is never an actor. |
| `--editor PATH` | The trusted editor executable. Without it the helper makes the sandbox and stops. |
| `--mcp` | Start the MCP editing server, `tilewright-editor sandbox-mcp`, with the same options, instead of the runner ([docs/MCP-EDITING.md](MCP-EDITING.md)). Needs `--editor`. |

The helper:

1. checks the actor and the label, that the platform is Linux, that the
   editor exists, that the world opens as a directory and is not a link
   (it keeps that descriptor and copies from it, so a path changed after
   the check is never read), that the parent is not the world or inside
   it, and that the parent is on persistent local storage (it refuses
   tmpfs, ramfs, overlay, network and unknown file systems), before it
   creates anything;
2. creates a new child `sandbox-<UTC time>-<16 random hex digits>` with
   mode 0700, never an existing one, and in it `world/`, `recovery/` and
   `proposals/`, each 0700, and checks that the four are distinct;
3. copies the world, from the descriptor it checked, into `world/` with
   exactly the prototype helper's rules
   and limits (`tools/provision-prototype.py`: regular files and
   directories only, no links or special files, at most 4,096 entries and
   64 MiB, files 0600, everything synced);
4. writes the receipt `provision.json` (0600, synced), then one line to
   standard error;
5. checks the directories' identities and mounts again, and replaces itself
   with `tilewright-editor sandbox`, passing `world/`, `recovery/` and
   `proposals/` as open descriptors, with the actor, the label and the
   receipt's SHA-256. Nothing else crosses: it lists its open descriptors
   in `/proc/self/fd` and closes every one above 2 but the three, its own
   (the sandbox itself and its parent among them) and any it inherited from
   whoever started it, whatever their number, above a lowered open-file
   limit too. If `/proc/self/fd` cannot be listed it starts nothing (the
   runner reaches its directories through `/proc/self/fd/N` anyway).

It prints nothing on standard output (only `--help`, which an operator
asks for, does): that is the runner's protocol, and
the runner's standard input and output are the helper's own. The line on
standard error is `tilewright sandbox: ` and a JSON object with ASCII
escapes, `{"sandbox": ..., "receipt": ..., "receipt_sha256": ...}`, at most
8 KiB (paths too long for that are left out, and the sandbox's name is
given instead). A refusal is one line on standard error, its detail
escaped the same way, and a non-zero exit.

A sandbox is never reused, adopted or deleted, not even after a failure
part-way: what was made is kept for inspection. Remove old sandboxes by
hand when they are no longer wanted.

```text
sandbox-20261115T182752Z-48f54df1c1ee2413/
  provision.json      the receipt
  world/              the copy the runner edits; Accept publishes here
  recovery/           the inverse of every accepted change
  proposals/          proposal-<body digest>.json, one per distinct submit
```

The receipt (`tilewright.sandbox-provision/1`) records the platform, the
identity of the four directories (device, inode, owner, mode and mount),
what was copied (files and bytes), the actor and the label. Its SHA-256 is
the `receipt` every proposal from the sandbox records. The prototype
helper and its receipt are unchanged.

## The runner

`tilewright-editor sandbox --world-fd N --recovery-fd N --proposals-fd N
--actor agent:<name> --receipt-sha256 HEX [--client-label TEXT]` is started
by the helper only. It takes each option once and nothing else, no path at
all, and refuses to start without the three descriptors (distinct, none a
standard stream), on another platform than Linux, with an actor that lacks
the `agent:` prefix, or when a store refuses: the recovery directory must
be private (0700, no access control list), on a checked local file system,
and empty, and the proposal directory private and checked; each is locked
for the run, so a second runner on the same sandbox is refused. The world
is opened with the prototype's source budget, its catalog with the preview
loader, and a session with the given actor.

Standard error gets one line when the runner is ready
(`tilewright-editor sandbox: ready actor=... revision=...`, escaped), and
one line for each request line too long to read. Nothing else is written
there.

## The line protocol

Each request is one JSON object on one line of standard input; each gets
exactly one reply, one JSON object on one line of standard output, in
order, flushed. A line may end in CR LF. Requests are read with World's
strict JSON reader: no comments, no duplicate keys, no trailing data. A
request has the member `request` and, for `edit`, `action`; any other
member is refused. This is a plain line protocol, not MCP: the MCP editing
server ([docs/MCP-EDITING.md](MCP-EDITING.md)) serves the same requests,
through the same code, as MCP tools.

Every reply has `ok` (whether the request was done), `request` (the
request's name, or `null` when the line named no known request) and
`state`, the session after the request:

```json
"state":{"revision":"…","phase":"ready","pending":false,"undo":1,"redo":0}
```

`revision` is the sandbox world's committed source revision; `phase` one of
`ready`, `pending`, `retaining`, `publishing`, `blocked`; `pending` whether
a change is pending review; `undo` and `redo` History's depths. `undo` is
also the number of steps a `submit` records.

A refusal (`"ok":false`) has `code`, a stable code; `engine`, the engine's
own code when the engine decided the refusal (the same as `code` for an
engine refusal, the engine's code beside `proposal.step_refused`, and
`null` for the runner's `request.*` codes and the proposal crate's own
`proposal.*` codes); `step`, the proposal step concerned, from 0, or
`null`; and `message`, for people (at most 4 KiB before escaping; it may
quote world text, as data).

A refusal changes nothing and the session goes on, with one exception: a
refusal that leaves the session blocked. See below.

### A blocked sandbox

A publication that fails or cannot be verified after the inverse was
retained (`publish.failed`, `publish.unverified`), or anything else that
leaves World's history needing a reload, blocks the session: `state.phase`
is `blocked`, and from then on every request but `status`, `review` and
`quit` is refused with `session.blocked`, `submit` among them. The steps
accepted since the last `submit` can then never be submitted from this
sandbox, and the sandbox cannot be started again: its recovery store holds
records, and the runner refuses a store that is not empty. The same holds
for a runner killed during an `accept`, from the moment it begins to retain
the inverse. The way forward is a new sandbox (and a new proposal); the
old one is kept for inspection.

A retention that fails (`recovery.failed`, at the recovery store's limits
of 1,024 records or 64 MiB too) does not block the session: the change
stays pending, and `status`, `review`, `cancel`, `undo`, `redo` and
`submit` still work. But the recovery store refuses every record after its
first failure, so no later `accept` in this run can succeed; the runner
answers every later `accept` at once with `recovery.failed` and says so,
and `status` reports `recovery_failed: true`. Cancel, `submit` the steps
so far, and start a new sandbox.

Every run ends this way in one respect: once a step was accepted, the
sandbox's recovery store holds records, so the sandbox cannot be started
again after the run ends, whether by `quit`, the end of input, a crash or
a block. Steps not submitted when the run ends cannot be submitted later.

So an agent should `submit` after each batch of steps worth keeping: a
later submit records every step from the start again, so nothing is lost
by submitting often.

| Request | Does | Reply members besides `ok`, `request`, `state` |
|---|---|---|
| `status` | Nothing. | `actor`, `client` (the label or `null`), `editor` (the version string the binary reports for itself, as `tilewright-editor version` prints it: the release version in a package, `0.1.0-dev` in any other build), `receipt` (the receipt's SHA-256), `catalog` (the catalog revision), `catalog_verified` (whether the catalog and its resources still verify), `view` (`plane`, `layer`, `bounds` `[x, y, width, height]`: where grants apply), `proposals` (how many submits this run retained or found retained), `recovery_failed` (whether the recovery store refused a record in this run, after which no `accept` can succeed). |
| `edit` | Prepares one edit: `action` is one Action in the proposal format's form ([docs/PROPOSALS.md](PROPOSALS.md#actions)), the same form a proposal records, so there is one Action form. Its `expected` must be the current `state.revision`. | `pending`: the change now pending. |
| `review` | Nothing. | `pending`: the pending change, or `null`. |
| `accept` | Accepts the pending change through the engine's gate (refused while its review is incomplete: 64 KiB of excerpt, 32 files), retains its inverse in `recovery/` and publishes it into `world/`. | `applied`: the step it added (`before`, `after` and `placed`: the uid a `place` assigned, the list of uids a `place_objects` assigned in object order, else `null`), or `null` when it removed one (an accepted undo). |
| `cancel` | Drops the pending change; writes nothing. With nothing pending it does nothing, as in the desktop. | `pending`: `null`. |
| `undo` | Prepares the undo of the last step, as a reviewed change: `accept` publishes it, `cancel` drops it. | `pending`. |
| `redo` | Prepares the redo of the last undone step, the same way. | `pending`. |
| `submit` | Records every applied step (the undo stack) as a proposal and retains it in `proposals/`. | `proposal`: `file`, `body_sha256`, `bytes`, `steps`, `base`, `result`, and `existed` (whether that file was already there). |
| `quit` | Ends the run after replying. The end of standard input does too. The sandbox cannot be started again once a step was accepted, so submit first: steps not submitted are lost. | None. |

`pending` holds the engine's bounded summary of the pending change:
`base` and `result` (revisions), `complete` (whether Accept may publish
it), `files` (each `path`, `change` `created`, `modified` or `removed`,
`before_bytes`, `after_bytes`), `files_omitted`, `warnings` (the review's
notes and deletion warnings), `warnings_omitted`, `review` (the review
lines exactly as a person sees them in the desktop), `review_omitted` and
`cut`. When a reply would pass 256 KiB, the runner keeps the first files,
then warnings, then review lines that fit, counts the rest in the
`_omitted` members and sets `cut`; the engine's own omissions are counted
there too. Accept is gated on the engine's review, never on what fitted in
the reply.

Undo and redo mirror the engine: each prepares a pending change that must
be accepted (or cancelled), exactly as in the desktop, so every change to
the sandbox, an undo included, is reviewed before it is published. An
undone step is not in a proposal; a redone step is, with its original
Action.

After a `submit` the session goes on: more edits, undos and submits. A
later `submit` records every applied step from the sandbox's start, so it
is a new proposal with a new digest and file, while the same steps give the
same file again (`existed: true`, nothing written). Every proposal from one
sandbox has the same base; a person applies one of them.

### Codes

The engine's codes ([docs/ENGINE.md](ENGINE.md#refusal-codes)) come back
as they are, for example `grant.missing`, `revision.stale`,
`pending.exists` (an edit, undo, redo or submit while a change is
pending), `pending.none` (accept with nothing pending),
`review.incomplete`, `history.nothing_to_undo`. An `action` that is not
in the proposal format's form is refused with that format's own code,
`proposal.invalid` (or `proposal.budget` for a string, stroke or value over
its bound). `submit` is refused with the proposal crate's codes:
`proposal.empty` with no step applied, `pending.exists`, `session.blocked`
in a blocked sandbox, `proposal.budget` past 256 steps or 16 MiB (undo
steps and submit again), `proposal.store` when the file cannot be
retained. Every request but `status`, `review` and `quit` can be refused
with `session.blocked`.

The runner's own codes, for lines that are not a request it can read:

| Code | The line |
|---|---|
| `request.too_large` | is longer than 64 KiB; it is skipped to its newline without being kept. |
| `request.not_utf8` | is not UTF-8. |
| `request.parse` | is not one strict JSON value (an empty line among them); the message names World's reader code. |
| `request.invalid` | is not an object, has no string `request`, or has a member its request does not take. |
| `request.unknown` | names a request that does not exist. |

### Limits

| Limit | Value |
|---|---|
| Request line | 64 KiB, without its newline. |
| Request JSON | One line, no comments, 4 + 8 levels deep, 12,352 values, 4,096 array items and 256 object members, strings and numbers as the proposal reader allows; large enough for every action a proposal records. |
| Reply line | 256 KiB; lists are cut to fit, as above. |
| Message | 4 KiB before escaping. |
| Review | The engine's: 64 KiB of excerpt and 32 files; past them Accept is refused. |
| Proposal | 256 steps and 16 MiB. |
| History and recovery | The engine's and the recovery store's: 1,024 records, 64 MiB. Past them the store refuses (`recovery.failed`) and no later `accept` in the run can succeed. |

### World text is data

Every string in a reply, world text among it, is written by the observe
crate's JSON string writer: quotes, backslashes, control characters and
invisible, bidirectional and other characters that change how text reads
are escaped (`\uXXXX`), so a reply is always one line of printable JSON and
world text cannot end a string or a line. The review lines are the
engine's, which already mark such characters. Lines on standard error are
escaped too: the runner's with the editor's terminal escaping, the
helper's as JSON with ASCII escapes.

## What the runner can touch

These hold for what the runner's requests can do; as said at the top, a
same-user process outside the protocol is not confined by them, and the
person's review when applying is the safeguard against it.

- **Only its sandbox.** It receives the three directories as descriptors
  and opens no path a request names: no request carries a path (tests pin
  each request's members, and refuse `path`, `file`, `root`, `fd` and any
  other extra member), and `tools/check-architecture.sh` refuses path,
  root and environment types in the module that reads requests and the
  standard library's file, network and process modules and the
  environment in the whole crate. It never sees the sandbox's parent or
  receipt, nor any descriptor the helper inherited.
- **Only a provisioned tree.** The recovery store refuses a directory that
  is not private, not on checked storage or not empty, and the proposal
  store one that is not private and checked, so pointing the runner at a
  used or shared tree fails before anything is read.
- **Accept publishes into the copy.** As in the prototype, through World's
  transactions under the session's retained root.
- **Submit writes one proposal file** through the proposal store
  (`proposal-<body digest>.json`, staged, verified, renamed, synced), the
  same bytes once.
- **Nothing else.** The runner writes only in `world/`, `recovery/` and
  `proposals/`; the helper writes `provision.json` and the copy.

Tests: `tools/test_sandbox_provision.py` (fresh unique children, refusals
before any state, nothing on standard output, the receipt, the
descriptors and streams the runner gets); the `tilewright-editor-sandbox`
crate's tests (every request, the engine's refusals, malformed and
oversized lines, hostile world text, the review gate, cut replies, the
256-step bound, the stores' refusals, random lines that each get exactly
one reply); and `crates/tilewright-editor-app/tests/sandbox.rs`, which
provisions a sandbox, drives the binary over its streams, submits, and
applies the proposal step by step to a second provisioned copy under a
person's actor, ending with byte-identical worlds and an untouched source.

## A short session

Trimmed (`…` for digests and review lines):

```text
> {"request":"status"}
< {"ok":true,"request":"status","actor":"agent:docs","client":"example client","editor":"0.1.0-dev","receipt":"310c…","catalog":"6156…","catalog_verified":true,"view":{"plane":"main","layer":0,"bounds":[0,0,8,6]},"proposals":0,"recovery_failed":false,"state":{"revision":"3345…","phase":"ready","pending":false,"undo":0,"redo":0}}
> {"request":"edit","action":{"kind":"move","uid":"marker_1","x":4,"y":2,"expected":"3345…"}}
< {"ok":true,"request":"edit","pending":{"base":"3345…","result":"7f41…","complete":true,"files":[{"path":"planes/main/layers/0/0_0.jsonc","change":"modified","before_bytes":4057,"after_bytes":4057}],"files_omitted":0,"warnings":[],"warnings_omitted":0,"review":["planes/main/layers/0/0_0.jsonc / source excerpt; scroll for more",…],"review_omitted":0,"cut":false},"state":{"revision":"3345…","phase":"pending","pending":true,"undo":0,"redo":0}}
> {"request":"accept"}
< {"ok":true,"request":"accept","applied":{"before":"3345…","after":"7f41…","placed":null},"state":{"revision":"7f41…","phase":"ready","pending":false,"undo":1,"redo":0}}
> {"request":"edit","action":{"kind":"paint","plane":"main","layer":0,"grid":"floor","x":0,"y":0,"proto":"stone","expected":"7f41…"}}
< {"ok":false,"request":"edit","code":"grant.missing","engine":"grant.missing","step":null,"message":"terrain capability or bounds refused","state":{"revision":"7f41…","phase":"ready","pending":false,"undo":1,"redo":0}}
> {"request":"submit"}
< {"ok":true,"request":"submit","proposal":{"file":"proposal-066a…json","body_sha256":"066a…","bytes":1781,"steps":1,"base":"3345…","result":"7f41…","existed":false},"state":{"revision":"7f41…","phase":"ready","pending":false,"undo":1,"redo":0}}
> {"request":"quit"}
< {"ok":true,"request":"quit","state":{"revision":"7f41…","phase":"ready","pending":false,"undo":1,"redo":0}}
```

## Not here

A blocked session's Reopen (start a new sandbox instead); rebasing
proposals; Windows and macOS editing.
