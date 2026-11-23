# ADR 0007: flatten as intent, with a recorded target

Status: accepted, 2026-11-23 (issue #58, slice C, the fourth of five pull
requests).

Raise and lower ([ADR 0006](0006-catalog-heights.md)) move heights by an
amount. Levelling an area to one height, the most common sculpting task
after them, needs a target instead, and it is also the only way to bring
back a tile whose height is more than 1,024 outside the granted range,
which no single adjustment can move.

## Owner decisions, 2026-11-18

1. **Flatten is the fourth sculpting pull request**, after the desktop
   sculpt mode and before smooth.
2. **The target is sampled once at press and recorded.** The desktop reads
   the height under the pointer when the stroke starts, from the committed
   world, and the action carries that height. A replay never samples.
3. **Tools are intent kinds expanded inside the engine** (ADR 0006,
   decision 3), and the grant is the `editing.heights` range (decision 2).

## Coordinator defaults (taken unless the owner overrules them)

- **The target must lie in the granted range**, and is bounded by the same
  sanity limit as the grant (±1,024).
- **Cells outside the range are flattened like any other**: the target is
  in the range, so the result is too.
- **The desktop's step keys do nothing for Flatten**, and the scripted
  demonstration and the neutral sample stay unchanged.

## Implementation decisions (this pull request, not the owner's)

These were chosen by the implementing agent; the owner can revisit any of
them.

- **The action** is `FlattenHeights { plane, layer, cells, target }`,
  `flatten_heights` in proposals and the MCP editing server. `cells`
  follows the brush stroke's rule (1 to 4,096, distinct, in order), as for
  `adjust_heights`. `target` is an integer from −1,024 to 1,024
  (`MAXIMUM_TARGET`, its own wire constant, equal to the catalog's hard
  bound so that every granted height is a target). It records no grid: the
  grid is always `height`. 0 is a target.
- **The rule**, frozen for this kind, per covered cell:
  - a cell of a chunk that does not exist is left out, as for
    `adjust_heights` (a height is the ground's; writing one would create
    a chunk that holds only heights);
  - a cell already at the target is left out;
  - every other cell becomes the target, wherever it was: inside the range,
    or outside it by any distance, up to the 32-bit extremes.

  So the tool never writes outside the range. Unlike a raise or lower it
  can bring back any tile, however far outside the range; the
  `adjust_heights` refusal for a cell more than 1,024 away now ends "leave
  it out of the stroke, or flatten it to a height in the range" (only its
  words changed; its code and rule did not).
- **A target outside the range is `grant.missing`**, with a message naming
  the target and the range ("flatten target 7 is outside the granted height
  range -6 to 6"). The catalog does not grant writing that height, as it
  does not grant a terrain value outside `editing.terrain`, which is
  refused with the same code. It is checked right after the grant, so
  before the view, as a paint's grant is. A target past ±1,024 is
  `input.budget`, as an amount past 1,024 is, and a proposal holding one
  is `proposal.invalid` when read.
- **No bound on the jump.** A cell's change is `|target − height|`, which
  for a stored height near the 32-bit extremes is more than 2 billion. The
  bound on `adjust_heights`' amount exists because the amount is part of
  the wire form and moves every cell by it; a flatten's wire form carries
  only the target, which is bounded, and its result is always in the
  range, which is bounded. Bounding the jump would make the far-outside
  cells that only a flatten can bring back unfixable, and the steepness a
  flatten creates beside its neighbours is what the desktop's undrawable
  check guards against (below). Nothing subtracts heights: the engine
  compares and assigns, and the stacked-height check is in 64 bits.
- **Stacking.** As for `adjust_heights`, a new height that would stack past
  32 bits on its layer (`layer × layer_height + target`, only on a very far
  layer) is refused with `target.unsupported`.
- **Nothing to change** is `change.none`, and the message counts the
  covered cells already at the target and those in chunks that do not
  exist: "flatten changes no cell: 3 covered cells already at the target 4,
  2 in map chunks that do not exist (painting terrain there creates them)".
- **The transaction** is one `terrain.set` on the `height` grid; each cell
  expects its committed height (0 in a chunk without a grid, which World
  then creates). A flatten to 0 that returns every height of a chunk to 0
  makes World remove the grid, and undo restores it exactly, as for
  `adjust_heights`.
- **Replay.** The target is recorded, and the range is read from the
  catalog each time the step is prepared. Under any range that still holds
  the target, the same world gives the same transaction, so the step
  applies even though the catalog differs. Under a range that no longer
  holds it, or none, the step is refused with `proposal.step_refused` and
  the engine's `grant.missing`. An editor from before this kind (1.59.0
  and earlier) refuses a whole proposal holding one with
  `proposal.invalid` ("unknown action kind").
- **No new refusal code.** The checks run in this order: the stroke rule
  (`input.budget`, `input.invalid`), the target's bound (`input.budget`),
  the grant and the target in it (`grant.missing`), the view
  (`scope.outside_view`), per cell the stacked height
  (`target.unsupported`), then no change (`change.none`).

### The desktop

- **Flatten is a third row of Sculpt mode's tool list.** F cycles Raise,
  Lower, Flatten and back to Raise.
- **The target is the committed height of the cell under the pointer at
  press**, read once from the committed World (never from a pending
  change) and fixed for the whole stroke, however many other heights the
  drag passes over. The release prepares one `flatten_heights` action with
  that target.
- **A press on a tile without a chunk is refused** ("Flatten needs a
  starting tile with ground: tile (x, y) has no map chunk."): there is no
  height to sample.
- **A press on a tile whose height is outside the range is refused**,
  naming the height and the range ("Flatten refused: this tile's height 30
  is outside the granted range -8 to 8; start on a tile inside it.").
  Clamping the target into the range was the alternative; it was not
  chosen because it would flatten to a height the person did not point at,
  which is the one result that could surprise. A tile far outside the
  range may not be drawn where it stands, so it cannot be pointed at:
  start on an in-range neighbour and let the brush cover its cell (a 3 × 3
  brush reaches it).
- **Undrawn tiles are counted, not marked.** The live marks are the cells
  Accept writes that this view draws. A changed cell whose tile is not
  drawn (a height far outside the range) has no geometry to mark, and
  none is invented: the status line adds "N not drawn at this view" and
  the `sculpt_preview` evidence an `undrawn` member (`count`, `first`),
  only when there are any. Raise and lower share the same code path.
- **The undrawable-change advice** for a flatten is "Zoom out, or flatten
  to a height nearer the tiles around it.": what makes a flatten
  undrawable is the step between the flattened cells and their neighbours. For a proposal step of either height kind, whose amount or target the
  person cannot change, the advice is only "Zoom out and prepare the step
  again."

## Not decided here

Smooth (the fifth pull request), falloff, and typing a target in the
desktop (agents and proposals already carry any target in the range).
