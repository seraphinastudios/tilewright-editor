# ADR 0008: smooth as intent, with a recorded kernel version

Status: accepted, 2026-11-23 (issue #58, slice C, the fifth of five pull
requests).

Raise, lower ([ADR 0006](0006-catalog-heights.md)) and flatten
([ADR 0007](0007-flatten-heights.md)) move heights by an amount or to a
target. Evening out rough ground needs a third kind of rule: each height
moves toward its surroundings. The new height of a cell depends on its
neighbours, so the rule has to say exactly which neighbours are read, how
they are weighted and how the result is rounded, or two editors would
smooth the same ground differently.

## Owner decisions, 2026-11-18

1. **Smooth is the fifth sculpting pull request**, after flatten.
2. **A fixed kernel, recorded by version; it reads only committed
   heights.** The action names the rule by a version number, so the rule
   can be improved later by a new version without changing what a
   recorded step means.
3. **Tools are intent kinds expanded inside the engine** (ADR 0006,
   decision 3), and the grant is the `editing.heights` range (decision 2).

## Coordinator defaults (taken unless the owner overrules them)

- **Smooth reads the active view only and ignores chunks that do not
  exist**: a neighbour outside the view, or in a chunk that does not
  exist, is left out of the average.
- **One fixed strength** (no amount); repeated strokes smooth further.
- **The desktop's step keys do nothing for Smooth**, as for Flatten, and
  the scripted demonstration and the neutral sample stay unchanged.

## Implementation decisions (this pull request, not the owner's)

These were chosen by the implementing agent; the owner can revisit any of
them.

### The action

`SmoothHeights { plane, layer, cells, kernel }`, `smooth_heights` in
proposals and the MCP editing server. `cells` follows the brush stroke's
rule (1 to 4,096, distinct, in order), as for `adjust_heights`. `kernel` is
the rule's version, an integer; only `1` exists, and it is frozen as
written below. It records no grid (always `height`) and no strength.

### Kernel version 1 (frozen)

Let the granted range be `min` to `max`, and `clamp(v)` be `v` limited to
it (`min` when `v < min`, `max` when `v > max`, else `v`). All arithmetic
is on mathematical integers (64 bits suffice; see below). For each covered
cell `(x, y)` whose chunk exists, with stored height `s`:

1. **Its own height** is read as `h = clamp(s)`.
2. **Its neighbours** are the eight tiles `(x + dx, y + dy)` with `dx`,
   `dy` each in −1, 0, 1, not both 0, on the action's plane and layer.
   A neighbour is **read** when both hold:
   - it is inside the active view (the catalog's plane, layer and bounds,
     as for the covered cells themselves);
   - its chunk exists (a chunk without a height grid holds 0 everywhere,
     and is read).

   A neighbour that is not read is left out entirely: it adds nothing to
   the sum and nothing to the weight. A read neighbour of stored height
   `t` counts as `clamp(t)`, whether or not it is itself covered: every
   value is the committed height, never another cell's new one.
3. **Weights**: 4 for the cell itself; 2 for a neighbour that shares an
   edge with it (`dx = 0` or `dy = 0`); 1 for a diagonal neighbour. All
   eight read: 4 + 4 × 2 + 4 × 1 = 16.
4. **Sum and divisor**: `D` is the sum, over the read neighbours, of
   `weight × (clamp(t) − h)`, and `W` is 4 plus the sum of the read
   neighbours' weights (so 4 to 16).
5. **Rounding**: `q = ⌊|D| / W⌋` and `r = |D| − q × W`; if `2r > W`, add 1
   to `q`. So `|D| / W` is rounded to the nearest integer, and an exact
   half rounds toward zero.
6. **The new height** is `n = h + q` when `D ≥ 0`, `n = h − q` when
   `D < 0`.
7. The cell is **changed** when `n ≠ s`, and then expects `s` and gets
   `n`; otherwise it is left out ("already even").

Equivalently: the new height is the weighted mean of the clamped heights
read, rounded to the nearest integer, with an exact half going toward the
cell's own clamped height `h`.

Properties that follow, and that the tests check:

- **The order of the cells does not matter.** Every value read is a
  committed height (a Jacobi pass, not a sequential one), so a cell's
  result is the same whatever else the stroke covers or in which order.
- **It never writes outside the range.** `n` is a weighted mean of values
  in the range, rounded to the nearer of two integers that bracket it, and
  `min` and `max` are integers. A covered cell outside the range is
  therefore brought into it (as a flatten does), and a cell inside the
  range with no change from smoothing is left out.
- **It does not depend on where height 0 is.** Shifting every height and
  the range by the same amount shifts every result by it, and negating
  them negates every result: no rounding direction favours up or down.
- **A uniform slope is kept** where all eight neighbours are read: the
  weights are symmetric, so the mean of a plane is its centre.
- **No arithmetic overflows.** `|clamp(t) − h|` is less than 2^32, `D` is
  at most 12 × 2^32 in size, and `n` lies between two heights of the
  range; 64-bit integers hold all of it.

Then, per changed cell, the stacked-height check of ADR 0006 and 0007
(`layer × layer_height + n` past 32 bits: target.unsupported); none
changed is change.none.

### Why these choices

- **The 3 × 3 neighbourhood with 1-2-1 weights** (the binomial kernel).
  The four edge neighbours alone (a plus shape) swap a checkerboard of 0
  and 1 forever: each 0 averages to 0.8 and each 1 to 0.2. An unweighted
  3 × 3 box inverts stripes: 0, 9, 0, 9 becomes 6, 3, 6, 3, and stripes
  of 0 and 1 swap on every stroke, forever. The 1-2-1 kernel's weights
  never invert a pattern (0, 9, 0, 9 becomes 4, 5, 4, 5; 0 and 1 stay),
  weigh near neighbours more than far ones, and treat both axes alike.
  The price, shared by every 3 × 3 kernel that never inverts, is that a
  straight step of 2 between two flat areas is already even: each side's
  mean is exactly half a step away, and the half goes toward the cell. So
  repeated strokes come to rest where neighbours differ by about 2 or
  less (a randomised check of thousands of small grids found no cycle and
  no resting step above 2); a step of 3 or more is smoothed.
- **Neighbours outside the stroke are read.** Smoothing blends a cell
  toward its surroundings; reading only covered cells would leave a
  one-cell stroke unable to change anything.
- **Rounding to the nearest, a half toward the cell's own height.** Floor
  would sink ground a little with every stroke; rounding a half away from
  zero would treat heights above and below 0 differently, so the same
  hill would smooth differently at another height. Toward the cell's own
  height has neither bias and moves a cell no further than it must.
- **Every height read is clamped into the range, the cell's own
  included.** Read as stored, one tile far outside the range (stored
  heights are unconstrained) would drag its neighbours to the end of the
  range; left out, it would make a cell beside it look smooth when it is
  not. Clamped, it counts as the end of the range, and the result never
  needs a separate clamp. A covered cell outside the range is smoothed
  like any other, so it moves into the range (allowed, as for flatten,
  rather than refused, as for raise and lower): the rule that cannot write
  outside the range is also the simplest to state.
- **The kernel is a recorded integer, not a name.** It is the smallest
  thing that can say "version 1". An unknown version is refused, never
  read as version 1. A proposal holding one is `proposal.invalid` when it
  is read, as an unknown action kind is: the reader cannot know what the
  step means, so it cannot show or verify it. The engine refuses one with
  `input.invalid` ("unknown smooth kernel 2; this editor knows kernel 1"),
  for callers that build an action in code. The MCP schema admits only 1
  (as `minimum` and `maximum` 1, so that `1.0`, which JSON Schema's
  `integer` admits and the server rewrites to 1, is taken alike).
- **No strength parameter.** A second parameter would need its own bound,
  wire form and tests, and a stronger smooth is two strokes.

### The view is part of the action's meaning

Which neighbours are read depends on the active view, which is the
catalog's. The covered cells must already lie in it (scope.outside_view
otherwise, as for every height kind), and a neighbour just outside it is
not read. So a recorded step prepared again under a catalog whose view
differs can read other neighbours along the view's edge, and then prepares
another transaction: a proposal step is reported as `proposal.step_diverged`
and not applied, as a raise under another range is. Under a view that
reads the same neighbours (for example the same bounds, or bounds that
differ only away from the stroke) the transaction is the same and the step
applies.

### Neighbours that are read but not written

Each changed cell expects its committed height in the `terrain.set`, as
for the other height kinds. A neighbour that is read but not written is
not in the transaction. World's `terrain.set` could take it as a no-op
cell (`value` equal to `expect`), but that is not free: a chunk whose
height grid holds only zeros loses the grid when a no-op cell is written
into it (World removes an all-zero grid), which would be a change the
person did not ask for, and it would add every read tile of the view
(up to 4,096) to each step for nothing else. It is not needed either:
Accept reloads the whole World and refuses unless it is still the one the
change was prepared on (`source.changed`), so a neighbour changed on disk between
Review and Accept, in any chunk, refuses the Accept and writes nothing.
The residual is the one every kind shares: a write by another program to a
chunk the transaction does not consult, during the moment World publishes,
is not prevented, but is caught right after (`publish.unverified`, the
session blocks until reopened). A proposal step is always prepared again
from the person's committed World, so it reads that World's neighbours.

### The rest

- **Nothing to change** is `change.none`, and the message counts the
  covered cells already even and those in chunks that do not exist:
  "smooth changes no cell: 3 covered cells already even, 2 in map chunks
  that do not exist (painting terrain there creates them)".
- **The transaction** is one `terrain.set` on the `height` grid, as for a
  raise; World creates a missing height grid and removes one that returns
  to all zeros, and undo restores it exactly.
- **Replay.** The kernel is recorded; the range and the view are read from
  the catalog each time the step is prepared. Without the grant the step
  is refused (`grant.missing`); under another range or view that changes
  any result, it diverges. An editor from before this kind (1.61.0 and
  earlier) refuses a whole proposal holding one with `proposal.invalid`
  ("unknown action kind").
- **No new refusal code.** The checks run in this order: the stroke rule
  (`input.budget`, `input.invalid`), the kernel (`input.invalid`), the
  grant (`grant.missing`), the view (`scope.outside_view`), per changed
  cell the stacked height (`target.unsupported`), then no change
  (`change.none`).

### The desktop

- **Smooth is a fourth row of Sculpt mode's tool list.** F cycles Raise,
  Lower, Flatten, Smooth and back to Raise. The step rows are dimmed and 1
  and 8 do nothing, as for Flatten.
- **Its row reads "Smooth: drag evens out heights"**, the pinned row
  "Sculpt: Smooth  F", the status line while dragging "Smoothing 37 cells,
  30 to change, 7 already even; release to review." (with the established
  "not drawn at this view" and "without a chunk" counts), and the frame
  evidence's `sculpt_preview` has `kernel` and `even` in place of
  `amount` and `at_end`.
- **No cache for the preview.** The plan is recomputed from the committed
  World on every pointer move, as for the other tools. Measured: a
  4,096-cell stroke over a whole 64 × 64 view plans in about 0.34 ms in a
  release build (4.3 ms in a debug build; a raise 0.035 ms), and on #89's
  benchmark view (3,400 cells of bent isometric ground) one pointer move
  with its frame on the CPU scene path takes about 32 ms for Smooth and
  37 ms for Raise in a release build (260 ms either way in a debug
  build): the rendering dominates, not the plan.
- **The stroke collects cells as the other tools do**; the release
  prepares one `smooth_heights` action with kernel 1. The live marks are
  the engine's plan on the committed World, as for the other tools: the
  cells Accept writes that this view draws, with undrawn ones counted.
- **The undrawable-change advice** for a person's own smooth is "Zoom
  out." A smooth moves each cell toward its neighbours, but the edge of a
  stroke can still come out steeper than the view can draw, and there is
  no amount to lessen. A proposal step gets "Zoom out and prepare the step
  again.", as the other height kinds.

## Frozen

Kernel version 1 as written above, and the `smooth_heights` wire form. A
different neighbourhood, weighting, rounding, range treatment or set of
neighbours read is a new kernel version, not a change to version 1.

## Not decided here

Strength or falloff control, smoothing across layers or planes, slope
marks, and roads (slice D).
