// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Fill and replace: which cells of the active view an
//! [`Action::FillTerrain`](crate::Action::FillTerrain) covers. The region is
//! computed from a World alone, so the engine's dispatch, the desktop's
//! hover preview and a proposal step's map highlight all read the same
//! cells from the same committed World.
use std::collections::BTreeSet;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::World;

use crate::brush::MAXIMUM_STROKE_CELLS;
use crate::session::refusal::refuse;
use crate::{RefusalCode, SessionError};

/// What a fill covers: the value of its start cell and every cell of the
/// active view it reaches that holds that value, start cell included.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Region<'w> {
    /// The start cell's value on the fill's grid; `None` for a cell with
    /// no terrain there (an empty cell, or one of a chunk that does not
    /// exist).
    pub value: Option<&'w str>,
    /// The covered cells, distinct and in increasing order of x, then y:
    /// the order of a brush stroke's cells.
    pub cells: Vec<[i64; 2]>,
}

/// The value of plane cell `[x, y]` on ground grid `index` (the grid's
/// position in the profile's `ground_layers`) of `plane`/`layer`. The terrain
/// planner reads every committed value through this, so the cells a fill
/// matches and the values their `terrain.set` expects cannot differ.
pub(crate) fn ground_value<'w>(
    world: &'w World,
    plane: &str,
    layer: i32,
    index: usize,
    [x, y]: [i64; 2],
) -> Result<Option<&'w str>, SessionError> {
    let local = usize::try_from(y.rem_euclid(32) * 32 + x.rem_euclid(32))
        .map_err(|_| refuse(RefusalCode::Internal, "cell overflow"))?;
    Ok(world
        .planes()
        .get(plane)
        .and_then(|p| p.chunks.get(&(layer, x.div_euclid(32), y.div_euclid(32))))
        .and_then(|c| c.ground.get(index))
        .and_then(|g| g.value(local)))
}

/// The cells a fill from `at` on ground grid `grid` covers in the active
/// view of `catalog`, read from `world`. With `contiguous`, the cells
/// reachable from `at` through edge neighbours (up, down, left, right;
/// never diagonals) that hold `at`'s value, without leaving the view;
/// without it, every cell of the view that holds that value. An empty
/// start cell covers empty cells the same way.
///
/// The result depends only on the World's values and the view, never on
/// the order cells are visited in.
///
/// # Errors
/// [`RefusalCode::ScopeOutsideView`] when `at` is not a tile of the active
/// view (its plane, layer and bounds), [`RefusalCode::TargetUnknown`] when
/// `grid` is not one of the profile's ground grids, and
/// [`RefusalCode::InputBudget`] when the view holds more cells than a brush
/// stroke may (a catalog loaded with larger limits than the editor's).
pub fn region<'w>(
    world: &'w World,
    catalog: &PreviewCatalog,
    plane: &str,
    layer: i32,
    grid: &str,
    at: [i64; 2],
    contiguous: bool,
) -> Result<Region<'w>, SessionError> {
    if !catalog.tile_in_active_view(plane, layer, at[0], at[1]) {
        return Err(refuse(
            RefusalCode::ScopeOutsideView,
            "a fill starts on a tile of the active view",
        ));
    }
    let (_, _, [_, _, width, height]) = catalog.view();
    if usize::try_from(width.saturating_mul(height))
        .map_or(true, |cells| cells > MAXIMUM_STROKE_CELLS)
    {
        return Err(refuse(
            RefusalCode::InputBudget,
            "a fill needs a view of at most the brush stroke's cell count",
        ));
    }
    let index = world
        .profile()
        .ground_layers
        .iter()
        .position(|g| g == grid)
        .ok_or_else(|| refuse(RefusalCode::TargetUnknown, "unknown ground grid"))?;
    let value = ground_value(world, plane, layer, index, at)?;
    let matches = |cell: [i64; 2]| -> Result<bool, SessionError> {
        Ok(catalog.tile_in_active_view(plane, layer, cell[0], cell[1])
            && ground_value(world, plane, layer, index, cell)? == value)
    };
    let cells = if contiguous {
        flood(at, MAXIMUM_STROKE_CELLS, matches)?
    } else {
        let (_, _, [min_x, min_y, width, height]) = catalog.view();
        let mut cells = BTreeSet::new();
        for dx in 0..width {
            for dy in 0..height {
                if let (Some(x), Some(y)) = (min_x.checked_add(dx), min_y.checked_add(dy))
                    && matches([x, y])?
                {
                    cells.insert([x, y]);
                }
            }
        }
        cells
    };
    Ok(Region {
        value,
        cells: cells.into_iter().collect(),
    })
}

/// The cells reachable from `at` through edge neighbours for which `matches`
/// holds, `at` included. It stops, refused with
/// [`RefusalCode::InputBudget`], as soon as it holds more than `cap` cells,
/// whatever `matches` admits.
fn flood(
    at: [i64; 2],
    cap: usize,
    mut matches: impl FnMut([i64; 2]) -> Result<bool, SessionError>,
) -> Result<BTreeSet<[i64; 2]>, SessionError> {
    let mut cells = BTreeSet::from([at]);
    let mut pending = vec![at];
    while let Some([x, y]) = pending.pop() {
        for next in [
            x.checked_sub(1).map(|x| [x, y]),
            x.checked_add(1).map(|x| [x, y]),
            y.checked_sub(1).map(|y| [x, y]),
            y.checked_add(1).map(|y| [x, y]),
        ]
        .into_iter()
        .flatten()
        {
            if !cells.contains(&next) && matches(next)? {
                if cells.len() >= cap {
                    return Err(refuse(
                        RefusalCode::InputBudget,
                        "a fill covers at most the brush stroke's cell count",
                    ));
                }
                cells.insert(next);
                pending.push(next);
            }
        }
    }
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use super::flood;
    use crate::RefusalCode;

    #[test]
    fn a_flood_stops_at_its_cap_whatever_matches() {
        // Everything matches: an unbounded plane.
        let error = flood([0, 0], 4096, |_| Ok(true)).unwrap_err();
        assert_eq!(error.code(), RefusalCode::InputBudget);
        // Exactly the cap fits: a 64 x 64 square.
        let square = |[x, y]: [i64; 2]| Ok((0..64).contains(&x) && (0..64).contains(&y));
        assert_eq!(flood([5, 5], 4096, square).unwrap().len(), 4096);
        assert_eq!(
            flood([5, 5], 4095, square).unwrap_err().code(),
            RefusalCode::InputBudget
        );
        // Diagonals are not neighbours.
        let diagonal = |[x, y]: [i64; 2]| Ok(x == y);
        assert_eq!(flood([0, 0], 4096, diagonal).unwrap().len(), 1);
    }
}
