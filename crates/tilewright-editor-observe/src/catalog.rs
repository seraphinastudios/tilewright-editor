// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `catalog`: the editor catalog's prototypes and the terrain and zone
//! palettes, optionally filtered as the desktop Place palette filters, and
//! the scatter palettes (`editing.palettes`) when the catalog has any, and
//! the granted height range (`editing.heights`) when it has one.
use crate::limits::{MAXIMUM_FIELDS, MAXIMUM_PROTOTYPES, MAXIMUM_TERRAIN_CHOICES, MAXIMUM_ZONES};
use crate::node::{Bounded, Map, Node, Pager};
use crate::world::Observed;
use tilewright_editor_preview::{MAXIMUM_PALETTE_ENTRIES, MAXIMUM_PALETTES};
use tilewright_editor_ui::{
    tools::{TerrainName, palette_matches, terrain_name},
    zones,
};

pub const FORMAT: &str = "tilewright.editor-catalog/1";

/// The catalog listing. `filter` keeps what contains it, ignoring case, as
/// [`palette_matches`] decides: prototype ids, terrain values and zone ids.
pub fn report(observed: &Observed, filter: Option<&str>) -> Map {
    report_paged(observed, filter, &Bounded)
}

/// The pageable lists of the report, by their path in it. `palettes` is
/// in the report only when the catalog has `editing.palettes`.
pub const SECTIONS: [&str; 4] = ["prototypes", "terrain_palette", "zone_palette", "palettes"];

/// [`report`] with its [`SECTIONS`] listed as `pager` decides.
pub fn report_paged(observed: &Observed, filter: Option<&str>, pager: &dyn Pager) -> Map {
    let world = observed.world();
    let keep = |id: &str| filter.is_none_or(|filter| palette_matches(id, filter));
    let header = observed
        .header(FORMAT)
        .with("catalog", observed.catalog_status(true))
        .optional_text("match", filter);
    let Some(catalog) = observed.catalog() else {
        let none = |(): ()| Node::Null;
        return header
            .paged(pager, "prototypes", "prototypes", [], 0, none)
            .paged(pager, "terrain_palette", "terrain_palette", [], 0, none)
            .paged(pager, "zone_palette", "zone_palette", [], 0, none);
    };
    let editing = catalog.editing();
    let prototypes = catalog.prototypes().filter(|(proto, _, _)| keep(proto));
    // Built only for the prototypes listed; the rest are counted.
    let prototype = |(proto, resource, anchor): (&str, &str, &'static str)| -> Node {
        let grant = editing.objects.get(proto);
        let terrain = editing
            .terrain
            .iter()
            .filter(|(_, values)| values.contains(proto))
            .map(|(grid, _)| grid.as_str());
        Map::new()
            .text("proto", proto)
            .text("resource", resource)
            .with("anchor", Node::Text(anchor.into()))
            .with("placeable", grant.is_some())
            .optional_text("layer", grant.map(|grant| grant.layer.as_str()))
            .bounded(
                "fields",
                grant.into_iter().flat_map(|grant| &grant.fields),
                MAXIMUM_FIELDS,
                |field| Node::Text(field.clone()),
            )
            .bounded(
                "defaults",
                grant.into_iter().flat_map(|grant| &grant.defaults),
                MAXIMUM_FIELDS,
                |(name, _)| Node::Text(name.clone()),
            )
            .with("portal", grant.is_some_and(|grant| grant.portal))
            .ids("terrain", terrain)
            .into()
    };
    // The same choices as the desktop palettes (`tools::terrain_palette`
    // and `tools::zone_palette`), filtered before they are bounded.
    let terrain = editing.terrain.iter().flat_map(|(grid, values)| {
        values
            .iter()
            .filter(|value| keep(value))
            .map(move |value| (grid, value))
    });
    let plane = world.planes().get(catalog.view().0);
    let zones = plane
        .into_iter()
        .flat_map(|plane| plane.zones.keys().map(move |id| (plane, id.as_str())))
        .filter(|(_, id)| keep(id));
    let report = header
        .paged(
            pager,
            "prototypes",
            "prototypes",
            prototypes,
            MAXIMUM_PROTOTYPES,
            prototype,
        )
        .paged(
            pager,
            "terrain_palette",
            "terrain_palette",
            terrain,
            MAXIMUM_TERRAIN_CHOICES,
            |(grid, value)| {
                let row = Map::new().text("grid", grid).text("value", value);
                // Only a value that is a terrain has the members, so the
                // report of a world without transitions is unchanged.
                match terrain_name(world, grid, value) {
                    TerrainName::Terrain(id) => row.text("terrain", id),
                    TerrainName::Variant(id) => row.text("terrain", id).with("variant", true),
                    TerrainName::Unnamed | TerrainName::Ungoverned => row,
                }
                .into()
            },
        )
        .paged(
            pager,
            "zone_palette",
            "zone_palette",
            zones,
            MAXIMUM_ZONES,
            |(plane, id)| {
                Map::new()
                    .text("id", id)
                    .optional_text("name", zones::display_name(plane, id))
                    .into()
            },
        );
    let report = palettes(report, editing, &keep, pager);
    // Only a catalog with the grant has the member, so every report of
    // another catalog is unchanged.
    match editing.heights {
        Some(grant) => report.with(
            "heights",
            Map::new()
                .with("min", Node::Integer(i64::from(grant.min)))
                .with("max", Node::Integer(i64::from(grant.max))),
        ),
        None => report,
    }
}

/// `palettes`, the catalog's `editing.palettes` kept by name, after the
/// other lists and paged like them. Only a catalog with palettes has the
/// member, so every report of another catalog is unchanged.
fn palettes(
    report: Map,
    editing: &tilewright_editor_preview::EditingCapabilities,
    keep: &dyn Fn(&str) -> bool,
    pager: &dyn Pager,
) -> Map {
    if editing.palettes.is_empty() {
        return report;
    }
    report.paged(
        pager,
        "palettes",
        "palettes",
        editing.palettes.iter().filter(|(name, _)| keep(name)),
        MAXIMUM_PALETTES,
        |(name, palette)| {
            Map::new()
                .text("name", name)
                .text("grid", &palette.grid)
                .bounded(
                    "entries",
                    &palette.entries,
                    MAXIMUM_PALETTE_ENTRIES,
                    |(value, weight)| {
                        Map::new()
                            .text("value", value)
                            .with("weight", Node::Integer(i64::from(*weight)))
                            .into()
                    },
                )
                .into()
        },
    )
}
