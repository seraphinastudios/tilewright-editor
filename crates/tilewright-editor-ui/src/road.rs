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
//!
//! Grading is a second step ([`BuiltRoad`], [`RoadMemory`]): once a road's
//! change is accepted the desktop remembers the cells that change
//! recorded, and "Grade road" prepares an ordinary
//! [`Action::FlattenHeights`] or [`Action::SmoothHeights`] over exactly
//! those cells, for a second Review and a second Undo step. Nothing new is
//! recorded for that either.
use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::World;

use crate::Action;
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

/// The refusal of a grade without `editing.heights`.
pub const GRADE_UNGRANTED: &str =
    "Grade road is unavailable: the catalog does not grant height editing (editing.heights).";

/// Which existing height edit grades a road.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum RoadGrade {
    /// `flatten_heights` to the committed height at the road's first point.
    Flatten,
    /// `smooth_heights` with kernel [`crate::sculpt::SMOOTH_KERNEL`].
    Smooth,
}

impl RoadGrade {
    pub const ALL: [Self; 2] = [Self::Flatten, Self::Smooth];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flatten => "flatten",
            Self::Smooth => "smooth",
        }
    }
}

/// A road whose paint or scatter was prepared: the plane and layer, the
/// cells that change recorded (all the corridor's cells, in the order the
/// action holds them, including those that already held the terrain), and
/// the road's first point, where a flatten reads its target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltRoad {
    plane: String,
    layer: i32,
    cells: Vec<[i64; 2]>,
    first: [i64; 2],
}

impl BuiltRoad {
    /// The road `road` finished into `action`, the
    /// [`Action::PaintStroke`] or [`Action::ScatterTerrain`]
    /// [`crate::tools::ToolState::road_action`] prepared from it; `None` for
    /// any other action, or one of another plane or layer.
    #[must_use]
    pub fn of(road: &Road, action: &Action) -> Option<Self> {
        let (Action::PaintStroke {
            plane,
            layer,
            cells,
            ..
        }
        | Action::ScatterTerrain {
            plane,
            layer,
            cells,
            ..
        }) = action
        else {
            return None;
        };
        (plane == road.plane() && *layer == road.layer()).then(|| Self {
            plane: plane.clone(),
            layer: *layer,
            cells: cells.clone(),
            first: road.points()[0],
        })
    }
    pub fn plane(&self) -> &str {
        &self.plane
    }
    pub fn layer(&self) -> i32 {
        self.layer
    }
    /// The cells the road's change recorded, which a grade covers.
    pub fn cells(&self) -> &[[i64; 2]] {
        &self.cells
    }
    /// The road's first clicked point.
    pub fn first(&self) -> [i64; 2] {
        self.first
    }
    /// The flatten target: the height of the road's first point in
    /// `committed` (the committed World), read once, now, as a Flatten
    /// stroke reads its start tile at the press
    /// ([`crate::sculpt::flatten_target`], the same rule and refusals, in
    /// words about the road).
    ///
    /// # Errors
    /// The refusal's words: no height grant, no chunk at the first point,
    /// or a height outside the granted range there.
    pub fn flatten_target(
        &self,
        committed: &World,
        catalog: &PreviewCatalog,
    ) -> Result<i32, String> {
        let [x, y] = self.first;
        let grant = catalog
            .editing()
            .heights
            .ok_or_else(|| GRADE_UNGRANTED.to_owned())?;
        let height = committed
            .planes()
            .get(&self.plane)
            .and_then(|plane| crate::heights::cell_height(plane, self.layer, x, y))
            .ok_or_else(|| {
                format!("Grade road refused: the road's first point ({x}, {y}) has no map chunk.")
            })?;
        if !grant.contains(height) {
            return Err(format!(
                "Grade road refused: the road's first point ({x}, {y}) has height {height}, outside the granted range {} to {}; a flatten target must lie inside it.",
                grant.min, grant.max
            ));
        }
        Ok(height)
    }
    /// The ordinary height edit `grade` prepares over the road's cells,
    /// against committed revision `expected`: a
    /// [`Action::FlattenHeights`] to [`Self::flatten_target`] or a
    /// [`Action::SmoothHeights`] with kernel
    /// [`crate::sculpt::SMOOTH_KERNEL`]. The engine checks it as it checks
    /// every height edit (the range, the view, the cell cap, nothing to
    /// change).
    ///
    /// # Errors
    /// [`Self::flatten_target`]'s refusals.
    pub fn grade(
        &self,
        grade: RoadGrade,
        committed: &World,
        catalog: &PreviewCatalog,
        expected: [u8; 32],
    ) -> Result<Action, String> {
        Ok(match grade {
            RoadGrade::Flatten => Action::FlattenHeights {
                plane: self.plane.clone(),
                layer: self.layer,
                cells: self.cells.clone(),
                target: self.flatten_target(committed, catalog)?,
                expected,
            },
            RoadGrade::Smooth => Action::SmoothHeights {
                plane: self.plane.clone(),
                layer: self.layer,
                cells: self.cells.clone(),
                kernel: crate::sculpt::SMOOTH_KERNEL,
                expected,
            },
        })
    }
}

/// The committed World revision and the active view (plane, layer,
/// bounds) a remembered road's cells belong to.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Anchor {
    revision: [u8; 32],
    view: (String, i32, [i64; 4]),
}

/// What the desktop remembers of the last road, through Review and
/// History: the road a change about to be prepared comes from (staged),
/// the road the pending change comes from, and the road whose change, or
/// a grade of it, was the last one accepted.
///
/// The remembered road is kept only while nothing could make its cells
/// stale or surprising: it is the road of the last accepted change (or of
/// a grade of it, which leaves the cells the road's), at the committed
/// revision and active view that Accept produced. Any other accepted
/// change, an accepted Undo or Redo (of the road, a grade or anything
/// else) drop it, and a committed revision or view that differs from its
/// own hides it on every read and forgets it for good at the next
/// [`Self::settle`]. Nothing else does: a Reopen, which only checks that
/// the files still hold the committed World, keeps it, and a change of
/// the World or catalog on disk pauses or blocks every edit, grades
/// included. Cancelling a pending change, the road's own or a
/// grade's, keeps it.
#[derive(Clone, Debug, Default)]
pub struct RoadMemory {
    staged: Option<BuiltRoad>,
    pending: Option<BuiltRoad>,
    accepted: Option<(BuiltRoad, Anchor)>,
}

/// What one dispatch did, for [`RoadMemory::settle`].
#[derive(Clone, Copy, Debug)]
pub struct Settled<'a> {
    /// Whether a change was pending before the dispatch.
    pub was_pending: bool,
    /// Whether a change is pending after it.
    pub pending: bool,
    /// The committed revision before the dispatch.
    pub before: [u8; 32],
    /// The committed revision after it.
    pub after: [u8; 32],
    /// The active view after it.
    pub view: (&'a str, i32, [i64; 4]),
}

impl RoadMemory {
    /// Name `road` as the origin of the change the next dispatch prepares:
    /// the road's own paint or scatter, or a grade of the remembered road.
    /// [`Self::settle`] consumes it whatever the dispatch does, and
    /// [`Self::unstage`] drops it when no dispatch follows.
    pub fn stage(&mut self, road: BuiltRoad) {
        self.staged = Some(road);
    }
    /// Drop a staged road that no dispatch consumed.
    pub fn unstage(&mut self) {
        self.staged = None;
    }
    /// Account for one dispatch (every edit, review and history action and
    /// a proposal's prepare goes through one): a commit (the committed
    /// revision moved) remembers the pending change's road, if it had
    /// one, and forgets the remembered road otherwise; a newly prepared
    /// change takes the staged road as its origin; no pending change
    /// leaves no pending road. Then a remembered road whose revision or
    /// view is not the current one is forgotten.
    pub fn settle(&mut self, settled: Settled<'_>) {
        let staged = self.staged.take();
        if settled.after != settled.before {
            let view = settled.view;
            self.accepted = self.pending.take().map(|road| {
                (
                    road,
                    Anchor {
                        revision: settled.after,
                        view: (view.0.to_owned(), view.1, view.2),
                    },
                )
            });
        }
        if settled.pending {
            if !settled.was_pending {
                self.pending = staged;
            }
        } else {
            self.pending = None;
        }
        if self
            .accepted
            .as_ref()
            .is_some_and(|(_, anchor)| !anchor.holds(settled.after, settled.view))
        {
            self.accepted = None;
        }
    }
    /// The remembered road, when its revision and view are `revision` and
    /// `view`: the committed World's and the active view's now.
    #[must_use]
    pub fn road(&self, revision: [u8; 32], view: (&str, i32, [i64; 4])) -> Option<&BuiltRoad> {
        self.accepted
            .as_ref()
            .filter(|(_, anchor)| anchor.holds(revision, view))
            .map(|(road, _)| road)
    }
    /// The road the pending change comes from, if it is one.
    #[must_use]
    pub fn pending(&self) -> Option<&BuiltRoad> {
        self.pending.as_ref()
    }
}

impl Anchor {
    fn holds(&self, revision: [u8; 32], (plane, layer, bounds): (&str, i32, [i64; 4])) -> bool {
        self.revision == revision
            && self.view.0 == plane
            && self.view.1 == layer
            && self.view.2 == bounds
    }
}

#[cfg(test)]
mod tests {
    use super::{BuiltRoad, Road, RoadMemory, Settled};
    use crate::Action;
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

    fn built(points: &[[i64; 2]]) -> BuiltRoad {
        let road = line(points);
        let action = road
            .stroke(sand(), Brush::default(), [0; 32], 1, [0, 0, 8, 6])
            .into_action()
            .unwrap();
        BuiltRoad::of(&road, &action).unwrap()
    }

    #[test]
    fn a_built_road_keeps_the_recorded_cells_and_the_first_point() {
        let road = line(&[[3, 1], [0, 1]]);
        let stroke = road.stroke(sand(), Brush::default(), [0; 32], 1, [0, 0, 8, 6]);
        let action = stroke.clone().into_action().unwrap();
        let built = BuiltRoad::of(&road, &action).unwrap();
        assert_eq!(built.cells(), stroke.cells().collect::<Vec<_>>());
        // The first clicked point, not the first cell in order.
        assert_eq!(built.first(), [3, 1]);
        assert_eq!(built.cells()[0], [0, 1]);
        // Only a paint or scatter of the road's own plane and layer.
        assert!(BuiltRoad::of(&road, &Action::Accept).is_none());
        let Action::PaintStroke {
            plane,
            layer,
            grid,
            cells,
            proto,
            expected,
        } = action
        else {
            panic!("a road paints");
        };
        let other = Action::PaintStroke {
            plane,
            layer: layer + 1,
            grid,
            cells,
            proto,
            expected,
        };
        assert!(BuiltRoad::of(&road, &other).is_none());
    }

    const VIEW: (&str, i32, [i64; 4]) = ("main", 0, [0, 0, 8, 6]);

    fn settled(was_pending: bool, pending: bool, before: u8, after: u8) -> Settled<'static> {
        Settled {
            was_pending,
            pending,
            before: [before; 32],
            after: [after; 32],
            view: VIEW,
        }
    }

    /// Prepare (staged or not), then accept: the memory after the commit.
    fn commit(memory: &mut RoadMemory, staged: Option<BuiltRoad>, from: u8, to: u8) {
        if let Some(road) = staged {
            memory.stage(road);
        }
        memory.settle(settled(false, true, from, from));
        memory.settle(settled(true, false, from, to));
    }

    #[test]
    fn the_remembered_road_lives_only_as_long_as_its_cells() {
        let road = built(&[[0, 1], [3, 1]]);
        let mut memory = RoadMemory::default();
        // Prepared and cancelled: nothing remembered, nothing pending.
        memory.stage(road.clone());
        memory.settle(settled(false, true, 1, 1));
        assert_eq!(memory.pending(), Some(&road));
        assert!(memory.road([1; 32], VIEW).is_none());
        memory.settle(settled(true, false, 1, 1));
        assert!(memory.pending().is_none());
        assert!(memory.road([1; 32], VIEW).is_none());
        // Accepted: remembered at the new revision and this view only.
        commit(&mut memory, Some(road.clone()), 1, 2);
        assert_eq!(memory.road([2; 32], VIEW), Some(&road));
        assert!(memory.road([1; 32], VIEW).is_none());
        assert!(memory.road([2; 32], ("main", 1, VIEW.2)).is_none());
        assert!(memory.road([2; 32], ("other", 0, VIEW.2)).is_none());
        assert!(memory.road([2; 32], ("main", 0, [0, 0, 8, 7])).is_none());
        // A grade prepared and cancelled keeps it; a grade accepted keeps
        // it at the grade's revision.
        memory.stage(road.clone());
        memory.settle(settled(false, true, 2, 2));
        memory.settle(settled(true, false, 2, 2));
        assert_eq!(memory.road([2; 32], VIEW), Some(&road));
        commit(&mut memory, Some(road.clone()), 2, 3);
        assert_eq!(memory.road([3; 32], VIEW), Some(&road));
        // A refused dispatch consumes the staged road: a later change
        // prepared without one is not the road's.
        memory.stage(built(&[[5, 5], [6, 5]]));
        memory.settle(settled(false, false, 3, 3));
        assert_eq!(memory.road([3; 32], VIEW), Some(&road));
        // Anything else accepted (an edit, an Undo, a Redo) drops it, and
        // it does not come back when the revision does.
        commit(&mut memory, None, 3, 2);
        assert!(memory.road([2; 32], VIEW).is_none());
        assert!(memory.road([3; 32], VIEW).is_none());
        commit(&mut memory, None, 2, 3);
        assert!(memory.road([3; 32], VIEW).is_none());
    }

    #[test]
    fn a_staged_road_never_attaches_to_a_change_already_pending() {
        let road = built(&[[0, 1], [3, 1]]);
        let mut memory = RoadMemory::default();
        // A stroke is pending; a road staged for a dispatch that the
        // pending change refuses stays unattached, and accepting the
        // stroke remembers nothing.
        memory.settle(settled(false, true, 1, 1));
        memory.stage(road.clone());
        memory.settle(settled(true, true, 1, 1));
        assert!(memory.pending().is_none());
        memory.settle(settled(true, false, 1, 2));
        assert!(memory.road([2; 32], VIEW).is_none());
        // An unstaged road is gone too.
        memory.stage(road.clone());
        memory.unstage();
        memory.settle(settled(false, true, 2, 2));
        assert!(memory.pending().is_none());
        // A commit seen with a revision or view that does not hold the
        // road forgets it at once.
        commit(&mut memory, Some(road.clone()), 2, 3);
        memory.settle(Settled {
            view: ("main", 0, [0, 0, 9, 9]),
            ..settled(false, false, 3, 3)
        });
        assert!(memory.road([3; 32], VIEW).is_none());
    }
}
