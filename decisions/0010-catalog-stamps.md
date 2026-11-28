# ADR 0010: stamps in the editor catalog, turned with R and mirrored with M

Status: accepted, 2026-11-27 (world-scale tools decision 4, the second of
three pull requests: the desktop's catalog stamps).

[ADR 0009](0009-place-objects.md) added the engine kind `place_objects`,
which records a stamp's objects inline as prototypes and final offsets.
This change gives the desktop its first source of such lists: named stamps
in the catalog, put down from Place mode. Rectangle capture from the map is
the third pull request.

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
  `[dx, dy] → [-dx, dy]` (across the anchor's column, the plane's `y` axis:
  a grid rule, the same in both projections, so a left-right mirror
  top-down and a flip across the down-left diagonal in an isometric view), R is
  `[dx, dy] → [-dy, dx]` (a quarter turn clockwise: `+x` goes to `+y`, which
  both projections draw a quarter turn clockwise from `+x`). Each key acts
  on the layout as shown; the state is a mirror flag applied first and then
  0 to 3 quarter turns, so M after `t` turns sets `4 - t`. Turning and
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

## Not decided here

Whether M should flip across the screen's vertical in an isometric view
(`[dx, dy] → [dy, dx]` there) rather than across the anchor's column,
rectangle capture (the next pull request), stamps of terrain, a preview of
the objects' art rather than their tiles, and a stamp list in the catalog
report or MCP.
