// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The tools on generated worlds: paging to the end, cursors and
//! revisions across refresh, a world that does not load, hostile world
//! text, the byte budget, and a tree left exactly as it was.
mod common;

use common::{
    MAIN_CHUNK, call, call_line, edit, fixture_copy, page_through, read_json, request, serve_raw,
    server, session, store_json, tree,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tilewright_editor_mcp::{MAXIMUM_RESULT_BYTES, Server};
use tilewright_editor_observe::limits;

/// The neutral fixture grown past every list bound, as the observe
/// crate's own tests grow it.
fn crowded() -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = fixture_copy("prototype-neutral");
    let count = 300;
    let mut catalog = read_json(&root, "preview.jsonc");
    for index in 0..count {
        let proto = format!("crowd_{index:03}");
        store_json(
            &root,
            &format!("defs/proto/{proto}.jsonc"),
            &json!({"format": "tilewright.def/1", "type": "proto", "id": proto, "fields": {}}),
        );
        catalog["prototypes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"proto": proto, "resource": "grass", "anchor": "ground"}));
        catalog["editing"]["terrain"][0]["values"]
            .as_array_mut()
            .unwrap()
            .push(json!(proto));
    }
    // The most scatter palettes a catalog may name, a paged list too.
    catalog["editing"]["palettes"] = (0..32)
        .map(|index| {
            json!({"name": format!("crowd_palette_{index:02}"), "grid": "floor",
                "entries": (0..8).map(|value| json!({"value": format!("crowd_{value:03}"),
                    "weight": index + value + 1})).collect::<Vec<_>>()})
        })
        .collect();
    // Seventy objects on tile (0, 0), each with a portal to that tile.
    let mut objects = Vec::new();
    for index in 0..70 {
        let uid = format!("crowd_object_{index}");
        objects.push(json!({
            "uid": uid, "at": [0, 0], "proto": "marker", "layer": "item",
            "fields": {"label": format!("Object {index}")}, "portal": {"layer": 0, "at": [0, 0]}
        }));
        catalog["editing"]["movable_uids"]
            .as_array_mut()
            .unwrap()
            .push(json!(uid));
    }
    store_json(&root, "preview.jsonc", &catalog);
    edit(&root, MAIN_CHUNK, |chunk| {
        chunk["objects"].as_array_mut().unwrap().extend(objects);
    });
    edit(&root, "planes/main/anchors.jsonc", |anchors| {
        for index in 0..10 {
            anchors["anchors"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": format!("crowd_anchor_{index}"), "layer": 0, "at": [0, 0]}));
        }
    });
    edit(&root, "planes/main/zones.jsonc", |zones| {
        for index in 0..count {
            zones["zones"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": format!("crowd_zone_{index:03}")}));
        }
    });
    edit(&root, "world.jsonc", |world| {
        for index in 0..70 {
            let id = format!("z_plane_{index:02}");
            world["planes"].as_array_mut().unwrap().push(json!(id));
            store_json(
                &root,
                &format!("planes/{id}/plane.jsonc"),
                &json!({"format": "tilewright.plane/1", "id": id, "kind": "shared", "chunk_size": 32}),
            );
        }
    });
    (directory, root)
}

const ORIGIN: &str = r#"{"plane":"main","layer":0,"x":0,"y":0}"#;

/// Every paged list of every tool on `root`, and the arguments to reach it.
fn sections() -> Vec<(&'static str, Value, &'static str)> {
    let origin: Value = serde_json::from_str(ORIGIN).unwrap();
    let mut all = Vec::new();
    for section in tilewright_editor_observe::overview::SECTIONS {
        all.push(("overview", json!({}), section));
    }
    for section in tilewright_editor_observe::tile::SECTIONS {
        all.push(("tile", origin.clone(), section));
    }
    for section in tilewright_editor_observe::catalog::SECTIONS {
        all.push(("catalog", json!({}), section));
        all.push(("catalog", json!({"match": "CROWD_1"}), section));
    }
    all.push(("lint", json!({}), "findings"));
    all
}

/// The list at `path` in a report.
fn at<'a>(report: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(report, |value, key| &value[key])
}

#[test]
fn paging_to_the_end_returns_every_item_exactly_once() {
    let (_directory, root) = crowded();
    let mut server = server(&root);
    let mut long = 0;
    for (tool, arguments, section) in sections() {
        let (everything, calls) = page_through(&mut server, tool, &arguments, section, 500);
        assert_eq!(calls, 1, "{tool} {section}: one page holds every item");
        let (by_ones, calls) = page_through(&mut server, tool, &arguments, section, 1);
        assert_eq!(by_ones, everything, "{tool} {section}");
        assert_eq!(calls, everything.len().max(1));
        let (by_sevens, _) = page_through(&mut server, tool, &arguments, section, 7);
        assert_eq!(by_sevens, everything, "{tool} {section}");
        // The report's first items are the page's, and its cursor goes on
        // where they end.
        let report = call(&mut server, tool, &arguments);
        let path = section.rsplit_once('.').map_or("", |(parent, _)| parent);
        let key = section.rsplit('.').next().unwrap();
        let parent = if path.is_empty() {
            report.structured().clone()
        } else {
            at(report.structured(), path).clone()
        };
        let first = parent[key].as_array().unwrap();
        assert_eq!(first[..], everything[..first.len()], "{tool} {section}");
        assert_eq!(
            parent[format!("{key}_more")],
            everything.len() - first.len()
        );
        assert_eq!(
            parent[format!("{key}_cursor")].is_string(),
            first.len() < everything.len(),
            "{tool} {section}"
        );
        if everything.len() > 64 {
            long += 1;
        }
    }
    // The crowd is past the CLI's bounds in planes, grants, prototypes,
    // palettes, objects and findings.
    assert!(long >= 8, "{long}");
}

#[test]
fn every_item_past_the_clis_bound_is_reachable() {
    let (_directory, root) = crowded();
    let mut server = server(&root);
    let (planes, _) = page_through(&mut server, "overview", &json!({}), "planes", 500);
    assert_eq!(planes.len(), 71);
    assert_eq!(planes[70]["id"], "z_plane_69");
    let (prototypes, _) = page_through(&mut server, "catalog", &json!({}), "prototypes", 500);
    assert!(prototypes.iter().any(|item| item["proto"] == "crowd_299"));
    let origin: Value = serde_json::from_str(ORIGIN).unwrap();
    let (objects, _) = page_through(&mut server, "tile", &origin, "objects", 500);
    let (arrivals, _) = page_through(&mut server, "tile", &origin, "arrivals", 500);
    assert_eq!((objects.len(), arrivals.len()), (70, 70));
    // The CLI bounds the tile at 16 objects; the report stays at it too.
    let report = call(
        &mut server,
        "tile",
        &json!({"plane":"main","layer":0,"x":0,"y":0,"limit":500}),
    );
    assert_eq!(report.structured()["objects"].as_array().unwrap().len(), 16);
    assert_eq!(report.structured()["objects_more"], 54);
}

fn cursor_of(server: &mut Server, tool: &str, arguments: &Value, section: &str) -> String {
    let mut arguments = arguments.clone();
    arguments["section"] = json!(section);
    arguments["limit"] = json!(1);
    call(server, tool, &arguments).structured()["next_cursor"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn error_code(called: &common::Called) -> String {
    let text = called.error_text();
    let rest = text.split_once(" failed: ").unwrap().1;
    rest.split_once(": ").unwrap().0.to_owned()
}

#[test]
#[allow(clippy::too_many_lines)]
fn cursors_are_bound_to_tool_section_arguments_and_snapshot() {
    let (_directory, root) = crowded();
    // Twins of tile (0, 0) of plane main, layer 0: another x, another y,
    // another layer, another plane, each with two objects, so a cursor at
    // offset 1 lies inside each of their lists and only the fingerprint
    // can refuse it.
    let twin = |uid: &str, at: [i64; 2]| json!({"uid": uid, "at": at, "proto": "marker", "layer": "item", "fields": {}});
    edit(&root, MAIN_CHUNK, |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        for index in 0..2 {
            objects.push(twin(&format!("twin_x_{index}"), [1, 0]));
            objects.push(twin(&format!("twin_y_{index}"), [0, 1]));
        }
    });
    let mut chunk = read_json(&root, "planes/main/layers/0/-1_0.jsonc");
    chunk["chunk"] = json!([0, 0]);
    for (plane, layer, tag) in [("main", 1, "layer"), ("z_plane_00", 0, "plane")] {
        chunk["plane"] = json!(plane);
        chunk["layer"] = json!(layer);
        chunk["objects"] = json!([
            twin(&format!("twin_{tag}_0"), [0, 0]),
            twin(&format!("twin_{tag}_1"), [0, 0])
        ]);
        store_json(
            &root,
            &format!("planes/{plane}/layers/{layer}/0_0.jsonc"),
            &chunk,
        );
    }
    let mut server = server(&root);
    let origin: Value = serde_json::from_str(ORIGIN).unwrap();
    let cursor = cursor_of(&mut server, "tile", &origin, "objects");
    let resume = |arguments: &Value, section: &str| {
        let mut arguments = arguments.clone();
        arguments["section"] = json!(section);
        arguments["cursor"] = json!(cursor);
        arguments
    };
    let mut elsewhere = origin.clone();
    elsewhere["x"] = json!(1);
    let mut below = origin.clone();
    below["y"] = json!(1);
    let mut lower = origin.clone();
    lower["layer"] = json!(1);
    let mut other_plane = origin.clone();
    other_plane["plane"] = json!("z_plane_00");
    for twin in [&elsewhere, &below, &lower, &other_plane] {
        let page = call(
            &mut server,
            "tile",
            &json!({
                "plane": twin["plane"], "layer": twin["layer"], "x": twin["x"], "y": twin["y"],
                "section": "objects"
            }),
        );
        assert_eq!(page.structured()["total"], 2, "{twin}");
    }
    for (tool, arguments) in [
        ("tile", resume(&origin, "arrivals")),
        ("tile", resume(&elsewhere, "objects")),
        ("tile", resume(&below, "objects")),
        ("tile", resume(&lower, "objects")),
        ("tile", resume(&other_plane, "objects")),
        ("catalog", resume(&json!({}), "prototypes")),
        ("lint", resume(&json!({}), "findings")),
    ] {
        let called = call(&mut server, tool, &arguments);
        assert_eq!(error_code(&called), "cursor.invalid", "{tool} {arguments}");
    }
    // A catalog cursor is bound to its `match`: given without it, or with
    // another, it is refused.
    let matched = cursor_of(
        &mut server,
        "catalog",
        &json!({"match": "crowd"}),
        "prototypes",
    );
    for arguments in [
        json!({"section": "prototypes", "cursor": matched}),
        json!({"section": "prototypes", "cursor": matched, "match": "CROWD"}),
        json!({"section": "prototypes", "cursor": matched, "match": ""}),
    ] {
        let called = call(&mut server, "catalog", &arguments);
        assert_eq!(error_code(&called), "cursor.invalid", "{arguments}");
    }
    let resumed = call(
        &mut server,
        "catalog",
        &json!({"section": "prototypes", "cursor": matched, "match": "crowd"}),
    );
    assert_eq!(resumed.structured()["offset"], 1);
    // The limit is not part of what a cursor is bound to.
    let mut arguments = resume(&origin, "objects");
    arguments["limit"] = json!(3);
    let page = call(&mut server, "tile", &arguments);
    assert_eq!(page.structured()["offset"], 1);
    // A cursor past the end, crafted from a real one.
    let past = cursor.rsplit_once('.').unwrap().0.to_owned() + ".70";
    let mut arguments = resume(&origin, "objects");
    arguments["cursor"] = json!(past);
    assert_eq!(
        error_code(&call(&mut server, "tile", &arguments)),
        "cursor.invalid"
    );
    // A refresh that finds the same world keeps the cursor.
    let refreshed = call(&mut server, "refresh", &json!({}));
    assert_eq!(refreshed.structured()["changed"], false);
    assert!(!call(&mut server, "tile", &resume(&origin, "objects")).is_error());
    // One that finds a changed world makes it stale.
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!("Retitled");
    });
    let refreshed = call(&mut server, "refresh", &json!({}));
    assert_eq!(refreshed.structured()["changed"], true);
    assert_ne!(
        refreshed.structured()["revision"],
        refreshed.structured()["previous_revision"]
    );
    assert_eq!(
        error_code(&call(&mut server, "tile", &resume(&origin, "objects"))),
        "cursor.stale"
    );
}

#[test]
fn a_catalog_only_change_is_a_changed_snapshot_with_the_same_revision() {
    let (_directory, root) = crowded();
    let mut server = server(&root);
    let status = |server: &mut Server| {
        call(server, "overview", &json!({})).structured()["catalog"]["status"].clone()
    };
    let cursor = cursor_of(&mut server, "catalog", &json!({}), "prototypes");
    let before = call(&mut server, "overview", &json!({}));
    assert_eq!(before.structured()["catalog"]["status"], "loaded");
    // An edit that keeps the catalog loaded: its view shrinks by a row.
    edit(&root, "preview.jsonc", |catalog| {
        catalog["view"]["bounds"] = json!([0, 0, 8, 5]);
    });
    let refreshed = call(&mut server, "refresh", &json!({}));
    let refreshed = refreshed.structured();
    assert_eq!(status(&mut server), "loaded");
    assert_eq!(refreshed["changed"], true);
    assert_eq!(refreshed["revision"], refreshed["previous_revision"]);
    assert_eq!(refreshed["revision"], before.structured()["revision"]);
    let stale = call(
        &mut server,
        "catalog",
        &json!({"section": "prototypes", "cursor": cursor}),
    );
    assert_eq!(error_code(&stale), "cursor.stale");
    // The same catalog again is the same snapshot as before the edit:
    // a fresh cursor survives a refresh that finds nothing new.
    let cursor = cursor_of(&mut server, "catalog", &json!({}), "prototypes");
    assert_eq!(
        call(&mut server, "refresh", &json!({})).structured()["changed"],
        false
    );
    assert!(
        !call(
            &mut server,
            "catalog",
            &json!({"section": "prototypes", "cursor": cursor})
        )
        .is_error()
    );
}

#[test]
fn an_invalid_catalog_whose_problem_changes_is_a_changed_snapshot() {
    let (_directory, root) = fixture_copy("prototype-neutral");
    edit(&root, "preview.jsonc", |catalog| {
        catalog["resources"][0]["path"] = json!("preview-assets/first_missing.atx1");
    });
    let mut server = server(&root);
    let problem = |server: &mut Server| {
        let overview = call(server, "overview", &json!({}));
        assert_eq!(overview.structured()["catalog"]["status"], "invalid");
        overview.structured()["catalog"]["problem"].clone()
    };
    let first = problem(&mut server);
    assert_eq!(
        call(&mut server, "refresh", &json!({})).structured()["changed"],
        false
    );
    edit(&root, "preview.jsonc", |catalog| {
        catalog["resources"][0]["path"] = json!("preview-assets/second_missing.atx1");
    });
    let refreshed = call(&mut server, "refresh", &json!({}));
    assert_eq!(refreshed.structured()["changed"], true);
    assert_ne!(problem(&mut server), first);
}

#[test]
fn the_revision_argument_pins_the_snapshot() {
    let (_directory, root) = fixture_copy("prototype-neutral");
    let mut server = server(&root);
    let revision = call(&mut server, "overview", &json!({})).structured()["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let origin: Value = serde_json::from_str(ORIGIN).unwrap();
    for (tool, mut arguments) in [
        ("overview", json!({})),
        ("tile", origin),
        ("catalog", json!({})),
        ("lint", json!({})),
        ("render", json!({"region": [0, 0, 1, 1]})),
    ] {
        arguments["revision"] = json!(revision);
        assert!(!call(&mut server, tool, &arguments).is_error(), "{tool}");
        arguments["revision"] = json!("0".repeat(64));
        let refused = call(&mut server, tool, &arguments);
        assert_eq!(error_code(&refused), "revision.changed");
        assert!(
            refused
                .error_text()
                .ends_with(&format!("; the current revision is {revision}")),
            "{}",
            refused.error_text()
        );
    }
    // Editing the files changes nothing until refresh.
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!("Retitled");
    });
    let pinned = call(&mut server, "overview", &json!({"revision": revision}));
    assert_eq!(
        pinned.structured()["planes"][0]["title"],
        "Authored geometric meadow"
    );
    call(&mut server, "refresh", &json!({}));
    let moved = call(&mut server, "overview", &json!({"revision": revision}));
    assert_eq!(error_code(&moved), "revision.changed");
    let current = call(&mut server, "overview", &json!({}));
    assert_eq!(current.structured()["planes"][0]["title"], "Retitled");
}

#[test]
fn a_world_that_does_not_load_gives_its_diagnostics_until_it_is_fixed() {
    let (_directory, root) = fixture_copy("prototype-neutral");
    let good = std::fs::read(root.join("world.jsonc")).unwrap();
    std::fs::write(root.join("world.jsonc"), b"{ not json").unwrap();
    let mut server = server(&root);
    assert_eq!(server.revision(), None);
    let origin: Value = serde_json::from_str(ORIGIN).unwrap();
    for (tool, arguments) in [
        ("overview", json!({})),
        ("tile", origin.clone()),
        ("catalog", json!({"section": "prototypes"})),
        ("lint", json!({})),
        ("render", json!({})),
    ] {
        let called = call(&mut server, tool, &arguments);
        assert_eq!(error_code(&called), "world.invalid", "{tool}");
        let document = called.result()["content"][1]["text"].as_str().unwrap();
        let parsed =
            tilewright_jsonc::Document::parse(document, &tilewright_jsonc::Limits::default())
                .unwrap();
        tilewright_editor_mcp::load_failure_schema()
            .check(parsed.root())
            .unwrap();
        let document: Value = serde_json::from_str(document).unwrap();
        assert!(document["problems"].as_u64().unwrap() >= 1);
        assert_eq!(document["diagnostics"][0]["path"], "world.jsonc");
        // Any revision given fails: the world has none.
        let mut pinned = arguments.clone();
        pinned["revision"] = json!("a".repeat(64));
        let refused = call(&mut server, tool, &pinned);
        assert_eq!(error_code(&refused), "revision.changed");
    }
    let refreshed = call(&mut server, "refresh", &json!({}));
    assert_eq!(
        refreshed.structured().clone(),
        json!({"revision": null, "valid": false, "diagnostics": refreshed.structured()["diagnostics"], "previous_revision": null, "changed": false})
    );
    std::fs::write(root.join("world.jsonc"), good).unwrap();
    let fixed = call(&mut server, "refresh", &json!({}));
    assert_eq!(fixed.structured()["valid"], true);
    assert_eq!(fixed.structured()["changed"], true);
    assert!(!call(&mut server, "overview", &json!({})).is_error());
}

#[test]
fn a_missing_root_is_a_world_that_does_not_load() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("absent");
    let mut server = server(&root);
    let called = call(&mut server, "overview", &json!({}));
    assert_eq!(error_code(&called), "world.invalid");
    assert!(!root.exists());
}

/// The marker world text carries in the marking tests.
const MARK: &str = "zq";

/// The neutral sample with the marker in every string the world spells
/// that a report shows: plane ids and titles, default zones, zone ids and
/// names, anchors, prototypes, uids, field names and values, defaults,
/// grants, terrain values and a render layer.
fn marked_world() -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = fixture_copy("prototype-neutral");
    edit(&root, "profile/profile.jsonc", |profile| {
        profile["render_layers"]
            .as_array_mut()
            .unwrap()
            .push(json!("zq_render_layer"));
    });
    edit(&root, "profile/schema/object.jsonc", |schema| {
        schema["fields"]["zq_field"] = json!({"type": "string"});
    });
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!("zq main title");
        plane["default_zone"] = json!("zq_zone");
    });
    edit(&root, "planes/main/zones.jsonc", |zones| {
        let zones = zones["zones"].as_array_mut().unwrap();
        zones.push(json!({"id": "zq_zone", "name": "zq zone name"}));
        zones.push(json!({"id": "zq_zone.child", "parent": "zq_zone", "name": "zq child"}));
    });
    edit(&root, "planes/main/anchors.jsonc", |anchors| {
        anchors["anchors"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "zq_anchor", "layer": 0, "at": [3, 2]}));
    });
    store_json(
        &root,
        "defs/proto/zq_proto.jsonc",
        &json!({"format": "tilewright.def/1", "type": "proto", "id": "zq_proto", "fields": {}}),
    );
    edit(&root, "preview.jsonc", |catalog| {
        catalog["prototypes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"proto": "zq_proto", "resource": "grass", "anchor": "ground"}));
        let editing = &mut catalog["editing"];
        editing["terrain"][0]["values"]
            .as_array_mut()
            .unwrap()
            .push(json!("zq_proto"));
        editing["movable_uids"]
            .as_array_mut()
            .unwrap()
            .push(json!("zq_uid"));
        editing["fields"]
            .as_array_mut()
            .unwrap()
            .push(json!({"uid": "zq_uid", "field": "zq_field"}));
        editing["objects"].as_array_mut().unwrap().push(json!({
            "proto": "zq_proto", "layer": "item", "fields": ["zq_field"],
            "defaults": {"zq_field": "zq default"}, "portal": true
        }));
    });
    edit(&root, MAIN_CHUNK, |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        objects.push(json!({
            "uid": "zq_uid", "at": [3, 2], "proto": "zq_proto", "layer": "item",
            "fields": {"zq_field": "zq value", "label": "zq label"},
            "portal": {"anchor": "zq_anchor"}
        }));
        objects.push(json!({
            "uid": "zq_uid_far", "at": [3, 2], "proto": "zq_proto", "layer": "item",
            "fields": {"label": "zq far"},
            "portal": {"plane": "zq_plane", "layer": 0, "at": [5, 5]}
        }));
    });
    edit(&root, "world.jsonc", |world| {
        world["planes"]
            .as_array_mut()
            .unwrap()
            .push(json!("zq_plane"));
    });
    store_json(
        &root,
        "planes/zq_plane/plane.jsonc",
        &json!({"format": "tilewright.plane/1", "id": "zq_plane", "title": "zq plane title",
                "kind": "shared", "chunk_size": 32}),
    );
    (directory, root)
}

/// Whether `text` is one of this editor's own strings, which an output
/// schema may leave unmarked: a format tag, a revision or digest, a
/// cursor, an enumerated value, or a stable code.
fn editor_vocabulary(text: &str) -> bool {
    const VALUES: [&str; 17] = [
        "absent",
        "invalid",
        "loaded",
        "top_down",
        "isometric",
        "location",
        "anchor",
        "error",
        "warning",
        "info",
        "null",
        "boolean",
        "integer",
        "number",
        "string",
        "json",
        "ground",
    ];
    const CODE_PREFIXES: [&str; 9] = [
        "catalog.", "preview.", "portal.", "anchor.", "zone.", "world.", "source.", "jsonc.",
        "shape.",
    ];
    text.starts_with("tilewright.editor-")
        || (text.len() == 64 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || text.starts_with("tw1.")
        || VALUES.contains(&text)
        || text == "standing"
        || tilewright_editor_observe::overview::SECTIONS
            .iter()
            .chain(&tilewright_editor_observe::tile::SECTIONS)
            .chain(&tilewright_editor_observe::catalog::SECTIONS)
            .chain(&tilewright_editor_observe::lint::SECTIONS)
            .any(|section| *section == text)
        || CODE_PREFIXES.iter().any(|prefix| {
            text.strip_prefix(prefix).is_some_and(|rest| {
                !rest.is_empty()
                    && rest
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || b"_.-".contains(&byte))
            })
        })
}

/// Every result of every tool on `root`: each report at the default and
/// the largest limit, every page of every section, `render` and `refresh`.
fn every_result(server: &mut Server, tiles: &[Value]) -> Vec<common::Called> {
    let mut results = Vec::new();
    let mut calls: Vec<(&str, Value, &[&str])> = vec![
        (
            "overview",
            json!({}),
            &tilewright_editor_observe::overview::SECTIONS,
        ),
        (
            "catalog",
            json!({}),
            &tilewright_editor_observe::catalog::SECTIONS,
        ),
        (
            "lint",
            json!({}),
            &tilewright_editor_observe::lint::SECTIONS,
        ),
    ];
    for tile in tiles {
        calls.push((
            "tile",
            tile.clone(),
            &tilewright_editor_observe::tile::SECTIONS,
        ));
    }
    for (tool, arguments, sections) in calls {
        for limit in [None, Some(500)] {
            let mut report = arguments.clone();
            if let Some(limit) = limit {
                report["limit"] = json!(limit);
            }
            results.push(call(server, tool, &report));
        }
        for section in sections {
            let mut paged = arguments.clone();
            paged["section"] = json!(section);
            paged["limit"] = json!(500);
            results.push(call(server, tool, &paged));
        }
    }
    results.push(call(server, "render", &json!({"region": [0, 0, 2, 2]})));
    results.push(call(server, "refresh", &json!({})));
    results
}

/// Checks the two marking rules on `results`: every string holding the
/// marker is at a position marked world text, and every unmarked string is
/// this editor's own. Returns how many marked strings held the marker.
fn check_marks(results: &[common::Called]) -> usize {
    let mut marked = 0;
    for called in results {
        assert!(!called.is_error(), "{}", called.line);
        for (text, world) in &called.strings {
            if text.contains(MARK) {
                assert!(world, "world text {text:?} is not marked: {}", called.line);
                marked += 1;
            } else if !world {
                assert!(
                    editor_vocabulary(text),
                    "unmarked {text:?} is not this editor's own: {}",
                    called.line
                );
            }
        }
    }
    marked
}

#[test]
fn every_world_string_is_marked_and_every_unmarked_string_is_the_editors() {
    let tiles = [
        json!({"plane": "main", "layer": 0, "x": 3, "y": 2}),
        json!({"plane": "main", "layer": 0, "x": 7, "y": 0}),
        json!({"plane": "main", "layer": 0, "x": 7, "y": 3}),
        json!({"plane": "zq_plane", "layer": 0, "x": 5, "y": 5}),
    ];
    let (_directory, root) = marked_world();
    let mut server = server(&root);
    let results = every_result(&mut server, &tiles);
    let marked = check_marks(&results);
    // Every kind of marked string turned up holding the marker.
    let all: String = results.iter().map(|called| called.line.as_str()).collect();
    for spelled in [
        "zq main title",
        "zq_zone",
        "zq zone name",
        "zq child",
        "zq_anchor",
        "zq_proto",
        "zq_uid",
        "zq_field",
        "zq value",
        "zq label",
        "zq_plane",
        "zq plane title",
        "zq_render_layer",
    ] {
        assert!(all.contains(spelled), "{spelled} is in no result");
    }
    assert!(marked >= 100, "{marked}");
    // The unmarked rule on the other worlds the tests build.
    for (_directory, root) in [crowded(), widest(), fixture_copy("meadow")] {
        let mut server = common::server(&root);
        let origin: Value = serde_json::from_str(ORIGIN).unwrap();
        let tiles = if root.join("planes/main").exists() {
            vec![origin]
        } else {
            vec![json!({"plane": "meadow", "layer": 0, "x": 1, "y": 1})]
        };
        for called in every_result(&mut server, &tiles) {
            if called.is_error() {
                // `render` without a catalog.
                assert!(
                    called
                        .error_text()
                        .starts_with("render failed: catalog.absent")
                );
                continue;
            }
            check_marks(std::slice::from_ref(&called));
        }
    }
}

#[test]
fn an_invalid_catalog_and_a_broken_world_carry_their_world_text_as_marked_or_world_data() {
    // A catalog whose resource is missing: its problem quotes the path.
    let (_directory, root) = marked_world();
    edit(&root, "preview.jsonc", |catalog| {
        catalog["resources"][0]["path"] = json!("preview-assets/zq_missing.atx1");
    });
    let mut server = server(&root);
    let overview = call(&mut server, "overview", &json!({}));
    let problem = overview.structured()["catalog"]["problem"]
        .as_str()
        .unwrap();
    assert!(problem.contains("zq_missing"), "{problem}");
    let lint = call(&mut server, "lint", &json!({}));
    check_marks(&[overview, lint]);
    // A world that does not load: its load-failure document, world data
    // by the instructions, conforms to its schema and marks the marker.
    edit(&root, "world.jsonc", |world| {
        world["planes"]
            .as_array_mut()
            .unwrap()
            .push(json!("zq_absent"));
    });
    call(&mut server, "refresh", &json!({}));
    let failed = call(&mut server, "overview", &json!({}));
    let document = failed.result()["content"][1]["text"].as_str().unwrap();
    let parsed =
        tilewright_jsonc::Document::parse(document, &tilewright_jsonc::Limits::default()).unwrap();
    let strings = tilewright_editor_mcp::load_failure_schema()
        .check(parsed.root())
        .unwrap();
    assert!(
        strings.iter().any(|(text, _)| text.contains(MARK)),
        "{document}"
    );
    for (text, world) in strings {
        assert!(!text.contains(MARK) || world, "{text}");
        assert!(world || editor_vocabulary(&text), "{text}");
    }
}

/// Text that tries everything, and a marker in it to find it again.
const HOSTILE: &str = "zq\"b\\s\u{1}\u{7}\u{1b}[31mred\u{1b}]0;t\u{7}\u{7f}\u{85}\u{9b}\u{2028}\u{2029}\u{202e}\u{2066}\u{200b}\u{feff}\u{e0041}\u{3164}\u{1d173}\nfake: line";

#[test]
fn hostile_world_text_is_escaped_and_every_world_string_is_marked() {
    let (_directory, root) = fixture_copy("prototype-neutral");
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!(HOSTILE);
    });
    edit(&root, "planes/main/zones.jsonc", |zones| {
        zones["zones"][0]["name"] = json!(HOSTILE);
    });
    edit(&root, MAIN_CHUNK, |chunk| {
        chunk["objects"][0]["fields"]["label"] = json!(HOSTILE);
    });
    // Identifiers the world may spell freely enough to carry the marker.
    edit(&root, "planes/main/anchors.jsonc", |anchors| {
        anchors["anchors"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "zq_anchor", "layer": 0, "at": [3, 2]}));
    });
    let mut server = server(&root);
    let mut seen = 0;
    for (tool, arguments) in [
        ("overview", json!({})),
        ("tile", json!({"plane": "main", "layer": 0, "x": 3, "y": 2})),
        ("catalog", json!({})),
        ("lint", json!({})),
        ("lint", json!({"section": "findings", "limit": 500})),
        (
            "tile",
            json!({"plane": "main", "layer": 0, "x": 3, "y": 2, "section": "objects"}),
        ),
        ("render", json!({"region": [0, 0, 2, 2]})),
    ] {
        // `call` checks the line for raw escaped characters, the text
        // copy against the structured result and the output schema.
        let called = call(&mut server, tool, &arguments);
        assert!(!called.is_error(), "{tool}: {}", called.line);
        for (text, world) in &called.strings {
            if text.contains("zq") {
                seen += 1;
                assert!(world, "{tool}: world text {text:?} is not marked");
            }
        }
        // A world line break cannot end the line of the reply.
        assert!(!called.line.contains('\n'));
    }
    assert!(seen >= 5, "{seen}");
    let overview = call(&mut server, "overview", &json!({}));
    assert_eq!(overview.structured()["planes"][0]["title"], HOSTILE);
}

// The observe crate's widest world, as its tests build it.
/// An identifier of World's longest, 128 characters, ending in `tag`.
fn long_id(prefix: char, tag: usize) -> String {
    let tag = format!("{tag:04}");
    format!("{prefix}{}{tag}", "x".repeat(127 - tag.len()))
}

/// Planes past the plane bound, with the longest ids and widest titles,
/// each with layers past the layer bound.
fn widest_planes(root: &Path, text: &str) {
    let root = root.to_path_buf();
    // Planes with the longest ids and titles, each with every layer full.
    let mut chunk = read_json(&root, "planes/main/layers/0/-1_0.jsonc");
    chunk["objects"] = json!([]);
    edit(&root, "world.jsonc", |world| {
        for index in 0..=limits::MAXIMUM_PLANES {
            let id = long_id('q', index);
            world["planes"].as_array_mut().unwrap().push(json!(id));
            store_json(
                &root,
                &format!("planes/{id}/plane.jsonc"),
                &json!({"format": "tilewright.plane/1", "id": id, "title": text, "kind": "shared", "chunk_size": 32, "default_zone": long_id('z', 0)}),
            );
            store_json(
                &root,
                &format!("planes/{id}/zones.jsonc"),
                &json!({"format": "tilewright.zones/1", "zones": [{"id": long_id('z', 0), "name": text}]}),
            );
            chunk["plane"] = json!(id);
            for layer in 0..=limits::MAXIMUM_LAYERS {
                chunk["layer"] = json!(layer);
                store_json(
                    &root,
                    &format!("planes/{id}/layers/{layer}/-1_0.jsonc"),
                    &chunk,
                );
            }
        }
    });
}

/// The neutral fixture with every list of every report past its bound, the
/// longest ids World allows and free text that prints at its widest.
fn widest() -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = widest_catalog(&long_id('q', 0));
    let text: String = std::iter::repeat_n('\u{e0041}', limits::MAXIMUM_TEXT_CHARACTERS).collect();
    widest_planes(&root, &text);
    (directory, root)
}

/// [`widest`] without its extra planes: one plane, and a catalog whose
/// lists overflow the byte budget. The objects' portals lead to plane
/// `portals`.
fn widest_catalog(portals: &str) -> (tempfile::TempDir, PathBuf) {
    let (directory, root) = fixture_copy("prototype-neutral");
    // 256 tag characters: each prints as a 12-byte JSON escape, the most.
    let text: String = std::iter::repeat_n('\u{e0041}', limits::MAXIMUM_TEXT_CHARACTERS).collect();
    // Field names are at most 64 characters.
    let field_names: Vec<String> = (0..=limits::MAXIMUM_FIELDS)
        .map(|i| format!("f{}{i:04}", "x".repeat(59)))
        .collect();
    edit(&root, "profile/profile.jsonc", |profile| {
        let layers = profile["render_layers"].as_array_mut().unwrap();
        for index in 0..=limits::MAXIMUM_RENDER_LAYERS {
            layers.push(json!(long_id('r', index)));
        }
    });
    edit(&root, "profile/schema/object.jsonc", |schema| {
        for name in &field_names {
            schema["fields"][name] = json!({"type": "string"});
        }
    });
    let mut catalog = read_json(&root, "preview.jsonc");
    let protos: Vec<String> = (0..=limits::MAXIMUM_PROTOTYPES)
        .map(|i| long_id('p', i))
        .collect();
    let defaults: serde_json::Map<String, Value> = field_names
        .iter()
        .map(|name| (name.clone(), json!("x")))
        .collect();
    for proto in &protos {
        store_json(
            &root,
            &format!("defs/proto/{proto}.jsonc"),
            &json!({"format": "tilewright.def/1", "type": "proto", "id": proto, "fields": {}}),
        );
        catalog["prototypes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"proto": proto, "resource": "marker", "anchor": "standing"}));
        catalog["editing"]["terrain"][0]["values"]
            .as_array_mut()
            .unwrap()
            .push(json!(proto));
        catalog["editing"]["objects"].as_array_mut().unwrap().push(
            json!({"proto": proto, "layer": "item", "fields": field_names, "defaults": defaults}),
        );
    }
    // Objects on tile (0, 0) with the longest uids and every field full.
    let fields: serde_json::Map<String, Value> = field_names
        .iter()
        .map(|name| (name.clone(), json!(text)))
        .collect();
    let mut objects = Vec::new();
    for index in 0..=limits::MAXIMUM_GRANTS {
        let uid = long_id('u', index);
        objects.push(json!({
            "uid": uid, "at": [0, 0], "proto": protos[0], "layer": "item", "fields": fields,
            "portal": {"plane": portals, "layer": 0, "at": [0, 0]}
        }));
        catalog["editing"]["movable_uids"]
            .as_array_mut()
            .unwrap()
            .push(json!(uid));
    }
    store_json(&root, "preview.jsonc", &catalog);
    edit(&root, MAIN_CHUNK, |chunk| {
        chunk["objects"].as_array_mut().unwrap().extend(objects);
    });
    edit(&root, "planes/main/zones.jsonc", |zones| {
        for index in 0..=limits::MAXIMUM_FINDINGS {
            zones["zones"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": long_id('z', index), "name": text}));
        }
    });
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!(text);
    });
    (directory, root)
}

#[test]
fn every_result_stays_within_the_byte_budget_and_pages_on_by_bytes() {
    let (_directory, root) = widest();
    let mut server = server(&root);
    let origin: Value = serde_json::from_str(ORIGIN).unwrap();
    let mut by_bytes = 0;
    for (tool, arguments, section) in [
        ("overview", json!({}), "planes"),
        ("tile", origin, "objects"),
        ("catalog", json!({}), "prototypes"),
        ("lint", json!({}), "findings"),
    ] {
        let mut report = arguments.clone();
        report["limit"] = json!(500);
        let called = call(&mut server, tool, &report);
        assert!(
            !called.is_error(),
            "{tool}: {}",
            called.line.get(..600).unwrap_or(&called.line)
        );
        assert!(called.line.len() <= MAXIMUM_RESULT_BYTES + 512, "{tool}");
        let mut paged = arguments.clone();
        paged["section"] = json!(section);
        paged["limit"] = json!(500);
        let page = call(&mut server, tool, &paged);
        assert!(page.line.len() <= MAXIMUM_RESULT_BYTES + 512, "{tool}");
        if page.structured()["byte_budget_reached"] == true {
            by_bytes += 1;
            let (items, calls) = page_through(&mut server, tool, &arguments, section, 500);
            assert!(calls > 1);
            assert_eq!(items.len(), page.structured()["total"]);
        }
    }
    assert!(by_bytes >= 1, "a page stopped at the byte budget");
}

#[test]
fn a_report_spends_its_byte_budget_in_report_order() {
    let (_directory, root) = widest();
    let mut server = server(&root);
    let report = call(&mut server, "overview", &json!({"limit": 500}));
    let report = report.structured();
    // Each section in report order: its items listed, and its items left.
    let lists: Vec<(&str, usize, u64)> = tilewright_editor_observe::overview::SECTIONS
        .iter()
        .map(|section| {
            let (path, key) = section.rsplit_once('.').unwrap_or(("", section));
            let parent = if path.is_empty() {
                report
            } else {
                at(report, path)
            };
            (
                *section,
                parent[key].as_array().unwrap().len(),
                parent[format!("{key}_more")].as_u64().unwrap(),
            )
        })
        .collect();
    // Once a list is cut short with items left, no later list lists any.
    let first_cut = lists
        .iter()
        .position(|(_, listed, more)| *more > 0 && *listed < 64)
        .expect("the widest world runs out of budget");
    for (section, listed, _) in &lists[first_cut + 1..] {
        assert_eq!(
            *listed, 0,
            "{section} listed items after the budget ran out: {lists:?}"
        );
    }
    // The planes come before the catalog, and are listed.
    assert!(lists[2].1 > 0, "{lists:?}");
    assert!(first_cut >= 2, "{lists:?}");
}

#[test]
fn the_byte_budget_runs_out_inside_the_catalog_in_report_order() {
    let (_directory, root) = widest_catalog("main");
    let mut server = server(&root);
    let report = call(&mut server, "overview", &json!({"limit": 500}));
    let report = report.structured();
    let listed = |path: &str| {
        let (parent, key) = path.rsplit_once('.').unwrap();
        let parent = at(report, parent);
        (
            parent[key].as_array().unwrap().len(),
            parent[format!("{key}_more")].as_u64().unwrap(),
        )
    };
    // The planes and the view's zones fit; the grants run out of budget.
    assert_eq!(report["planes_more"], 0);
    assert!(!report["planes"].as_array().unwrap().is_empty());
    let (zones, zones_more) = listed("catalog.view_zones");
    assert!(zones > 0 && zones_more == 0, "{zones} {zones_more}");
    let grants: Vec<(usize, u64)> = tilewright_editor_observe::overview::SECTIONS[4..]
        .iter()
        .map(|section| listed(section))
        .collect();
    let cut = grants
        .iter()
        .position(|(listed, more)| *more > 0 && *listed < 64)
        .expect("the grants run out of budget");
    for (listed, _) in &grants[cut + 1..] {
        assert_eq!(*listed, 0, "{grants:?}");
    }
}

#[test]
fn nothing_in_or_beside_the_world_changes() {
    let (directory, root) = crowded();
    let beside = tree(directory.path());
    let before = tree(&root);
    let mut server = server(&root);
    let mut lines = vec![
        request(1, "server/discover", json!({})),
        request(2, "tools/list", json!({})),
        call_line(3, "refresh", &json!({})),
        "{ not json".to_owned(),
        call_line(4, "render", &json!({"out": "image.png"})),
        call_line(5, "render", &json!({"region": [0, 0, 2, 2]})),
        call_line(6, "edit", &json!({})),
    ];
    for (index, (tool, arguments, section)) in sections().into_iter().enumerate() {
        let id = i64::try_from(index).unwrap() + 10;
        lines.push(call_line(id, tool, &arguments));
        let mut paged = arguments.clone();
        paged["section"] = json!(section);
        paged["limit"] = json!(1);
        lines.push(call_line(id + 1000, tool, &paged));
    }
    let replies = session(&mut server, &lines);
    assert_eq!(replies.len(), lines.len());
    assert_eq!(tree(&root), before, "the world changed");
    assert_eq!(
        tree(directory.path()),
        beside,
        "an entry appeared beside the world"
    );
    // A world that stops loading, and refresh over it.
    std::fs::write(root.join("world.jsonc"), b"{").unwrap();
    let broken = tree(&root);
    let beside = tree(directory.path());
    let replies = session(
        &mut server,
        &[
            call_line(1, "refresh", &json!({})),
            call_line(2, "overview", &json!({})),
            call_line(3, "lint", &json!({})),
        ],
    );
    assert_eq!(replies.len(), 3);
    assert_eq!(tree(&root), broken);
    assert_eq!(tree(directory.path()), beside);
}

#[test]
fn the_log_holds_no_world_text() {
    let (_directory, root) = fixture_copy("prototype-neutral");
    edit(&root, "planes/main/plane.jsonc", |plane| {
        plane["title"] = json!(HOSTILE);
    });
    let mut server = server(&root);
    let (_, log) = serve_raw(&mut server, b"");
    assert!(log.starts_with("tilewright-editor mcp: serving revision "));
    assert!(log.is_ascii() && !log.contains("zq"));
    let missing = Path::new("/nonexistent/tilewright/world");
    let (_, log) = serve_raw(&mut common::server(missing), b"");
    assert!(log.contains("the world does not load"));
}
