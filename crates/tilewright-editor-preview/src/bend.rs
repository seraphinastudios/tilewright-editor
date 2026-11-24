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
//! view, and it grows linearly with the zoom, so a verdict at one zoom
//! holds at every lower zoom.
//!
//! Art is judged at the editor's largest zoom ([`LARGEST_ZOOM`]), or, when
//! it is so large that the renderer cannot draw it there even flat (a side
//! of more than about 1,023 pixels), at the largest zoom at which it draws
//! flat. Every zoom below is covered. Above it a flat tile of the art does
//! not draw; a bent tile is 16 smaller quads, so a view in which every
//! tile of the art is bent can draw at higher zooms, and such views are
//! not covered. Zooms are treated as
//! continuous, so this does not depend on which zooms a front end steps
//! through.
//!
//! Within one neighbourhood each quad corner's position is linear in the
//! nine heights, so a quad's span is the largest of a few linear functions
//! and is convex in them. Over heights that lie at most `d` apart its
//! largest value is therefore taken where each of the nine heights is one
//! of the two ends: 512 patterns, which depend only on `d` because the
//! span depends only on differences. Convex and drawable at `d = 0` (flat
//! art the renderer draws as one quad, the same at every `d`), the
//! drawable differences form a range from 0 up, so a binary search over
//! `d` finds its end. A pattern whose four corners come out equal (only
//! the middle tile differs, say) is drawn flat, as at `d = 0`; the mesh it
//! would otherwise be spans less than that flat quad on any diamond of at
//! least 3 x 2 pixels, so the bound holds for it too. Every verdict here is
//! the renderer's own `compile` of one tile, so the arithmetic is the
//! preview's, not a copy.
//!
//! What this does not decide: the scene's quad and raster-sample budgets,
//! which count the whole view and depend on the window and on how many
//! tiles are bent.

use crate::{PreviewCatalog, PreviewLimits, ReadyResources};
use std::collections::BTreeMap;
use tilewright_render_world::{
    Anchor, Camera, HeightField, Projection, Stretch, TextureRef, WorldObject, WorldView,
};
use tilewright_scene::{Digest256, ResourceId, Sampling, SemanticId};
use tilewright_source::World;

/// The largest zoom of any editor view: the desktop's zoom stops at 8, and
/// so does `render --scale` (`MAXIMUM_RENDER_SCALE`); the MCP server's
/// render stops at 4. Tests in those crates hold them to this.
pub const LARGEST_ZOOM: f32 = 8.0;

/// The stretching every preview uses: mean corners, no cliffs, one pixel
/// per height unit at zoom 1.
pub(crate) fn preview_stretch() -> Stretch {
    Stretch::default()
}

/// The largest difference between neighbouring heights that some of a
/// catalog's ground art can be bent over.
#[derive(Clone, Debug, PartialEq)]
pub struct BendLimit {
    /// Heights that lie at most this far apart can be bent, tile by tile,
    /// at every zoom up to `zoom`; a 3 × 3 pattern of heights one more
    /// apart cannot be at `zoom`.
    pub largest_difference: u32,
    /// The zoom the art was judged at: [`LARGEST_ZOOM`], or, for art the
    /// renderer cannot draw there even flat, the largest zoom at which it
    /// draws flat. Views in which every tile of that art is bent can draw
    /// above it and are not covered.
    pub zoom: f32,
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
/// some of it cannot be bent over heights `difference` apart; `None` when
/// all of it can, and always for top-down projection, whose ground shifts
/// with its height but never bends.
///
/// The art is every resource a prototype of the catalog draws with the
/// `ground` anchor, used in the World or not, and the diamond-sized
/// reference art of the preview's tile picking. Only the tightest size is
/// reported; other art may also fall short of `difference`.
pub fn bend_limit(
    world: &World,
    catalog: &PreviewCatalog,
    resources: &ReadyResources,
    difference: u32,
    limits: PreviewLimits,
) -> Option<BendLimit> {
    let mut art: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
    for (resource, anchor) in catalog.prototypes.values() {
        if *anchor != Anchor::Ground {
            continue;
        }
        if let Some(texture) = resources.textures.get(resource) {
            let names = art.entry((texture.width, texture.height)).or_default();
            if !names.contains(resource) {
                names.push(resource.clone());
            }
        }
    }
    tightest(crate::catalog::projection(world), art, difference, limits)
}

/// [`bend_limit`] of ground art sized as `art`'s keys, with the resources
/// named by its values, and of the tile-picking art.
fn tightest(
    projection: Projection,
    mut art: BTreeMap<(u32, u32), Vec<String>>,
    difference: u32,
    limits: PreviewLimits,
) -> Option<BendLimit> {
    let Projection::Isometric {
        tile_width,
        tile_height,
    } = projection
    else {
        return None;
    };
    art.entry((tile_width, tile_height)).or_default();
    let mut tightest: Option<BendLimit> = None;
    for ((width, height), mut names) in art {
        let Some((largest, zoom)) =
            largest_difference(projection, width, height, difference, limits)
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
                zoom,
                width,
                height,
                diamond: [tile_width, tile_height],
                resources: names,
            });
        }
    }
    tightest
}

/// The largest zoom up to [`LARGEST_ZOOM`] at which the renderer draws this
/// art flat, or `None` when it draws it at none of the editor's zooms.
fn flat_zoom(
    projection: Projection,
    width: u32,
    height: u32,
    limits: PreviewLimits,
) -> Option<f32> {
    // A flat tile is one quad of the art's size times the zoom; start just
    // under that bound and step down to the first zoom the renderer takes.
    #[allow(clippy::cast_precision_loss)]
    let bound = (limits.scene.maximum_dimension as f32 - 1.0) / width.max(height) as f32;
    let mut zoom = LARGEST_ZOOM.min(bound);
    for _ in 0..64 {
        if zoom <= 0.0 {
            return None;
        }
        if draws(projection, width, height, zoom, 0, limits) {
            return Some(zoom);
        }
        zoom = zoom.next_down();
    }
    None
}

/// The largest difference below `up_to` that art of this size can be bent
/// over, and the zoom it was judged at ([`flat_zoom`]); `None` when it can
/// be bent over `up_to` itself, or is drawn at no zoom.
fn largest_difference(
    projection: Projection,
    width: u32,
    height: u32,
    up_to: u32,
    limits: PreviewLimits,
) -> Option<(u32, f32)> {
    let zoom = flat_zoom(projection, width, height, limits)?;
    let drawn = |difference: u32| draws(projection, width, height, zoom, difference, limits);
    if drawn(up_to) {
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
    Some((low, zoom))
}

/// Every pattern of a 3 × 3 neighbourhood whose heights are 0 or
/// `difference`, row-major.
fn patterns(difference: i32) -> impl Iterator<Item = Vec<Option<i32>>> {
    (0..512_u16).map(move |pattern| {
        (0..9)
            .map(|bit| {
                Some(if pattern >> bit & 1 == 1 {
                    difference
                } else {
                    0
                })
            })
            .collect()
    })
}

/// Whether the renderer draws one ground tile of this art at `zoom` under
/// every [`patterns`] of `difference`.
fn draws(
    projection: Projection,
    width: u32,
    height: u32,
    zoom: f32,
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
    patterns(difference).all(|heights| {
        let view = WorldView {
            revision: 1,
            clock_millis: 0,
            camera: Camera {
                projection,
                center: [0.5, 0.5],
                zoom,
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
// Zooms are the ones the search returns, never recomputed, so they compare
// exactly.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn isometric(tile_width: u32, tile_height: u32) -> Projection {
        Projection::Isometric {
            tile_width,
            tile_height,
        }
    }

    /// The largest drawable difference and its zoom, searched up to the
    /// editor's widest range with 0 (2 × 1,024), or 2,048 when that draws.
    fn limit(diamond: [u32; 2], art: [u32; 2]) -> (u32, f32) {
        largest_difference(
            isometric(diamond[0], diamond[1]),
            art[0],
            art[1],
            2048,
            PreviewLimits::default(),
        )
        .unwrap_or((2048, LARGEST_ZOOM))
    }

    fn sizes(entries: &[((u32, u32), &[&str])]) -> BTreeMap<(u32, u32), Vec<String>> {
        entries
            .iter()
            .map(|(size, names)| (*size, names.iter().map(|name| (*name).to_owned()).collect()))
            .collect()
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
            let (largest, zoom) = limit(diamond, art);
            assert_eq!(zoom, LARGEST_ZOOM);
            assert!(
                (2 * measured..2 * measured + 2).contains(&largest),
                "{diamond:?} {art:?}: {largest}"
            );
        }
        // Diamond-only art draws beyond the editor's widest range.
        assert_eq!(limit([32, 16], [32, 16]).0, 2048);
        assert_eq!(limit([64, 32], [64, 32]).0, 2048);
        assert_eq!(limit([32, 16], [32, 32]).0, 921);
    }

    #[test]
    fn the_largest_difference_draws_and_one_more_does_not() {
        let projection = isometric(32, 16);
        let limits = PreviewLimits::default();
        let at = |difference| draws(projection, 32, 32, LARGEST_ZOOM, difference, limits);
        assert!(at(921));
        assert!(!at(922));
        // Every smaller difference draws too.
        assert!((0..921).step_by(37).all(at));
        // Top-down ground never bends.
        assert!(draws(
            Projection::TopDown { tile_size: 32 },
            32,
            128,
            LARGEST_ZOOM,
            2048,
            limits
        ));
    }

    #[test]
    fn every_two_height_pattern_is_checked() {
        let all: BTreeSet<Vec<Option<i32>>> = patterns(7).collect();
        assert_eq!(all.len(), 512);
        assert!(
            all.iter().all(|field| field.len() == 9
                && field.iter().all(|height| matches!(height, Some(0 | 7))))
        );
    }

    #[test]
    fn art_too_large_to_draw_flat_at_the_largest_zoom_is_judged_where_it_draws_flat() {
        let limits = PreviewLimits::default();
        // 1024 x 1024 on a 128 x 64 diamond draws flat only below zoom 8.
        let projection = isometric(128, 64);
        let zoom = flat_zoom(projection, 1024, 1024, limits).unwrap();
        assert!(zoom < LARGEST_ZOOM && zoom > 7.99, "{zoom}");
        assert!(!draws(projection, 1024, 1024, zoom.next_up(), 0, limits));
        let (largest, judged) = limit([128, 64], [1024, 1024]);
        assert_eq!(judged, zoom);
        // At zoom 1 the art bends over 123; at its largest zoom over less.
        assert!(largest < 123, "{largest}");
        assert!(draws(projection, 1024, 1024, 1.0, 123, limits));
        assert!(!draws(projection, 1024, 1024, 1.0, 124, limits));
        assert!(draws(projection, 1024, 1024, zoom, largest, limits));
        assert!(!draws(projection, 1024, 1024, zoom, largest + 1, limits));
        // Each is judged at the largest zoom that draws flat, not left out.
        // 1025 and 1050 need the step down: their bound rounds up in `f32`.
        for (diamond, art, expected, largest, steps) in [
            ([32, 16], [32, 1100], 7.446_363_4, 0, false),
            ([128, 64], [1025, 1025], 7.991_219, 11, true),
            ([64, 32], [64, 1050], 7.800_952, 3, true),
        ] {
            let projection = isometric(diamond[0], diamond[1]);
            #[allow(clippy::cast_precision_loss)]
            let bound = 8191.0 / art[1] as f32;
            let zoom = flat_zoom(projection, art[0], art[1], limits).unwrap();
            assert_eq!(zoom < bound, steps, "{art:?}: {zoom} {bound}");
            assert_eq!(zoom, expected, "{art:?}");
            assert!(!draws(
                projection,
                art[0],
                art[1],
                zoom.next_up(),
                0,
                limits
            ));
            assert_eq!(limit(diamond, art), (largest, expected), "{art:?}");
        }
        // Art too large for even zoom 1 is still judged, below 1.
        let (_, below) = limit([32, 16], [32, 8192]);
        assert!(below < 1.0, "{below}");
    }

    #[test]
    fn the_tightest_size_is_reported_with_its_sorted_resources() {
        let limits = PreviewLimits::default();
        let found = tightest(
            isometric(32, 16),
            sizes(&[((32, 32), &["b", "a"]), ((32, 48), &["tall_b", "tall_a"])]),
            2048,
            limits,
        )
        .unwrap();
        assert_eq!(found.largest_difference, 430);
        assert_eq!((found.width, found.height), (32, 48));
        assert_eq!(found.resources, ["tall_a", "tall_b"]);
        assert_eq!(found.diamond, [32, 16]);
        // Within the tighter limit nothing is reported.
        assert_eq!(
            tightest(
                isometric(32, 16),
                sizes(&[((32, 32), &["b"]), ((32, 48), &["tall"])]),
                430,
                limits
            ),
            None
        );
        // Top-down never bends.
        assert_eq!(
            tightest(
                Projection::TopDown { tile_size: 32 },
                sizes(&[((32, 1000), &["tall"])]),
                2048,
                limits
            ),
            None
        );
    }

    #[test]
    fn a_tall_diamond_limits_through_its_tile_picking_art() {
        // Small ground art, but the picking art is the 4 x 1000 diamond.
        let found = tightest(
            isometric(4, 1000),
            sizes(&[((4, 8), &["flat"])]),
            2048,
            PreviewLimits::default(),
        )
        .unwrap();
        assert_eq!((found.width, found.height), (4, 1000));
        assert!(found.resources.is_empty());
        assert!(found.largest_difference < 2048);
    }
}
