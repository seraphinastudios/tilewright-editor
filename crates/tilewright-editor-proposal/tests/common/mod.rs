// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Fixture worlds, sessions and the canonical proposal writer for tests.
#![allow(dead_code)]

use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_proposal::hex;
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::uid::SeededUidSource;
use tilewright_editor_ui::{Action, Actor, PortalTarget, Session, stamp::StampObject};

pub const AGENT: &str = "agent:sandbox \"test\"";
pub const PERSON: &str = "human:tester";
pub const RECEIPT: [u8; 32] = [7; 32];
/// The version a calling application gives.
pub const EDITOR: &str = "1.2.3-test+build.4";

pub fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Parse, change and rewrite a JSON file.
pub fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

/// The catalog's one scatter palette in the fixtures here: grass 3, sand 1.
pub fn speckle() -> Value {
    json!([{"name": "speckle", "grid": "floor",
        "entries": [{"value": "grass", "weight": 3}, {"value": "sand", "weight": 1}]}])
}

/// [`plain_world`] with the scatter palette [`speckle`] and the height
/// range -8 to 8 in its catalog.
pub fn world(configure: impl FnOnce(&Path)) -> tempfile::TempDir {
    plain_world(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["palettes"] = speckle();
            catalog["editing"]["heights"] = json!({"min": -8, "max": 8});
        });
        configure(root);
    })
}

/// The neutral fixture with one unused zone, `spare`, to delete, and its
/// catalog as committed.
pub fn plain_world(configure: impl FnOnce(&Path)) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    edit_json(dir.path(), "planes/main/zones.jsonc", |zones| {
        zones["zones"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "spare"}));
    });
    configure(dir.path());
    dir
}

pub fn open(root: &Path, actor: &str) -> Session {
    let document = WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    Session::new(
        History::new(document),
        catalog,
        resources,
        Actor::new(actor).unwrap(),
    )
}

/// An agent's sandbox session, with predictable uids.
pub fn sandbox(root: &Path) -> Session {
    let mut session = open(root, AGENT);
    session.set_uid_source(Box::new(SeededUidSource::new(11)));
    session
}

#[derive(Default)]
pub struct Records(pub Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}

pub fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}

/// The edit actions of every kind, in an order in which each applies
/// after the ones before it on the neutral fixture; the fill twice, once
/// as a fill and once as a replace.
pub const KINDS: usize = 22;
#[allow(clippy::too_many_lines)]
pub fn edit(kind: usize, expected: [u8; 32]) -> Action {
    let main = || String::from("main");
    match kind {
        0 => Action::Move {
            uid: "marker_1".into(),
            x: 4,
            y: 2,
            expected,
        },
        1 => Action::SetField {
            uid: "marker_1".into(),
            field: "label".into(),
            value: json!("Renamed \"marker\" \\ \u{e9}"),
            expected,
        },
        2 => Action::Paint {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            x: 0,
            y: 0,
            proto: "sand".into(),
            expected,
        },
        3 => Action::PaintStroke {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            cells: vec![[0, 1], [1, 1]],
            proto: "sand".into(),
            expected,
        },
        4 => Action::PaintZones {
            plane: main(),
            layer: 0,
            cells: vec![[6, 0]],
            zone: Some("path".into()),
            expected,
        },
        5 => Action::Remove {
            uid: "marker_2".into(),
            expected,
        },
        6 => Action::Place {
            plane: main(),
            layer: 0,
            x: 2,
            y: 2,
            proto: "marker".into(),
            expected,
        },
        7 => Action::RenameZone {
            plane: main(),
            zone: "path".into(),
            name: Some("Trail".into()),
            expected,
        },
        8 => Action::SetZoneParent {
            plane: main(),
            zone: "path".into(),
            parent: Some("meadow".into()),
            expected,
        },
        9 => Action::CreateZone {
            plane: main(),
            zone: "camp".into(),
            name: Some("Camp".into()),
            parent: None,
            expected,
        },
        10 => Action::DeleteZone {
            plane: main(),
            zone: "spare".into(),
            expected,
        },
        11 => Action::CreateAnchor {
            plane: main(),
            anchor: "camp".into(),
            x: 2,
            y: 2,
            expected,
        },
        12 => Action::MoveAnchor {
            plane: main(),
            anchor: "arrival".into(),
            x: 6,
            y: 3,
            expected,
        },
        13 => Action::DeleteAnchor {
            plane: main(),
            anchor: "lookout".into(),
            expected,
        },
        14 => Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Anchor {
                plane: main(),
                anchor: "arrival".into(),
            }),
            expected,
        },
        // The sand block east of x = 4, not the sand painted at x < 2.
        15 => Action::FillTerrain {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            x: 5,
            y: 0,
            proto: "grass".into(),
            contiguous: true,
            expected,
        },
        // Every sand cell left in the view.
        16 => Action::FillTerrain {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            x: 1,
            y: 1,
            proto: "grass".into(),
            contiguous: false,
            expected,
        },
        // Speckle over the now all-grass west half of the view.
        17 => Action::ScatterTerrain {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            cells: (0..4).flat_map(|x| (0..6).map(move |y| [x, y])).collect(),
            palette: "speckle".into(),
            seed: 1234,
            expected,
        },
        // Raise the view's top two rows, which have no height grid yet.
        18 => Action::AdjustHeights {
            plane: main(),
            layer: 0,
            cells: (0..8).flat_map(|x| (0..2).map(move |y| [x, y])).collect(),
            amount: 5,
            expected,
        },
        // Smooth the view's rows 1 to 3, across the raised edge.
        19 => Action::SmoothHeights {
            plane: main(),
            layer: 0,
            cells: (0..8).flat_map(|x| (1..4).map(move |y| [x, y])).collect(),
            kernel: 1,
            expected,
        },
        // A stamp: two markers on one tile, a stone and a bush near them.
        20 => Action::PlaceObjects {
            plane: main(),
            layer: 0,
            x: 5,
            y: 3,
            objects: stamp_objects(),
            expected,
        },
        // Flatten the view's top three rows to -2.
        _ => Action::FlattenHeights {
            plane: main(),
            layer: 0,
            cells: (0..8).flat_map(|x| (0..3).map(move |y| [x, y])).collect(),
            target: -2,
            expected,
        },
    }
}

/// The members of the stamp of [`edit`]'s kind 20.
pub fn stamp_objects() -> Vec<StampObject> {
    [
        ("marker", [0, 0]),
        ("stone", [1, 1]),
        ("marker", [0, 0]),
        ("bush", [-1, 1]),
    ]
    .into_iter()
    .map(|(proto, offset)| StampObject {
        proto: proto.into(),
        offset,
    })
    .collect()
}

/// Dispatch `action` and Accept it.
pub fn accept(session: &mut Session, action: Action, records: &mut Records) {
    let kind = format!("{action:?}");
    session
        .dispatch(action, records)
        .unwrap_or_else(|error| panic!("{kind}: {} {error}", error.code()));
    session
        .dispatch(Action::Accept, records)
        .unwrap_or_else(|error| panic!("accept {kind}: {} {error}", error.code()));
}

/// The canonical proposal file for `body`, with its digest: what the
/// crate writes, so a test can change a body and keep it well formed.
pub fn document(body: &Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "{{\"body\":{body},\"body_sha256\":{},\"format\":\"tilewright.editor-proposal/1\"}}",
        Value::from(hex(&sha256(&body)))
    )
    .into_bytes()
}

pub fn sha256(text: &str) -> [u8; 32] {
    tilewright_edit::digest(Some(text)).unwrap()
}

/// The body of a proposal document.
pub fn body(bytes: &[u8]) -> Value {
    serde_json::from_slice::<Value>(bytes).unwrap()["body"].clone()
}

/// Recompute the transaction digest of every step of `body`.
pub fn fix_digests(body: &mut Value) {
    let Some(steps) = body.get_mut("steps").and_then(Value::as_array_mut) else {
        return;
    };
    for step in steps {
        if let Some(text) = step.get("transaction").and_then(Value::as_str) {
            let digest = hex(&sha256(text));
            step["transaction_sha256"] = Value::from(digest);
        }
    }
}
