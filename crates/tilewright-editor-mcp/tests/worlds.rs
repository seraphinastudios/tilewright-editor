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
fn cursors_are_bound_to_tool_section_arguments_and_snapshot() {
    let (_directory, root) = crowded();
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
    for (tool, arguments) in [
        ("tile", resume(&origin, "arrivals")),
        ("tile", resume(&elsewhere, "objects")),
        ("catalog", resume(&json!({}), "prototypes")),
        ("lint", resume(&json!({}), "findings")),
    ] {
        let called = call(&mut server, tool, &arguments);
        assert_eq!(error_code(&called), "cursor.invalid", "{tool} {arguments}");
    }
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
    let cursor = cursor_of(&mut server, "catalog", &json!({}), "prototypes");
    let before = call(&mut server, "overview", &json!({}));
    edit(&root, "preview.jsonc", |catalog| {
        catalog["prototypes"].as_array_mut().unwrap().pop();
    });
    let refreshed = call(&mut server, "refresh", &json!({}));
    let refreshed = refreshed.structured();
    assert_eq!(refreshed["changed"], true);
    assert_eq!(refreshed["revision"], refreshed["previous_revision"]);
    assert_eq!(refreshed["revision"], before.structured()["revision"]);
    let stale = call(
        &mut server,
        "catalog",
        &json!({"section": "prototypes", "cursor": cursor}),
    );
    assert_eq!(error_code(&stale), "cursor.stale");
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
            "portal": {"plane": long_id('q', 0), "layer": 0, "at": [0, 0]}
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
    widest_planes(&root, &text);
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
