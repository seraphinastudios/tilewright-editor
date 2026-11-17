# Editing engine

Every editing front end (the desktop app today; a command line and MCP
editing tools later, issue #57) drives one engine: `Session` in the
`tilewright-editor-ui` crate. The engine has no SDL, GPU, font, layout or
direct file access. A front end builds `Action`s, calls `Session::dispatch`,
and reads the result back; it adds no editing logic of its own and cannot
bypass anything the desktop app enforces. See
[ADR 0002](../decisions/0002-one-editing-engine.md).

## What the engine owns

- **Preparation.** Every edit is one World transaction, prepared against the
  revision its author observed, limited by the catalog's grants and active
  view, and fenced so a pending change cannot be replaced before Accept or
  Cancel.
- **Review.** After every dispatch the engine builds the review of the
  pending change (`Session::review`): notes and deletion warnings first,
  within their own 16 KiB allowance, then a change-centred excerpt of every
  changed file, within 64 KiB of escaped excerpt rows and at most 32 changed
  files. A review that does not fit is incomplete and starts with "Change
  exceeds the visible review limit. Cancel this edit."
- **The review gate.** `Action::Accept` through `Session::dispatch` is
  refused, with the code review.incomplete, while the review is incomplete. The
  pending change stays for Cancel. The desktop app checks the same flag
  first to disable its Accept button, but the engine does not rely on it.
- **The actor.** `Session::new` takes an `Actor`, the `actor` of every
  transaction the session prepares: non-empty, at most 256 characters (the
  transaction format's limit) of printable ASCII (U+0020 to U+007E), not
  starting or ending with a space, so no control, invisible, combining or
  right-to-left character can hide in it or change how it reads; for example
  `editor:prototype` (the desktop app), `human:<login>` or `agent:<client>`.
  A redo replays the accepted edit with its actor. An undo replays World's
  inverse of it, which records no actor. Transaction text alone therefore
  does not attribute every step: the applied steps below record the actor
  of each step themselves.
- **Applied steps.** `Session::applied_steps` reads History's undo stack,
  oldest first (`History::applied` is the same list without the engine's
  part): for each accepted edit its sealed forward transaction, exactly as
  accepted, the revisions before and after it, and, when this session
  prepared it, its origin: the edit `Action` as dispatched, the actor in
  force and, for `Place`, the uid it assigned. An undone step leaves the
  list (it moves to the redo stack with its origin); a redone one returns
  with its original Action; an edit that changes nothing, a cancelled one
  and undo and redo transactions themselves are never steps. Steps of a
  History the session was built with have no origin. Reading them changes
  nothing: what is published, the recovery records and every other
  behaviour stay as they were. `Session::dispatch_placing` dispatches a
  `Place` that assigns a given generated uid (refused with the code
  input.invalid for anything else, and with uid.unavailable when the
  World already has it), for one dispatch only; the session's own uid
  source is restored after it. [Proposals](PROPOSALS.md) are built on
  both. The kept origins add memory that History's 64 MiB byte budget
  does not count: one Action per undo and redo entry, so at most
  `MAX_HISTORY` (1,024, the bound on undo and redo together) times the
  largest Action. That is a stroke of 4,096 cells (about 64 KiB) or a
  field value within the engine's 64 KiB encoded budget (a few times that
  in memory): roughly 64 MiB for maximal strokes and a few hundred MiB at
  worst for maximal field values.
- **Summaries.** `Session::pending_summary` gives a bounded structured view
  of the pending change for callers that do not draw it: base and result
  revisions, at most 32 changed files with their change kind and sizes (and
  how many more were left out), the review lines, the notes and warnings
  the review shows (and how many it left out), and whether Accept may
  publish it. World text in it is unescaped data; a front end that prints
  or returns it escapes and labels it.

Questions that need no editing session (what a world holds, one tile, the
catalog, lint findings, a picture of a region) are answered by the read-only
commands of [docs/CLI.md](CLI.md) and the MCP server's observe tools of
[docs/MCP.md](MCP.md), which build no `Session` at all.

The front end owns everything else: layout and drawing, input, the frame
selection, the recovery store it passes to `dispatch` and the recovery
descriptor it reports back with `Session::set_recovery_descriptor`.

## Addressing

Every edit action names its target by uid, plane, layer, grid and plane
coordinates, never by the frame selection: `Move`, `SetField`, `Remove`
and `SetPortal` by uid; `Paint`, `PaintStroke`, `FillTerrain`, `PaintZones`
and `Place` by plane tile; zone and anchor actions by plane and id. `Select` and
`ClearSelection` only change what the desktop app highlights; an accepted
placement selects the new object.

Per-prototype object grants (`editing.objects`: moving, fields, deletion,
placement and portals of objects), terrain and zone painting, and anchor
creation and moves apply only in the catalog's active view: its plane,
spatial layer and tile bounds. Zone management and anchor deletion apply to
the view's plane, whatever the bounds. Per-prototype object grants cover
placed objects only; an object carried in an inventory is refused with the
code target.unsupported.

Per-uid grants (`movable_uids` and `fields`) are not bounded by the view's
tiles. A per-uid field edit applies to its object wherever it is. A per-uid
move still needs a placed (not carried) object in the active plane and
spatial layer, and a destination inside the view.

## Fill and replace

`FillTerrain { plane, layer, grid, x, y, proto, contiguous, expected }`
records a fill as it was meant, not its cells. When it is prepared, the
engine works the cells out from the committed World
(`tilewright_editor_ui::fill::region`):

- The start tile `x, y` must be a tile of the active view (its plane, spatial
  layer and bounds). Its value on ground grid `grid` is the value matched; a
  tile with no terrain on that grid (an empty cell, or one of a chunk that
  does not exist) matches tiles with no terrain.
- With `contiguous` (fill): the tiles reachable from the start tile through
  edge neighbours (x ± 1 or y ± 1; diagonal tiles are not neighbours) that
  hold the matched value, never stepping outside the view, so an area that
  joins only outside the view is two areas.
- Without (replace): every tile of the view that holds the matched value.

The cells are a set, ordered by x, then y, the order of a stroke's cells, so
the result depends only on the World and the view, never on the order cells
are visited in. They then go through the brush stroke's planner unchanged:
the stroke rule (at least one, at most the 4,096-cell cap, distinct and
ordered; a view holds at most 4,096 cells, so a fill always fits, but the
rule is still checked), the terrain grant and view checks, one
`terrain.set` setting `proto` with each cell expecting its committed value
(`null` for no terrain) and cells already holding `proto` left out. A fill
of an area that already holds `proto` is refused with the code change.none. Filling a region of
a chunk that does not exist creates it, as a stroke there does.

The start tile is checked first, exactly as a one-cell `Paint` there: an
ungranted grid or value is refused with the code grant.missing wherever
the tile is, a tile outside the view with scope.outside_view, and a stale
revision with revision.stale. No refusal code is new. Because the cells are expanded
from the committed World each time, a recorded fill prepared again on the
same World under the same active view gives the same transaction byte for
byte (and under another actor, all but the actor), and one whose covered
cells changed since is refused by World's per-cell `expect`, or differs, and
is not applied. The active view is an input of a fill as much as the World:
under a catalog with other view bounds, the same fill on the same World
can cover other cells, and then it prepares another change.

`Session::fill_changes` answers, for a `FillTerrain`, the cells dispatching
it would change now, from the same code; it prepares nothing. The desktop's
hover preview and a proposal step's map highlight use it, or the region
function it is built on.

## Refusal codes

Every `SessionError` carries a stable `RefusalCode` (`code()`, and
`as_str()` for its text) beside its message (`message()`, also `Display`).
Messages are for people and may be reworded; codes are stable.

| Code | Meaning |
|---|---|
| `session.blocked` | The session is blocked after an uncertain publication; only Reopen is accepted. |
| `pending.exists` | A change is pending review: Accept or Cancel it first. |
| `pending.none` | Accept found no pending change. |
| `revision.stale` | The action names a source revision other than the open one. |
| `source.changed` | The source on disk differs from the World the session opened. |
| `source.invalid` | The source cannot be read, is not UTF-8, does not parse or load, or lacks a document the action needs. |
| `catalog.refused` | The editor catalog or its resources no longer verify. |
| `selection.stale` | A selection names a stale frame or an unknown semantic key. |
| `grant.missing` | The catalog does not grant this edit. |
| `scope.outside_view` | The target is outside the active view, or for zone management and anchor deletion outside the view's plane. |
| `target.unknown` | The named object, plane, zone, anchor, grid or field does not exist. |
| `target.exists` | A new id is already used. |
| `target.in_use` | The target is still referenced and cannot be deleted. |
| `target.unsupported` | The target exists but cannot take this edit, for example an object in an inventory. |
| `hierarchy.cycle` | A zone parent would make a cycle. |
| `input.identifier` | A new id does not have the World identifier shape. |
| `input.invalid` | The action's input is malformed, for example unordered stroke cells. |
| `input.budget` | The action's input exceeds a value, length or count budget. |
| `change.none` | The action would change nothing. |
| `actor.invalid` | The actor is empty, too long, has a character outside printable ASCII, or starts or ends with a space. |
| `world.refused` | World refused the prepared transaction; nothing is pending. |
| `review.incomplete` | The pending change's review is incomplete, so Accept is refused. |
| `history.full` | History has no room for another step. |
| `history.nothing_to_undo` | There is no step to undo. |
| `history.nothing_to_redo` | There is no step to redo. |
| `recovery.failed` | The recovery store did not retain the inverse; nothing was published. |
| `publish.failed` | Publication failed; the status reports what was applied. |
| `publish.unverified` | Publication was applied, but the reopened source differs or does not reopen. |
| `reopen.blocked` | Reopen found differing source or an uncertain publication. |
| `uid.unavailable` | No unused uid could be drawn for a new object. |
| `proposal.invalid` | A proposal is malformed, records a non-edit action, or a step disagrees with its action or actor. |
| `proposal.budget` | A proposal exceeds a step, size or length bound. |
| `proposal.digest` | A proposal's body digest or a step's transaction digest does not match its bytes. |
| `proposal.chain` | A proposal's revisions do not link from its base through every step to its result, or to the session it is built from. |
| `proposal.empty` | A proposal would have no steps. |
| `proposal.base_stale` | The world is not at the revision the next proposal step starts from. |
| `proposal.step_diverged` | A proposal step, prepared again, gave another transaction than it recorded; it was cancelled. |
| `proposal.step_refused` | The engine refused to prepare a proposal step again; its own code is reported beside this one. |
| `proposal.closed` | The proposal was stopped by a Cancel, or every step is applied. |
| `proposal.store` | The proposal store could not retain or read a proposal file. |
| `proposal.resume_ambiguous` | The world is at more than one position of a proposal's chain, so where to resume it is not known. |
| `internal.unexpected` | An unexpected internal failure: a bug, not a property of the input. |

A test checks that every code is listed here exactly once. The
`proposal.*` codes are those of the `tilewright-editor-proposal` crate's
refusals; [docs/PROPOSALS.md](PROPOSALS.md) says when each is given. The
sandbox runner returns these codes as they are and adds its own `request.*`
codes for lines it cannot read as a request ([docs/SANDBOX.md](SANDBOX.md)).
