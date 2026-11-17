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
/// view (its plane, layer and bounds), and [`RefusalCode::TargetUnknown`]
/// when `grid` is not one of the profile's ground grids.
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
    let mut cells = BTreeSet::new();
    if contiguous {
        cells.insert(at);
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
                    cells.insert(next);
                    pending.push(next);
                }
            }
        }
    } else {
        let (_, _, [min_x, min_y, width, height]) = catalog.view();
        for dx in 0..width {
            for dy in 0..height {
                if let (Some(x), Some(y)) = (min_x.checked_add(dx), min_y.checked_add(dy))
                    && matches([x, y])?
                {
                    cells.insert([x, y]);
                }
            }
        }
    }
    Ok(Region {
        value,
        cells: cells.into_iter().collect(),
    })
}
