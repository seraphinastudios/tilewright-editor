# Change-set proposals

An agent edits only in its own provisioned sandbox, where it may prepare
and accept steps; what it hands over is a proposal. Only a person applies
a proposal to their world, one step at a time, through the normal Review
and Accept (issue #57, [ADR 0003](../decisions/0003-change-set-proposals.md)).
This page describes the format, its bounds and checks, and how a proposal
is applied, as the `tilewright-editor-proposal` crate implements them.
Making sandboxes, submitting from an agent and opening a proposal in the
desktop (what the person sees for each step: PR 6d) are later work;
nothing described here lets an agent publish to a person's world, and
nothing publishes except the person's own Accept.

## The document

A proposal is one UTF-8 file of strict JSON (`tilewright.editor-proposal/1`):

```json
{"body":{"actor":"agent:sandbox","base":"a07d…2bf4","catalog":"6156…360d","client":null,
 "editor":"0.1.0","receipt":"0707…0707","result":"7d06…281b","steps":[{"action":{
 "expected":"a07d…2bf4","kind":"move","uid":"marker_1","x":4,"y":2},"after":"7d06…281b",
 "before":"a07d…2bf4","placed":null,"transaction":"{\"actor\":\"agent:sandbox\",…}",
 "transaction_sha256":"6b16…d12d"}]},"body_sha256":"49e8…c67f","format":"tilewright.editor-proposal/1"}
```

(Shown wrapped and with digests shortened; the writer emits one line.)

Top level, exactly these members:

| Member | Value |
|---|---|
| `format` | `"tilewright.editor-proposal/1"`. |
| `body` | The body below. |
| `body_sha256` | SHA-256 of the body's exact bytes, 64 lowercase hexadecimal digits. |

The body, exactly these members:

| Member | Value |
|---|---|
| `editor` | The version string the writing editor build reports for itself, as the caller passes it to `Proposal::from_session`: today the workspace crate version (0.1.0 in every release), not the release tag. 1 to 64 printable ASCII characters, no space. Informational; nothing checks it against the reader. |
| `actor` | The actor of every step, a valid engine actor (1 to 256 printable ASCII characters). In a sandbox it comes from the operator's flag, never from the client. |
| `client` | `null`, or an untrusted label of at most 256 bytes, such as the name an MCP client gave itself. It is never an actor and is shown escaped (`Proposal::client_display`). |
| `base` | The World revision the first step starts from. |
| `result` | The World revision the last step ends at. |
| `catalog` | The revision of the editor catalog (`preview.jsonc`) the steps were made under. |
| `receipt` | The digest of the sandbox's provisioning receipt, recorded as given. |
| `steps` | The applied steps, oldest first: 1 to 256. |

Each step, exactly these members:

| Member | Value |
|---|---|
| `action` | The edit Action that prepared the step, in the typed form below. |
| `placed` | For `place`, the uid it assigned (a generated uid, `o.` and 26 base32 characters); `null` for every other action. |
| `transaction` | The sealed World transaction the step was accepted as, exactly, with the sandbox's actor. |
| `transaction_sha256` | SHA-256 of `transaction`'s bytes. |
| `before` | The World revision the step starts from. |
| `after` | The World revision it ends at. |

Revisions and digests are 64 lowercase hexadecimal digits.

### Actions

An action is an object with a `kind` and exactly the members of that kind,
each always present; an absent option is `null`. `expected` is the revision
its author saw. Coordinates are integers within ±(2^53 − 1), the World
format's range; layers fit 32 bits; strings are at most 4,096 bytes and
hold no NUL.

| `kind` | Members besides `kind` and `expected` |
|---|---|
| `move` | `uid`, `x`, `y` |
| `set_field` | `uid`, `field`, `value` (any JSON value, nested at most 8 deep and 64 KiB encoded, as the engine allows) |
| `paint` | `plane`, `layer`, `grid`, `x`, `y`, `proto` |
| `paint_stroke` | `plane`, `layer`, `grid`, `cells` (1 to 4,096 `[x, y]` pairs, distinct and in increasing order, the engine's own rule for a stroke), `proto` |
| `paint_zones` | `plane`, `layer`, `cells` (as for `paint_stroke`), `zone` (string or `null`) |
| `remove` | `uid` |
| `place` | `plane`, `layer`, `x`, `y`, `proto` |
| `rename_zone` | `plane`, `zone`, `name` (string or `null`) |
| `set_zone_parent` | `plane`, `zone`, `parent` (string or `null`) |
| `create_zone` | `plane`, `zone`, `name`, `parent` |
| `delete_zone` | `plane`, `zone` |
| `create_anchor`, `move_anchor` | `plane`, `anchor`, `x`, `y` |
| `delete_anchor` | `plane`, `anchor` |
| `set_portal` | `uid`, `target`: `null`, `{"kind": "anchor", "plane", "anchor"}` or `{"kind": "location", "plane", "layer", "at": [x, y]}` |

Selection, review, history and reopen actions (`Select`, `ClearSelection`,
`Accept`, `Cancel`, `Undo`, `Redo`, `Reopen`) have no form and are never in
a proposal.

### What a reader requires and what the writer emits

A reader takes any document the strict reader accepts: whitespace, member
order and string escapes are free. A number may be re-spelled only within
its kind: an integer is written without fraction or exponent (an integer
member refuses `4.0` and `4e0`), and other numbers keep their value. In a
field value, `1` (an integer) and `1.0` (a number) are different values,
so a tool that rewrites one as the other changes the action, and the step
then diverges or is refused when the proposal is applied. A reader refuses
comments, a byte-order mark, duplicate keys, unknown or missing members,
wrong types, non-finite numbers, integers beyond ±(2^53 − 1) and number
tokens longer than 128 bytes. The body digest covers the body's bytes
exactly as they are in the file, from its `{` to its `}`, so two writers
that format a number differently within its kind still read each other's
proposals. A decoded proposal's bytes (`Proposal::bytes`) are the body as
read in the writer's frame, so files that differ only outside the body are
the same proposal, with the same bytes and file name.

The writer emits one fixed form: minified, members in sorted order, no
trailing newline, strings escaping only `"`, `\` and characters below
U+0020, and numbers as `serde_json` writes them (for example `1e+16`,
`-0.0`). Tests pin these exact bytes, so a dependency change that alters
them is noticed.

A store keeps a proposal as `proposal-<body_sha256>.json`.

## Bounds

All in `tilewright_editor_proposal::limits`:

| Bound | Value |
|---|---|
| Steps | 256 (History keeps up to 1,024) |
| File | 16 MiB, refused before it is read (History may retain 64 MiB) |
| One step's transaction | 1 MiB |
| Files one step changes | 32, the engine's review limit, so each step can be reviewed completely |
| Action strings | 4,096 bytes |
| Stroke cells | 4,096, the brush's limit |
| Field values | 8 levels of nesting, 4,096 values (itself included) and 64 KiB encoded, the engine's budget; objects at most 256 members, World's reader limit |
| JSON values in one proposal | 256 × (3 × 4,096 + 64) + 64 = 3,162,176 (object keys are not counted); it follows from the bounds above, so no proposal within them reaches it. A hostile document just under it can take several hundred MB of memory for well under a second while it is read |
| Client label | 256 bytes |
| Editor version | 64 bytes |

## Building and reading

`Proposal::from_session(session, editor, receipt, client)` records a session's
applied steps (`Session::applied_steps`: the undo stack, so undone steps are
not in it). It is refused with pending.exists while a change is pending,
with session.blocked in a blocked session, with `proposal.empty` when no
step is applied, with `proposal.invalid` for a step the session did not
prepare itself, for an editor version that is empty or holds a space or a
character outside printable ASCII, or for a step that does not read back
with the action and uid it recorded, with `proposal.budget` for an editor
version over 64 bytes, with `proposal.chain` when the steps do not end at the
session's revision, and with `proposal.budget` over a bound. What it builds
is read back with `Proposal::decode` before it is returned.

`Proposal::decode` checks everything that needs no world, in this order,
and every `Proposal` value has passed all of it:

1. size, UTF-8 and strict JSON within the bounds (duplicate keys, comments,
   a byte-order mark, non-finite numbers and out-of-range integers refused);
2. the format tag and exactly the documented members, types and bounds,
   stroke cells included;
3. the body digest, then each step's transaction digest;
4. the chain: the first step starts at `base`, each step at the revision the
   one before ended at, the last ends at `result`, and no step leaves the
   revision unchanged; each action's `expected` is its step's `before`;
5. each transaction: a sealed World transaction (`base` and `result`
   recorded) by the proposal's actor that changes 1 to 32 files; a `place`
   step's transaction is one `object.insert` whose object has its `placed`
   uid and no inventory (the engine's Place never writes one), no other
   step records a uid, and no two steps place the same uid.

None of this shows that a step's transaction is what its Action
prepares: verification compares the Action's `expected` revision with the
step's `before`, and checks the transaction's seal, actor and changed
files and a placement's uid, but never that the transaction is what the
Action prepares. Only
dispatching the Action again when the proposal is applied, and comparing
what it prepares with the recorded transaction, establishes that.

## Applying

`Progress` is plain data (the proposal's body digest, the next step, the
prepared step and whether it is stopped) that a front end drives on the
person's session:

- `Progress::begin` requires the session to be at the proposal's `base` with
  nothing pending (`proposal.base_stale` otherwise: another world, a world
  that moved on, or a proposal already applied). This is the strict base:
  a proposal applies only to the exact revision it was made from, and is
  never rebased. If the person's catalog revision differs from the
  proposal's, it says so (`Notice::CatalogDiffers`) and goes on: every step
  is dispatched under the person's grants anyway.
- `Progress::resume` continues a proposal applied part-way (after the
  editor closed, a lost `Progress` or a stop). The session must have
  nothing pending and be at exactly one position of the proposal's chain:
  its base or one step's `after`; the next step is the one after it. A
  revision at no position is `proposal.base_stale`; one at several
  positions (a chain that returns to a revision it passed) is
  `proposal.resume_ambiguous`. Resuming widens nothing, since every
  remaining step is dispatched under the person's grants and reviewed, but
  the steps before count as applied only because the world is at their
  result revision: nothing records who applied them.
- `Progress::prepare_step` dispatches the next step's Action through
  `Session::dispatch` (a `place` through `Session::dispatch_placing` with
  its recorded uid), with the person's actor and catalog. An engine refusal
  is `proposal.step_refused` with the engine's own code beside it (for
  example grant.missing when the person's catalog does not grant what the
  sandbox's did, uid.unavailable when the uid is taken). The transaction it
  prepares must equal the recorded one in everything but the actor;
  otherwise the change is cancelled and the step is refused with
  `proposal.step_diverged`, naming the step and which members differ, with
  no world text. An equal step is pending for the person's normal Review
  and Accept. The review shows the pending diff of that freshly prepared
  change, never the recorded transaction text (the desktop's review is PR
  6d).
- `Progress::settle` (which `prepare_step` calls first) reads what the
  person did with the prepared step:
  - `Pending`: exactly the prepared change is still pending;
    `OtherPending`: something else is pending, the person's own change,
    which a front end must not label as the step;
  - `Applied`: the session is at the step's `after` revision with exactly
    the prepared transaction on top of its undo stack;
  - `Stopped`: the session is back at the step's base with its undo and
    redo stacks as they were, a Cancel; `proposal.closed` from then on;
  - `NotApplied`: back at the base otherwise, as after an Accept followed
    by an Undo; the step can be prepared again;
  - `proposal.base_stale` otherwise, for example when the person accepted
    an edit of their own after the step: nothing is stopped, and once the
    session is at the step's result with the step on top again (the person
    undid their edit) `settle` reports it applied.

A refused step (`proposal.base_stale`, `proposal.step_refused`,
`proposal.step_diverged`) leaves nothing pending, the world and its undo
stack as they were, and can be tried again; only the person's Cancel of a
prepared step stops a proposal. `pending.exists` and `session.blocked` are
given when the session cannot prepare a change at all, and
`proposal.invalid` when a `Progress` is used with another proposal than
the one it began. `next()` counts the steps settled as applied: when the
person later undoes a settled step, it is ahead of the world, and
preparing is `proposal.base_stale` until a Redo (or `resume`).

Every applied step carries the person's actor and is a normal undo entry
and recovery record. `Progress::proposal` gives the proposal's body digest
so a front end can record which proposal each step came from; the desktop
records it in its evidence (PR 6d). Nothing in this crate records it.

A proposal recorded by another editor build may diverge when it is
applied: a different title, member order or number spelling in the
prepared transaction is a difference. That fails safe
(`proposal.step_diverged`, nothing changed); the `editor` member tells a
person why. In practice a proposal applies where the person's editor
prepares identical transaction text, which is the same engine version.

### Equal but for the actor

The recorded transaction carries the sandbox's actor and the one prepared
again carries the person's. The actor is a top-level member of the
transaction text that nothing else depends on: `base` and `result` are
digests of file bytes, and files never hold the actor. Two transactions are
the same step when every other top-level member is the same value, in the
same order: format, title, every operation with every precondition
(`test`, `expect`) and value, and the digests of every file consulted and
written. Values are compared in the strict reader's canonical form, so
whitespace and comments do not matter, while member order does, and a
member on one side only is a difference. Both sides must have a string
actor. A recorded transaction with a member World's transaction format
does not have is already refused when the proposal is read
(`proposal.invalid`).

## What the digests protect

The body digest and each transaction digest detect corruption and
accidental edits. They do not prevent forgery: whoever writes a proposal,
including an agent that runs as the same user with a shell, can write
matching digests, and the recorded `receipt`, `actor` and `client` are
only what it says. Safety comes from three things instead: every step is
dispatched again through the engine under the person's own catalog grants;
the strict base, so a proposal applies only to the exact world it was made
from and cannot be replayed; and the person's Review and Accept of every
step. Nothing in the crate publishes on its own.

## The store

`ProposalStore::open(root)` takes a directory the caller already opened (in
a sandbox, the `proposals/` directory the provisioner will pass): the
current user's alone (mode 0700, no access control list) on a checked
local file system, locked for as long as the store is held. Linux only:
elsewhere it is refused with `proposal.store`, as recovery is.

`ProposalStore::retain` writes `proposal-<body digest>.json` with the
recovery store's sequence: stage beside it, verify the bytes and the
private 0600 access, check the name is still free, rename, sync the
directory (a sync that is not durable fails), verify again. The same bytes
already there are success and write nothing; other bytes, a link, a
directory or a leftover stage at the name are refused and left in place.
The lock keeps cooperating writers apart; like the recovery store's, it
does not stop a process of the same user that ignores it.

Precisely, with the World crates at 0.2.0: the store never writes to the
final name. `SourceDirectory::write` removes whatever is at the name it
writes before creating it, so the store writes only the stage
(`<name>.stage`), after checking nothing is there, and refuses (and keeps)
a leftover stage. `SourceDirectory::rename` replaces its target, so the
store renames only after checking the final name is free. Between that
check and the rename, and between the stage check and the stage write, a
process that ignores the lock can create an entry that the store then
replaces: a window of a few system calls, which only a same-user process
can use. A create-only write (`SourceDirectory::create`, expected in World
0.2.2 and not used here) would close the stage window; the final-name
window needs a rename that does not replace, which 0.2.0 does not offer.

`read_proposal(root, name)` reads a store's name only, at most 16 MiB
(larger is refused before it is read), refuses a link or anything but a
regular file, decodes and verifies it, and requires the name to carry the
body's digest.
