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

## Owner decisions

On 2026-11-18:

1. **Smooth is the fifth sculpting pull request**, after flatten.
2. **A fixed kernel, recorded by version; it reads only committed
   heights.** The action names the rule by a version number, so the rule
   can be improved later by a new version without changing what a
   recorded step means.
3. **Tools are intent kinds expanded inside the engine** (ADR 0006,
   decision 3), and the grant is the `editing.heights` range (decision 2).

On 2026-11-23 (question UI, after review):

4. **Kernel 1 is the wider rule: each cell looks two tiles out** (the 5 × 5
   binomial below). The first version of this pull request read only the
   eight neighbours (a 3 × 3 rule weighted 1-2-1); the review measured that
   it leaves a raise of 8 at 0, 2, 6, 8 after one stroke and then calls
   every later stroke "already even", because a 3 × 3 rule rests wherever
   the slope bends by at most 2 per tile, whatever the step. Nothing was
   recorded with that rule (it was never merged), so it is not a version:
   kernel 1 is the 5 × 5 rule.

## Coordinator defaults (taken unless the owner overrules them)

- **Smooth reads the active view only and ignores chunks that do not
  exist**: a cell outside the view, or in a chunk that does not exist, is
  left out of the average.
- **One fixed strength** (no amount); a repeated stroke smooths further
  until the ground rests (below).
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
2. **The cells around it** are the 24 tiles `(x + dx, y + dy)` with `dx`
   and `dy` each from −2 to 2, not both 0, on the action's plane and
   layer. Such a cell is **read** when both hold:
   - it is inside the active view (the catalog's plane, layer and bounds,
     as for the covered cells themselves);
   - its chunk exists (a chunk without a height grid holds 0 everywhere,
     and is read).

   A cell that is not read is left out entirely: it adds nothing to the
   sum and nothing to the weight. A read cell of stored height `t` counts
   as `clamp(t)`, whether or not it is itself covered: every value is the
   committed height, never another cell's new one.
3. **Weights**: with `b(−2) = 1, b(−1) = 4, b(0) = 6, b(1) = 4, b(2) = 1`
   (the binomial 1, 4, 6, 4, 1), the cell `(dx, dy)` away weighs
   `b(dx) × b(dy)`:

   | dy \ dx | −2 | −1 | 0 | 1 | 2 |
   |---|---|---|---|---|---|
   | −2 | 1 | 4 | 6 | 4 | 1 |
   | −1 | 4 | 16 | 24 | 16 | 4 |
   | 0 | 6 | 24 | **36** | 24 | 6 |
   | 1 | 4 | 16 | 24 | 16 | 4 |
   | 2 | 1 | 4 | 6 | 4 | 1 |

   The cell itself weighs 36; all 25 weigh 256.
4. **Sum and divisor**: `D` is the sum, over the read cells around, of
   `weight × (clamp(t) − h)`, and `W` is 36 plus the sum of their weights
   (so 36 to 256).
5. **Rounding**: `q = ⌊|D| / W⌋` and `r = |D| − q × W`; if `2r > W`, add 1
   to `q`. So `|D| / W` is rounded to the nearest integer, and an exact
   half rounds toward zero.
6. **The new height** is `n = h + q` when `D ≥ 0`, `n = h − q` when
   `D < 0`.
7. The cell is **changed** when `n ≠ s`, and then expects `s` and gets
   `n`; otherwise it is left out ("already even").

Equivalently: the new height is the weighted mean of the clamped heights
read (the cell's own included), rounded to the nearest integer, with an
exact half going toward the cell's own clamped height `h`, for either
sign.

Properties that follow, and that the tests and a separate search check
(the numbers are in the pull request):

- **The order of the cells does not matter.** Every value read is a
  committed height (a Jacobi pass, not a sequential one), so a cell's
  result is the same whatever else the stroke covers or in which order.
- **No overshoot, and never outside the range.** `n` is a weighted mean
  of clamped values, rounded to the nearer of the two integers that
  bracket it, so it lies between the smallest and largest clamped value
  read. A covered cell outside the range is therefore brought into it (as
  a flatten does).
- **Symmetric.** Mirroring or transposing the ground mirrors or transposes
  the result; shifting every height and the range by the same amount
  shifts every result by it, and negating them negates every result: no
  rounding direction favours up or down, and the same hill smooths the
  same at any height.
- **An even slope is kept where all 25 cells are read**: the weights are
  symmetric, so the mean of a plane is its centre. Within two tiles of the
  view's edge, or of a chunk that does not exist, the mean is one-sided, so
  there a slope's end is pulled toward the inside, and repeated whole-view
  strokes slowly flatten a slope's ends at the view's edge.
- **No cycles.** Repeated strokes over the same cells always came to rest
  in the search: no pattern swaps back and forth.
- **No arithmetic overflows.** `|clamp(t) − h|` is less than 2^32, `D` is
  at most 220 × 2^32 in size, and `n` lies between two heights of the
  range; 64-bit integers hold all of it.

Then, per changed cell, the stacked-height check of ADR 0006 and 0007
(`layer × layer_height + n` past 32 bits: target.unsupported); none
changed is change.none.

### Where smoothing rests

A cell is "already even" when its height is within half a unit of the
weighted mean of the cells it reads. Along a straight profile with every
cell read, that is `|h(x−2) + 4·h(x−1) − 10·h(x) + 4·h(x+1) + h(x+2)| ≤ 8`.
What decides rest is how sharply the slope bends, not how large a step
is; an even slope of any steepness is kept. Measured (whole-view strokes;
the first row in each pair is after one stroke):

| Start | After one stroke | At rest (strokes) |
|---|---|---|
| step 0 \| 1 | unchanged | unchanged (0) |
| step 0 \| 2 | 0, 1, 1, 2 | the same (1) |
| step 0 \| 3 | 0, 1, 2, 3 | the same (1) |
| step 0 \| 4 | 0, 1, 3, 4 | the same (1) |
| step 0 \| 8 (a raise of 8) | 0, 2, 6, 8 | 0, 1, 3, 5, 7, 8 (2) |
| step 0 \| 64 | 0, 4, 20, 44, 60, 64 | a ramp with steps up to 6 (25) |
| an even slope of 1 or 4 | kept where every cell is read | kept in the middle; ends flattened at the view's edge |
| a spike of 8 among zeros | 1 and its 4 edge neighbours 1 | all 0 (3) |
| a checkerboard or stripes of 0 and 1 | kept in the middle, changed near the view's edge | two flat areas, 0 and 1 (4) |

In the desktop the same raise of 8 (columns 4 to 7 of the sample's 8 × 6
view, a 7 × 7 brush) becomes 0, 0, 0, 2, 6, 8, 8, 8 after one stroke and
0, 0, 1, 3, 5, 7, 8, 8 after the second; the third is "already even".
Across resting monotone profiles of 9 tiles with heights from 0 to 16
(82,253 of them, exhaustively), the slope bends by at most 3 per tile at a
resting cell (for example 0, 1, 3, 6), against 2 for the 3 × 3 rule: the
wider rule rests later in the sense that matters, a raise of 8 ends with
steps of at most 2 instead of 4, but it does not flatten an even slope.

### Why these choices

- **The 5 × 5 binomial** (the owner's decision 4). Its weights never
  invert a pattern (each axis's 1, 4, 6, 4, 1 has a frequency response
  that is never negative), so no ground flickers from stroke to stroke; it
  weighs near cells more than far ones and treats both axes alike; and,
  unlike every 3 × 3 rule that never inverts, it reaches far enough to
  spread a raise of 8 over 5 tiles instead of 3. The four edge neighbours
  alone (a plus shape) and an unweighted 3 × 3 box both make patterns
  swap forever (the search finds them); the 3 × 3 binomial does not, but
  rests at 0, 2, 6, 8. The cost is reading 24 cells instead of 8 (about
  1 ms for a whole 4,096-tile view, below) and a view-edge band two tiles
  deep.
- **Cells outside the stroke are read.** Smoothing blends a cell toward
  its surroundings; reading only covered cells would leave a one-cell
  stroke unable to change anything.
- **Rounding to the nearest, a half toward the cell's own height.** Floor
  would sink ground a little with every stroke; rounding a half away from
  zero would treat heights above and below 0 differently, so the same
  hill would smooth differently at another height. Toward the cell's own
  height has neither bias, moves a cell no further than it must, and with
  this kernel the search found no cycles.
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

### How a kernel 2 would be added

A new rule is a new version, never a change to version 1: the engine
gains a second rule selected by `kernel = 2`, the proposal reader and the
MCP schema admit 2, and the desktop chooses which version its strokes
record. Steps recorded with kernel 1 keep meaning exactly what is written
here, and an editor that knows only kernel 1 refuses a proposal holding
kernel 2 with `proposal.invalid`, as it refuses an unknown action kind.

### The view is part of the action's meaning

Which cells are read depends on the active view, which is the catalog's.
The covered cells must already lie in it (scope.outside_view otherwise, as
for every height kind), and a cell outside it is not read. So a covered
cell within two tiles of the view's edge depends on where the view ends:
a recorded step prepared again under a catalog whose view reads other
cells along its edge prepares another transaction, and a proposal step is
reported as `proposal.step_diverged` and not applied, as a raise under
another range is. This holds for a cell two tiles from the edge as much
as for one beside it (both tested). Under a view that reads the same cells
(the same bounds, or bounds that differ only more than two tiles from
every covered cell) the transaction is the same and the step applies.

### Cells that are read but not written

Each changed cell expects its committed height in the `terrain.set`, as
for the other height kinds. A cell that is read but not written is not in
the transaction. World's `terrain.set` could take it as a no-op
cell (`value` equal to `expect`), but that is not free: a chunk whose
height grid holds only zeros loses the grid when a no-op cell is written
into it (World removes an all-zero grid), which would be a change the
person did not ask for, and it would add every read tile of the view
(up to 4,096) to each step for nothing else. It is not needed either:
Accept reloads the whole World and refuses unless it is still the one the
change was prepared on (`source.changed`), so a cell read changed on disk
between Review and Accept, one tile away or two, in any chunk, refuses the
Accept and writes nothing (tested with cells in a chunk the transaction
does not touch).
The residual is the one every kind shares: a write by another program to a
chunk the transaction does not consult, during the moment World publishes,
is not prevented, but is caught right after (`publish.unverified`, the
session blocks until reopened). A proposal step is always prepared again
from the person's committed World, so it reads that World's heights.

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
  4,096-cell stroke over a whole 64 × 64 view (25 reads per cell) plans in
  about 1.0 ms in a release build (10 ms in a debug build; a raise
  0.035 ms), and on #89's benchmark view (3,400 cells of bent isometric
  ground) one pointer move with its frame on the CPU scene path takes
  about 34.7 ms for Smooth and 34.5 ms for Raise in a release build (284 ms
  and 262 ms in a debug build): the rendering dominates, not the plan.
- **The stroke collects cells as the other tools do**; the release
  prepares one `smooth_heights` action with kernel 1. The live marks are
  the engine's plan on the committed World, as for the other tools: the
  cells Accept writes that this view draws, with undrawn ones counted.
- **The undrawable-change advice** for a person's own smooth is "Zoom
  out." A smooth moves each cell toward its surroundings, but the edge of a
  stroke can still come out steeper than the view can draw, and there is
  no amount to lessen. A proposal step gets "Zoom out and prepare the step
  again.", as the other height kinds.

## Frozen

Kernel version 1 as written above (the 5 × 5 binomial, clamped reads,
cells outside the view or in missing chunks left out, rounding a half
toward the cell's own height, Jacobi), and the `smooth_heights` wire form.
A different neighbourhood, weighting, rounding, range treatment or set of
cells read is a new kernel version, not a change to version 1.

## Not decided here

Strength or falloff control, smoothing across layers or planes, slope
marks, and roads (slice D).
