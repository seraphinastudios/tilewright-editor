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

- **A hard bound of ±1,024, a sanity limit only.** A catalog's `min` and
  `max` must lie from −1,024 to 1,024 (`MAXIMUM_HEIGHT`), and `min` must be
  below `max` (a single value would allow no raise or lower). The bound
  keeps the arithmetic and the wire form small and simple. It is **not** a
  promise that a view can draw every range inside it: no single number
  is. A bent isometric tile is a 4 × 4 mesh that maps its ground art, and
  art taller than the diamond multiplies the differences between corner
  heights. So the range a project's views can draw depends on its ground
  art and the zoom (the desktop's largest zoom and `render --scale` are
  8). Choosing a range its views can draw is the project's job, as the
  owner put it: "the project chooses a range its views can draw".
  Guidance, measured by the reviewer on renderer 0.7.1 (Mean corners,
  1 pixel per unit, the worst 3 × 3 pattern of ±H around a tile, zoom 8):

  | Projection, diamond | Ground art | Largest drawable ±H at zoom 8 |
  |---|---|---|
  | top-down | any | no limit (top-down ground shifts, never bends) |
  | isometric 32 × 16 | 32 × 16 (diamond only) | ±1,433 |
  | isometric 64 × 32 | 64 × 32 (diamond only) | ±1,525 |
  | isometric 32 × 16 | 32 × 32 (the neutral sample's art) | ±460 |
  | isometric 64 × 32 | 64 × 64 | ±487 |
  | isometric 128 × 64 | 128 × 128 | ±495 |
  | isometric 32 × 16 | 32 × 48 | ±215 |
  | isometric 64 × 32 | 64 × 128 | ±129 |

  At a lower zoom the drawable range is wider. Lint's
  `catalog.heights_undrawable` warning
  ([docs/CLI.md](../docs/CLI.md#lint-findings)) computes this bound for a
  catalog; its unit tests reproduce this table on renderer 0.7.2. The range a
  stroke can produce is what counts: every height in the view and the
  ring around it, and the 0 of tiles without a height grid. The desktop
  refuses an edit whose result cannot be drawn at its current view: the
  change is cancelled, whatever the action, and the notice gives the
  renderer's reason and what to do (for a height adjustment: zoom out, or
  raise or lower by less; for a flatten, ADR 0007: zoom out, or flatten
  to a height nearer the tiles around it; for any other edit: zoom out or
  pan and try again), and editing continues instead of pausing.
- **One range per catalog.** The grant applies to the active view's plane
  and spatial layer, as `editing.zones.paint` does. Heights are one grid
  per chunk, not a ground grid, so the per-grid shape of `editing.terrain`
  does not fit, and the owner chose one range over per-layer ranges.
- **Versioning (ADR 0004).** `editing.heights` is an optional member in
  both catalog formats, `tilewright.editor-preview/1` and `/2`. The latest
  release (1.53.0) closes `editing` to `movable_uids`, `terrain`, `fields`,
  `objects`, `zones`, `anchors` and `palettes`, and refuses anything else
  with "unknown or missing catalog key", and its proposal reader refuses
  the kind with "unknown action kind". So an older editor refuses a
  catalog with a height range instead of ignoring it. A catalog without
  the member loads, reports and draws exactly as before.
- **The action** is `AdjustHeights { plane, layer, cells, amount }`,
  `adjust_heights` in proposals and the MCP editing server: one kind for
  raising (positive) and lowering (negative). `cells` follows the brush
  stroke's rule (1 to 4,096, distinct, in order), as scatter does, so the
  desktop's stroke can become this action unchanged. `amount` is an integer
  from −1,024 to 1,024, not 0 (`MAXIMUM_AMOUNT`, its own constant, so the
  wire form does not move with the catalog bound). It records no grid: the
  grid is always `height`.
- **The rule**, frozen for this kind, per covered cell of height `h`,
  computed in 64-bit integers so no stored height or amount can overflow:
  - at or beyond the end the amount moves toward (`h ≥ max` for a raise,
    `h ≤ min` for a lower): left out, like a cell exactly at that end;
  - otherwise the cell becomes `clamp(h + amount, min, max)`;
  - except a cell beyond the other end that moving by the amount does not
    bring into the range (`h + amount < min` for a raise, `> max` for a
    lower): the whole action is refused with `target.unsupported`, and the
    message names the cell, its height and the amount it needs. When that
    is more than 1,024, the most one action can move, the message says
    instead that this tool cannot bring the cell in and that it should be
    left out of the stroke, or flattened to a height in the range
    (`flatten_heights`, [ADR 0007](0007-flatten-heights.md), which can
    bring back any cell).

  So the tool never writes outside the range, never moves a cell by more
  than the amount and never moves one away from the range, and it can bring
  back any tile within 1,024 of the range. The range is named by the grant,
  not copied into the action: under another range a step may give other
  heights and is then reported as diverged, as for a scatter palette.
- **Stacking.** The preview draws a tile at `layer × layer_height +
  height` in 32 bits. A write that would stack past that range (only on a
  very far layer) is refused with `target.unsupported`, computed in 64-bit
  integers.
- **Cells of chunks that do not exist are left out**, as the zone brush
  leaves them out. A height belongs to the ground, and such a cell has
  none. Writing one would make World create a chunk document that holds
  only a height grid. Paint and fill create the chunk, because terrain is
  what a chunk is for. If every covered cell is in such a chunk, the action
  changes nothing (`change.none`). A chunk that exists but has no height
  grid gets one. Each cell expects 0, World's value for a missing grid, and
  World removes the grid again when all its heights return to 0.
- **Cells that would not change are left out**: those at or beyond the end
  the amount moves toward. A stroke left with no change is `change.none`,
  and the message counts the covered cells at or beyond that end ("at or
  above the top", "at or below the bottom") and those in chunks that do
  not exist, so an agent knows to paint terrain first.
  Every other cell expects its committed height, so World refuses the
  transaction if the source changed.
- **No new refusal code.** The checks run in this order: the stroke rule
  (`input.budget`, `input.invalid`), the amount (`input.invalid` for 0,
  `input.budget` past 1,024), the grant (`grant.missing`), the view
  (`scope.outside_view`), then per cell a height the amount cannot bring
  into the range or that would stack past 32 bits (`target.unsupported`),
  and no change (`change.none`).

## Not decided here

The desktop sculpt mode and its live preview, flatten, smooth, falloff,
slope marks, a display scale for heights (the renderer's `Stretch` has no
profile member), and whether the neutral sample gains heights. Each is a
later pull request or needs its own decision.
