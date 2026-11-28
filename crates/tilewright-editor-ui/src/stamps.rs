// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The catalog's stamps (`editing.stamps`) as Place mode offers them, and
//! the turn and mirror R and M apply to a stamp's offsets before it becomes
//! a [`crate::Action::PlaceObjects`]. The engine never turns or flips
//! anything (ADR 0009): what it records is the final offsets this module
//! computes, so the convention here is the desktop's, not the kind's.
//!
//! The convention, in plane tiles about the anchor tile (offset `[0, 0]`):
//!
//! - **Mirror** (M) flips the layout across the anchor's column, the
//!   plane's `y` axis through the anchor: `[dx, dy]` becomes `[-dx, dy]`.
//!   It is a grid rule, the same in both projections: top-down the column
//!   is vertical on screen, so M is a left-right mirror; in an isometric
//!   view the column runs down-left on screen, so M flips across that
//!   diagonal, not across the screen's vertical.
//! - **Turn** (R) turns it a quarter turn clockwise: `[dx, dy]` becomes
//!   `[-dy, dx]`, so `+x` goes to `+y`. Both projections draw `+y` a quarter
//!   turn clockwise from `+x` (top-down: right, then down; isometric:
//!   down-right, then down-left), so this is clockwise on screen.
//! - Each key acts on the layout as it is shown: R turns what is shown, M
//!   mirrors what is shown. The state is a number of quarter turns `t` and
//!   a mirror flag `m`; an offset is first mirrored when `m` is set, then
//!   turned `t` times. M after `t` turns therefore sets `t` to `4 - t` as it
//!   flips `m`, since mirroring a turned layout is the same as turning the
//!   mirrored one the other way.
//!
//! Every object's own art is unchanged: only where each object goes turns.
use crate::stamp::StampObject;
use tilewright_editor_preview::{PreviewCatalog, Stamp};

/// How Place mode shows and places the chosen stamp: quarter turns
/// clockwise and whether it is mirrored first (see the module notes).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StampTurn {
    quarter_turns: u8,
    mirrored: bool,
}

impl StampTurn {
    /// The catalog's own layout.
    pub const NONE: Self = Self {
        quarter_turns: 0,
        mirrored: false,
    };
    /// Quarter turns clockwise, 0 to 3, applied after the mirror.
    pub const fn quarter_turns(self) -> u8 {
        self.quarter_turns
    }
    /// The clockwise turn in degrees: 0, 90, 180 or 270.
    pub const fn degrees(self) -> u16 {
        self.quarter_turns as u16 * 90
    }
    /// Whether the layout is mirrored across the anchor's column (before
    /// turning).
    pub const fn mirrored(self) -> bool {
        self.mirrored
    }
    /// R: the shown layout turned a quarter turn clockwise.
    #[must_use]
    pub const fn turned(self) -> Self {
        Self {
            quarter_turns: (self.quarter_turns + 1) % 4,
            mirrored: self.mirrored,
        }
    }
    /// M: the shown layout mirrored across the anchor's column.
    #[must_use]
    pub const fn flipped(self) -> Self {
        Self {
            quarter_turns: (4 - self.quarter_turns) % 4,
            mirrored: !self.mirrored,
        }
    }
    /// Where an object at catalog offset `[dx, dy]` goes: mirrored when
    /// [`Self::mirrored`], then turned [`Self::quarter_turns`] times. Both
    /// keep each axis's magnitude, so an offset inside the engine's bound
    /// stays inside it.
    pub const fn apply(self, [dx, dy]: [i64; 2]) -> [i64; 2] {
        let mut offset = if self.mirrored { [-dx, dy] } else { [dx, dy] };
        let mut turns = self.quarter_turns;
        while turns > 0 {
            offset = [-offset[1], offset[0]];
            turns -= 1;
        }
        offset
    }
}

/// The catalog's stamps by name, in name order.
pub fn catalog_stamps(catalog: &PreviewCatalog) -> impl Iterator<Item = (&str, &Stamp)> {
    catalog
        .editing()
        .stamps
        .iter()
        .map(|(name, stamp)| (name.as_str(), stamp))
}

/// The objects of `stamp` as [`crate::Action::PlaceObjects`] records them:
/// in catalog order, each prototype with its offset after `turn`.
pub fn stamp_objects(stamp: &Stamp, turn: StampTurn) -> Vec<StampObject> {
    stamp
        .objects
        .iter()
        .map(|member| StampObject {
            proto: member.proto.clone(),
            offset: turn.apply(member.offset),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::StampTurn;
    use crate::stamp::{MAXIMUM_STAMP_OBJECTS, MAXIMUM_STAMP_OFFSET};

    /// Every combination the keys can reach, from the catalog layout.
    fn reachable() -> Vec<StampTurn> {
        let mut all = vec![StampTurn::NONE];
        let mut index = 0;
        while index < all.len() {
            for next in [all[index].turned(), all[index].flipped()] {
                if !all.contains(&next) {
                    all.push(next);
                }
            }
            index += 1;
        }
        all
    }

    #[test]
    fn the_catalog_bounds_are_the_engines() {
        assert_eq!(
            tilewright_editor_preview::MAXIMUM_STAMP_MEMBERS,
            MAXIMUM_STAMP_OBJECTS
        );
        assert_eq!(
            tilewright_editor_preview::MAXIMUM_STAMP_MEMBER_OFFSET,
            MAXIMUM_STAMP_OFFSET
        );
    }

    #[test]
    fn r_turns_clockwise_and_m_mirrors_across_the_column_what_is_shown() {
        let none = StampTurn::NONE;
        // The stated convention on an asymmetric layout: east, south-east.
        assert_eq!(none.apply([2, 1]), [2, 1]);
        assert_eq!(none.turned().apply([1, 0]), [0, 1]);
        assert_eq!(none.turned().apply([2, 1]), [-1, 2]);
        assert_eq!(none.turned().turned().apply([2, 1]), [-2, -1]);
        assert_eq!(none.turned().turned().turned().apply([2, 1]), [1, -2]);
        assert_eq!(none.flipped().apply([2, 1]), [-2, 1]);
        // Each key acts on the shown layout: M after R mirrors the turned
        // layout, R after M turns the mirrored one.
        let shown = none.turned().apply([2, 1]);
        assert_eq!(none.turned().flipped().apply([2, 1]), [-shown[0], shown[1]]);
        let shown = none.flipped().apply([2, 1]);
        assert_eq!(none.flipped().turned().apply([2, 1]), [-shown[1], shown[0]]);
        // The anchor never moves.
        for turn in reachable() {
            assert_eq!(turn.apply([0, 0]), [0, 0]);
        }
        assert_eq!(none.degrees(), 0);
        assert_eq!(none.turned().turned().turned().degrees(), 270);
    }

    #[test]
    fn four_turns_or_two_mirrors_return_to_the_catalog_layout() {
        let offsets = [[0, 0], [3, -1], [-4095, 7], [1, 4095], [-2, -9]];
        for start in reachable() {
            let mut four = start;
            for _ in 0..4 {
                four = four.turned();
            }
            assert_eq!(four, start);
            assert_eq!(start.flipped().flipped(), start);
            // The keys reach exactly the eight symmetries of the square,
            // each a distinct map of an asymmetric layout.
            for offset in offsets {
                let moved = start.apply(offset);
                assert_eq!(
                    moved.map(i64::unsigned_abs).iter().max(),
                    offset.map(i64::unsigned_abs).iter().max()
                );
            }
        }
        let all = reachable();
        assert_eq!(all.len(), 8);
        let images: std::collections::BTreeSet<_> =
            all.iter().map(|turn| turn.apply([2, 1])).collect();
        assert_eq!(images.len(), 8);
        // Every key sequence lands on the state its own arithmetic gives:
        // an offset is mirrored first, then turned.
        for turn in all {
            let [dx, dy] = [5, 2];
            let mut expected = if turn.mirrored() { [-dx, dy] } else { [dx, dy] };
            for _ in 0..turn.quarter_turns() {
                expected = [-expected[1], expected[0]];
            }
            assert_eq!(turn.apply([dx, dy]), expected);
        }
    }

    #[test]
    fn turned_offsets_stay_inside_the_engine_bound() {
        let edge = MAXIMUM_STAMP_OFFSET;
        for turn in reachable() {
            for offset in [[edge, -edge], [-edge, edge], [edge, 0], [0, -edge]] {
                assert!(crate::stamp::offset_in_bounds(turn.apply(offset)));
            }
        }
    }
}
