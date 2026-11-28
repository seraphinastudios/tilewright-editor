// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Capture from the map (world-scale tools decision 4, the third stamps
//! pull request): a rectangle of tiles dragged in Place mode is copied into
//! a session clipboard, and the copy is then stamped like a catalog stamp,
//! as one [`crate::Action::PlaceObjects`] (ADR 0009). The copy is
//! "copied, not named" like every stamp, and records each object's
//! **prototype and position only** (owner decision 3, 2026-11-27): the
//! objects it puts down get the grant's render layer and default fields,
//! and none of the originals' own field values, layering or portal links.
//!
//! The rules, all here so the desktop and its tests share them:
//!
//! - **The rectangle** spans the two corner tiles of the drag, both
//!   included, and is clipped to the active view (the catalog's plane,
//!   spatial layer and bounds). Its tile with the smallest `x` and `y`
//!   (top-left top-down, the top corner in an isometric view) is the
//!   copy's anchor: every offset is from it, so all are `>= 0`.
//! - **What counts** is every top-level object placed on a tile of the
//!   rectangle, on the view's plane and spatial layer. Objects of other
//!   layers or planes, inventory entries and tiles outside the view never
//!   count.
//! - **Order:** tile by tile, row by row (`y`, then `x`), and on one tile
//!   in its stacking order (the chunk's list order), so a stamped copy
//!   stacks each tile's objects as they were.
//! - **Refusals** copy nothing and leave the clipboard as it was: no
//!   object ([`CaptureRefusal::Empty`]), more than
//!   [`MAXIMUM_STAMP_OBJECTS`] ([`CaptureRefusal::TooMany`]), or any
//!   prototype `editing.objects` does not grant
//!   ([`CaptureRefusal::Ungranted`], naming them). Grants cannot change
//!   within a session (a changed catalog pauses editing), so a copy with
//!   an ungranted prototype could never be stamped: the engine would
//!   refuse every click (`grant.missing`). Refusing the copy itself, with
//!   the names, says so once and before any click, and leaves no row that
//!   can never work, as catalog stamps are refused at load (ADR 0010).
use std::collections::BTreeSet;
use std::fmt;

use tilewright_editor_preview::PreviewCatalog;
use tilewright_source::World;

use crate::stamp::{MAXIMUM_STAMP_OBJECTS, StampObject};

/// Most ungranted prototypes a refusal names; the rest are counted.
pub const NAMED_UNGRANTED: usize = 4;

/// A rectangle of plane tiles of the active view, both corners included.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Area {
    /// The tile with the smallest `x` and `y`: the copy's anchor.
    pub from: [i64; 2],
    /// The tile with the largest `x` and `y`.
    pub to: [i64; 2],
}

impl Area {
    /// The rectangle between corner tiles `a` and `b` (in any order),
    /// clipped to the active view's bounds; `None` when none of its tiles
    /// is in the view.
    pub fn between(catalog: &PreviewCatalog, a: [i64; 2], b: [i64; 2]) -> Option<Self> {
        let (_, _, [min_x, min_y, width, height]) = catalog.view();
        let max_x = min_x.checked_add(width)?.checked_sub(1)?;
        let max_y = min_y.checked_add(height)?.checked_sub(1)?;
        let from = [a[0].min(b[0]).max(min_x), a[1].min(b[1]).max(min_y)];
        let to = [a[0].max(b[0]).min(max_x), a[1].max(b[1]).min(max_y)];
        (from[0] <= to[0] && from[1] <= to[1]).then_some(Self { from, to })
    }
    /// Its width and height in tiles.
    pub fn size(self) -> [i64; 2] {
        [self.to[0] - self.from[0] + 1, self.to[1] - self.from[1] + 1]
    }
    /// Whether plane tile `at` is in it.
    pub fn contains(self, at: [i64; 2]) -> bool {
        (self.from[0]..=self.to[0]).contains(&at[0]) && (self.from[1]..=self.to[1]).contains(&at[1])
    }
    /// Its tiles, row by row.
    pub fn tiles(self) -> impl Iterator<Item = [i64; 2]> {
        (self.from[1]..=self.to[1])
            .flat_map(move |y| (self.from[0]..=self.to[0]).map(move |x| [x, y]))
    }
}

/// What a rectangle holds: its objects as a copy would record them, in
/// capture order with offsets from [`Area::from`], and the prototypes among
/// them `editing.objects` does not grant, in id order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Contents {
    pub objects: Vec<StampObject>,
    pub ungranted: Vec<String>,
}

impl Contents {
    /// The rectangle's objects in the committed `world` (see the module
    /// notes for which objects count and their order).
    pub fn of(world: &World, catalog: &PreviewCatalog, area: Area) -> Self {
        let (plane_id, layer, _) = catalog.view();
        let Some(plane) = world.planes().get(plane_id) else {
            return Self::default();
        };
        let size = i64::from(plane.chunk_size);
        if size <= 0 {
            return Self::default();
        }
        let chunks = |from: i64, to: i64| from.div_euclid(size)..=to.div_euclid(size);
        // Each counted object's plane tile and prototype.
        let mut found: Vec<([i64; 2], &str)> = Vec::new();
        for cy in chunks(area.from[1], area.to[1]) {
            for cx in chunks(area.from[0], area.to[0]) {
                let Some(chunk) = plane.chunks.get(&(layer, cx, cy)) else {
                    continue;
                };
                for object in &chunk.objects {
                    let Some([lx, ly]) = object.at else {
                        continue;
                    };
                    let tile = [cx * size + i64::from(lx), cy * size + i64::from(ly)];
                    if area.contains(tile) {
                        found.push((tile, &object.proto));
                    }
                }
            }
        }
        // Row by row; a stable sort keeps each tile's stacking order, since
        // a tile's objects all come from one chunk in list order.
        found.sort_by_key(|([x, y], _)| (*y, *x));
        let grants = &catalog.editing().objects;
        let ungranted: BTreeSet<&str> = found
            .iter()
            .map(|(_, proto)| *proto)
            .filter(|proto| !grants.contains_key(*proto))
            .collect();
        Self {
            objects: found
                .into_iter()
                .map(|(tile, proto)| StampObject {
                    proto: proto.to_owned(),
                    offset: [tile[0] - area.from[0], tile[1] - area.from[1]],
                })
                .collect(),
            ungranted: ungranted.into_iter().map(str::to_owned).collect(),
        }
    }
    /// Why a copy of these contents is refused, if it is.
    pub fn refusal(&self) -> Option<CaptureRefusal> {
        if self.objects.is_empty() {
            Some(CaptureRefusal::Empty)
        } else if self.objects.len() > MAXIMUM_STAMP_OBJECTS {
            Some(CaptureRefusal::TooMany(self.objects.len()))
        } else if !self.ungranted.is_empty() {
            Some(CaptureRefusal::Ungranted(self.ungranted.clone()))
        } else {
            None
        }
    }
}

/// Why a rectangle is not copied. Nothing is copied then and the clipboard
/// keeps what it held.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureRefusal {
    /// No object counts in the rectangle.
    Empty,
    /// More objects than one stamp places: how many there are.
    TooMany(usize),
    /// The prototypes Place may not put down, in id order.
    Ungranted(Vec<String>),
}

impl CaptureRefusal {
    /// The short form for the status line while the rectangle is dragged:
    /// what release would refuse.
    pub fn short(&self) -> String {
        match self {
            Self::Empty => "no objects".into(),
            Self::TooMany(count) => {
                format!("{count} objects, more than {MAXIMUM_STAMP_OBJECTS}")
            }
            Self::Ungranted(protos) => format!("not granted: {}", named(protos)),
        }
    }
}

impl fmt::Display for CaptureRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "Nothing copied: the rectangle holds no object on the view's plane and layer.",
            ),
            Self::TooMany(count) => write!(
                f,
                "Nothing copied: the rectangle holds {count} objects, and a copy holds at most {MAXIMUM_STAMP_OBJECTS}."
            ),
            Self::Ungranted(protos) => write!(
                f,
                "Nothing copied: Place cannot put down {} (not granted by editing.objects).",
                named(protos)
            ),
        }
    }
}

/// The first [`NAMED_UNGRANTED`] prototypes by name, then how many more.
fn named(protos: &[String]) -> String {
    let shown = protos[..protos.len().min(NAMED_UNGRANTED)].join(", ");
    match protos.len().saturating_sub(NAMED_UNGRANTED) {
        0 => shown,
        more => format!("{shown} and {more} more"),
    }
}

/// Copy area in Place mode: armed (the next press on a tile starts the
/// rectangle), or the rectangle being dragged from `start` to `end`, the
/// last tile of the view the pointer was over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureGesture {
    Armed,
    Dragging { start: [i64; 2], end: [i64; 2] },
}

/// A copy captured from the map: 1 to [`MAXIMUM_STAMP_OBJECTS`] granted
/// prototypes with their offsets from the rectangle's anchor tile, and the
/// rectangle it came from. It lives only in the session's tool state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedCopy {
    objects: Vec<StampObject>,
    area: Area,
}

impl CapturedCopy {
    /// Copy `area` of the committed `world`, or say why not.
    ///
    /// # Errors
    /// [`Contents::refusal`].
    pub fn capture(
        world: &World,
        catalog: &PreviewCatalog,
        area: Area,
    ) -> Result<Self, CaptureRefusal> {
        let contents = Contents::of(world, catalog, area);
        match contents.refusal() {
            Some(refusal) => Err(refusal),
            None => Ok(Self {
                objects: contents.objects,
                area,
            }),
        }
    }
    /// The objects in capture order, with offsets from the anchor, as
    /// copied (before any turn or mirror).
    pub fn objects(&self) -> &[StampObject] {
        &self.objects
    }
    /// The rectangle it was copied from.
    pub fn area(&self) -> Area {
        self.area
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureRefusal, named};

    #[test]
    fn a_refusal_names_four_prototypes_and_counts_the_rest() {
        let protos: Vec<String> = ["a", "b", "c", "d", "e", "f"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(named(&protos[..1]), "a");
        assert_eq!(named(&protos[..4]), "a, b, c, d");
        assert_eq!(named(&protos), "a, b, c, d and 2 more");
        assert_eq!(
            CaptureRefusal::Ungranted(protos[..2].to_vec()).to_string(),
            "Nothing copied: Place cannot put down a, b (not granted by editing.objects)."
        );
        assert_eq!(
            CaptureRefusal::TooMany(65).short(),
            "65 objects, more than 64"
        );
    }
}
