// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Terrain mode's Road rule: a line of clicked tiles that becomes one
//! ordinary brush stroke. A road is local input only, like a drag stroke:
//! it keeps the clicked points and nothing else, and its corridor is the
//! brush stamped along the straight cell line between consecutive points
//! by [`Stroke::stamp`], the same code that stamps between a drag's pointer
//! samples, clipped to the view and capped the same way. Finishing it
//! yields the [`Action::PaintStroke`](crate::Action::PaintStroke) or
//! [`Action::ScatterTerrain`](crate::Action::ScatterTerrain) a drag
//! through the same tiles in the same order would; no new kind of edit
//! exists for roads, so nothing about a road is recorded but its cells.
use crate::brush::{Brush, Stroke, StrokeTarget};

/// Most points one road holds. The view holds at most the stroke cap's
/// cells, so this only bounds a person clicking on and on.
pub const MAXIMUM_ROAD_POINTS: usize = 256;

/// A road being drawn: the tiles clicked, in click order, of one grid of
/// one plane layer, and the scatter seed it keeps once one is drawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Road {
    plane: String,
    layer: i32,
    grid: String,
    points: Vec<[i64; 2]>,
    /// The scatter seed drawn for this road: at its first point when a
    /// palette is chosen, else when one is chosen while it is drawn. It is
    /// kept for the rest of the road, so the preview draws the values
    /// finishing records.
    seed: Option<u32>,
}

impl Road {
    pub(crate) fn begin(plane: String, layer: i32, grid: String, at: [i64; 2]) -> Self {
        Self {
            plane,
            layer,
            grid,
            points: vec![at],
            seed: None,
        }
    }
    pub fn plane(&self) -> &str {
        &self.plane
    }
    pub fn layer(&self) -> i32 {
        self.layer
    }
    pub fn grid(&self) -> &str {
        &self.grid
    }
    /// The clicked tiles, in click order.
    pub fn points(&self) -> &[[i64; 2]] {
        &self.points
    }
    pub fn seed(&self) -> Option<u32> {
        self.seed
    }
    pub(crate) fn set_seed(&mut self, seed: u32) {
        self.seed.get_or_insert(seed);
    }
    /// Add `at` after the last point. A click on the last point adds
    /// nothing, so Backspace always takes away something visible.
    pub(crate) fn push(&mut self, at: [i64; 2]) -> bool {
        if self.points.last() == Some(&at) {
            return false;
        }
        self.points.push(at);
        true
    }
    /// Take away the last point; `false` when it was the only one.
    pub(crate) fn pop(&mut self) -> bool {
        self.points.pop();
        !self.points.is_empty()
    }
    /// The corridor as a stroke: `brush` stamped at every point in click
    /// order by [`Stroke::stamp`], which also stamps along the line from the
    /// previous point, clips to `bounds` and caps. A drag pressed at the
    /// first point and moved through the others covers the same cells.
    #[must_use]
    pub fn stroke(
        &self,
        target: StrokeTarget,
        brush: Brush,
        expected: [u8; 32],
        generation: u64,
        bounds: [i64; 4],
    ) -> Stroke {
        let mut stroke = Stroke::begin(
            self.plane.clone(),
            self.layer,
            self.grid.clone(),
            target,
            brush,
            expected,
            generation,
            bounds,
        );
        for &point in &self.points {
            stroke.stamp(point);
        }
        stroke
    }
}

#[cfg(test)]
mod tests {
    use super::Road;
    use crate::brush::{Brush, BrushShape, MAXIMUM_STROKE_CELLS, StrokeTarget};

    fn line(points: &[[i64; 2]]) -> Road {
        let mut road = Road::begin("main".into(), 0, "floor".into(), points[0]);
        for &point in &points[1..] {
            road.push(point);
        }
        road
    }

    fn sand() -> StrokeTarget {
        StrokeTarget::Terrain("sand".into())
    }

    #[test]
    fn the_corridor_is_the_cap_of_the_stroke_code_too() {
        let last = i64::try_from(MAXIMUM_STROKE_CELLS).unwrap() - 1;
        let bounds = [0, 0, 10_000, 10];
        let brush = Brush::default();
        let full = line(&[[0, 0], [last, 0]]).stroke(sand(), brush, [0; 32], 1, bounds);
        assert_eq!(full.len(), MAXIMUM_STROKE_CELLS);
        assert!(!full.capped());
        // One more point past the cap adds nothing, and nothing after it.
        let capped =
            line(&[[0, 0], [last, 0], [last, 2], [0, 5]]).stroke(sand(), brush, [0; 32], 1, bounds);
        assert!(capped.capped());
        assert_eq!(capped.len(), MAXIMUM_STROKE_CELLS);
        assert!(capped.cells().all(|[_, y]| y == 0));
    }

    #[test]
    fn the_corridor_is_clipped_to_the_view_and_follows_click_order() {
        let brush = Brush {
            shape: BrushShape::Square,
            radius: 2,
        };
        let bounds = [0, 0, 8, 6];
        let road = line(&[[0, 0], [7, 5]]);
        let stroke = road.stroke(sand(), brush, [0; 32], 1, bounds);
        assert!(
            stroke
                .cells()
                .all(|[x, y]| (0..8).contains(&x) && (0..6).contains(&y))
        );
        assert!(stroke.cells().any(|cell| cell == [0, 0]));
        // The line rule depends on direction: a reversed road may cover
        // other cells, and the clicks' order decides which.
        let one =
            line(&[[0, 0], [6, 5]]).stroke(sand(), Brush::default(), [0; 32], 1, [0, 0, 9, 9]);
        let other =
            line(&[[6, 5], [0, 0]]).stroke(sand(), Brush::default(), [0; 32], 1, [0, 0, 9, 9]);
        let (one, other): (Vec<_>, Vec<_>) = (one.cells().collect(), other.cells().collect());
        assert_ne!(one, other);
    }

    #[test]
    fn a_repeated_point_adds_nothing_and_the_last_pop_ends_the_road() {
        let mut road = line(&[[1, 1]]);
        assert!(!road.push([1, 1]));
        assert!(road.push([2, 1]));
        assert_eq!(road.points(), [[1, 1], [2, 1]]);
        assert!(road.pop());
        assert!(!road.pop());
        // The first seed is kept.
        road.set_seed(3);
        road.set_seed(4);
        assert_eq!(road.seed(), Some(3));
    }
}
