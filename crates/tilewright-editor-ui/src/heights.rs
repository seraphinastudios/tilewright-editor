// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Read-only height display: the stored height of a tile, the height tint
//! of the active view and the inspector's height line. World stores one
//! whole-number height per tile and spatial layer in each chunk (default 0,
//! any 32-bit value); a profile's `layer_height` stacks layer `L` at
//! `L × layer_height` (FORMAT.md). Nothing in this module edits anything,
//! and all arithmetic on heights is done in wider integers, so no stored
//! value can overflow or panic here.
use std::collections::BTreeMap;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::{Plane, World};

/// The height ramp: eight bands from dark (low) to light (high). Lightness
/// rises from band to band by at least 9 CIE L* (tested), so the order reads
/// without telling hues apart; the hue runs from violet through teal and
/// green to pale yellow. The tint is opaque ([`TINT_ALPHA`]), so a tile shows
/// its band's own colour over any ground. The ends are darker and lighter
/// than any zone tint.
pub const RAMP: [[f32; 3]; 8] = [
    [0.17, 0.11, 0.35],
    [0.20, 0.24, 0.52],
    [0.16, 0.38, 0.58],
    [0.12, 0.52, 0.56],
    [0.20, 0.64, 0.48],
    [0.45, 0.74, 0.36],
    [0.75, 0.83, 0.30],
    [0.98, 0.93, 0.55],
];
/// Opacity of a height tint over its tile: opaque. Any translucency lets the
/// ground decide part of a tile's lightness, and over light and dark ground
/// (pale art beside a tile without ground) neighbouring bands then swap.
/// Opaque, the band order over any ground is the ramp's own. It hides the
/// ground art under the tint while the tint is shown; the tint is a toggle.
pub const TINT_ALPHA: f32 = 1.0;

/// The stored height of plane tile `x, y` on `layer`: `None` when its chunk
/// does not exist (or the plane's chunk size is 0). A chunk without a
/// height grid holds 0 everywhere.
pub fn cell_height(plane: &Plane, layer: i32, x: i64, y: i64) -> Option<i32> {
    let size = i64::from(plane.chunk_size);
    if size <= 0 {
        return None;
    }
    let chunk = plane
        .chunks
        .get(&(layer, x.div_euclid(size), y.div_euclid(size)))?;
    let local = usize::try_from(y.rem_euclid(size) * size + x.rem_euclid(size)).ok()?;
    Some(chunk.heights.get(local).copied().unwrap_or(0))
}

/// The height of every tile of the active view whose chunk exists, in
/// row-major order. The view holds at most the preview's cell budget (4,096
/// tiles).
pub fn view_heights(world: &World, catalog: &PreviewCatalog) -> Vec<([i64; 2], i32)> {
    let (plane, layer, [min_x, min_y, width, height]) = catalog.view();
    let Some(plane) = world.planes().get(plane) else {
        return Vec::new();
    };
    let mut heights = Vec::new();
    for y in min_y..min_y.saturating_add(height) {
        for x in min_x..min_x.saturating_add(width) {
            if let Some(h) = cell_height(plane, layer, x, y) {
                heights.push(([x, y], h));
            }
        }
    }
    heights
}

/// The lowest and highest stored height of the tiles a tint is drawn over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeightRange {
    pub min: i32,
    pub max: i32,
}

impl HeightRange {
    /// The range of `heights`; `None` when there are none.
    pub fn of(heights: impl IntoIterator<Item = i32>) -> Option<Self> {
        heights.into_iter().fold(None, |range, h| {
            Some(match range {
                None => Self { min: h, max: h },
                Some(Self { min, max }) => Self {
                    min: min.min(h),
                    max: max.max(h),
                },
            })
        })
    }

    /// Whether every tile has the same height. A flat range has no tint.
    pub const fn is_flat(self) -> bool {
        self.min == self.max
    }

    /// The ramp band of height `h`: `None` for a flat range or a height
    /// outside it. Otherwise the nearest of the eight evenly spaced band
    /// positions, a half rounded up, in integers:
    /// `((h − min) × 14 + (max − min)) div (2 × (max − min))`. So `min` is
    /// always band 0 and `max` always band 7, whatever the span.
    pub fn band(self, h: i32) -> Option<usize> {
        if self.is_flat() || h < self.min || h > self.max {
            return None;
        }
        let steps = i64::try_from(RAMP.len() - 1).ok()?;
        let offset = i64::from(h) - i64::from(self.min);
        let span = i64::from(self.max) - i64::from(self.min);
        // Both are below 2^32, so neither product nor sum leaves i64.
        let band = (offset * 2 * steps + span) / (2 * span);
        usize::try_from(band).ok()
    }
}

/// The RGBA colour of ramp band `band` drawn over a tile.
pub fn tint_color(band: usize) -> [f32; 4] {
    let [r, g, b] = RAMP[band.min(RAMP.len() - 1)];
    [r, g, b, TINT_ALPHA]
}

/// The RGBA colour of ramp band `band` as an opaque legend swatch.
pub fn swatch_color(band: usize) -> [f32; 4] {
    let [r, g, b] = RAMP[band.min(RAMP.len() - 1)];
    [r, g, b, 1.0]
}

/// The tint of every tile of `heights` (as [`view_heights`] lists them) and
/// the range they span: no tile is tinted when the range is flat.
pub fn tints(heights: &[([i64; 2], i32)]) -> (Option<HeightRange>, BTreeMap<[i64; 2], [f32; 4]>) {
    let range = HeightRange::of(heights.iter().map(|(_, h)| *h));
    let tints = range
        .map(|range| {
            heights
                .iter()
                .filter_map(|(at, h)| Some((*at, tint_color(range.band(*h)?))))
                .collect()
        })
        .unwrap_or_default();
    (range, tints)
}

/// Says that zone tints draw over the height tint, so tiles inside zones do
/// not match the legend's swatches, and that O hides them (outside Zone
/// mode).
pub const UNDER_ZONES: &str = "Zone tints cover it: O hides them";
/// [`UNDER_ZONES`] in Zone mode, where zones always show and O does nothing.
pub const UNDER_ZONES_IN_ZONE_MODE: &str = "Zone tints cover it in Zone mode";

/// The legend of the height tint: a line with the range of the view's
/// heights, then for a range that is not flat the darkest and the lightest
/// band, each with the band its swatch shows, and `zone_line` last when
/// given (zone tints are drawn over the tint: [`UNDER_ZONES`] or
/// [`UNDER_ZONES_IN_ZONE_MODE`]). The two band lines name the ends again, so
/// they stay readable when a narrow sidebar cuts the first.
pub fn legend(range: Option<HeightRange>, zone_line: Option<&str>) -> Vec<(String, Option<usize>)> {
    match range {
        None => vec![("Heights: no tiles in view".into(), None)],
        Some(range) if range.is_flat() => {
            vec![(format!("Heights: all {}, no tint", range.min), None)]
        }
        Some(HeightRange { min, max }) => {
            let mut rows = vec![
                (format!("Heights {min} to {max}"), None),
                (format!("Darkest: lowest, {min}"), Some(0)),
                (format!("Lightest: highest, {max}"), Some(RAMP.len() - 1)),
            ];
            if let Some(line) = zone_line {
                rows.push((line.into(), None));
            }
            rows
        }
    }
}

/// The inspector's height line for plane tile `x, y` on `layer`: its stored
/// height, and with a profile `layer_height` also the layer's base
/// (`layer × layer_height`) and the sum, computed in 128-bit integers, as
/// "Height: 39 (7 + base 32)": the stacked height first, so it stays when a
/// narrow sidebar cuts the line.
pub fn inspector_line(world: &World, plane: &str, layer: i32, x: i64, y: i64) -> String {
    let Some(h) = world
        .planes()
        .get(plane)
        .and_then(|plane| cell_height(plane, layer, x, y))
    else {
        return "Height: none, no chunk here".into();
    };
    match world.profile().layer_height {
        None => format!("Height: {h}"),
        Some(layer_height) => {
            let base = i128::from(layer) * i128::from(layer_height);
            let total = base + i128::from(h);
            format!("Height: {total} ({h} + base {base})")
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn range(min: i32, max: i32) -> HeightRange {
        HeightRange { min, max }
    }

    /// CIE L* of a gamma-encoded sRGB colour.
    fn lightness([r, g, b]: [f32; 3]) -> f64 {
        let linear = |c: f32| {
            let c = f64::from(c);
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let y = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
        if y > 216.0 / 24_389.0 {
            116.0 * y.cbrt() - 16.0
        } else {
            24_389.0 / 27.0 * y
        }
    }

    #[test]
    fn the_ramp_gets_lighter_band_by_band() {
        let luma = |[r, g, b]: [f32; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
        for pair in RAMP.windows(2) {
            assert!(luma(pair[1]) - luma(pair[0]) > 0.09, "{pair:?}");
            // 9.6 to 12.9 L* per step.
            assert!(lightness(pair[1]) - lightness(pair[0]) > 9.0, "{pair:?}");
        }
        // Darker and lighter than every zone tint at its ends.
        for zone in crate::zones::TINTS {
            assert!(luma(RAMP[0]) < luma(zone) && luma(zone) < luma(RAMP[7]));
        }
    }

    #[test]
    fn an_opaque_tint_shows_its_band_over_any_ground() {
        // The renderer blends in gamma-encoded sRGB:
        // `alpha × tint + (1 − alpha) × ground`. Opaque, the drawn colour is
        // the band's own over black, white and the sample's grass and sand
        // (their art's mean colours, about 92, 146, 78 and 205, 180, 118 of
        // 255), so band order over any ground is the ramp's order.
        let grounds = [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [92.0 / 255.0, 146.0 / 255.0, 78.0 / 255.0],
            [205.0 / 255.0, 180.0 / 255.0, 118.0 / 255.0],
        ];
        for (band, own) in RAMP.iter().enumerate() {
            let [r, g, b, alpha] = tint_color(band);
            for ground in grounds {
                let drawn = [r, g, b]
                    .iter()
                    .zip(ground)
                    .map(|(tint, under)| alpha * tint + (1.0 - alpha) * under)
                    .collect::<Vec<f32>>();
                assert_eq!(drawn, own, "band {band} over {ground:?}");
            }
        }
    }

    #[test]
    fn band_golden_values() {
        // Span 7: one band per unit.
        let seven = range(0, 7);
        assert_eq!(
            (0..=7).map(|h| seven.band(h).unwrap()).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6, 7]
        );
        // Span 12 (the legend's example): 7/12 per unit, halves rounded up.
        let twelve = range(0, 12);
        assert_eq!(
            (0..=12)
                .map(|h| twelve.band(h).unwrap())
                .collect::<Vec<_>>(),
            [0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6, 7]
        );
        // Span 1 and 2: the ends are always the darkest and lightest band.
        assert_eq!(range(5, 6).band(5), Some(0));
        assert_eq!(range(5, 6).band(6), Some(7));
        assert_eq!(range(-1, 1).band(0), Some(4), "3.5 rounds up");
        // Negative heights work like any other.
        assert_eq!(range(-24, -10).band(-17), Some(4), "7 of 14 is 3.5");
        // Flat or outside: no band.
        assert_eq!(range(3, 3).band(3), None);
        assert_eq!(seven.band(-1), None);
        assert_eq!(seven.band(8), None);
    }

    #[test]
    fn extreme_heights_neither_overflow_nor_panic() {
        let full = range(i32::MIN, i32::MAX);
        assert_eq!(full.band(i32::MIN), Some(0));
        assert_eq!(full.band(i32::MAX), Some(7));
        assert_eq!(full.band(0), Some(4), "just above the middle");
        assert_eq!(full.band(-1), Some(3));
        assert_eq!(range(i32::MAX - 1, i32::MAX).band(i32::MAX), Some(7));
        assert_eq!(range(i32::MIN, i32::MIN + 1).band(i32::MIN), Some(0));
        assert_eq!(
            HeightRange::of([i32::MAX, i32::MIN, 0]),
            Some(range(i32::MIN, i32::MAX))
        );
    }

    #[test]
    fn tints_and_legend_follow_the_range() {
        let heights = [([0, 0], 0), ([1, 0], 6), ([2, 0], 12)];
        let (found, tints) = tints(&heights);
        assert_eq!(found, Some(range(0, 12)));
        assert_eq!(tints[&[0, 0]], tint_color(0));
        assert_eq!(tints[&[1, 0]], tint_color(4));
        assert_eq!(tints[&[2, 0]], tint_color(7));
        assert_eq!(
            legend(found, None),
            [
                ("Heights 0 to 12".to_owned(), None),
                ("Darkest: lowest, 0".to_owned(), Some(0)),
                ("Lightest: highest, 12".to_owned(), Some(7)),
            ]
        );
        // A flat view: no tint at all, and the legend says so.
        let (flat, none) = super::tints(&[([0, 0], 5), ([1, 0], 5)]);
        assert!(none.is_empty());
        assert_eq!(
            legend(flat, Some(UNDER_ZONES)),
            [("Heights: all 5, no tint".to_owned(), None)]
        );
        let (empty, none) = super::tints(&[]);
        assert!(empty.is_none() && none.is_empty());
        assert_eq!(
            legend(empty, Some(UNDER_ZONES)),
            [("Heights: no tiles in view".to_owned(), None)]
        );
        for line in [UNDER_ZONES, UNDER_ZONES_IN_ZONE_MODE] {
            let mut under = legend(found, None);
            under.push((line.to_owned(), None));
            assert_eq!(legend(found, Some(line)), under);
        }
        assert_eq!(tint_color(7)[3], TINT_ALPHA);
        assert_eq!(swatch_color(0), [0.17, 0.11, 0.35, 1.0]);
    }
}
