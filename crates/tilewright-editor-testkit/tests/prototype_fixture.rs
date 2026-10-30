// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt::Write, fs};
use tilewright_editor_testkit::fixture_world;
use tilewright_source::{FieldValue, LoadOptions, Overlay};

fn limits() -> LoadOptions {
    LoadOptions {
        maximum_files: 64,
        maximum_entries: 256,
        maximum_total_bytes: 1024 * 1024,
        maximum_diagnostics: 32,
        ..LoadOptions::default()
    }
}

#[test]
fn authored_world_loads_with_editable_fields_and_cross_chunk_ground() {
    let root = fixture_world("prototype-neutral");
    let world =
        tilewright_source::load_with(&root, &limits()).expect("valid released World project");
    assert_eq!(world.profile().id, "sample.meadow");
    let marker = world.object("marker_1").unwrap();
    assert_eq!(marker.at, Some([3, 2]));
    assert_eq!(marker.layer.as_deref(), Some("item"));
    assert!(
        marker
            .fields
            .contains(&("label".into(), FieldValue::String("Trail marker".into())))
    );
    let plane = &world.planes()["main"];
    let chunk = &plane.chunks[&(0, 0, 0)];
    assert_eq!(chunk.ground[0].value(4 * 32 + 6), None);
    assert_eq!(chunk.ground[0].value(3), Some("grass"));
    assert_eq!(chunk.ground[0].value(4), Some("sand"));
    assert_eq!(plane.chunks[&(0, -1, 0)].ground[0].value(31), Some("sand"));
    assert_eq!(
        world.object_location("marker_negative").unwrap().chunk,
        (0, -1, 0)
    );
    let transitions = world.profile().transitions.as_ref().unwrap();
    let resolved = transitions.resolve_chunk(plane, 0, 0, 0);
    assert!(
        resolved
            .value(1, 2 * 32 + 4)
            .unwrap()
            .starts_with("grass_edge_")
    );
    assert_eq!(
        transitions.paint(transitions.terrain("sand").unwrap()),
        Some("sand")
    );
}

#[test]
fn nested_schema_rejects_wrong_typed_data_without_changing_source() {
    let root = fixture_world("prototype-neutral");
    let path = "planes/main/layers/0/0_0.jsonc";
    let original = fs::read_to_string(root.join(path)).unwrap();
    let mut invalid: serde_json::Value = serde_json::from_str(&original).unwrap();
    invalid["objects"][0]["fields"]["settings"]["visits"] = serde_json::json!("many");
    let overlay = Overlay::from([(path.into(), Some(invalid.to_string()))]);
    assert!(tilewright_source::load_edited(&root, &limits(), &overlay).is_err());
    assert_eq!(fs::read_to_string(root.join(path)).unwrap(), original);
}

#[test]
fn resource_bytes_provenance_and_transition_mapping_are_complete() {
    let root = fixture_world("prototype-neutral");
    let repo = root.ancestors().nth(4).unwrap();
    let catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("preview.jsonc")).unwrap()).unwrap();
    let provenance: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join("provenance/assets.json")).unwrap()).unwrap();
    let mappings: BTreeSet<_> = catalog["prototypes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["proto"].as_str().unwrap())
        .collect();
    let world = tilewright_source::load_with(&root, &limits()).unwrap();
    for proto in world.profile().transitions.as_ref().unwrap().protos() {
        assert!(mappings.contains(proto), "unmapped transition {proto}");
    }
    for resource in catalog["resources"].as_array().unwrap() {
        let path = root.join(resource["path"].as_str().unwrap());
        let data = fs::read(&path).unwrap();
        assert_eq!(&data[..4], b"ATX1");
        let width = u32::from_le_bytes(data[4..8].try_into().unwrap());
        let height = u32::from_le_bytes(data[8..12].try_into().unwrap());
        assert_eq!(
            data.len(),
            12 + usize::try_from(width * height * 4).unwrap()
        );
        assert_eq!(u64::from(width), resource["width"].as_u64().unwrap());
        assert_eq!(u64::from(height), resource["height"].as_u64().unwrap());
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(&data) {
            write!(digest, "{byte:02x}").unwrap();
        }
        assert_eq!(resource["sha256"].as_str().unwrap(), digest);
        let relative = path.strip_prefix(repo).unwrap().to_str().unwrap();
        let asset = provenance["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["path"] == relative)
            .expect("asset has provenance");
        for key in ["sha256", "author", "license", "notice", "width", "height"] {
            assert_eq!(asset[key], resource[key], "provenance mismatch: {key}");
        }
        assert_eq!(asset["license"], "MIT");
        assert!(repo.join(asset["source"].as_str().unwrap()).is_file());
    }
}
