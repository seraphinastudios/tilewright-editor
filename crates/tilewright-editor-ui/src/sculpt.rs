// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sculpting. Raise and lower: the heights an
//! [`Action::AdjustHeights`](crate::Action::AdjustHeights) writes into the
//! cells it covers.
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
use tilewright_source::World;

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
    let grant = catalog.editing().heights.ok_or_else(|| {
        refuse(
            RefusalCode::GrantMissing,
            "height editing refused: the catalog does not grant editing.heights",
        )
    })?;
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
    // The layer's stacked base, as the preview adds it (i64: exact).
    let base = i64::from(layer) * i64::from(world.profile().layer_height.unwrap_or(0));
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
                        "cell ({x}, {y}) has height {before}, {side} the granted range {low} to {high}: it is {needed} from the range, more than one adjustment can move (1024), so this tool cannot bring it in; leave it out of the stroke"
                    )
                } else {
                    format!(
                        "cell ({x}, {y}) has height {before}, {side} the granted range {low} to {high}: {verb} it into the range needs an amount of at least {needed}"
                    )
                };
                return Err(refuse(RefusalCode::TargetUnsupported, &message));
            }
            Adjusted::To(after) => {
                if i32::try_from(base + i64::from(after)).is_err() {
                    return Err(refuse(
                        RefusalCode::TargetUnsupported,
                        &format!(
                            "cell ({x}, {y}): height {after} on layer {layer} would stack past the 32-bit range the preview draws"
                        ),
                    ));
                }
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

#[cfg(test)]
mod tests {
    use super::{Adjusted, MAXIMUM_AMOUNT, adjusted, check_amount};
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
}
