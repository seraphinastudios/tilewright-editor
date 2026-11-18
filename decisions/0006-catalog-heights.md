# ADR 0006: a height range in the editor catalog, and raise/lower as intent

Status: accepted, 2026-11-18 (issue #58, slice C, the second of five pull
requests).

The World format stores one whole-number height per tile per spatial layer
(any 32-bit value, 0 when a chunk has no height grid) and has no rule
between neighbouring heights. The editor could not change them. Very large
heights overflow the preview's arithmetic, and steep neighbours can make
an isometric view undrawable, so editing heights needs a limit as well as
a grant.

## Owner decisions, 2026-11-18

1. **Sculpting comes as five pull requests**, one tool each, in this order:
   show heights, raise/lower in the engine (this one), the desktop sculpt
   mode, flatten, smooth.
2. **The grant is a range**: `editing.heights {min, max}` in the catalog.
   A write outside the range is refused; raise and lower stop at its ends.
3. **Tools are intent kinds expanded inside the engine**, as fill and
   scatter are: a proposal step records what was meant and replays to
   exactly the same transaction.
4. **No slope marks or movement-limit logic** until the World format says
   what its movement limits mean (an issue in tilewright-world).

## Coordinator defaults (taken unless the owner overrules them)

- **Flat amounts, once per stroke.** An adjustment adds the same amount to
  every covered cell, with no falloff. A stroke's cells are a set, so a
  cell the brush passes over twice changes once; dragging does not
  accumulate.
- **Any amount in a bounded range.** The desktop will offer steps of 1 and
  8 (pull request 3); the action itself takes any non-zero amount up to
  1,024 either way.
- **The neutral sample stays unchanged**: no heights and no grant. Tests
  use temporary copies with generated heights.

## Implementation decisions (this pull request, not the owner's)

These were chosen by the implementing agent; the owner can revisit any of
them.

- **A hard bound of ±512.** A catalog's `min` and `max` must lie from −512
  to 512 (`MAXIMUM_HEIGHT`). The reason is what the preview can draw. The
  renderer bends isometric ground by the mean of the four tiles around
  each corner, at 1 pixel per unit and up to zoom 8 (the desktop's largest
  zoom and `render --scale 8`). Two opposite corners of a tile share only
  that tile, so they differ by at most three quarters of the spread of the
  heights around it. With every height within ±512, including the 0 that
  a tile without a height grid has, the spread is at most 1,024. A tile's
  corners then differ by at most 768 units, which is 6,144 pixels at zoom
  8. That leaves 2,046 pixels of the renderer's 8,192-pixel quad limit for
  the tile's own diamond, so a tile up to 255 pixels tall stays drawable.
  A bound of ±1,024 would allow 12,288 pixels, which is undrawable at zoom
  8. The preview adds a height to `layer × layer_height` in 32 bits; with
  this bound the height part cannot be what overflows. Inside the bound,
  the range is the project's choice. The bound guarantees nothing about
  heights the editor did not write: a world may already hold larger ones.
- **One range per catalog.** The grant applies to the active view's plane
  and spatial layer, as `editing.zones.paint` does. Heights are one grid
  per chunk, not a ground grid, so the per-grid shape of `editing.terrain`
  does not fit, and the owner chose one range over per-layer ranges.
- **Versioning (ADR 0004).** `editing.heights` is an optional member in
  both catalog formats, `tilewright.editor-preview/1` and `/2`. The released
  editor (1.52.0) closes `editing` to `movable_uids`, `terrain`, `fields`,
  `objects`, `zones`, `anchors` and `palettes`, and refuses anything else
  with "unknown or missing catalog key". So an older editor refuses a
  catalog with a height range instead of ignoring it. A catalog without
  the member loads, reports and draws exactly as before.
- **The action** is `AdjustHeights { plane, layer, cells, amount }`,
  `adjust_heights` in proposals and the MCP editing server: one kind for
  raising (positive) and lowering (negative). `cells` follows the brush
  stroke's rule (1 to 4,096, distinct, in order), as scatter does, so the
  desktop's stroke can become this action unchanged. `amount` is an integer
  from −1,024 to 1,024, not 0: 1,024 is the widest range there can be, so a
  larger amount could change nothing more. It records no grid: the grid is
  always `height`.
- **The rule**, frozen for this kind: each covered cell becomes
  `clamp(height + amount, min, max)`, computed in 64-bit integers, so no
  input can overflow. The range is named by the grant, not copied into the
  action. Under another range a step may give other heights and is then
  reported as diverged, as for a scatter palette.
- **A covered cell whose height is already outside the range refuses the
  whole action** with `target.unsupported`, and the message names the cell
  and the range. Clamping it would move it into the range by a jump of any
  size, which is not a raise or lower by `amount`. Moving it partway would
  write a height outside the range, which the owner's decision refuses.
  Leaving it out silently would make a lower that does not lower. The
  person widens the range or leaves that cell out.
- **Cells of chunks that do not exist are left out**, as the zone brush
  leaves them out. A height belongs to the ground, and such a cell has
  none. Writing one would make World create a chunk document that holds
  only a height grid. Paint and fill create the chunk, because terrain is
  what a chunk is for. If every covered cell is in such a chunk, the action
  changes nothing (`change.none`). A chunk that exists but has no height
  grid gets one. Each cell expects 0, World's value for a missing grid, and
  World removes the grid again when all its heights return to 0.
- **Cells that would not change are left out**: those already at the end
  the amount moves toward. A stroke left with no change is `change.none`.
  Every other cell expects its committed height, so World refuses the
  transaction if the source changed.
- **No new refusal code.** The checks run in this order: the stroke rule
  (`input.budget`, `input.invalid`), the amount (`input.invalid` for 0,
  `input.budget` past 1,024), the grant (`grant.missing`), the view
  (`scope.outside_view`), out-of-range heights (`target.unsupported`) and
  no change (`change.none`).

## Not decided here

The desktop sculpt mode and its live preview, flatten, smooth, falloff,
slope marks, a display scale for heights (the renderer's `Stretch` has no
profile member), and whether the neutral sample gains heights. Each is a
later pull request or needs its own decision.
