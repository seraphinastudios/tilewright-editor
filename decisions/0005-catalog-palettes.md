# ADR 0005: scatter palettes in the editor catalog

Status: accepted, 2026-11-17 (issue #58, slice B).

The world-scale tools (#58) need named, weighted sets of terrain values for
a scatter brush, shared by everyone who edits a world. There was nowhere to
keep them: the World format has no palette document, and the editor has no
user-settings store.

## Owner decisions, 2026-11-17

1. **Shared palettes live in the editor catalog**: an optional
   `editing.palettes` member of `preview.jsonc`, next to the grants: named
   weighted sets of values the catalog already grants. No World format
   change.
2. **Tools are recorded as intent**, expanded inside the engine: a scatter
   step records the palette's name and a seed, not the cells' values, and
   replays to exactly the same transaction.
3. Personal palettes and multi-object stamps come later, each with its own
   design.

## Implementation decisions (this PR, not the owner's)

These were chosen by the implementing agent to carry out the owner's
decisions; the owner can revisit any of them.

- **One member in both catalog formats.** `editing.palettes` is an optional
  member, accepted the same way in `tilewright.editor-preview/1` and `/2`.
  ADR 0004's versioning rule allows this: optional members may be added
  within a version, because every member of the format is closed and an
  older editor refuses one it does not know. That holds for the released
  editor (1.50.0): its loader accepts only `movable_uids`, `terrain`,
  `fields`, `objects`, `zones` and `anchors` in `editing` and refuses
  anything else with "unknown or missing catalog key", so an older editor
  refuses a catalog with palettes instead of ignoring them. A catalog
  without the member loads exactly as before, in both formats, so no
  existing catalog, report or image changes. A new format version would
  have been needed only if palettes weakened a promise a format tag makes
  (as a new license value does); they do not.
- **Form.** `[{name, grid, entries: [{value, weight}]}]`: a name (a World
  identifier, at most 64 bytes, unique), a ground grid with an
  `editing.terrain` grant, and 1 to 32 entries, each a value granted on
  that grid (no duplicates) and an integer weight from 1 to 10,000; at most
  32 palettes. Anything else refuses the whole catalog. The bounds keep a
  palette's total weight below 2^19, so the draw cannot overflow.
- **Order does not matter.** A palette's meaning is its set of (value,
  weight) pairs; the editor takes them in increasing byte order of the
  value, whatever order the catalog lists them in.
- **Named, not copied.** A scatter action records the palette's name and is
  resolved against the catalog when it is prepared. A recorded step under a
  catalog whose palette of that name has other values or weights prepares
  another transaction and is reported as diverged; no new mechanism was
  needed, because a proposal step must reproduce its transaction exactly.
- **A frozen draw.** Each cell's value comes from a hash of the seed, the
  spatial layer and the cell (SplitMix64's output function, then an
  integer weighted choice), defined in
  [docs/ENGINE.md](../docs/ENGINE.md#scatter). It is part of what a
  recorded `scatter_terrain` step means, so it is frozen for that kind and
  pinned by golden values; a different draw needs a new kind. No random
  number crate is used.
- **The seed** is an integer from 0 to 2^32 − 1, safe in every JSON reader.
  The desktop draws one per stroke from the session's uid source: the
  operating system's random source, or the deterministic source that tests
  and the scripted demonstration install.

## Not decided here

A terrain palette that lists the World format's terrains (with their
transitions) instead of prototypes, a scatter over a fill or replace region,
personal palettes, and a preview that shows each cell's drawn value rather
than which cells change. Each is a later slice or needs its own decision.
