// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Sculpting. Raise and lower: the heights an
//! [`Action::AdjustHeights`](crate::Action::AdjustHeights) writes into the
//! cells it covers.
//!
//! The rule is part of what a recorded `adjust_heights` step means, so it
//! is fixed here for that action kind (`docs/ENGINE.md`, "Raise and
//! lower"): each covered cell of an existing chunk becomes
//! `clamp(height + amount, min, max)` with the catalog's `editing.heights`
//! range, computed in 64-bit integers. The amount is flat and applies once
//! per stroke: a stroke's cells are a set, so a cell is changed once
//! however often the brush passed over it.
use tilewright_editor_preview::{HeightGrant, PreviewCatalog};
use tilewright_source::World;

use crate::session::refusal::refuse;
use crate::{RefusalCode, SessionError};

/// The largest amount one action raises or lowers by: twice
/// [`tilewright_editor_preview::MAXIMUM_HEIGHT`], the most any granted
/// range spans, so a larger amount could change nothing more.
pub const MAXIMUM_AMOUNT: i32 = 2 * tilewright_editor_preview::MAXIMUM_HEIGHT;

/// One cell an adjustment changes, its height before and its new height.
pub type HeightChange = ([i64; 2], i32, i32);

/// `height` raised by `amount` (lowered when negative) and kept within
/// `grant`. Checked arithmetic: any `i32` height and amount give a value
/// in the range, never an overflow.
#[must_use]
pub fn adjusted(height: i32, amount: i32, grant: HeightGrant) -> i32 {
    let raised = i64::from(height) + i64::from(amount);
    let kept = raised.clamp(i64::from(grant.min), i64::from(grant.max));
    // `kept` lies between two i32 values, so this never fails.
    i32::try_from(kept).unwrap_or(if raised < 0 { grant.min } else { grant.max })
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
/// are left out (a height is ground's, and there is none), as are cells
/// already at the end of the range the amount moves toward.
///
/// # Errors
/// In this order: the stroke rule (input codes); the amount
/// ([`check_amount`]); [`RefusalCode::GrantMissing`] without
/// `editing.heights`; [`RefusalCode::ScopeOutsideView`] for a cell outside
/// the active view; [`RefusalCode::TargetUnsupported`] when a covered cell
/// of an existing chunk holds a height outside the granted range (moving
/// it into the range could jump it far, so nothing is changed);
/// [`RefusalCode::ChangeNone`] when no cell would change.
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
    let mut changes = Vec::new();
    for &[x, y] in cells {
        // `None`: the chunk does not exist, so the cell is left out.
        let Some(before) = crate::heights::cell_height(found, layer, x, y) else {
            continue;
        };
        if !grant.contains(before) {
            return Err(refuse(
                RefusalCode::TargetUnsupported,
                &format!(
                    "cell ({x}, {y}) has height {before}, outside the granted range {} to {}",
                    grant.min, grant.max
                ),
            ));
        }
        let after = adjusted(before, amount, grant);
        if after != before {
            changes.push(([x, y], before, after));
        }
    }
    if changes.is_empty() {
        return Err(refuse(
            RefusalCode::ChangeNone,
            "height adjustment changes no cell: every covered cell is at the end of the range or in a chunk that does not exist",
        ));
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::{MAXIMUM_AMOUNT, adjusted, check_amount};
    use crate::RefusalCode;
    use tilewright_editor_preview::HeightGrant;

    const RANGE: HeightGrant = HeightGrant { min: -8, max: 16 };

    #[test]
    fn adjusting_adds_the_amount_and_stops_at_the_range_ends() {
        assert_eq!(adjusted(0, 8, RANGE), 8);
        assert_eq!(adjusted(0, -8, RANGE), -8);
        assert_eq!(adjusted(8, 8, RANGE), 16);
        assert_eq!(adjusted(9, 8, RANGE), 16);
        assert_eq!(adjusted(15, 1, RANGE), 16);
        assert_eq!(adjusted(16, 1, RANGE), 16);
        assert_eq!(adjusted(-7, -1, RANGE), -8);
        assert_eq!(adjusted(-8, -1, RANGE), -8);
        assert_eq!(adjusted(16, -1, RANGE), 15);
        assert_eq!(adjusted(-8, 1, RANGE), -7);
        assert_eq!(adjusted(3, 1024, RANGE), 16);
        assert_eq!(adjusted(3, -1024, RANGE), -8);
    }

    #[test]
    fn adjusting_never_overflows_at_the_integer_extremes() {
        let wide = HeightGrant {
            min: i32::MIN,
            max: i32::MAX,
        };
        assert_eq!(adjusted(i32::MAX, i32::MAX, wide), i32::MAX);
        assert_eq!(adjusted(i32::MIN, i32::MIN, wide), i32::MIN);
        assert_eq!(adjusted(i32::MAX, i32::MIN, wide), -1);
        assert_eq!(adjusted(i32::MAX, 1, RANGE), 16);
        assert_eq!(adjusted(i32::MIN, -1, RANGE), -8);
        let point = HeightGrant { min: 5, max: 5 };
        assert_eq!(adjusted(5, 1, point), 5);
        assert_eq!(adjusted(5, -1, point), 5);
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
