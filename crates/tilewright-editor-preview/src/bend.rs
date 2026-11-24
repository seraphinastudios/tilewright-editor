// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! How large a difference between neighbouring heights the preview can bend
//! a catalog's isometric ground art over, decided from the catalog alone.
//!
//! The shared renderer draws a bent isometric tile as a 4 × 4 mesh mapped
//! over its four corner heights, and refuses the whole view when one of
//! those mesh quads spans more than its scene's largest dimension less one
//! pixel ([`tilewright_render_world::compile`]). A quad's span is measured
//! from the tile's own geometry: its art's size, the diamond, the zoom, the
//! preview's stretching ([`preview_stretch`]) and its corner heights, which
//! are the means of the heights of the tile's 3 × 3 neighbourhood. It does
//! not depend on where the tile lies, on the window or on what else is in
//! view, and it grows with the zoom, so the editor's largest zoom
//! ([`LARGEST_ZOOM`]) is the worst case of every view.
//!
//! Within one neighbourhood each quad corner's position is linear in the
//! nine heights, so a quad's span is the largest of a few linear functions
//! and is convex in them. Over heights that lie at most `d` apart its
//! largest value is therefore taken where each of the nine heights is one
//! of the two ends: 512 patterns, which depend only on `d` because the
//! span depends only on differences. Convex and drawable at `d = 0` (flat
//! art the renderer draws as one quad, the same at every `d`), the
//! drawable differences form a range from 0 up, so a binary search over
//! `d` finds its end. Every verdict here is the renderer's own `compile` of
//! one tile at that zoom, so the arithmetic is the preview's, not a copy.
//!
//! What this does not decide: the scene's quad and raster-sample budgets,
//! which count the whole view and depend on the window and on how many
//! tiles are bent, and art the renderer cannot draw at the largest zoom
//! even flat (an image more than about 1,023 pixels on a side), which no
//! height range changes.

use crate::{PreviewCatalog, PreviewLimits, ReadyResources};
use std::collections::BTreeMap;
use tilewright_render_world::{
    Anchor, Camera, HeightField, Projection, Stretch, TextureRef, WorldObject, WorldView,
};
use tilewright_scene::{Digest256, ResourceId, Sampling, SemanticId};
use tilewright_source::World;

/// The largest zoom of any editor view: the desktop's zoom stops at 8, and
/// so does `render --scale`; the MCP server's render stops at 4.
pub const LARGEST_ZOOM: f32 = 8.0;

/// The stretching every preview uses: mean corners, no cliffs, one pixel
/// per height unit at zoom 1.
pub(crate) fn preview_stretch() -> Stretch {
    Stretch::default()
}

/// The largest difference between neighbouring heights that some of a
/// catalog's ground art can be bent over at [`LARGEST_ZOOM`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BendLimit {
    /// Heights that lie at most this far apart can be bent at every zoom;
    /// a 3 × 3 pattern of heights one more apart cannot be at the largest.
    pub largest_difference: u32,
    /// The limiting art's size in pixels.
    pub width: u32,
    pub height: u32,
    /// The projection's diamond, `tile_width` by `tile_height`.
    pub diamond: [u32; 2],
    /// The catalog's resources of that size drawn as ground, in id order.
    /// Empty when the limit is the diamond-sized art the preview draws its
    /// tile picking with, which no resource of the catalog has the size of.
    pub resources: Vec<String>,
}

/// The tightest [`BendLimit`] of the ground art `catalog` can draw, when
/// some of it cannot be bent over heights `difference` apart at
/// [`LARGEST_ZOOM`]; `None` when all of it can, and always for top-down
/// projection, whose ground shifts with its height but never bends.
///
/// The art is every resource a prototype of the catalog draws with the
/// `ground` anchor, used in the World or not, and the diamond-sized
/// reference art of the preview's tile picking. Art the renderer cannot
/// draw at the largest zoom even flat is left out: no height changes it.
pub fn bend_limit(
    world: &World,
    catalog: &PreviewCatalog,
    resources: &ReadyResources,
    difference: u32,
    limits: PreviewLimits,
) -> Option<BendLimit> {
    let projection = crate::catalog::projection(world);
    let Projection::Isometric {
        tile_width,
        tile_height,
    } = projection
    else {
        return None;
    };
    let mut sizes: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
    sizes.entry((tile_width, tile_height)).or_default();
    for (resource, anchor) in catalog.prototypes.values() {
        if *anchor != Anchor::Ground {
            continue;
        }
        if let Some(texture) = resources.textures.get(resource) {
            let names = sizes.entry((texture.width, texture.height)).or_default();
            if !names.contains(resource) {
                names.push(resource.clone());
            }
        }
    }
    let mut tightest: Option<BendLimit> = None;
    for ((width, height), mut names) in sizes {
        let Some(largest) = largest_difference(projection, width, height, difference, limits)
        else {
            continue;
        };
        if tightest
            .as_ref()
            .is_none_or(|limit| largest < limit.largest_difference)
        {
            names.sort();
            tightest = Some(BendLimit {
                largest_difference: largest,
                width,
                height,
                diamond: [tile_width, tile_height],
                resources: names,
            });
        }
    }
    tightest
}

/// The largest difference below `up_to` that art of this size can be bent
/// over at [`LARGEST_ZOOM`], or `None` when it can be bent over `up_to`
/// itself, or cannot be drawn there even flat.
fn largest_difference(
    projection: Projection,
    width: u32,
    height: u32,
    up_to: u32,
    limits: PreviewLimits,
) -> Option<u32> {
    let drawn = |difference: u32| draws(projection, width, height, difference, limits);
    if !drawn(0) || drawn(up_to) {
        return None;
    }
    // `drawn(low)` holds and `drawn(high)` does not.
    let (mut low, mut high) = (0, up_to);
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if drawn(middle) {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(low)
}

/// Whether the renderer draws one ground tile of this art at
/// [`LARGEST_ZOOM`] under every pattern of its 3 × 3 neighbourhood whose
/// heights are 0 or `difference`.
fn draws(
    projection: Projection,
    width: u32,
    height: u32,
    difference: u32,
    limits: PreviewLimits,
) -> bool {
    let Ok(difference) = i32::try_from(difference) else {
        return false;
    };
    let texture = TextureRef {
        id: ResourceId(1),
        revision: 1,
        digest: Digest256([1; 32]),
        width,
        height,
    };
    (0..512_u16).all(|pattern| {
        let heights = (0..9)
            .map(|bit| {
                Some(if pattern >> bit & 1 == 1 {
                    difference
                } else {
                    0
                })
            })
            .collect();
        let view = WorldView {
            revision: 1,
            clock_millis: 0,
            camera: Camera {
                projection,
                center: [0.5, 0.5],
                zoom: LARGEST_ZOOM,
                width: 64,
                height: 64,
                sampling: Sampling::Nearest,
            },
            clear_color: [0.0, 0.0, 0.0, 1.0],
            heights: HeightField {
                origin: [-1, -1],
                width: 3,
                height: 3,
                heights,
            },
            stretch: preview_stretch(),
            ground_layer_ranks: 1,
            objects: vec![WorldObject::new(
                SemanticId(1),
                [0, 0],
                0,
                Anchor::Ground,
                texture,
            )],
        };
        tilewright_render_world::compile(&view, limits.scene).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isometric(tile_width: u32, tile_height: u32) -> Projection {
        Projection::Isometric {
            tile_width,
            tile_height,
        }
    }

    /// The largest drawable difference, searched up to the editor's widest
    /// range with 0 (2 × 1,024), or 2,048 when that draws.
    fn limit(diamond: [u32; 2], art: [u32; 2]) -> u32 {
        largest_difference(
            isometric(diamond[0], diamond[1]),
            art[0],
            art[1],
            2048,
            PreviewLimits::default(),
        )
        .unwrap_or(2048)
    }

    #[test]
    fn the_limits_match_the_measured_table_of_adr_0006() {
        // ADR 0006 measured the largest ±H of the worst pattern at zoom 8:
        // a difference of 2H draws, and of 2H + 2 does not.
        for (diamond, art, measured) in [
            ([32, 16], [32, 32], 460),
            ([64, 32], [64, 64], 487),
            ([128, 64], [128, 128], 495),
            ([32, 16], [32, 48], 215),
            ([64, 32], [64, 128], 129),
        ] {
            let largest = limit(diamond, art);
            assert!(
                (2 * measured..2 * measured + 2).contains(&largest),
                "{diamond:?} {art:?}: {largest}"
            );
        }
        // Diamond-only art draws beyond the editor's widest range.
        assert_eq!(limit([32, 16], [32, 16]), 2048);
        assert_eq!(limit([64, 32], [64, 32]), 2048);
        assert_eq!(limit([32, 16], [32, 32]), 921);
    }

    #[test]
    fn the_largest_difference_draws_and_one_more_does_not() {
        let projection = isometric(32, 16);
        let limits = PreviewLimits::default();
        assert!(draws(projection, 32, 32, 921, limits));
        assert!(!draws(projection, 32, 32, 922, limits));
        // Every smaller difference draws too.
        assert!(
            (0..921)
                .step_by(37)
                .all(|d| draws(projection, 32, 32, d, limits))
        );
        // Top-down ground never bends.
        assert!(draws(
            Projection::TopDown { tile_size: 32 },
            32,
            128,
            2048,
            limits
        ));
    }

    #[test]
    fn art_too_large_to_draw_flat_at_the_largest_zoom_is_left_out() {
        assert_eq!(
            largest_difference(isometric(32, 16), 32, 1100, 2048, PreviewLimits::default()),
            None
        );
    }
}
