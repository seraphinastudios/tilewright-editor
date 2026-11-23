// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sculpting. Raise and lower: the heights an
//! [`Action::AdjustHeights`](crate::Action::AdjustHeights) writes into the
//! cells it covers; flatten ([`flatten_plan`]): an
//! [`Action::FlattenHeights`](crate::Action::FlattenHeights)'s.
//!
//! The rule is part of what a recorded `adjust_heights` step means, so it
//! is fixed here for that action kind (`docs/ENGINE.md`, "Raise and
//! lower"; [`adjusted`]). With the catalog's `editing.heights` range and
//! 64-bit arithmetic, each covered cell of an existing chunk is:
//!
//! - left out when it is at or beyond the end the amount moves toward;
//! - refused, with the whole action, when it is beyond the other end and
//!   moving it by the amount does not bring it into the range;
//! - otherwise set to `clamp(height + amount, min, max)`.
//!
//! Cells of chunks that do not exist are left out. The amount is flat and
//! applies once per stroke: a stroke's cells are a set, so a cell is
//! changed once however often the brush passed over it.
use tilewright_editor_preview::{HeightGrant, PreviewCatalog};
use tilewright_source::{Plane, World};

use crate::session::refusal::refuse;
use crate::{RefusalCode, SessionError};

/// The largest amount one action raises or lowers by, either way. Part of
/// the `adjust_heights` wire form (proposals and the MCP schema), so it is
/// its own constant, independent of the catalog's
/// [`tilewright_editor_preview::MAXIMUM_HEIGHT`].
pub const MAXIMUM_AMOUNT: i32 = 1024;

/// One cell an adjustment changes, its height before and its new height.
pub type HeightChange = ([i64; 2], i32, i32);

/// What an adjustment by `amount` does to one cell of height `height`
/// under `grant` (frozen for `adjust_heights`, see `docs/ENGINE.md`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Adjusted {
    /// The cell is at or beyond the end the amount moves toward: left out.
    AtEnd,
    /// The cell's new height, `clamp(height + amount, min, max)`: another
    /// height than before, since the range holds more than one value.
    To(i32),
    /// The cell is beyond the other end, and moving it by the amount does
    /// not bring it into the range: it needs an amount of at least this
    /// magnitude.
    Short(u64),
}

/// The rule for one cell: a cell at or beyond the end the amount moves
/// toward is left out; any other cell moves by the amount and is kept
/// within the range, unless it starts beyond the other end and the amount
/// does not reach the range. Computed in 64-bit integers, so any `i32`
/// height and amount are exact.
#[must_use]
pub fn adjusted(height: i32, amount: i32, grant: HeightGrant) -> Adjusted {
    let (height, min, max) = (
        i64::from(height),
        i64::from(grant.min),
        i64::from(grant.max),
    );
    let raised = height + i64::from(amount);
    let (at_end, short) = if amount > 0 {
        (height >= max, (raised < min).then(|| min - height))
    } else {
        (height <= min, (raised > max).then(|| height - max))
    };
    if at_end {
        return Adjusted::AtEnd;
    }
    if let Some(needed) = short {
        return Adjusted::Short(needed.unsigned_abs());
    }
    // Between two i32 values, so this never fails.
    i32::try_from(raised.clamp(min, max)).map_or(Adjusted::AtEnd, Adjusted::To)
}

/// The amount rule: not 0, at most [`MAXIMUM_AMOUNT`] either way.
///
/// # Errors
/// [`RefusalCode::InputInvalid`] for 0, [`RefusalCode::InputBudget`]
/// past the bound.
pub fn check_amount(amount: i32) -> Result<(), SessionError> {
    if amount == 0 {
        return Err(refuse(
            RefusalCode::InputInvalid,
            "a height adjustment needs a non-zero amount",
        ));
    }
    if amount.unsigned_abs() > MAXIMUM_AMOUNT.unsigned_abs() {
        return Err(refuse(
            RefusalCode::InputBudget,
            "a height adjustment amount is at most 1024 either way",
        ));
    }
    Ok(())
}

/// The cells of adjustment `cells` by `amount` that its `terrain.set` on
/// the `height` grid changes in `world` under `catalog`, each with its
/// height before and after, in stroke order: what dispatching an
/// [`Action::AdjustHeights`](crate::Action::AdjustHeights) prepares, and
/// what a proposal step's words count. Cells of chunks that do not exist
/// are left out (a height is the ground's, and there is none), as are
/// cells at or beyond the end of the range the amount moves toward
/// ([`adjusted`]).
///
/// # Errors
/// In this order: the stroke rule (input codes); the amount
/// ([`check_amount`]); [`RefusalCode::GrantMissing`] without
/// `editing.heights`; [`RefusalCode::ScopeOutsideView`] for a cell outside
/// the active view; [`RefusalCode::TargetUnsupported`] when a covered cell
/// beyond the other end of the range would still be outside it after
/// moving by the amount ([`Adjusted::Short`]), or when a new height on
/// this layer would leave the 32-bit range once stacked
/// (`layer × layer_height + height`, as the preview draws it);
/// [`RefusalCode::ChangeNone`] when no cell would change, saying how many
/// were at the end of the range and how many in chunks that do not exist.
pub fn plan(
    world: &World,
    catalog: &PreviewCatalog,
    plane: &str,
    layer: i32,
    cells: &[[i64; 2]],
    amount: i32,
) -> Result<Vec<HeightChange>, SessionError> {
    crate::session::stroke_cells(cells)?;
    check_amount(amount)?;
    let grant = granted(catalog)?;
    let (found, base) = located(world, catalog, plane, layer, cells)?;
    let (mut at_end, mut missing) = (0usize, 0usize);
    let mut changes = Vec::new();
    for &[x, y] in cells {
        // `None`: the chunk does not exist, so the cell is left out.
        let Some(before) = crate::heights::cell_height(found, layer, x, y) else {
            missing += 1;
            continue;
        };
        match adjusted(before, amount, grant) {
            Adjusted::AtEnd => at_end += 1,
            Adjusted::Short(needed) => {
                let (side, verb) = if amount > 0 {
                    ("below", "raising")
                } else {
                    ("above", "lowering")
                };
                let (low, high) = (grant.min, grant.max);
                let message = if needed > u64::from(MAXIMUM_AMOUNT.unsigned_abs()) {
                    format!(
                        "cell ({x}, {y}) has height {before}, {side} the granted range {low} to {high}: it is {needed} from the range, more than one adjustment can move (1024), so this tool cannot bring it in; leave it out of the stroke, or flatten it to a height in the range"
                    )
                } else {
                    format!(
                        "cell ({x}, {y}) has height {before}, {side} the granted range {low} to {high}: {verb} it into the range needs an amount of at least {needed}"
                    )
                };
                return Err(refuse(RefusalCode::TargetUnsupported, &message));
            }
            Adjusted::To(after) => {
                stacks(base, after, [x, y], layer)?;
                changes.push(([x, y], before, after));
            }
        }
    }
    if changes.is_empty() {
        let end = if amount > 0 {
            "at or above the top"
        } else {
            "at or below the bottom"
        };
        return Err(refuse(
            RefusalCode::ChangeNone,
            &format!(
                "height adjustment changes no cell: {at_end} covered cells already {end} of the range, {missing} in map chunks that do not exist (painting terrain there creates them)"
            ),
        ));
    }
    Ok(changes)
}

/// The catalog's `editing.heights` range.
///
/// # Errors
/// [`RefusalCode::GrantMissing`] without it.
fn granted(catalog: &PreviewCatalog) -> Result<HeightGrant, SessionError> {
    catalog.editing().heights.ok_or_else(|| {
        refuse(
            RefusalCode::GrantMissing,
            "height editing refused: the catalog does not grant editing.heights",
        )
    })
}

/// The plane of a height edit whose `cells` all lie in the active view,
/// and its `layer`'s stacked base as the preview adds it (`layer ×
/// layer_height`, in 64 bits: exact).
///
/// # Errors
/// [`RefusalCode::ScopeOutsideView`] for a cell outside the active view,
/// [`RefusalCode::TargetUnknown`] for an unknown plane.
fn located<'w>(
    world: &'w World,
    catalog: &PreviewCatalog,
    plane: &str,
    layer: i32,
    cells: &[[i64; 2]],
) -> Result<(&'w Plane, i64), SessionError> {
    if !cells
        .iter()
        .all(|&[x, y]| catalog.tile_in_active_view(plane, layer, x, y))
    {
        return Err(refuse(
            RefusalCode::ScopeOutsideView,
            "height editing is limited to the active view",
        ));
    }
    let found = world
        .planes()
        .get(plane)
        .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown plane"))?;
    let base = i64::from(layer) * i64::from(world.profile().layer_height.unwrap_or(0));
    Ok((found, base))
}

/// Refuses a new height `after` of cell `[x, y]` that would stack past
/// the 32-bit range on `layer` (`base + after`, as the preview draws it).
fn stacks(base: i64, after: i32, [x, y]: [i64; 2], layer: i32) -> Result<(), SessionError> {
    if i32::try_from(base + i64::from(after)).is_err() {
        return Err(refuse(
            RefusalCode::TargetUnsupported,
            &format!(
                "cell ({x}, {y}): height {after} on layer {layer} would stack past the 32-bit range the preview draws"
            ),
        ));
    }
    Ok(())
}

/// The largest target height a flatten names, either way. Part of the
/// `flatten_heights` wire form (proposals and the MCP schema), so it is its
/// own constant; it equals the catalog's
/// [`tilewright_editor_preview::MAXIMUM_HEIGHT`], so every granted height
/// is a target and no target lies past the hard bound of any range.
pub const MAXIMUM_TARGET: i32 = 1024;

/// The target's sanity rule: at most [`MAXIMUM_TARGET`] either way.
///
/// # Errors
/// [`RefusalCode::InputBudget`] past the bound.
pub fn check_target(target: i32) -> Result<(), SessionError> {
    if target.unsigned_abs() > MAXIMUM_TARGET.unsigned_abs() {
        return Err(refuse(
            RefusalCode::InputBudget,
            "a flatten target is a height from -1024 to 1024",
        ));
    }
    Ok(())
}

/// What a flatten to `target` does to one cell of height `height`
/// (frozen for `flatten_heights`, see `docs/ENGINE.md`): `None`, left
/// out, when it is already at the target; otherwise its new height, the
/// target, wherever the cell was, inside the range or outside it.
#[must_use]
pub fn flattened(height: i32, target: i32) -> Option<i32> {
    (height != target).then_some(target)
}

/// The target a desktop Flatten stroke pressed on plane tile `[x, y]` of
/// `layer` records: that tile's height in `world`, which the caller passes
/// as the committed World, sampled once at the press. Refused, with words
/// for the person, without `editing.heights`, on a tile whose chunk does
/// not exist (there is no height to sample), and on a tile whose height is
/// outside the granted range: a flatten to it would be refused, and
/// clamping it into the range would flatten to a height nobody pointed at.
///
/// # Errors
/// The refusal's words.
pub fn flatten_target(
    world: &World,
    catalog: &PreviewCatalog,
    plane: &str,
    layer: i32,
    [x, y]: [i64; 2],
) -> Result<i32, String> {
    let grant = granted(catalog).map_err(|error| error.message().to_owned())?;
    let height = world
        .planes()
        .get(plane)
        .and_then(|found| crate::heights::cell_height(found, layer, x, y))
        .ok_or_else(|| {
            format!("Flatten needs a starting tile with ground: tile ({x}, {y}) has no map chunk.")
        })?;
    if !grant.contains(height) {
        return Err(format!(
            "Flatten refused: this tile's height {height} is outside the granted range {} to {}; start on a tile inside it.",
            grant.min, grant.max
        ));
    }
    Ok(height)
}

/// The cells of flatten `cells` to `target` that its `terrain.set` on the
/// `height` grid changes in `world` under `catalog`, each with its height
/// before and after (`target`), in stroke order: what dispatching an
/// [`Action::FlattenHeights`](crate::Action::FlattenHeights) prepares, and
/// what a proposal step's words and the desktop's live preview count.
/// Cells of chunks that do not exist and cells already at `target` are
/// left out ([`flattened`]). A cell outside the granted range is set to
/// the target like any other: the target is in the range, so the result
/// is too.
///
/// # Errors
/// In this order: the stroke rule (input codes); the target past its
/// bound ([`check_target`]); [`RefusalCode::GrantMissing`] without
/// `editing.heights`, or with a range that does not hold `target`;
/// [`RefusalCode::ScopeOutsideView`] for a cell outside the active view;
/// [`RefusalCode::TargetUnsupported`] when the target would stack past
/// the 32-bit range on this layer; [`RefusalCode::ChangeNone`] when no
/// cell would change, saying how many were already at the target and how
/// many in chunks that do not exist.
pub fn flatten_plan(
    world: &World,
    catalog: &PreviewCatalog,
    plane: &str,
    layer: i32,
    cells: &[[i64; 2]],
    target: i32,
) -> Result<Vec<HeightChange>, SessionError> {
    crate::session::stroke_cells(cells)?;
    check_target(target)?;
    let grant = granted(catalog)?;
    if !grant.contains(target) {
        let (low, high) = (grant.min, grant.max);
        return Err(refuse(
            RefusalCode::GrantMissing,
            &format!("flatten target {target} is outside the granted height range {low} to {high}"),
        ));
    }
    let (found, base) = located(world, catalog, plane, layer, cells)?;
    let (mut there, mut missing) = (0usize, 0usize);
    let mut changes = Vec::new();
    for &[x, y] in cells {
        let Some(before) = crate::heights::cell_height(found, layer, x, y) else {
            missing += 1;
            continue;
        };
        match flattened(before, target) {
            None => there += 1,
            Some(after) => {
                stacks(base, after, [x, y], layer)?;
                changes.push(([x, y], before, after));
            }
        }
    }
    if changes.is_empty() {
        return Err(refuse(
            RefusalCode::ChangeNone,
            &format!(
                "flatten changes no cell: {there} covered cells already at the target {target}, {missing} in map chunks that do not exist (painting terrain there creates them)"
            ),
        ));
    }
    Ok(changes)
}

/// The only smooth kernel version this editor knows: the rule of
/// [`smoothed`], frozen for `smooth_heights` steps that record it
/// (`decisions/0008-smooth-heights.md`). Part of the wire form.
pub const SMOOTH_KERNEL: u32 = 1;

/// The kernel rule: [`SMOOTH_KERNEL`] only.
///
/// # Errors
/// [`RefusalCode::InputInvalid`] for any other version.
pub fn check_kernel(kernel: u32) -> Result<(), SessionError> {
    if kernel != SMOOTH_KERNEL {
        return Err(refuse(
            RefusalCode::InputInvalid,
            &format!("unknown smooth kernel {kernel}; this editor knows kernel {SMOOTH_KERNEL}"),
        ));
    }
    Ok(())
}

/// The kernel-1 weights along one axis, for offsets -2 to 2: the binomial
/// 1, 4, 6, 4, 1. A cell `(dx, dy)` away weighs the product of its two
/// axes' weights, so the cell itself weighs 36 and all 25 weigh 256.
pub const SMOOTH_AXIS: [i64; 5] = [1, 4, 6, 4, 1];

/// The 24 cells around a cell that kernel 1 reads, up to two tiles away
/// either way, in row order (`dy`, then `dx`, each from -2 to 2), each
/// with its weight ([`SMOOTH_AXIS`]' product). The cell itself weighs
/// [`SMOOTH_CENTRE`].
pub const SMOOTH_NEIGHBOURS: [([i64; 2], i64); 24] = smooth_neighbours();
/// The kernel-1 weight of the cell itself.
pub const SMOOTH_CENTRE: i64 = SMOOTH_AXIS[2] * SMOOTH_AXIS[2];

const fn smooth_neighbours() -> [([i64; 2], i64); 24] {
    let mut found = [([0, 0], 0); 24];
    let (mut index, mut dy) = (0, 0);
    while dy < 5 {
        let mut dx = 0;
        while dx < 5 {
            if dx != 2 || dy != 2 {
                // Both are below 5, so these casts are exact.
                #[allow(clippy::cast_possible_wrap)]
                let offset = [dx as i64 - 2, dy as i64 - 2];
                found[index] = (offset, SMOOTH_AXIS[dx] * SMOOTH_AXIS[dy]);
                index += 1;
            }
            dx += 1;
        }
        dy += 1;
    }
    found
}

/// Kernel 1 for one cell of stored height `height` whose 24 surrounding
/// cells, in [`SMOOTH_NEIGHBOURS`]' order, hold `around` (`None`: not
/// read, because outside the active view or in a chunk that does not
/// exist), under `grant` (frozen, see `decisions/0008-smooth-heights.md`):
/// every height read, the cell's own included, is clamped into the range;
/// the new height is the cell's clamped height `h` plus the weighted sum of
/// the read cells' differences from `h`, divided by the total weight read
/// (the centre's included), rounded to the nearest integer with an exact
/// half toward zero, that is toward `h`. The result always lies between
/// the smallest and largest clamped height read, so in the range; the cell
/// changes when it differs from `height`.
#[must_use]
pub fn smoothed(height: i32, around: [Option<i32>; 24], grant: HeightGrant) -> i32 {
    let clamp = |value: i32| i64::from(value.clamp(grant.min, grant.max));
    let own = clamp(height);
    let (mut sum, mut weight) = (0_i64, SMOOTH_CENTRE);
    for (read, (_, neighbour)) in around.into_iter().zip(SMOOTH_NEIGHBOURS) {
        if let Some(read) = read {
            sum += neighbour * (clamp(read) - own);
            weight += neighbour;
        }
    }
    let magnitude = sum.unsigned_abs();
    let divisor = weight.unsigned_abs();
    let mut quotient = magnitude / divisor;
    if 2 * (magnitude - quotient * divisor) > divisor {
        quotient += 1;
    }
    // At most the largest difference read, so the sum lies in the range.
    let step = i64::try_from(quotient).unwrap_or(0);
    let new = if sum < 0 { own - step } else { own + step };
    i32::try_from(new).unwrap_or(height)
}

/// What a smooth plan leaves out besides its changes: covered cells
/// already even (kernel 1 gives their own height) and covered cells in
/// chunks that do not exist.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SmoothCounts {
    pub even: usize,
    pub missing: usize,
}

/// The cells of smooth `cells` with `kernel` that its `terrain.set` on the
/// `height` grid changes in `world` under `catalog`, each with its height
/// before and after ([`smoothed`]), in stroke order, and the counts of the
/// cells it leaves out: what dispatching an
/// [`Action::SmoothHeights`](crate::Action::SmoothHeights) prepares, and
/// what a proposal step's words and the desktop's live preview count. Every
/// height read is a committed one, never another cell's new height, so the
/// order of `cells` does not change any result. A cell up to two tiles away is read only
/// inside the active view and in a chunk that exists.
///
/// # Errors
/// In this order: the stroke rule (input codes); the kernel
/// ([`check_kernel`]); [`RefusalCode::GrantMissing`] without
/// `editing.heights`; [`RefusalCode::ScopeOutsideView`] for a cell outside
/// the active view; [`RefusalCode::TargetUnsupported`] when a new height
/// would stack past the 32-bit range on this layer;
/// [`RefusalCode::ChangeNone`] when no cell would change, saying how many
/// were already even and how many in chunks that do not exist.
pub fn smooth_plan(
    world: &World,
    catalog: &PreviewCatalog,
    plane: &str,
    layer: i32,
    cells: &[[i64; 2]],
    kernel: u32,
) -> Result<(Vec<HeightChange>, SmoothCounts), SessionError> {
    crate::session::stroke_cells(cells)?;
    check_kernel(kernel)?;
    let grant = granted(catalog)?;
    let (found, base) = located(world, catalog, plane, layer, cells)?;
    let read = |x: i64, y: i64, [dx, dy]: [i64; 2]| {
        let (x, y) = (x.checked_add(dx)?, y.checked_add(dy)?);
        if !catalog.tile_in_active_view(plane, layer, x, y) {
            return None;
        }
        crate::heights::cell_height(found, layer, x, y)
    };
    let mut counts = SmoothCounts::default();
    let mut changes = Vec::new();
    for &[x, y] in cells {
        let Some(before) = crate::heights::cell_height(found, layer, x, y) else {
            counts.missing += 1;
            continue;
        };
        let around = SMOOTH_NEIGHBOURS.map(|(offset, _)| read(x, y, offset));
        let after = smoothed(before, around, grant);
        if after == before {
            counts.even += 1;
            continue;
        }
        stacks(base, after, [x, y], layer)?;
        changes.push(([x, y], before, after));
    }
    if changes.is_empty() {
        return Err(refuse(
            RefusalCode::ChangeNone,
            &format!(
                "smooth changes no cell: {} covered cells already even, {} in map chunks that do not exist (painting terrain there creates them)",
                counts.even, counts.missing
            ),
        ));
    }
    Ok((changes, counts))
}

#[cfg(test)]
mod tests {
    use super::{
        Adjusted, MAXIMUM_AMOUNT, MAXIMUM_TARGET, SMOOTH_KERNEL, adjusted, check_amount,
        check_kernel, check_target, flattened, smoothed,
    };
    use crate::RefusalCode;
    use tilewright_editor_preview::HeightGrant;

    const RANGE: HeightGrant = HeightGrant { min: -8, max: 16 };

    #[test]
    fn adjusting_adds_the_amount_and_stops_at_the_range_ends() {
        use Adjusted::{AtEnd, To};
        assert_eq!(adjusted(0, 8, RANGE), To(8));
        assert_eq!(adjusted(0, -8, RANGE), To(-8));
        assert_eq!(adjusted(8, 8, RANGE), To(16));
        assert_eq!(adjusted(9, 8, RANGE), To(16));
        assert_eq!(adjusted(15, 1, RANGE), To(16));
        assert_eq!(adjusted(16, 1, RANGE), AtEnd);
        assert_eq!(adjusted(-7, -1, RANGE), To(-8));
        assert_eq!(adjusted(-8, -1, RANGE), AtEnd);
        assert_eq!(adjusted(16, -1, RANGE), To(15));
        assert_eq!(adjusted(-8, 1, RANGE), To(-7));
        assert_eq!(adjusted(3, 1024, RANGE), To(16));
        assert_eq!(adjusted(3, -1024, RANGE), To(-8));
    }

    #[test]
    fn cells_outside_the_range_are_left_out_moved_in_or_refused() {
        use Adjusted::{AtEnd, Short, To};
        // At or beyond the end the amount moves toward: left out.
        assert_eq!(adjusted(17, 1, RANGE), AtEnd);
        assert_eq!(adjusted(-9, -1, RANGE), AtEnd);
        // Beyond the other end: moved by the amount, kept in the range.
        assert_eq!(adjusted(20, -4, RANGE), To(16));
        assert_eq!(adjusted(20, -10, RANGE), To(10));
        assert_eq!(adjusted(20, -100, RANGE), To(-8));
        assert_eq!(adjusted(-12, 4, RANGE), To(-8));
        assert_eq!(adjusted(-12, 30, RANGE), To(16));
        // ... unless the amount does not reach the range.
        assert_eq!(adjusted(20, -3, RANGE), Short(4));
        assert_eq!(adjusted(-12, 3, RANGE), Short(4));
        assert_eq!(adjusted(-9, 0x7fff_ffff, RANGE), To(16));
    }

    #[test]
    fn adjusting_never_overflows_at_the_integer_extremes() {
        use Adjusted::{AtEnd, Short, To};
        let wide = HeightGrant {
            min: i32::MIN,
            max: i32::MAX,
        };
        assert_eq!(adjusted(i32::MAX, i32::MAX, wide), AtEnd);
        assert_eq!(adjusted(i32::MIN, i32::MIN, wide), AtEnd);
        assert_eq!(adjusted(i32::MAX, i32::MIN, wide), To(-1));
        assert_eq!(adjusted(i32::MIN, i32::MAX, wide), To(-1));
        assert_eq!(adjusted(i32::MAX, 1, RANGE), AtEnd);
        assert_eq!(adjusted(i32::MIN, -1, RANGE), AtEnd);
        assert_eq!(adjusted(i32::MAX, -1024, RANGE), Short(2_147_483_631));
        assert_eq!(adjusted(i32::MIN, 1024, RANGE), Short(2_147_483_640));
        assert_eq!(adjusted(i32::MIN, i32::MAX, RANGE), To(-1));
    }

    #[test]
    fn the_amount_is_not_zero_and_at_most_the_bound() {
        assert_eq!(MAXIMUM_AMOUNT, 1024);
        for amount in [1, -1, 1024, -1024, 8, -8] {
            assert!(check_amount(amount).is_ok(), "{amount}");
        }
        assert_eq!(
            check_amount(0).unwrap_err().code(),
            RefusalCode::InputInvalid
        );
        for amount in [1025, -1025, i32::MAX, i32::MIN] {
            assert_eq!(
                check_amount(amount).unwrap_err().code(),
                RefusalCode::InputBudget,
                "{amount}"
            );
        }
    }

    #[test]
    fn flattening_sets_the_target_and_leaves_cells_at_it_out() {
        assert_eq!(flattened(0, 8), Some(8));
        assert_eq!(flattened(8, 8), None);
        assert_eq!(flattened(-8, 8), Some(8));
        assert_eq!(flattened(9, 8), Some(8));
        // Far outside any range, at the integer extremes: set to the target.
        assert_eq!(flattened(i32::MAX, -1024), Some(-1024));
        assert_eq!(flattened(i32::MIN, 1024), Some(1024));
        assert_eq!(flattened(i32::MIN, i32::MIN), None);
    }

    #[test]
    fn the_target_is_at_most_the_hard_bound_either_way() {
        assert_eq!(MAXIMUM_TARGET, tilewright_editor_preview::MAXIMUM_HEIGHT);
        for target in [0, 1, -1, 1024, -1024] {
            assert!(check_target(target).is_ok(), "{target}");
        }
        for target in [1025, -1025, i32::MAX, i32::MIN] {
            assert_eq!(
                check_target(target).unwrap_err().code(),
                RefusalCode::InputBudget,
                "{target}"
            );
        }
    }

    /// The 24 cells around a cell, in [`super::SMOOTH_NEIGHBOURS`]' order,
    /// from `at(dx, dy)`.
    fn around(at: impl Fn(i64, i64) -> Option<i32>) -> [Option<i32>; 24] {
        super::SMOOTH_NEIGHBOURS.map(|([dx, dy], _)| at(dx, dy))
    }
    /// Every cell around at `value`.
    fn all(value: i32) -> [Option<i32>; 24] {
        around(|_, _| Some(value))
    }
    const WIDE: HeightGrant = HeightGrant {
        min: -100,
        max: 100,
    };

    #[test]
    fn the_weights_are_the_binomial_one_four_six_four_one_squared() {
        let weight = |dx: i64, dy: i64| {
            super::SMOOTH_NEIGHBOURS
                .iter()
                .find(|(offset, _)| *offset == [dx, dy])
                .map(|(_, weight)| *weight)
        };
        assert_eq!(super::SMOOTH_CENTRE, 36);
        assert_eq!(weight(0, 0), None);
        assert_eq!(weight(1, 0), Some(24));
        assert_eq!(weight(0, -1), Some(24));
        assert_eq!(weight(1, 1), Some(16));
        assert_eq!(weight(2, 0), Some(6));
        assert_eq!(weight(-2, 1), Some(4));
        assert_eq!(weight(2, -2), Some(1));
        let total: i64 = super::SMOOTH_NEIGHBOURS.iter().map(|(_, w)| w).sum();
        assert_eq!(total + super::SMOOTH_CENTRE, 256);
    }

    #[test]
    fn smoothing_takes_the_weighted_mean_of_the_five_by_five_around() {
        // Even ground stays.
        assert_eq!(smoothed(5, all(5), WIDE), 5);
        // A spike of 16 among zeros: 36 × 16 / 256 = 2.25, so 2; a pit
        // of 0 among 16s: 220 × 16 / 256 = 13.75, so 14; and below 0.
        assert_eq!(smoothed(16, all(0), WIDE), 2);
        assert_eq!(smoothed(0, all(16), WIDE), 14);
        assert_eq!(smoothed(-16, all(0), WIDE), -2);
        assert_eq!(smoothed(0, all(-16), WIDE), -14);
        // Two tiles away counts: only (2, 0) and (-2, 0) at 8, weight 6
        // each: 96 / 256 = 0.375, so 0; at 16, 0.75, so 1.
        let far = |value| around(|dx, dy| Some(if dy == 0 && dx.abs() == 2 { value } else { 0 }));
        assert_eq!(smoothed(0, far(8), WIDE), 0);
        assert_eq!(smoothed(0, far(16), WIDE), 1);
        // One edge neighbour alone, weight 24 (total 60): 10 → 4, 5 → 2.
        let one = |value| around(|dx, dy| (dx == 1 && dy == 0).then_some(value));
        assert_eq!(smoothed(0, one(10), WIDE), 4);
        assert_eq!(smoothed(0, one(5), WIDE), 2);
        assert_eq!(smoothed(0, one(-10), WIDE), -4);
    }

    #[test]
    fn an_exact_half_rounds_toward_the_cells_own_height() {
        // Only (2, 0) and (-2, 0) read, weight 6 each (total 48):
        // differences 1 and 3 make 24 / 48 = 0.5; 5 and 7, 1.5.
        let pair = |a, b| {
            around(|dx, dy| match (dx, dy) {
                (2, 0) => Some(a),
                (-2, 0) => Some(b),
                _ => None,
            })
        };
        assert_eq!(smoothed(0, pair(1, 3), WIDE), 0);
        assert_eq!(smoothed(0, pair(-1, -3), WIDE), 0);
        assert_eq!(smoothed(0, pair(5, 7), WIDE), 1);
        assert_eq!(smoothed(0, pair(-5, -7), WIDE), -1);
        // The same around 5 and -5: 5.5 stays 5, -5.5 stays -5 (floor
        // would give -6, half away from zero 6 and -6).
        assert_eq!(smoothed(5, pair(6, 8), WIDE), 5);
        assert_eq!(smoothed(-5, pair(-6, -8), WIDE), -5);
        assert_eq!(smoothed(6, pair(5, 3), WIDE), 6);
        assert_eq!(smoothed(-6, pair(-5, -3), WIDE), -6);
        // All 24 read: 1 on the four edge neighbours (4 × 24) and on two
        // diagonal ones (2 × 16) makes 128 / 256 = 0.5.
        let half = around(|dx, dy| {
            Some(i32::from(
                dx.abs() + dy.abs() == 1 || (dx == dy && dx.abs() == 1),
            ))
        });
        assert_eq!(smoothed(0, half, WIDE), 0);
        let mirrored = half.map(|value| value.map(|value| 1 - value));
        assert_eq!(smoothed(1, mirrored, WIDE), 1);
    }

    #[test]
    fn cells_not_read_are_left_out_of_the_weight() {
        // Nothing read: the cell keeps its height.
        assert_eq!(smoothed(7, [None; 24], WIDE), 7);
        // One edge neighbour at 12, weight 24 of 60: 4.8, so 5.
        let edge = around(|dx, dy| (dx == 0 && dy == 1).then_some(12));
        assert_eq!(smoothed(0, edge, WIDE), 5);
        // Counted as 0 instead, it would be 288 / 256, rounded to 1.
        assert_ne!(smoothed(0, edge, WIDE), 1);
    }

    #[test]
    fn every_height_read_is_clamped_into_the_range() {
        let range = HeightGrant { min: -8, max: 8 };
        // One edge neighbour far above: it counts as 8, so 192 / 256 = 0.75.
        let spiked = |value| around(|dx, dy| Some(if (dx, dy) == (1, 0) { value } else { 0 }));
        assert_eq!(smoothed(0, spiked(1000), range), 1);
        assert_eq!(smoothed(0, spiked(-1000), range), -1);
        // A cell outside the range is read as its end, so it moves in.
        assert_eq!(smoothed(30, [None; 24], range), 8);
        assert_eq!(smoothed(-30, all(-8), range), -8);
        // 36 × 8 / 256 = 1.125.
        assert_eq!(smoothed(30, all(0), range), 1);
        // The range's ends are results.
        assert_eq!(smoothed(8, all(8), range), 8);
        assert_eq!(smoothed(-8, all(-8), range), -8);
    }

    #[test]
    fn smoothing_never_overflows_at_the_integer_extremes() {
        let wide = HeightGrant {
            min: i32::MIN,
            max: i32::MAX,
        };
        // (36 × MAX + 220 × MIN) / 256 = -1,543,503,872.14.
        assert_eq!(smoothed(i32::MAX, all(i32::MIN), wide), -1_543_503_872);
        assert_eq!(smoothed(i32::MIN, all(i32::MAX), wide), 1_543_503_871);
        assert_eq!(smoothed(i32::MAX, [None; 24], wide), i32::MAX);
        assert_eq!(
            smoothed(i32::MIN, [None; 24], HeightGrant { min: -8, max: 8 }),
            -8
        );
    }

    #[test]
    fn only_kernel_one_exists() {
        assert_eq!(SMOOTH_KERNEL, 1);
        assert!(check_kernel(1).is_ok());
        for kernel in [0, 2, u32::MAX] {
            assert_eq!(
                check_kernel(kernel).unwrap_err().code(),
                RefusalCode::InputInvalid,
                "{kernel}"
            );
        }
    }
}
