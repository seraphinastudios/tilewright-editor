# ADR 0009: stamps as one recorded edit, `place_objects`

Status: accepted, 2026-11-26 (world-scale tools decision 4, the first of
three pull requests: the engine side).

Placing a group of objects (a fence run, a camp, a market stall and its
crates) one click at a time takes many reviews, many undo steps and many
uids to keep track of. A stamp places the group as one change. The World
format needs nothing new for it: one transaction may hold many
`object.insert` operations, it has one review, one recovery record and one
exact inverse. The format has no occupancy rule (objects on a tile keep
list order, their stacking order), no rotation or mirroring of objects,
and no footprints as data, so a stamp can only reason about head tiles.

## Owner decisions, 2026-11-26

1. **Both sources in the first slice**: named stamps in the editor
   catalog, and capture from the map (a rectangle copied to a session
   clipboard, then stamped).
2. **Rotate and mirror the offsets.** R and M turn or flip the layout;
   each object's art stays as it is.

## Coordinator decisions, 2026-11-26

- **One kind, `place_objects`, that records its objects inline**: "copied,
  not named". With capture, an edit cannot name a stamp, so a catalog
  stamp and a captured copy are only two sources of the same list. Each
  object is a prototype and its final offset, after any rotation or
  mirroring, so the engine has no transform rule to freeze.
- **`placed` becomes a list for this kind**, and the proposal format stays
  `tilewright.editor-proposal/1`: an editor from before it refuses the
  whole proposal with `proposal.invalid` when it reads it. It reads a
  step's `placed` before its action, so it stops at the list ("placed is a
  string"; checked against main aabece9), and the unknown kind would
  refuse it next.
- **Refuse the whole stamp** when any object is outside the view or not
  granted; **warn** for occupied tiles, never refuse; objects only (no
  terrain); at most 64 objects.
- Order: this engine pull request (kind, proposal step, MCP, step words,
  map highlight, this record), then the desktop's catalog stamps with R
  and M, then rectangle capture.

## The kind, frozen

These are part of what a recorded `place_objects` step means. Changing
any of them needs a new kind.

- **Action.** `PlaceObjects { plane, layer, x, y, objects, expected }`;
  in proposals and the MCP editing server
  `{"kind": "place_objects", "plane", "layer", "x", "y", "objects":
  [{"proto", "offset": [dx, dy]}], "expected"}`. `x, y` is the anchor
  tile. An object is exactly a prototype and an offset: no layer, fields,
  `sub_layer`, `z`, portal, inventory or uid. The render layer and the
  field defaults come from the catalog's `editing.objects` grant of the
  prototype when the step is prepared, as for `place`.
- **Members.** 1 to 64 objects. None is `input.invalid` in the engine and
  `proposal.invalid` in a proposal; more than 64 is `input.budget` and
  `proposal.budget`. Each offset is an integer from −4,095 to 4,095 on
  both axes (beyond: `input.budget`, and `proposal.invalid` when read). An
  object's tile is the anchor plus its offset; one that would overflow 64
  bits lies outside every view. Objects may repeat, and two may share a
  tile.
- **Order of inserts.** One `object.insert` per object, in object order,
  and nothing else in the transaction. Each is byte for byte the
  operation `place` writes for one object on that tile: on top of the
  tile's stack (no `index`), with members uid, prototype, render layer,
  then the grant's defaults in catalog order under `fields`. Objects that
  share a tile stack in object order.
- **Uids.** One generated uid per object (`o.` and 26 base32 characters,
  128 bits), drawn in object order from the session's uid source, each
  unused in the World and among the stamp's own, with the same bounded
  redraw as `place`. The step records them as `placed`, a list in object
  order; replay assigns exactly those (`Session::dispatch_placing_objects`)
  and refuses a taken one with `uid.unavailable` rather than draw again.
  Uids stay independent of each other and of their tiles, as the World
  format recommends: none is derived from a seed or an index.
- **Refusals**, all before anything is prepared, in this order: the
  members (count, offsets); every object's grant (`grant.missing`,
  before the view, as for `place`); every object's tile in the active
  view (`scope.outside_view`: the whole stamp, never clipped; the anchor
  itself need not be in the view); the revision (`revision.stale`); the
  uids (`uid.unavailable`). World then validates every object
  (`world.refused`). No refusal code is new.
- **The warning.** When any object lands on an occupied tile, the review
  shows first: "Warning: N of the M new objects land on tiles that
  already hold an object or another new one; each is added on top." (with
  singular forms for one). A tile is occupied when the committed World
  holds a top-level object there, on the same plane and spatial layer, or
  an earlier object of the same stamp lands on it. It never refuses and
  never makes the review incomplete. It is shown when the stamp is
  prepared, not on the Undo or Redo of an accepted stamp: a Redo lands on
  the same World the original review judged, and its review shows the
  insertion notes only.
- **Verification** of a proposal (no world needed) requires a
  `place_objects` step's `placed` to be a list of one distinct generated
  uid per object, and its transaction to be exactly one `object.insert`
  per object, in object order, each on the action's plane and spatial
  layer at the object's tile, without `index`, of the object's prototype,
  with its uid and no inventory. A `place` step's `placed` stays a string:
  a list of one is not a placement, nor a string a stamp.

## Implementation decisions (this pull request, not the owner's)

These were chosen by the implementing agent; the owner can revisit any of
them.

- **Member names.** `objects`, as the World format lists objects, and
  `offset`, not `at`: in World `at` is a tile (chunk-local in a chunk, a
  plane tile in a portal), and an offset is neither.
- **The offset bound, 4,095,** is the widest a view allows: a view holds at
  most 4,096 tiles, so no two of its tiles are further apart than 4,095 on
  one axis. Every stamp that fits in a view therefore fits the bound when
  its anchor is a tile of the view too (or, more generally, within 4,095
  tiles of every object on each axis); a caller that anchors a stamp far
  outside the view must move the anchor closer. It keeps
  every tile computation small and every recorded number short.
- **64 objects, and the review's own limits.** A stamp's inserts are small
  and usually fall in one or a few chunks. The 64-object cap does not by
  itself keep a stamp inside the 64 KiB review excerpt: (corrected
  2026-11-28) each changed file's excerpt runs from its first change to its
  last, before and after, and each insert goes on top of its tile's stack,
  so a stamp whose objects spread through large chunk files can exceed it
  (seen with copies of 53 and 61 objects over four chunks of 6 to 17 KB).
  What bounds a stamp's review is therefore the chunk files it touches, as
  for any change: past the excerpt budget, or over more than 32 changed
  files, the review is incomplete, the change stays pending with "Change
  exceeds the visible review limit. Cancel this edit." and Accept is
  refused with `review.incomplete`, writing nothing. No stamp-specific
  limit was added; a narrower excerpt per insert would be the engine-side
  remedy. An MCP request of 64 objects with the longest World identifiers
  (128 bytes) at the widest offsets is under 16 KiB of the 64 KiB a
  message may be (a test pins it).
- **An accepted stamp leaves the selection as it is.** A single placement
  selects the new object; a selection holds one object, and picking one
  object of a stamp would be arbitrary.
- **`applied.placed`** in the sandbox runner and the MCP editing server is
  the list for a stamp, a string for `place` and `null` otherwise, so no
  existing reply changes. The MCP output schema says this with a new
  `Schema::Either` (an `anyOf` of any type, flat with `null`), because the
  existing `AnyOf` is for objects only.
- **Step words** name the count, up to four prototypes in the order they
  first appear with their counts, and the anchor: "Place 4 objects (marker
  2, stone 1, bush 1) anchored at (5, 3) on main/0", with ", some outside
  the view" when any object's tile is. The **map highlight** is each
  object's tile, in object order, once per tile; the anchor is not
  highlighted for itself.
- **One state for the uids of a pending change.** The step origin keeps
  one list of uids (one for `place`, all for `place_objects`), read
  through `StepOrigin::placed` and `StepOrigin::placed_objects` by the
  action's kind, so the origin did not grow.

## Not decided here

The catalog's `editing.stamps`, the desktop's Stamp choice with R and M,
its preview and the rectangle capture (the next two pull requests);
multi-tile footprints; copying fields, `sub_layer`, `z` or portals of
captured objects; terrain in stamps (prefabs).
