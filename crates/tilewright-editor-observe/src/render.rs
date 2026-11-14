// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `render`: a region of the catalog's view drawn by the CPU reference
//! renderer, the one every pick mask and the scripted demo's evidence
//! come from. This module only computes pixels: the binary encodes them as
//! a PNG and writes the one file `--out` names (see `docs/CLI.md`), and
//! nothing here opens, creates or writes a file.
//!
//! The image is the World scene only: ground and placed objects, as the
//! desktop editor draws them, with no cell overlay, zone tint, selection
//! or interface. The camera is the desktop's default view camera for the
//! region (centred on the region, art pixels at `--scale` screen pixels
//! each), and the image is that camera's viewport sized to hold
//! everything the region draws, centred on the region: what the desktop
//! shows of the same region, uncut.
use crate::cli::{EXIT_NOT_LOADED, Outcome, printed, refused};
use crate::limits::{
    MAXIMUM_RENDER_DIAGNOSTICS, MAXIMUM_RENDER_PIXELS, MAXIMUM_RENDER_SIDE, MAXIMUM_RENDER_TILES,
};
use crate::node::{Map, Node};
use crate::world::{Catalog, Observed, load_failure};
use std::ffi::OsString;
use std::path::Path;
use tilewright_editor_document::revision_hex;
use tilewright_editor_preview::{Extent, PreviewLimits, ReferenceFrame, ViewSpec};

pub const FORMAT: &str = "tilewright.editor-render/1";

/// The parsed arguments of `render`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub root: OsString,
    pub out: OsString,
    /// Tiles `[x, y, width, height]`, width and height at least 1; `None`
    /// is the whole view.
    pub region: Option<[i64; 4]>,
    /// Screen pixels per art pixel, 1 to `MAXIMUM_RENDER_SCALE`.
    pub scale: u32,
    pub json: bool,
}

/// A drawn region: its pixels, and the report to print once the binary has
/// encoded and written them.
pub struct Rendered {
    pub frame: ReferenceFrame,
    report: Map,
    json: bool,
}

impl Rendered {
    /// The success report, with the PNG's length and SHA-256 digest.
    pub fn finish(self, png_bytes: usize, sha256: &[u8; 32]) -> Outcome {
        let report = self.report.with(
            "png",
            Map::new()
                .count("bytes", png_bytes)
                .with("sha256", Node::Text(revision_hex(sha256))),
        );
        printed(&report, self.json, crate::cli::EXIT_SUCCESS)
    }
}

/// Opens the world of `request`; its load failure is the output (exit 3).
///
/// # Errors
/// The outcome to print when the world does not load.
pub fn open(request: &Request) -> Result<Observed, Outcome> {
    Observed::open(Path::new(&request.root)).map_err(|diagnostics| {
        let mut outcome = printed(&load_failure(&diagnostics), request.json, EXIT_NOT_LOADED);
        outcome.stderr = format!(
            "tilewright-editor: the world does not load: {} problem(s)\n",
            diagnostics.len()
        );
        outcome
    })
}

/// Side of the square viewport a region is first compiled in, to find what
/// it draws: the largest image side. The renderer keeps every quad that
/// reaches into a viewport with its full corners and drops the rest.
const MEASURE: u32 = MAXIMUM_RENDER_SIDE;

/// Why an image cannot be framed.
const TOO_LARGE: &str = "The image would exceed 8,192 pixels a side or 16,777,216 pixels.";

/// Draws `request`'s region of `observed`'s catalog view.
///
/// # Errors
/// The refusal to print (exit 4).
pub fn render(observed: &Observed, request: &Request) -> Result<Rendered, Outcome> {
    let json = request.json;
    let (catalog, resources) = match &observed.catalog {
        Catalog::Absent => {
            return Err(refused(
                "catalog.absent",
                "The world has no editor catalog (preview.jsonc), so it has no view to render.",
                json,
            ));
        }
        Catalog::Invalid(_) => {
            return Err(refused(
                "catalog.invalid",
                "The editor catalog does not load; lint reports why.",
                json,
            ));
        }
        Catalog::Loaded(loaded) => (&loaded.0, &loaded.1),
    };
    let (plane, layer, bounds) = catalog.view();
    let region = request.region.unwrap_or(bounds);
    let tiles = region_tiles(region).ok_or_else(|| {
        refused(
            "output.budget",
            "The region has more than 4,096 tiles.",
            json,
        )
    })?;
    if !inside(region, bounds) {
        return Err(refused(
            "target.unknown",
            "The region is not inside the catalog's view.",
            json,
        ));
    }
    let world = observed.world();
    let unprepared = || {
        refused(
            "preview.unprepared",
            "The view cannot be prepared for display; lint reports why.",
            json,
        )
    };
    let too_large = || refused("output.budget", TOO_LARGE, json);
    // First compile in the largest viewport to find what the region draws,
    // then again in a viewport that holds exactly that, centred on the same
    // point. Both viewports have even sides, so the camera moves every quad
    // by whole pixels between them and the second frame holds what the
    // first measured.
    let mut measuring = PreviewLimits::default();
    measuring.scene.maximum_pixels = u64::from(MEASURE) * u64::from(MEASURE);
    measuring.scene.maximum_raster_samples =
        measuring.scene.maximum_raster_samples.saturating_mul(4);
    let measured = tilewright_editor_preview::prepare(
        world,
        catalog,
        resources,
        &view(catalog, world, region, request.scale, [MEASURE, MEASURE]),
        measuring,
    )
    .map_err(|_| unprepared())?;
    let [width, height] = frame(measured.extent()).ok_or_else(too_large)?;
    let prepared = tilewright_editor_preview::prepare(
        world,
        catalog,
        resources,
        &view(catalog, world, region, request.scale, [width, height]),
        PreviewLimits::default(),
    )
    .map_err(|_| unprepared())?;
    let frame = prepared.reference_frame().map_err(|_| {
        refused(
            "render.failed",
            "The reference renderer could not draw the view.",
            json,
        )
    })?;
    let report = report(
        observed,
        request,
        (plane, layer, region, tiles),
        &frame,
        &prepared.diagnostics,
    );
    Ok(Rendered {
        frame,
        report,
        json,
    })
}

/// The success report without the PNG's facts, which
/// [`Rendered::finish`] adds.
fn report(
    observed: &Observed,
    request: &Request,
    (plane, layer, region, tiles): (&str, i32, [i64; 4], usize),
    frame: &ReferenceFrame,
    diagnostics: &[tilewright_editor_preview::PreviewDiagnostic],
) -> Map {
    observed
        .header(FORMAT)
        .with("catalog", observed.catalog_status(true))
        .with(
            "image",
            Map::new()
                .text("plane", plane)
                .with("layer", layer)
                .with("region", Node::List(region.map(Node::Integer).to_vec()))
                .count("tiles", tiles)
                .with("scale", request.scale)
                .with("width", frame.width)
                .with("height", frame.height),
        )
        .bounded(
            "diagnostics",
            diagnostics,
            MAXIMUM_RENDER_DIAGNOSTICS,
            |diagnostic| {
                Map::new()
                    .with("code", Node::Text(diagnostic.code.into()))
                    .text("message", &diagnostic.message)
                    .into()
            },
        )
}

/// The number of tiles of `region`, if it is at most
/// [`MAXIMUM_RENDER_TILES`]. Width and height are at least 1.
fn region_tiles(region: [i64; 4]) -> Option<usize> {
    region[2]
        .checked_mul(region[3])
        .and_then(|tiles| usize::try_from(tiles).ok())
        .filter(|tiles| *tiles <= MAXIMUM_RENDER_TILES)
}

/// Whether tile rectangle `region` lies inside `bounds`, both
/// `[x, y, width, height]` with positive sizes.
fn inside(region: [i64; 4], bounds: [i64; 4]) -> bool {
    let end = |rectangle: [i64; 4], axis: usize| rectangle[axis].checked_add(rectangle[axis + 2]);
    (0..2).all(|axis| {
        region[axis] >= bounds[axis]
            && matches!((end(region, axis), end(bounds, axis)), (Some(inner), Some(outer)) if inner <= outer)
    })
}

/// The desktop's default view of the catalog, narrowed to `region`: the
/// camera centred on it at zoom `scale`, with a `size` viewport.
fn view(
    catalog: &tilewright_editor_preview::PreviewCatalog,
    world: &tilewright_source::World,
    region: [i64; 4],
    scale: u32,
    size: [u32; 2],
) -> ViewSpec {
    let mut view = catalog.default_view(world, size[0], size[1], 1);
    view.bounds = region;
    #[allow(clippy::cast_precision_loss)]
    let center = [
        region[0] as f64 + region[2] as f64 / 2.0,
        region[1] as f64 + region[3] as f64 / 2.0,
    ];
    view.camera.center = center;
    #[allow(clippy::cast_precision_loss)]
    let zoom = scale as f32;
    view.camera.zoom = zoom;
    view
}

/// The image size for what a region draws in the measuring viewport: the
/// smallest viewport with even sides, centred where the measuring one is,
/// that holds every quad corner, at least 2 × 2. `None` when an identity
/// drew nothing there (so it may lie wholly outside it), or the size is
/// past the image bounds.
fn frame(extent: Extent) -> Option<[u32; 2]> {
    if extent.undrawn > 0 {
        return None;
    }
    let center = f64::from(MEASURE / 2);
    let half = |low: f32, high: f32| -> Option<u32> {
        let reach = (center - f64::from(low).floor()).max(f64::from(high).ceil() - center);
        // Within the image bounds, so the cast is exact.
        (reach <= f64::from(MAXIMUM_RENDER_SIDE / 2)).then(|| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let reach = reach.max(1.0) as u32;
            reach
        })
    };
    let [width, height] = match extent.bounds {
        None => [2, 2],
        Some([left, top, right, bottom]) => [2 * half(left, right)?, 2 * half(top, bottom)?],
    };
    fits(width, height).then_some([width, height])
}

/// Whether a `width` × `height` image is within the image bounds.
const fn fits(width: u32, height: u32) -> bool {
    width <= MAXIMUM_RENDER_SIDE
        && height <= MAXIMUM_RENDER_SIDE
        && (width as u64) * (height as u64) <= MAXIMUM_RENDER_PIXELS
}

#[cfg(test)]
mod tests {
    use super::{Extent, MEASURE, fits, frame, inside, region_tiles};

    #[test]
    fn the_tile_bound_is_inclusive_and_overflow_is_refused() {
        assert_eq!(region_tiles([0, 0, 64, 64]), Some(4_096));
        assert_eq!(region_tiles([0, 0, 4_096, 1]), Some(4_096));
        assert_eq!(region_tiles([0, 0, 4_097, 1]), None);
        assert_eq!(region_tiles([0, 0, 1, 4_097]), None);
        assert_eq!(region_tiles([0, 0, i64::MAX, 2]), None);
        assert_eq!(
            region_tiles([0, 0, 4_096, 1]),
            Some(tilewright_editor_preview::PreviewLimits::default().maximum_cells)
        );
    }

    #[test]
    fn a_region_is_inside_only_when_every_edge_is() {
        let bounds = [-2, 3, 8, 6];
        assert!(inside(bounds, bounds));
        assert!(inside([-2, 3, 1, 1], bounds));
        assert!(inside([5, 8, 1, 1], bounds));
        for region in [
            [-3, 3, 1, 1],
            [-2, 2, 1, 1],
            [6, 3, 1, 1],
            [-2, 9, 1, 1],
            [-2, 3, 9, 6],
            [-2, 3, 8, 7],
            [i64::MAX, 3, 1, 1],
        ] {
            assert!(!inside(region, bounds), "{region:?}");
        }
        assert!(!inside([0, 0, 1, 1], [i64::MAX - 1, 0, 2, 1]));
    }

    #[test]
    fn the_pixel_bounds_are_inclusive() {
        assert!(fits(8_192, 2_048));
        assert!(fits(2_048, 8_192));
        assert!(fits(4_096, 4_096));
        assert!(!fits(8_194, 2));
        assert!(!fits(2, 8_194));
        assert!(!fits(8_192, 2_050));
        assert!(!fits(4_098, 4_096));
    }

    fn extent(bounds: [f32; 4]) -> Extent {
        Extent {
            bounds: Some(bounds),
            undrawn: 0,
        }
    }

    #[test]
    fn the_frame_is_centred_even_and_holds_every_corner() {
        let center = f32::from(u16::try_from(MEASURE / 2).unwrap());
        // 10 pixels left and 30 right of the centre, 5 up and 3.5 down.
        let measured = extent([center - 10.0, center - 5.0, center + 30.0, center + 3.5]);
        assert_eq!(frame(measured), Some([60, 10]));
        // A fraction past a pixel edge takes the whole pixel.
        let measured = extent([center - 10.25, center - 1.0, center + 1.0, center + 1.0]);
        assert_eq!(frame(measured), Some([22, 2]));
        assert_eq!(
            frame(extent([center, center, center, center])),
            Some([2, 2])
        );
        assert_eq!(
            frame(Extent {
                bounds: None,
                undrawn: 0
            }),
            Some([2, 2])
        );
        // Everything of the measuring viewport fits exactly; a pixel past
        // its edge does not.
        let whole = f32::from(u16::try_from(MEASURE).unwrap());
        assert_eq!(
            frame(extent([0.0, center - 1.0, whole, center + 1.0])),
            Some([8_192, 2])
        );
        assert_eq!(frame(extent([-0.5, center, 1.0, center])), None);
        assert_eq!(frame(extent([center, center, whole + 0.5, center])), None);
        // Within each side but past the pixel bound.
        assert_eq!(frame(extent([0.0, 0.0, whole, 2.0 * center])), None);
        assert_eq!(
            frame(extent([0.0, center - 1_024.0, whole, center + 1_024.0])),
            Some([8_192, 2_048])
        );
        assert_eq!(
            frame(extent([0.0, center - 1_025.0, whole, center + 1_024.0])),
            None
        );
        // An identity with no quad in the measuring viewport is refused.
        assert_eq!(
            frame(Extent {
                bounds: Some([center, center, center, center]),
                undrawn: 1
            }),
            None
        );
    }
}
