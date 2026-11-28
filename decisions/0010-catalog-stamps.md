# ADR 0010: stamps in the editor catalog, turned with R and mirrored with M

Status: accepted, 2026-11-27 (world-scale tools decision 4, the second of
three pull requests: the desktop's catalog stamps).

[ADR 0009](0009-place-objects.md) added the engine kind `place_objects`,
which records a stamp's objects inline as prototypes and final offsets.
This change gives the desktop its first source of such lists: named stamps
in the catalog, put down from Place mode. Rectangle capture from the map is
the third pull request; its decisions are recorded below, under
[Capture from the map](#capture-from-the-map).

## Owner decisions, 2026-11-26

1. Both sources, named catalog stamps and capture from the map.
2. R and M turn and mirror the offsets; each object's art stays as it is.

## Implementation decisions (this pull request, not the owner's)

These were chosen by the implementing agent; the owner can revisit any of
them.

- **Form.** An optional `editing.stamps` member:
  `[{name, objects: [{proto, offset: [dx, dy]}]}]`. A name is a World
  identifier of at most 64 bytes, unique; 1 to 64 objects
  (`MAXIMUM_STAMP_MEMBERS`, the engine's limit); each offset two integers
  within ±4,095 (`MAXIMUM_STAMP_MEMBER_OFFSET`, the engine's bound); at most
  32 stamps, as for palettes. The member names are the engine's (`objects`,
  `offset`), so a catalog stamp reads like the action it becomes. Objects
  may repeat and share a tile, as the engine allows; the review warns.
- **Every prototype must be granted** by `editing.objects`, or the whole
  catalog is refused with a message naming the stamp and the object. The
  engine would refuse such a stamp on every click (grant.missing), so it is
  a catalog mistake, and refusing it at load is how the other grant-backed
  members (`editing.palettes`, `editing.terrain`) behave. Warning instead
  would leave a row that can never work.
- **A stamp that cannot fit the view is a lint warning, not a refusal**
  (`catalog.stamp_unplaceable`, when no tile of the view can anchor it with
  every object inside, as listed or turned: too wide or tall, or reaching
  too far from its anchor). The view is the catalog's own
  choice and may change independently of the stamps, and such a catalog is
  otherwise sound.
- **Versioning (ADR 0004).** `editing.stamps` is an optional member in both
  catalog formats, `tilewright.editor-preview/1` and `/2`: optional members
  may be added within a version, because every format member is closed. An
  editor from before stamps closes `editing` to `movable_uids`, `terrain`,
  `fields`, `objects`, `zones`, `anchors`, `palettes` and `heights` and
  refuses a catalog with stamps ("unknown or missing catalog key") instead
  of ignoring them. A catalog without the member loads, reports, lints and
  draws exactly as before.
- **Copied, not named.** A click records the stamp's objects after the turn
  and mirror, never the stamp's name (ADR 0009), so a recorded step replays
  under a catalog that no longer has the stamp, or has another stamp of that
  name.
- **The turn and mirror convention**, in plane tiles about the anchor: M is
  `[dx, dy] → [-dx, dy]` top-down (across the anchor's column, the plane's
  `y` axis, a left-right mirror on screen; an isometric view flips left to
  right on screen instead, under
  [Mirror in an isometric view](#mirror-in-an-isometric-view)), R is
  `[dx, dy] → [-dy, dx]` (a quarter turn clockwise: `+x` goes to `+y`, which
  both projections draw a quarter turn clockwise from `+x`). Each key acts
  on the layout as shown; the state is a mirror flag applied first and then
  0 to 3 quarter turns, so a top-down M after `t` turns sets `4 - t` (an
  isometric M sets `3 - t`, under the same section). Turning and
  mirroring keep each axis's magnitude, so a stamp inside the engine's
  offset bound stays inside it. It is the desktop's convention only:
  nothing in the engine or the proposal format depends on it.
- **Keys.** R and M act only with a stamp chosen in Place mode. R is Reopen
  everywhere else, and stays Reopen in a blocked session even with a stamp
  chosen: nothing can be placed there, so R keeps its old meaning (Reopen
  re-inspects the source; it does not unblock the session), and M and the
  Turn and Mirror buttons are inert. M was free. Text drafts
  and the palette filter take both as text, and the road fence refuses the
  stamp controls. R and M are view state: they work while a change is
  pending, like the palette filter.
- **The palette filter** narrows the stamp rows by name with the
  prototypes' rule, so a stamp below a long palette can be found by typing.
- **The preview** is the click's own action: the hover computes the action
  a click would prepare and marks each object's tile with the brush
  preview's copies of the exact cell quads; objects outside the view are
  counted in the status line, since they have no cell to draw.
- **Not in the catalog report or the MCP catalog tool.** Listing stamps
  there would change an MCP reply; an agent builds its own `place_objects`
  list.

## Capture from the map

Added 2026-11-28 with the third pull request, which settles the
"rectangle capture" item this record left open. A copy captured from the
map is the second source of a `place_objects` list; it reuses every rule
above (R and M, the preview, the click, Review) and adds no catalog
member, no engine rule and no proposal change.

**Owner decision 3, 2026-11-27:** capture copies prototype and position
only; copies get the grant's default fields, and their own field values,
layering and portal links do not come along; an ungranted prototype in
the rectangle makes the copy refuse, and the status line says so; it uses
the existing `place_objects` kind.

Implementation decisions (the implementing agent's; the owner can
revisit any of them):

- **The gesture.** C in Place mode arms Copy area; a press on a view tile,
  a drag and a release span the rectangle. C was free (only Ctrl or Cmd
  with C in a text draft copies text). A key, not a modifier-drag, so a
  touchpad needs no held key and the armed state shows on the status line.
  No control is laid out before the first C, so a session that never
  copies draws exactly as before; once armed or holding a copy, Place mode
  shows "Copy area  C" and "Drop copy  Esc" buttons. C again, Esc, a mode
  change or an opened portal step stops it.
- **The rectangle and what counts.** Both corners are view tiles (a
  pointer off the map keeps the last one), so the rectangle is clipped to
  the view. Every top-level object on its tiles, on the view's plane and
  spatial layer, counts; other layers, inventory objects and tiles outside
  the view never do.
- **Order and anchor.** Row by row (`y`, then `x`), each tile in stacking
  order, so stacks come out as they were; the anchor is the rectangle's
  smallest-`x`, smallest-`y` tile, so every copied offset is `>= 0` and a
  click puts that corner on the clicked tile.
- **Refuse the copy, not every click.** No object, more than 64 (the
  engine's limit) or an ungranted prototype copies nothing, with the
  reason, and the status line says it while the rectangle is dragged.
  Copying such a list and letting the engine refuse each click
  (`grant.missing`) was the alternative; grants cannot change within a
  session, so that copy could never be stamped, and refusing it once is
  how catalog stamps with an ungranted prototype are refused (at load).
- **The clipboard is session state.** One copy, held in the tool state,
  never written or recorded; a new session starts without it. It is the
  chosen stamp after a copy; choosing a prototype or a catalog stamp
  replaces it and a new copy replaces a catalog stamp. Esc drops it only
  when nothing is pending (a pending change is cancelled first), so Esc
  after a stamped click keeps the copy for the next one.
- **Only when the session can prepare.** No rectangle starts, and C does
  not arm, while a change is pending; no rectangle starts while editing
  is paused; and a release copies nothing when a change became pending,
  editing paused or the session blocked during the drag. A focus loss
  ends a drag copying nothing, as it ends a stroke. So the copy is always
  of the committed World as shown.
- **Discoverability.** `HELP` (the window title and the start-up line)
  names C and Esc for the copy, as it names R and M.

## Mirror in an isometric view

Added 2026-11-28, settling the item this record first left open.

**Owner decision, 2026-11-28:** in an isometric view M flips the shown
stamp left to right on screen, across the screen's vertical through the
anchor, instead of across the anchor's column (which runs down-left there).
Top-down nothing changes: the column is the screen's vertical.

Implementation decisions (the implementing agent's; the owner can revisit
any of them):

- **The rule.** An isometric view draws `+x` down-right and `+y`
  down-left, so offset `[dx, dy]` sits `dx - dy` half-diamonds right of the
  anchor and `dx + dy` down. Flipping left to right negates the first and
  keeps the second, so M is `[dx, dy] → [dy, dx]` there. R is unchanged.
- **One state, read in the view.** The state stays a column-mirror flag and
  0 to 3 quarter turns on the grid. The screen flip is the column mirror
  followed by three quarter turns, so an isometric M after `t` turns sets
  `3 - t` (mod 4) as it flips the flag. M reads the projection when it is
  pressed (`StampView`, from the World's profile). The keys reach the same
  eight layouts in both projections, and two M or four R give the shown
  layout back.
- **The buttons show what the view shows.** Turn and Mirror read the state
  as a turn applied after the view's own mirror, so the same keys read the
  same in both projections: M alone is "Turn: 0°" and "Mirror: on", R then
  M "Turn: 270°" and "Mirror: on".
- **No format change.** A click still records `place_objects` with the
  final offsets, so the engine, the proposal format and replays do not see
  the rule; the preview remains the click's own action.

## Not decided here

Stamps of terrain, a preview of the objects' art rather than their tiles,
a stamp list in the catalog report or MCP, copying field values, render
layers, `sub_layer`, `z` or portals of captured objects (decided against
for now by the owner), keeping a copy across sessions or saving it as a
catalog stamp, and selecting objects one by one rather than by rectangle.
