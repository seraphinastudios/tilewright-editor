# Change-set proposals

An agent edits only in its own provisioned sandbox, where it may prepare
and accept steps; what it hands over is a proposal. Only a person applies
a proposal to their world, one step at a time, through the normal Review
and Accept (issue #57, [ADR 0003](../decisions/0003-change-set-proposals.md)).
This page describes the format, its bounds and checks, and how a proposal
is applied, as the `tilewright-editor-proposal` crate implements them.
Sandboxes and submitting from an agent are in [docs/SANDBOX.md](SANDBOX.md):
the provisioning helper's sandbox mode and the headless runner, whose
`submit` builds a proposal with `Proposal::from_session` and retains it in
the sandbox's `proposals/` directory, and in
[docs/MCP-EDITING.md](MCP-EDITING.md): the same requests as MCP tools, whose
`edit` takes an Action in this format's form. The desktop opens a proposal
and offers its steps one at a time ([Applying in the desktop](#applying-in-the-desktop));
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
| `actor` | The actor of every step, a valid engine actor (1 to 256 printable ASCII characters). In a sandbox it comes from the operator's flag (`agent:<name>`, [docs/SANDBOX.md](SANDBOX.md)), never from the client. |
| `client` | `null`, or an untrusted label of at most 256 bytes, such as the name an MCP client gave itself. It is never an actor and is shown escaped (`Proposal::client_display`). |
| `base` | The World revision the first step starts from. |
| `result` | The World revision the last step ends at. |
| `catalog` | The revision of the editor catalog (`preview.jsonc`) the steps were made under. |
| `receipt` | The digest of the sandbox's provisioning receipt, recorded as given: in a sandbox, the SHA-256 of its `provision.json`. |
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
| `fill_terrain` | `plane`, `layer`, `grid`, `x`, `y`, `proto`, `contiguous` (`true` or `false`) |
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

A `fill_terrain` step records the fill as it was meant (where it starts,
the terrain, fill or replace), not its cells: the engine works the cells
out from the world each time the step is prepared ([docs/ENGINE.md](ENGINE.md#fill-and-replace)),
so on the world it was recorded from it gives the same transaction again.
An editor from before `fill_terrain` refuses a whole proposal that holds
one, with `proposal.invalid` (an unknown action kind) when it reads it,
before anything is prepared: such a proposal needs an editor release that
has the fill tool.

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
  change, never the recorded transaction text (the desktop's review below).
- `Progress::settle` (which `prepare_step` calls first) reads what the
  person did with the prepared step:
  - `Pending`: exactly the prepared change is still pending;
    `OtherPending`: something else is pending, the person's own change,
    which a front end must not label as the step;
  - `Applied`: the session is at the step's `after` revision with exactly
    the prepared transaction on top of its undo stack;
  - `NotApplied`: back at the step's base with exactly the prepared
    transaction on top of the redo stack, as after an Accept followed by
    an Undo; the step can be prepared again;
  - `Stopped`: back at the base otherwise, which is a Cancel (also when
    the person then accepted and undid an edit of their own: that edit, not
    the step, is on the redo stack); `proposal.closed` from then on;
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
the one it began.

What a front end does in each case:

- `next()` counts the steps settled as applied. When the person later
  undoes a settled step, it is ahead of the world and preparing is
  `proposal.base_stale`: a Redo of that step, or `resume` on a new
  `Progress`, continues.
- After `NotApplied`, preparing the step again offers it for review. If the
  person instead Redoes it, `next()` is behind the world and preparing is
  `proposal.base_stale`: `resume` on a new `Progress` continues after it.
- A `Progress` belongs to one session, whose undo and redo stacks it reads.
  After the session is reopened, make a new `Progress` with `resume`. A
  `Progress` kept across a reopen reads a step accepted before it as
  `proposal.base_stale` for good (the new session's undo stack is empty)
  and a pending step lost with the old session as `Stopped`.
- After a `Stopped`, `resume` on a new `Progress` continues if the person
  wants to after all: it is the same strict base check and review.

Every applied step carries the person's actor and is a normal undo entry
and recovery record. `Progress::proposal` gives the proposal's body digest
so a front end can record which proposal each step came from; the desktop
records it in its evidence (below). Nothing in this crate records it.

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

## Applying in the desktop

The person starts the desktop with the proposal file (Linux only):

```sh
python3 tools/provision-prototype.py --world WORLD --destination NEW-SESSION \
  --editor target/debug/tilewright-editor --font-pack /absolute/font-pack \
  --proposal SANDBOX/proposals/proposal-<digest>.json --actor human:<name>
```

The helper reads and checks the file before it creates anything (a regular
file, not a link, at most 16 MiB, named `proposal-<64 hex>.json`), copies
its bytes into a private `proposal/` child of the new session, records its
name, size and SHA-256 in the receipt and passes the editor that directory
as a descriptor with `--proposal-fd` and `--proposal-name`. The sandbox's
file is never written or kept open. `--actor human:<name>` names the person
as the actor of every change the session accepts; without it the actor is
`editor:prototype`, as before. The editor reads the copy with
`read_proposal`; a refusal stops it at start-up with the code
(`proposal refused: proposal.digest: …`; also `proposal.store`,
`proposal.budget`, `proposal.invalid`).

At start-up the desktop begins the proposal when the world is at its base,
resumes it when the world is at exactly one step's result, and otherwise
shows that it does not apply here, with both revisions; normal editing
stays available either way.

**The panel.** A Proposal block at the top of the sidebar, while a proposal
is open. Every row puts the fact that matters first, so a one-line cut
never hides it:

- "Proposal <short digest>, recorded actor: <actor>": the file lives where
  the agent can write, so the actor is only what it says; a long actor is
  what gets cut;
- "Recorded actor is not an agent actor", when it does not start with
  `agent:`; the proposal is still offered (the person decides);
- "Client label (unverified): …", escaped;
- "Recorded by this editor build (0.1.0)", or "Recorded by another editor
  build (0.2.0; this is 0.1.0): steps may diverge";
- where the world is ("World: at its base", "after step j of n", "not on
  its path", "at N places on its path");
- "Step k of n:" and the step in words;
- the state line;
- "Applied k of n, remaining m";
- "Catalog differs; your grants apply", when it does.

Only the current step is described, so a 256-step proposal reads the same
as a short one.

The step's words come from its decoded Action and from the person's
committed world, never from the recorded transaction. They say what the
step does, to which id and where to first ("Move marker_1 to (4, 2), from
(3, 2), on main/0", "Set label on marker_1 to …", "Create zone Z under P,
named N, on main", "Fill from (5, 0) with grass: 23 cells of sand (floor on
main/0)", "Replace every sand in the view with grass: 23 cells, from
(5, 0) (floor on main/0)"), then the rest; from the world
they add where an object or anchor being moved or deleted is now, how many
objects a deleted object carries ("and the 3 objects it carries"), what a
fill's start cell holds and how many cells the engine's fill would change
there ("no change, already sand" when none), and
"outside the view" for a tile outside it. Every string from the proposal
or the world is escaped with the observe crate's set (controls,
bidirectional and line separators, invisible characters) and a backslash,
and cut at 48 characters.

**The map highlight.** The tiles the panel's step concerns are darkened on
the map, worked out like its words from the decoded Action and the person's
committed world (never from the recorded transaction), when the proposal's
state changes, not per frame:

| `kind` | Highlighted |
|---|---|
| `move` | the object's tile now (`from`) and its destination (`to`), on the object's plane and layer |
| `set_field`, `remove` | the object's tile (`object`) |
| `set_portal` | the object's tile (`object`) and, when set, where the portal leads (`target`): the location, or the named anchor's tile |
| `paint` | the cell (`cell`) |
| `paint_stroke`, `paint_zones` | every cell of the stroke (`cell`) |
| `fill_terrain` | the cell it starts from, then every cell it would change in the person's committed world (`cell`), worked out by the engine's own fill; just the start cell when it would change none |
| `place` | the tile it places on (`target`) |
| `create_anchor`, `delete_anchor` | the anchor's tile (`anchor`); a new anchor is on the view's layer |
| `move_anchor` | the anchor's tile now (`from`) and its destination (`to`) |
| `create_zone`, `rename_zone`, `set_zone_parent`, `delete_zone` | nothing: the zone overlay shows zones |

An object or anchor that is not in the person's world gives no tiles, and
neither does an object carried in an inventory: it has no tile of its own
(the frame evidence says `"not placed"`); the carrier's tile is not
highlighted. The highlight is
shown for the step the panel offers to prepare (ready, undone, or refused)
and for the step that is pending, where the map shows the pending world, so
the highlight marks where the change is; there is none when every step is
applied, after a Cancel, while the person's own change is pending, when the
world moved or is at several places on the path, when the proposal does not
apply, or while the session is blocked. The tint is black at 50 % over the
tile (25 % for a `from` tile): it darkens, where the brush preview's white
lightens and the zone tints colour, and selection is not drawn on the map.
Only tiles on the active view's plane and layer and inside its bounds are
drawn. For the others the step's words say "outside the view" after the
tile they name, a portal's location or anchor tile included, and a field
change says "(outside the view)" when its object's tile is. The
highlight draws after a zone tint of the same tile and is covered, as ground
is, by objects on the tile and nearer tiles; on a cell the person's own
running brush stroke previews, only the preview is drawn. It is unpickable:
a click picks what it picked without it. A stroke's highlight holds at most
4,096 tiles, the stroke bound; a tile named twice is highlighted once, with
its first role. Darkening does not show on near-black ground (a pit tile, or
an empty cell over the map background): the Proposal block's words are what
the step does; the highlight only helps find it.

The highlight is the first thing to give way: when the overlays would take
the frame past the scene limits, the frame is composed again without it,
keeping the person's brush preview, zone tints and badges, and only if that
still does not fit is it composed without any overlay, as before. The status
line is never used for it (a pending step's Review status, the review limit
and error notices stay); the Proposal block says "Highlight left out: too
many cells for one frame" as a notice row, and the frame evidence's
`highlight` member says `left_out: true`.

The block takes at most 240 logical pixels at the minimum 720×480 window,
its control included, so an open text draft and the inspector rows below
it stay on screen; a taller window gives it up to a third of the sidebar.
The header, label, caveat, editor and where rows are one line each, cut
with "..." when long (the full strings are in `proposal-open.json`); the
step's words wrap between words to at most three lines and the state line
to two; each row is first cut to about what three lines of the sidebar can
hold. When the rows need more lines than the block has (nine at the
minimum window), the least important lines go first: the notice, the
counts, the third line of the words (while a step is refused or diverged,
the notice, a likely cause, goes after these two), the second line of the
state line, the second of the words, where the world is, the editor row,
the actor caveat and the label. The header, the first line of the words
and the first of the state line always stay, and so does the "Highlight
left out" notice when there is one. A row cut short ends in
"...".

**The control.** One button under the block, also on the **G** key:
"Prepare step k" prepares the next step through the same path as every
edit; "Continue from step j" (the world is after a step the progress has
not reached) and "Resume at step j" (after a Cancel) start again from where
the world is, preparing nothing. There is no control while a change is
pending, the world is not on the path or at several places on it, the
session is blocked or every step is applied. G waits for the displayed
frame, as Enter does. Accept (Enter) and Cancel (Esc) are the ordinary
ones; nothing in the desktop accepts a step for the person.

**The states**, as the state line says them:

| State | What happened | What the person can do |
|---|---|---|
| Ready to prepare. | The world is where the next step starts. | Prepare it. |
| Step k pending: matches its recording. Accept or Cancel below. | The step is prepared; the Review status reads "Step k of n matches its recording (recorded as agent:…). Accept applies it; Cancel stops the proposal." and shows the session's own review of the freshly prepared change. | Accept or Cancel. |
| Your own change is pending: the proposal waits. | The pending change is the person's; the Review is today's. | Accept or Cancel it. |
| Step k undone: prepare it again (or Redo). | An applied step was undone. | Prepare it again, or Redo. |
| The world is at several places on its path: Redo continues the proposal. | The steps return to a revision they passed, so where to continue is ambiguous, and there is a change to redo (the person undid a step). | Redo. |
| The world is at several places on its path: undo your last edits to continue. | As above with nothing to redo: the person's own edits brought the world back to a revisited revision. | Undo them. |
| The world is at several places on its path: the proposal cannot continue from here. | As above with nothing to undo or redo. | Nothing in this session. |
| The world moved: undo your own edits to continue. | The person accepted an edit of their own; the world is on no step of the path. | Undo it. |
| The world moved: undo your edits or continue from step j. | The world is after a later step than the progress (the person made a step's edit themselves). | Continue. |
| The world moved: every step is in it; continue to finish. | The world is at the proposal's result while the progress is behind (the person made the last steps' edits themselves). | Continue. |
| Refused: proposal.step_refused (engine code). Nothing changed. | The engine refused the step under the person's grants. | Try again after changing what refused it. |
| Diverged from its recording. Nothing changed. (With "Recorded by another editor build." when the versions differ.) | The step prepared another change than it recorded and was cancelled. | Preparing again is offered; it gives the same result unless the world or catalog changes. Another editor build may record differently. |
| Stopped by Cancel. Resume continues it. | The person cancelled a prepared step. | Resume. |
| Stopped by Cancel; it cannot resume from where the world is. | As above, and the world is now at no single place on the path (or a change is pending). | Undo own edits, or finish the pending change. |
| All n steps applied. | | |
| Does not apply to this world; nothing can be prepared. | Neither begin nor resume applied at start-up. | Edit normally. |
| This session cannot continue (blocked). | The session or the rendering is blocked. | Inspect recovery, as for any blocked session. |

A refusal stays on the state line until the person's next action of any
kind (a selection included); it does not come back after an edit and its
undo. The progress survives the engine's Reopen (R), which keeps the
session's History; after a closed editor, the next start resumes.

**Evidence.** Nothing is written without a proposal, so existing evidence
is unchanged. With one, `proposal-open.json`
(`tilewright.prototype-proposal/1`: the body digest, file name, recording
editor and this one, agent actor, escaped client label, the person's
actor, base, result, catalog, step count, notices, and `begun`,
`resumed_after` or `applies: false` with the code and the world's
revision), and for each step the person's Accept applies
`proposal-step-<sequence>.json` (`tilewright.prototype-proposal-step/1`:
the sequence, the proposal's digest and file, step k of n, the person's
actor, the recorded transaction's SHA-256 and the SHA-256 of the
transaction the session applied, which differ by the actor, the step's
before and after revisions, and the recovery record that Accept retained).
The sequence counts every applied step, so a step applied again after an
undo gets a new record. If a record cannot be written, the step is still
applied (the person's Accept succeeded), the panel moves on, the notice
reads "Step k applied; its evidence record … could not be written: …",
and the next record takes the next number; no number is reused. Frame
captures gain a `proposal` member while a proposal is open: `proposal`
(the digest), `steps`, `applied` (steps settled as applied), `state` (a
key such as `ready`, `pending`, `refused`), `pending_step` (whether the
pending change is the proposal's step) and `highlight`: `null` when no step
is highlighted, else the `step` (from 1), `concerns` (`tiles`, `no tiles`
for a zone change, or `not placed`), the counts `in_view` and
`outside_view`, `left_out` (whether the frame left the highlight out to fit
the scene limits), and the first 8 `tiles`, each with its `role` (`from`, `to`,
`object`, `cell`, `target` or `anchor`), escaped `plane`, `layer`, `x`, `y`
and `in_view`. `--demo-script --reference` with
a proposal runs the proposal instead of the demonstration: it captures
`proposal-opened`, then prepares, captures `proposal-step-k-pending`,
accepts as the scripted demonstration accepts its own edits, and captures
`proposal-step-k-applied` for each step; with a window, `--demo-script`
and a proposal are refused.

**A hand test in a real window.** None of this was tested in a window: the
tests drive the application object, its key and pointer routing and its
CPU frames. A person checks:

1. Start the desktop with `--proposal SANDBOX/proposals/proposal-….json
   --actor human:<you>` as above. The window opens with the Proposal
   block at the top right; read it at 1280×800.
2. Before pressing anything, the tiles of step 1 are darkened on the map
   and match the step's words (a move: its tile now, lighter, and its
   destination; a stroke: its cells; nothing for a zone change). On a dark
   pit tile or an empty cell the darkening does not show; the words are
   what count. They stay
   darkened while the step is pending, and move to step 2's tiles after
   Accept.
3. Press **G** once: the Review shows the diff, and the status line reads
   "Step 1 of n matches its recording (recorded as agent:…)…". Hold G
   down: nothing repeats. With a proposal recorded by another editor
   version, the editor row starts "Recorded by another editor build" and
   ends "steps may diverge" (cut when the versions are long).
4. Press G and then Enter as fast as you can: the step is accepted only
   once its diff has been drawn. Press Enter: the step is applied and the
   panel moves to step 2.
5. Press G, then **Esc**: the panel says "Stopped by Cancel. Resume
   continues it." and the button "Resume at step 2 (G)". Press G: nothing
   is prepared. Press G again: step 2 is prepared.
6. Undo an applied step (Z, then Enter): the panel says "Step k undone:
   prepare it again (or Redo)". Press G to prepare it again; or press Y
   and Enter to continue instead.
7. Make an edit of your own between steps: the panel says "Your own change
   is pending: the proposal waits"; after Enter, "The world moved: undo
   your own edits to continue". Undo it (Z, Enter) and continue.
8. Make the window as small as it goes (720×480): the block, its button
   and some sidebar rows below are visible. Open a zone name draft (Zone,
   choose a zone, Name): its text field and buttons are visible below the
   block, and the caret is in the field, not on the Prepare button.
9. Click the "Prepare step k (G)" button: the same as G. Click Accept: the
   same as Enter.
10. Open `NEW-SESSION/evidence/proposal-open.json` and
   `proposal-step-*.json`: your actor, one record per applied step, each
   naming a recovery record that is in `NEW-SESSION/recovery/`.
11. Quit after applying some steps, then provision a new session from that
    session's `world/` with the same `--proposal`: the panel says "World:
    after step j of n" and offers step j+1.

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
a sandbox, the `proposals/` directory the provisioner passes): the
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

Precisely, with the World crates at 0.2.2: the store never writes to the
final name. `SourceDirectory::write` removes whatever is at the name it
writes before creating it, so the store writes only the stage
(`<name>.stage`), after checking nothing is there, and refuses (and keeps)
a leftover stage. `SourceDirectory::rename` replaces its target, so the
store renames only after checking the final name is free. Between that
check and the rename, and between the stage check and the stage write, a
process that ignores the lock can create an entry that the store then
replaces: a window of a few system calls, which only a same-user process
can use. World 0.2.2 offers a create-only write (`SourceDirectory::create`),
which would close the stage window; the store does not use it yet, and
adopting it is a follow-up. The final-name window needs a rename that does
not replace, which 0.2.2 does not offer.

`read_proposal(root, name)` reads a store's name only, at most 16 MiB
(larger is refused before it is read), refuses a link or anything but a
regular file, decodes and verifies it, and requires the name to carry the
body's digest.
