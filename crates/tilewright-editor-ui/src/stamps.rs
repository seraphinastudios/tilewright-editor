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
//! - **Mirror** (M) flips the layout left to right on screen, so its rule
//!   depends on the active view's projection ([`StampView`]), read when M
//!   is pressed. Top-down it flips across the anchor's column, the plane's
//!   `y` axis through the anchor, which is vertical on screen: `[dx, dy]`
//!   becomes `[-dx, dy]`. In an isometric view `+x` is drawn down-right
//!   and `+y` down-left, so a tile `[dx, dy]` sits `dx - dy` half-diamonds
//!   right of the anchor and `dx + dy` down; flipping left to right
//!   negates the first and keeps the second, so `[dx, dy]` becomes
//!   `[dy, dx]` (across the anchor's vertical on screen).
//! - **Turn** (R) turns it a quarter turn clockwise: `[dx, dy]` becomes
//!   `[-dy, dx]`, so `+x` goes to `+y`. Both projections draw `+y` a quarter
//!   turn clockwise from `+x` (top-down: right, then down; isometric:
//!   down-right, then down-left), so this is clockwise on screen. It is the
//!   same rule in both.
//! - Each key acts on the layout as it is shown: R turns what is shown, M
//!   mirrors what is shown. The state is a number of quarter turns `t` and
//!   a flag `m`, held on the grid: an offset is first mirrored across the
//!   anchor's column when `m` is set, then turned `t` times. R adds one
//!   turn. Top-down M sets `t` to `4 - t` as it flips `m`, since mirroring
//!   a turned layout is the same as turning the mirrored one the other way;
//!   in an isometric view M sets `t` to `3 - t` (mod 4) as it flips `m`,
//!   since the screen flip `[dx, dy] → [dy, dx]` is the column mirror
//!   followed by three quarter turns. Either way the keys reach the same
//!   eight distinct layouts, the symmetries of the square, and two M or
//!   four R give the shown layout back.
//! - The Turn and Mirror buttons show the state as the view sees it
//!   ([`StampTurn::shown`]): a turn applied after an optional screen
//!   mirror, so M alone reads "Turn: 0°" and "Mirror: on" in both
//!   projections.
//!
//! Every object's own art is unchanged: only where each object goes turns.
use crate::stamp::StampObject;
use tilewright_editor_preview::{PreviewCatalog, Stamp};

/// The active view's projection, as far as M cares: which grid rule is a
/// left-right flip on screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StampView {
    /// Square tiles: M flips across the anchor's column, `[-dx, dy]`.
    TopDown,
    /// Diamond tiles: M flips across the screen's vertical through the
    /// anchor, `[dy, dx]`.
    Isometric,
}

impl StampView {
    /// The view of a World drawn with `projection`.
    pub const fn of(projection: &tilewright_source::Projection) -> Self {
        match projection {
            tilewright_source::Projection::TopDown { .. } => Self::TopDown,
            tilewright_source::Projection::Isometric { .. } => Self::Isometric,
        }
    }
}

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
    /// turning): the state on the grid, whichever view M was pressed in
    /// ([`Self::shown`] is the view's reading).
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
    /// The shown layout mirrored across the anchor's column: M in a
    /// top-down view.
    #[must_use]
    pub const fn flipped(self) -> Self {
        Self {
            quarter_turns: (4 - self.quarter_turns) % 4,
            mirrored: !self.mirrored,
        }
    }
    /// M: the shown layout flipped left to right on screen in `view`.
    /// Top-down that is [`Self::flipped`]; in an isometric view the screen
    /// flip `[dx, dy] → [dy, dx]` is the column mirror then three quarter
    /// turns, so applied to the shown layout it sets `3 - t` turns.
    #[must_use]
    pub const fn flipped_on_screen(self, view: StampView) -> Self {
        match view {
            StampView::TopDown => self.flipped(),
            StampView::Isometric => Self {
                quarter_turns: (7 - self.quarter_turns) % 4,
                mirrored: !self.mirrored,
            },
        }
    }
    /// The state as `view` shows it on the Turn and Mirror buttons: the
    /// clockwise turn in degrees and whether a screen mirror (M's rule in
    /// `view`) comes before it. Top-down that is [`Self::degrees`] and
    /// [`Self::mirrored`]; in an isometric view the column mirror is the
    /// screen mirror then one quarter turn, so a mirrored state shows one
    /// turn more.
    pub const fn shown(self, view: StampView) -> (u16, bool) {
        let turns = match view {
            StampView::Isometric if self.mirrored => (self.quarter_turns + 1) % 4,
            _ => self.quarter_turns,
        };
        (turns as u16 * 90, self.mirrored)
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

/// `objects` (a copy captured from the map) as a click records them: in
/// their order, each offset after `turn`, exactly as for a catalog stamp.
pub fn turned_objects(objects: &[StampObject], turn: StampTurn) -> Vec<StampObject> {
    objects
        .iter()
        .map(|object| StampObject {
            proto: object.proto.clone(),
            offset: turn.apply(object.offset),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{StampTurn, StampView};
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

    /// Every combination M and R reach in `view`, from the catalog layout.
    fn reachable_in(view: StampView) -> Vec<StampTurn> {
        let mut all = vec![StampTurn::NONE];
        let mut index = 0;
        while index < all.len() {
            for next in [all[index].turned(), all[index].flipped_on_screen(view)] {
                if !all.contains(&next) {
                    all.push(next);
                }
            }
            index += 1;
        }
        all
    }

    /// Where offset `[dx, dy]` is drawn relative to the anchor, in
    /// half-diamonds right and down, in an isometric view (`+x` down-right,
    /// `+y` down-left).
    fn isometric_screen([dx, dy]: [i64; 2]) -> [i64; 2] {
        [dx - dy, dx + dy]
    }

    #[test]
    fn the_view_is_the_worlds_projection() {
        use tilewright_source::Projection;
        assert_eq!(
            StampView::of(&Projection::TopDown { tile_size: 32 }),
            StampView::TopDown
        );
        assert_eq!(
            StampView::of(&Projection::Isometric {
                tile_width: 64,
                tile_height: 32
            }),
            StampView::Isometric
        );
    }

    #[test]
    fn top_down_m_is_the_column_mirror_and_reads_as_before() {
        for turn in reachable() {
            assert_eq!(turn.flipped_on_screen(StampView::TopDown), turn.flipped());
            assert_eq!(
                turn.shown(StampView::TopDown),
                (turn.degrees(), turn.mirrored())
            );
        }
        assert_eq!(reachable_in(StampView::TopDown), reachable());
    }

    #[test]
    fn isometric_m_flips_left_to_right_on_screen_what_is_shown() {
        let iso = StampView::Isometric;
        let none = StampTurn::NONE;
        // The stated rule: [dx, dy] -> [dy, dx] from the catalog layout.
        assert_eq!(none.flipped_on_screen(iso).apply([2, 1]), [1, 2]);
        assert_eq!(none.flipped_on_screen(iso).apply([1, 0]), [0, 1]);
        // M alone reads as a mirror and no turn, as top-down.
        assert_eq!(none.flipped_on_screen(iso).shown(iso), (0, true));
        let offsets = [[2, 1], [1, 0], [0, 1], [3, -1], [-4095, 7], [1, 4095]];
        for start in reachable_in(iso) {
            let flipped = start.flipped_on_screen(iso);
            for offset in offsets {
                // Left to right on screen: across negated, down kept.
                let [across, down] = isometric_screen(start.apply(offset));
                assert_eq!(isometric_screen(flipped.apply(offset)), [-across, down]);
                // R after M still turns the shown layout clockwise, by the
                // grid rule.
                let shown = flipped.apply(offset);
                assert_eq!(flipped.turned().apply(offset), [-shown[1], shown[0]]);
            }
            assert_eq!(flipped.flipped_on_screen(iso), start);
            // The buttons' reading: a screen mirror, then that many turns.
            let (degrees, mirrored) = start.shown(iso);
            for [dx, dy] in offsets {
                let mut expected = if mirrored { [dy, dx] } else { [dx, dy] };
                for _ in 0..degrees / 90 {
                    expected = [-expected[1], expected[0]];
                }
                assert_eq!(start.apply([dx, dy]), expected);
            }
        }
        // The same eight distinct layouts as top-down, each a distinct
        // reading on the buttons.
        let all = reachable_in(iso);
        assert_eq!(all.len(), 8);
        let mut sorted = all.clone();
        sorted.sort_by_key(|turn| (turn.quarter_turns(), turn.mirrored()));
        let mut top_down = reachable();
        top_down.sort_by_key(|turn| (turn.quarter_turns(), turn.mirrored()));
        assert_eq!(sorted, top_down);
        let images: std::collections::BTreeSet<_> =
            all.iter().map(|turn| turn.apply([2, 1])).collect();
        assert_eq!(images.len(), 8);
        let readings: std::collections::BTreeSet<_> =
            all.iter().map(|turn| turn.shown(iso)).collect();
        assert_eq!(readings.len(), 8);
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
