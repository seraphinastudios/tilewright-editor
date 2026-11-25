// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `lint`: the editor's own findings about a world that loads. World's
//! diagnostics come with a world that does not load (see
//! [`crate::world::load_failure`]); everything here is computed by the
//! editor's existing checks and the shared renderer's own compile, with no
//! analysis of its own. Reachability is deliberately left out until the
//! profile can say what blocks movement.
use crate::limits::MAXIMUM_FINDINGS;
use crate::node::{Bounded, Map, Node, Pager};
use crate::world::{Catalog, Observed};
use std::collections::BTreeSet;
use tilewright_editor_preview::{BendLimit, HeightGrant, LARGEST_ZOOM, PreviewLimits};
use tilewright_editor_ui::{anchors, portals, zones};
use tilewright_source::World;

pub const FORMAT: &str = "tilewright.editor-lint/1";

/// How much a finding matters. Only errors change the exit code.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    /// The desktop editor's editing entry point (`prototype`, started by
    /// the provisioning helper) cannot open or show the world as it is.
    /// `open ROOT` and these read-only commands only need World to load it.
    Error,
    /// Probably a mistake, but the editing entry point opens the world.
    Warning,
    /// Worth knowing; nothing is wrong.
    Info,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

/// One finding: a stable code, a severity, where, and a message this
/// editor wrote. `detail` is text from elsewhere (a loader message), kept
/// apart from the message because it may quote world text.
#[derive(Clone)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub location: Map,
    pub message: String,
    pub detail: Option<String>,
}

impl Finding {
    fn node(&self) -> Node {
        Map::new()
            .with("code", Node::Text(self.code.into()))
            .with("severity", Node::Text(self.severity.as_str().into()))
            .with("location", self.location.clone())
            .text("message", &self.message)
            .optional_text("detail", self.detail.as_deref())
            .into()
    }
}

/// The finding for a world the editing entry point's load budget refuses,
/// with the World diagnostic codes it refused with. Its text is all this
/// editor's: the budget's numbers, never world text.
fn over_budget(codes: &[&'static str]) -> Finding {
    let budget = tilewright_editor_document::prototype_load_options();
    Finding {
        code: "world.over_editor_budget",
        severity: Severity::Error,
        location: Map::new().ids("codes", codes.iter().copied()),
        message: format!(
            "The editing entry point (prototype) refuses this world: its load budget is {} files, {} directory entries and {} bytes, and per document {} bytes, {} values and {} array items.",
            budget.maximum_files,
            budget.maximum_entries,
            budget.maximum_total_bytes,
            budget.limits.max_input_bytes,
            budget.limits.max_values,
            budget.limits.max_array_items,
        ),
        detail: None,
    }
}

/// The finding for a catalog with `resources` project-art resources, if
/// any. Its text is all this editor's: a count, never world text.
fn project_art(resources: usize) -> Option<Finding> {
    (resources > 0).then(|| Finding {
        code: "catalog.project_art",
        severity: Severity::Info,
        location: Map::new().text("path", "preview.jsonc"),
        message: format!(
            "The editor catalog has {resources} resource(s) with the license project: the project's own art, under its own license. Every image the editor makes of this world (evidence frames, render files and MCP render images) carries a visible mark, and its record a project_art member; check the art's license before sharing them."
        ),
        detail: None,
    })
}

/// The finding for a catalog whose height grant lets neighbouring heights
/// lie further apart than its ground art can be bent over at the largest
/// zoom ([`tilewright_editor_preview::bend_limit`]), if it does. Its text
/// is all this editor's: numbers, never world text; the resources' ids are
/// in its location.
fn undrawable_heights(observed: &Observed) -> Option<Finding> {
    let Catalog::Loaded(loaded) = &observed.catalog else {
        return None;
    };
    let (catalog, resources) = &**loaded;
    let grant = catalog.editing().heights?;
    // Tiles without a height have 0, so 0 is always one of the heights.
    let low = grant.min.min(0);
    let high = grant.max.max(0);
    let difference = high.abs_diff(low);
    let limit = tilewright_editor_preview::bend_limit(
        observed.world(),
        catalog,
        resources,
        difference,
        PreviewLimits::default(),
    )?;
    Some(Finding {
        code: "catalog.heights_undrawable",
        severity: Severity::Warning,
        location: Map::new()
            .text("path", "preview.jsonc")
            .ids("resources", limit.resources.iter().map(String::as_str)),
        message: undrawable_message(grant, difference, &limit),
        detail: None,
    })
}

/// The message, at most 256 characters with any numbers the catalog can
/// hold, so that it is never clipped.
fn undrawable_message(grant: HeightGrant, difference: u32, limit: &BendLimit) -> String {
    let BendLimit {
        largest_difference: largest,
        zoom,
        width,
        height,
        ..
    } = *limit;
    let art = if limit.resources.is_empty() {
        "the tile-picking art".to_owned()
    } else {
        format!("{width} x {height} ground art")
    };
    // The zoom rounded down to hundredths: the verdict that ranges within
    // the limit bend holds at every lower zoom. Art too large to draw flat
    // at the largest zoom is judged at the largest at which it draws flat,
    // just above the rounded value.
    #[allow(clippy::float_cmp)]
    let (at, shown) = if zoom == LARGEST_ZOOM {
        (format!("at zoom {zoom} (the largest)"), format!("{zoom}"))
    } else {
        let down = (f64::from(zoom) * 100.0).floor() / 100.0;
        let shown = format!("{down:.2}");
        let near = if down == f64::from(zoom) {
            "at"
        } else {
            "just above"
        };
        (
            format!("{near} zoom {shown} (its largest flat zoom)"),
            shown,
        )
    };
    let head = format!(
        "Heights {} to {} (and 0) can differ by {difference}, but {at} the preview cannot bend {art} over more than {largest}.",
        grant.min, grant.max,
    );
    if largest == 0 {
        return format!("{head} No height range bends there.");
    }
    // An example range as near the grant as it can be: as wide as is safe,
    // with 0 in it.
    let largest = i32::try_from(largest).unwrap_or(i32::MAX);
    let mut from = grant.min.min(0).max(-(largest / 2));
    let mut to = from + largest;
    if to > grant.max.max(0) {
        to = grant.max.max(0);
        from = to - largest;
    }
    format!(
        "{head} Per tile, {from} to {to} bends up to zoom {shown}; a whole view can exceed the frame budget."
    )
}

/// Every finding about `observed`, in a fixed order: the catalog, the
/// prepared view, portals in World order, then anchors and zones by plane
/// and id.
pub fn findings(observed: &Observed) -> Vec<Finding> {
    let world = observed.world();
    let mut found = Vec::new();
    if let Some(codes) = &observed.over_editor_budget {
        found.push(over_budget(codes));
    }
    match &observed.catalog {
        Catalog::Absent => found.push(Finding {
            code: "catalog.absent",
            severity: Severity::Error,
            location: Map::new().text("path", "preview.jsonc"),
            message: "The world has no editor catalog, so the desktop editor's editing entry point cannot open it; open and the read-only commands still read it.".into(),
            detail: None,
        }),
        Catalog::Invalid(problem) => found.push(Finding {
            code: "catalog.invalid",
            severity: Severity::Error,
            location: Map::new().text("path", "preview.jsonc"),
            message:
                "The editor catalog does not load, so the editing entry point cannot open this world."
                    .into(),
            detail: Some(problem.clone()),
        }),
        Catalog::Loaded(loaded) => {
            let (catalog, resources) = &**loaded;
            found.extend(project_art(catalog.project_art_resources()));
            found.extend(undrawable_heights(observed));
            let (plane, layer, _) = catalog.view();
            let view = || Map::new().text("plane", plane).with("layer", layer);
            // The view the desktop editor opens, prepared as it prepares it.
            let spec = catalog.default_view(world, 1280, 800, 1);
            match tilewright_editor_preview::prepare(
                world,
                catalog,
                resources,
                &spec,
                PreviewLimits::default(),
            ) {
                Ok(prepared) => {
                    for diagnostic in prepared.diagnostics {
                        found.push(Finding {
                            code: diagnostic.code,
                            severity: Severity::Info,
                            location: view(),
                            message: diagnostic.message,
                            detail: None,
                        });
                    }
                }
                Err(error) => found.push(Finding {
                    code: "preview.unprepared",
                    severity: Severity::Error,
                    location: view(),
                    message: "The catalog's view cannot be prepared for display, so the desktop editor's editing entry point cannot open this world.".into(),
                    detail: Some(error.to_string()),
                }),
            }
        }
    }
    portal_findings(observed, &mut found);
    let holders = anchors::portals(world);
    let named: BTreeSet<(&str, &str)> = holders
        .iter()
        .filter_map(|holder| match holder.target {
            anchors::Target::Anchor { plane, anchor } => Some((plane, anchor)),
            anchors::Target::Location { .. } => None,
        })
        .collect();
    for (plane_id, plane) in world.planes() {
        for (id, anchor) in &plane.anchors {
            if !named.contains(&(plane_id.as_str(), id.as_str())) {
                found.push(Finding {
                    code: "anchor.unreferenced",
                    severity: Severity::Info,
                    location: Map::new()
                        .text("plane", plane_id)
                        .text("anchor", id)
                        .with("layer", anchor.layer)
                        .with("at", anchor.at),
                    message: "No portal names this anchor; scripts or saves outside the world may still use its id.".into(),
                    detail: None,
                });
            }
        }
    }
    for (plane_id, plane) in world.planes() {
        for zone in zones::unused_zones(plane) {
            found.push(Finding {
                code: "zone.unused",
                severity: Severity::Info,
                location: Map::new().text("plane", plane_id).text("zone", zone),
                message: "No tile, chunk palette or child zone uses this zone and it is not the plane's default zone, so it could be deleted.".into(),
                detail: None,
            });
        }
    }
    found
}

/// The review's warnings about where each portal leads
/// ([`portals::check_target`]), for every portal in the world.
fn portal_findings(observed: &Observed, found: &mut Vec<Finding>) {
    let world = observed.world();
    let catalog = observed.catalog();
    let grid = catalog.map_or_else(
        || world.profile().ground_layers.first().map(String::as_str),
        |catalog| portals::pick_grid(world, catalog),
    );
    for holder in anchors::portals(world) {
        let check = portals::check_target(world, grid, None, &holder);
        let location = || holder_location(world, &holder);
        if check.own_tile {
            found.push(Finding {
                code: "portal.target_own_tile",
                severity: Severity::Warning,
                location: location(),
                message: "The portal leads to the tile its object stands on.".into(),
                detail: None,
            });
        }
        let Some(target) = check.location else {
            continue;
        };
        if target.missing_chunk {
            found.push(Finding {
                code: "portal.target_no_chunk",
                severity: Severity::Warning,
                location: location(),
                message: "The portal's location target is in a map chunk that does not exist; World does not check portal targets.".into(),
                detail: None,
            });
        }
        if let Some(grid) = target.missing_ground {
            found.push(Finding {
                code: "portal.target_no_ground",
                severity: Severity::Warning,
                location: location().text("grid", grid),
                message: "The portal's location target has no ground on the pick grid; World does not check portal targets.".into(),
                detail: None,
            });
        }
    }
}

/// Where a portal's object is: its chunk's path, plane, layer, tile, uid,
/// and whether it is carried in an inventory.
fn holder_location(world: &World, holder: &anchors::PortalHolder<'_>) -> Map {
    let path = world.planes().get(holder.plane).and_then(|plane| {
        let size = i64::from(plane.chunk_size.max(1));
        let [x, y] = holder.tile?;
        plane
            .chunks
            .get(&(holder.layer, x.div_euclid(size), y.div_euclid(size)))
            .map(|chunk| chunk.path.as_str())
    });
    Map::new()
        .optional_text("path", path)
        .text("plane", holder.plane)
        .with("layer", holder.layer)
        .with("tile", holder.tile.map_or(Node::Null, Node::from))
        .optional_text("uid", holder.uid)
        .with("carried", holder.depth > 0)
}

/// The pageable lists of the report, by their path in it.
pub const SECTIONS: [&str; 1] = ["findings"];

/// The lint report as the CLI prints it, and whether it has an error.
pub fn report(observed: &Observed) -> (Map, bool) {
    report_paged(observed, &findings(observed), &Bounded)
}

/// The lint report of `found`, the [`findings`] about `observed`, with
/// its [`SECTIONS`] listed as `pager` decides, and whether it has an
/// error.
pub fn report_paged(observed: &Observed, found: &[Finding], pager: &dyn Pager) -> (Map, bool) {
    let count = |severity: Severity| {
        found
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };
    let errors = count(Severity::Error);
    let counts = Map::new()
        .count("error", errors)
        .count("warning", count(Severity::Warning))
        .count("info", count(Severity::Info));
    (
        observed
            .header(FORMAT)
            .with("catalog", observed.catalog_status(false))
            .with("counts", counts)
            .paged(
                pager,
                "findings",
                "findings",
                found,
                MAXIMUM_FINDINGS,
                Finding::node,
            ),
        errors > 0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::MAXIMUM_TEXT_CHARACTERS;

    #[test]
    fn the_heights_message_is_never_clipped() {
        // The longest numbers a catalog and its art can give.
        let limit = |resources: Vec<String>, largest, zoom| BendLimit {
            largest_difference: largest,
            zoom,
            width: 8192,
            height: 8192,
            diamond: [8192, 8192],
            resources,
        };
        let grant = HeightGrant {
            min: -1024,
            max: -1023,
        };
        for largest in [0, 1, 1023, 2047] {
            // Art is at most 8,192 pixels on a side, so it draws flat from zoom
            // 8,191 / 8,192 (a few steps below at most).
            for zoom in [LARGEST_ZOOM, 7.999_023, 7.5, 0.999_877_9, 0.999_871_9] {
                for resources in [Vec::new(), vec!["art".to_owned()]] {
                    let message = undrawable_message(grant, 2048, &limit(resources, largest, zoom));
                    assert!(
                        message.chars().count() <= MAXIMUM_TEXT_CHARACTERS,
                        "{}: {message}",
                        message.chars().count()
                    );
                }
            }
        }
    }

    #[test]
    fn a_limit_of_0_offers_no_range() {
        let limit = BendLimit {
            largest_difference: 0,
            zoom: LARGEST_ZOOM,
            width: 100,
            height: 300,
            diamond: [3, 2],
            resources: vec!["art".to_owned()],
        };
        let message = undrawable_message(HeightGrant { min: 0, max: 1 }, 1, &limit);
        let mut lower = limit.clone();
        lower.zoom = 7.446_363_4;
        lower.largest_difference = 11;
        assert_eq!(
            undrawable_message(HeightGrant { min: 0, max: 20 }, 20, &lower),
            "Heights 0 to 20 (and 0) can differ by 20, but just above zoom 7.44 (its largest flat zoom) the preview cannot bend 100 x 300 ground art over more than 11. Per tile, 0 to 11 bends up to zoom 7.44; a whole view can exceed the frame budget."
        );
        lower.zoom = 7.5;
        assert!(
            undrawable_message(HeightGrant { min: 0, max: 20 }, 20, &lower)
                .contains("but at zoom 7.50 (its largest flat zoom)")
        );
        assert_eq!(
            message,
            "Heights 0 to 1 (and 0) can differ by 1, but at zoom 8 (the largest) the preview cannot bend 100 x 300 ground art over more than 0. No height range bends there."
        );
    }
}
