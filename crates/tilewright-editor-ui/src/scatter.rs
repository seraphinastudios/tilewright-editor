// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Scatter: the value an [`Action::ScatterTerrain`](crate::Action::ScatterTerrain)
//! writes into each cell it covers, drawn from a catalog palette by weight.
//!
//! The draw is part of what a recorded scatter step means, so it is fixed
//! here and must never change for this action kind (`docs/ENGINE.md`,
//! "Scatter"). Golden vectors in this module's tests and in
//! `tests/scatter.rs` pin it.
//!
//! - **Mixing.** `mix` is the `SplitMix64` output function: with wrapping
//!   64-bit arithmetic, `z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9`,
//!   `z = (z ^ (z >> 27)) * 0x94d049bb133111eb`, then `z ^ (z >> 31)`.
//! - **A cell's draw.** With `G = 0x9e3779b97f4a7c15`, start from
//!   `h = mix(seed + G)`, then for each of the words `layer`, `x`, `y` in
//!   that order (each a two's-complement 64-bit integer: the spatial layer
//!   sign-extended, the plane cell coordinates as they are) set
//!   `h = mix((h ^ word) + G)`. The draw depends only on the seed, the layer
//!   and the cell, never on which other cells a stroke covers or in which
//!   order, so a replay or an overlapping stroke with the same seed gives a
//!   cell the same value.
//! - **Choosing by weight.** With `total` the sum of the palette's
//!   weights, `r = ((h >> 32) * total) >> 32`, so `0 <= r < total`. The
//!   entries are taken in increasing byte order of their values; the first
//!   whose running sum of weights exceeds `r` is the cell's value. Only
//!   integers are used, so every platform gives the same answer.
use tilewright_editor_preview::Palette;

const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// The `SplitMix64` output function.
const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The 64-bit draw for plane cell `[x, y]` of spatial `layer` under `seed`.
#[must_use]
#[allow(clippy::cast_sign_loss)]
pub const fn draw(seed: u32, layer: i32, [x, y]: [i64; 2]) -> u64 {
    let mut h = mix((seed as u64).wrapping_add(GAMMA));
    h = mix((h ^ (layer as i64 as u64)).wrapping_add(GAMMA));
    h = mix((h ^ (x as u64)).wrapping_add(GAMMA));
    mix((h ^ (y as u64)).wrapping_add(GAMMA))
}

/// The palette value a draw `h` chooses, by weight. `None` only for a
/// palette without entries, which the catalog never loads.
#[must_use]
pub fn choose(palette: &Palette, h: u64) -> Option<&str> {
    let total = u64::from(palette.total_weight());
    let r = ((h >> 32) * total) >> 32;
    let mut sum = 0u64;
    for (value, weight) in &palette.entries {
        sum += u64::from(*weight);
        if r < sum {
            return Some(value);
        }
    }
    None
}

/// The value scatter writes into plane cell `cell` of spatial `layer`
/// with `seed`: [`choose`] of [`draw`].
#[must_use]
pub fn value(palette: &Palette, seed: u32, layer: i32, cell: [i64; 2]) -> Option<&str> {
    choose(palette, draw(seed, layer, cell))
}

#[cfg(test)]
mod tests {
    use super::{choose, draw, mix};
    use tilewright_editor_preview::Palette;

    #[test]
    fn the_mixer_and_the_draw_match_their_golden_vectors() {
        // SplitMix64's published first output for state 0 is
        // mix(0 + G) = 0xe220a8397b1dcdaf.
        assert_eq!(mix(super::GAMMA), 0xe220_a839_7b1d_cdaf);
        for (seed, layer, cell, expected) in GOLDEN {
            assert_eq!(draw(seed, layer, cell), expected, "{seed} {layer} {cell:?}");
        }
    }

    /// From an independent implementation of the definition above (a
    /// short script outside this repository, written from the module text).
    const GOLDEN: [(u32, i32, [i64; 2], u64); 6] = [
        (0, 0, [0, 0], 0x2130_748a_aac8_0268),
        (1234, 0, [5, 3], 0x4597_f873_b77c_60ba),
        (u32::MAX, -1, [-1, -1], 0x054d_8435_bd3a_3fa4),
        (7, 3, [-32, 31], 0x22ed_dce7_5418_801a),
        (42, i32::MIN, [i64::MAX, i64::MIN], 0x3ef0_3339_12c0_401a),
        (
            42,
            0,
            [9_007_199_254_740_991, -9_007_199_254_740_991],
            0xa72a_fd30_f663_826b,
        ),
    ];

    #[test]
    fn choosing_maps_the_draw_onto_the_weights_in_value_order() {
        let palette = Palette {
            grid: "floor".into(),
            entries: [("b".to_owned(), 3), ("a".to_owned(), 1)].into(),
        };
        // total 4: r = high 32 bits * 4 >> 32, so the top two bits pick.
        assert_eq!(choose(&palette, 0), Some("a"));
        assert_eq!(choose(&palette, 0x3fff_ffff_ffff_ffff), Some("a"));
        assert_eq!(choose(&palette, 0x4000_0000_0000_0000), Some("b"));
        assert_eq!(choose(&palette, u64::MAX), Some("b"));
        assert_eq!(choose(&Palette::default(), 5), None);
    }
}
