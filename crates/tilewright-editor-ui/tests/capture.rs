// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Capture from the map (`tilewright_editor_ui::capture`): which objects a
//! rectangle of the view copies and in which order, its refusals, and a
//! copy stamped through `ToolState` as the one `PlaceObjects` the engine
//! already has, with the grant's defaults instead of the originals' own
//! fields, layering and portal links. Every world is a temporary copy of
//! the MIT neutral sample (view `[0, 0, 8, 6]`, chunk size 32).
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, SelectionKey, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::capture::{Area, CaptureGesture, CaptureRefusal, CapturedCopy, Contents};
use tilewright_editor_ui::stamp::{MAXIMUM_STAMP_OBJECTS, StampObject};
use tilewright_editor_ui::stamps::{StampTurn, StampView};
use tilewright_editor_ui::tools::{ToolMode, ToolState};
use tilewright_editor_ui::uid::SeededUidSource;
use tilewright_editor_ui::{Action, Session, SessionPhase};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

fn copy_tree(source: &Path, destination: &Path) {
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
fn open(root: &Path) -> Session {
    let document = WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let mut session = Session::new(
        History::new(document),
        catalog,
        resources,
        tilewright_editor_ui::Actor::new("editor:prototype").unwrap(),
    );
    session.set_uid_source(Box::new(SeededUidSource::new(42)));
    session
}
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    configure(dir.path());
    let session = open(dir.path());
    (dir, session)
}
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}
fn object(proto: &str, dx: i64, dy: i64) -> StampObject {
    StampObject {
        proto: proto.into(),
        offset: [dx, dy],
    }
}
/// The objects of rectangle `a`-`b` in the session's committed World.
fn contents(session: &Session, a: [i64; 2], b: [i64; 2]) -> Contents {
    let area = Area::between(session.catalog(), a, b).unwrap();
    Contents::of(
        session.history().document().world(),
        session.catalog(),
        area,
    )
}
fn cell(x: i64, y: i64) -> SelectionKey {
    SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
    }
}
/// Extra top-level objects in the chunk at `(0, 0)`, appended in order:
/// `(uid, proto, tile)`.
fn with_objects(root: &Path, objects: &[(&str, &str, [u16; 2])]) {
    edit_json(root, CHUNK, |chunk| {
        let list = chunk["objects"].as_array_mut().unwrap();
        for (uid, proto, at) in objects {
            list.push(json!({"uid": uid, "at": at, "proto": proto, "layer": "item"}));
        }
    });
}

#[test]
fn rectangles_of_every_shape_copy_their_objects_row_by_row() {
    let (_dir, session) = fixture_with(|_| {});
    // The sample's view holds gate_east (7, 0), marker_1 (3, 2), gate_west
    // (0, 3) and marker_2 (1, 4); marker_negative (-1, 1) is outside it.
    let whole = contents(&session, [0, 0], [7, 5]);
    assert_eq!(
        whole.objects,
        [
            object("gate", 7, 0),
            object("marker", 3, 2),
            object("gate", 0, 3),
            object("marker", 1, 4),
        ]
    );
    assert!(whole.ungranted.is_empty());
    // A corner order never matters: the anchor is the top-left tile.
    for (a, b) in [
        ([1, 2], [3, 4]),
        ([3, 4], [1, 2]),
        ([1, 4], [3, 2]),
        ([3, 2], [1, 4]),
    ] {
        let area = Area::between(session.catalog(), a, b).unwrap();
        assert_eq!(
            area,
            Area {
                from: [1, 2],
                to: [3, 4]
            }
        );
        assert_eq!(area.size(), [3, 3]);
        assert_eq!(
            contents(&session, a, b).objects,
            [object("marker", 2, 0), object("marker", 0, 2)]
        );
    }
    // One tile, a row, a column.
    assert_eq!(
        contents(&session, [3, 2], [3, 2]).objects,
        [object("marker", 0, 0)]
    );
    assert_eq!(
        contents(&session, [0, 3], [7, 3]).objects,
        [object("gate", 0, 0)]
    );
    assert_eq!(
        contents(&session, [3, 0], [3, 5]).objects,
        [object("marker", 0, 2)]
    );
    // An empty tile and an empty rectangle hold nothing.
    for (a, b) in [([2, 2], [2, 2]), ([4, 1], [6, 5])] {
        let empty = contents(&session, a, b);
        assert!(empty.objects.is_empty());
        assert_eq!(empty.refusal(), Some(CaptureRefusal::Empty));
        let area = Area::between(session.catalog(), a, b).unwrap();
        assert_eq!(
            CapturedCopy::capture(
                session.history().document().world(),
                session.catalog(),
                area
            ),
            Err(CaptureRefusal::Empty)
        );
    }
}

#[test]
fn a_rectangle_is_clipped_to_the_view_and_never_reaches_past_it() {
    let (_dir, session) = fixture_with(|_| {});
    let catalog = session.catalog();
    // Corners past every edge are clipped to the view's own.
    assert_eq!(
        Area::between(catalog, [-3, -9], [20, 40]),
        Some(Area {
            from: [0, 0],
            to: [7, 5]
        })
    );
    assert_eq!(
        Area::between(catalog, [-1, 1], [0, 1]),
        Some(Area {
            from: [0, 1],
            to: [0, 1]
        })
    );
    // A rectangle wholly outside the view is none.
    assert_eq!(Area::between(catalog, [-5, 0], [-1, 5]), None);
    assert_eq!(Area::between(catalog, [8, 0], [9, 5]), None);
    assert_eq!(Area::between(catalog, [0, 6], [7, 6]), None);
    assert_eq!(Area::between(catalog, [i64::MIN, i64::MIN], [-1, -1]), None);
    // marker_negative at (-1, 1) is never copied; the clipped rectangle's
    // anchor is the view's corner.
    assert_eq!(
        contents(&session, [-1, 0], [3, 2]).objects,
        [object("marker", 3, 2)]
    );
}

#[test]
fn a_stack_keeps_its_order_and_other_layers_never_count() {
    let (_dir, session) = fixture_with(|root| {
        // Interleaved in the list: the stack on (3, 2) is marker_1, lamp,
        // crate; the stone on (5, 5) sits between them in the file.
        with_objects(
            root,
            &[
                ("lamp_1", "lamp", [3, 2]),
                ("stone_1", "stone", [5, 5]),
                ("crate_1", "crate", [3, 2]),
            ],
        );
        // The same tiles on spatial layer 1 of the same plane.
        let source = root.join(CHUNK);
        let mut other: Value = serde_json::from_slice(&fs::read(source).unwrap()).unwrap();
        other["layer"] = json!(1);
        other["objects"] = json!([
            {"uid": "upper_crate", "at": [3, 2], "proto": "crate", "layer": "item"},
            {"uid": "upper_lamp", "at": [4, 4], "proto": "lamp", "layer": "item"},
        ]);
        let directory = root.join("planes/main/layers/1");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("0_0.jsonc"),
            serde_json::to_vec_pretty(&other).unwrap(),
        )
        .unwrap();
    });
    assert!(
        session
            .history()
            .document()
            .world()
            .object("upper_crate")
            .is_some()
    );
    assert_eq!(
        contents(&session, [3, 2], [5, 5]).objects,
        [
            object("marker", 0, 0),
            object("lamp", 0, 0),
            object("crate", 0, 0),
            object("stone", 2, 3),
        ]
    );
}

/// The sample with `count` more crates over the first two rows of the
/// view, (0, 0)-(7, 1), in tile order, round and round.
fn with_crates(count: usize) -> (tempfile::TempDir, Session) {
    let crates: Vec<(String, [u16; 2])> = (0..count)
        .map(|index| {
            let tile = u16::try_from(index % 16).unwrap();
            (format!("crate_{index}"), [tile % 8, tile / 8])
        })
        .collect();
    fixture_with(|root| {
        let objects: Vec<_> = crates
            .iter()
            .map(|(uid, at)| (uid.as_str(), "crate", *at))
            .collect();
        with_objects(root, &objects);
    })
}

#[test]
fn more_than_sixty_four_objects_or_an_ungranted_prototype_copy_nothing() {
    // With gate_east at (7, 0), 63 crates make 64 objects in the first two
    // rows: exactly the most a copy holds.
    let (_dir, session) = with_crates(MAXIMUM_STAMP_OBJECTS - 1);
    let world = session.history().document().world();
    let catalog = session.catalog();
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Place);
    assert_eq!(capture(&mut tools, world, catalog, [0, 0], [7, 1]), 64);
    let held = tools.copy_choice().unwrap().clone();
    // One more crate is one too many.
    let (_dir, session) = with_crates(MAXIMUM_STAMP_OBJECTS);
    let world = session.history().document().world();
    let catalog = session.catalog();
    let two_rows = contents(&session, [0, 0], [7, 1]);
    assert_eq!(two_rows.objects.len(), 65);
    assert_eq!(two_rows.refusal(), Some(CaptureRefusal::TooMany(65)));
    assert_eq!(
        CaptureRefusal::TooMany(65).to_string(),
        "Nothing copied: the rectangle holds 65 objects, and a copy holds at most 64."
    );
    tools.toggle_capture().unwrap();
    tools.begin_capture([0, 0]).unwrap();
    tools.extend_capture([7, 1]);
    assert_eq!(
        tools.finish_capture(world, catalog),
        Some(Err(CaptureRefusal::TooMany(65)))
    );
    // Refused: the earlier copy stays and Copy area stays armed for
    // another try.
    assert_eq!(tools.copy_choice(), Some(&held));
    assert_eq!(tools.capture_gesture(), Some(CaptureGesture::Armed));
    // A smaller rectangle of the same world copies.
    tools.begin_capture([0, 0]).unwrap();
    tools.extend_capture([7, 0]);
    assert_eq!(tools.finish_capture(world, catalog), Some(Ok(33)));

    // An ungranted prototype: marker without its grant.
    let (_dir, session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            let objects = catalog["editing"]["objects"].as_array_mut().unwrap();
            objects.retain(|grant| grant["proto"] != "marker");
        });
    });
    let mixed = contents(&session, [0, 0], [7, 5]);
    assert_eq!(mixed.ungranted, ["marker"]);
    let refusal = mixed.refusal().unwrap();
    assert_eq!(refusal, CaptureRefusal::Ungranted(vec!["marker".into()]));
    assert_eq!(
        refusal.to_string(),
        "Nothing copied: Place cannot put down marker (not granted by editing.objects)."
    );
    assert_eq!(refusal.short(), "not granted: marker");
    // A rectangle without the ungranted object copies.
    assert!(contents(&session, [7, 0], [7, 0]).refusal().is_none());
}

#[test]
fn a_copy_is_the_chosen_stamp_until_a_prototype_or_catalog_stamp_replaces_it() {
    let (_dir, session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["stamps"] = json!([
                {"name": "post", "objects": [{"proto": "lamp", "offset": [0, 0]}]}
            ]);
        });
    });
    let world = session.history().document().world();
    let catalog = session.catalog();
    let mut tools = ToolState::default();
    // Copy area is Place mode's own.
    assert!(tools.toggle_capture().is_err());
    tools.set_mode(ToolMode::Place);
    tools.choose_placement(catalog, "crate").unwrap();
    tools.choose_stamp(catalog, "post").unwrap();
    tools.turn_stamp().unwrap();
    assert!(tools.begin_capture([1, 2]).is_err(), "not armed");
    assert_eq!(tools.finish_capture(world, catalog), None);
    assert!(tools.toggle_capture().unwrap());
    assert!(!tools.toggle_capture().unwrap());
    assert!(tools.toggle_capture().unwrap());
    // A release without a drag copies nothing.
    assert_eq!(tools.finish_capture(world, catalog), None);
    tools.begin_capture([3, 4]).unwrap();
    tools.extend_capture([1, 0]);
    tools.extend_capture([0, 2]);
    assert_eq!(
        tools.capture_area(catalog),
        Some(Area {
            from: [0, 2],
            to: [3, 4]
        })
    );
    assert_eq!(tools.finish_capture(world, catalog), Some(Ok(3)));
    // The copy replaces the catalog stamp, in its captured layout, and
    // Copy area ends.
    assert_eq!(tools.capture_gesture(), None);
    assert_eq!(tools.stamp_choice(), None);
    assert!(tools.stamp_chosen());
    assert_eq!(tools.stamp_turn(), StampTurn::NONE);
    let copied = vec![
        object("marker", 3, 0),
        object("gate", 0, 1),
        object("marker", 1, 2),
    ];
    assert_eq!(tools.copy_choice().unwrap().objects(), copied.as_slice());
    assert_eq!(tools.stamp_objects(catalog).unwrap(), copied);
    // A mode change keeps it, like a catalog stamp; it ends Copy area.
    tools.toggle_capture().unwrap();
    tools.set_mode(ToolMode::Terrain);
    assert_eq!(tools.capture_gesture(), None);
    tools.set_mode(ToolMode::Place);
    assert_eq!(tools.copy_choice().unwrap().objects(), copied.as_slice());
    // A catalog stamp replaces it; so does a prototype.
    tools.choose_stamp(catalog, "post").unwrap();
    assert!(tools.copy_choice().is_none());
    assert_eq!(tools.stamp_turn(), StampTurn::NONE);
    capture(&mut tools, world, catalog, [3, 2], [3, 2]);
    tools.choose_placement(catalog, "bush").unwrap();
    assert!(!tools.stamp_chosen());
    // Dropping it goes back to the chosen prototype.
    capture(&mut tools, world, catalog, [3, 2], [3, 2]);
    assert!(tools.drop_copy());
    assert!(!tools.drop_copy());
    assert_eq!(tools.placement_choice(), Some("bush"));
    let action = tools
        .place_action(catalog, &cell(5, 5), [0; 32], false)
        .unwrap();
    assert!(matches!(action, Action::Place { .. }), "{action:?}");
}

fn capture(
    tools: &mut ToolState,
    world: &tilewright_source::World,
    catalog: &tilewright_editor_preview::PreviewCatalog,
    a: [i64; 2],
    b: [i64; 2],
) -> usize {
    if tools.capture_gesture().is_none() {
        tools.toggle_capture().unwrap();
    }
    tools.begin_capture(a).unwrap();
    tools.extend_capture(b);
    tools.finish_capture(world, catalog).unwrap().unwrap()
}

#[test]
fn a_stamped_copy_is_place_objects_turned_and_mirrored_in_all_eight_states() {
    let (_dir, session) = fixture_with(|_| {});
    let world = session.history().document().world();
    let catalog = session.catalog();
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Place);
    tools.choose_placement(catalog, "crate").unwrap();
    // marker_1 (3, 2), gate_west (0, 3), marker_2 (1, 4): an asymmetric L.
    assert_eq!(capture(&mut tools, world, catalog, [0, 2], [3, 4]), 3);
    let copied = [[3, 0], [0, 1], [1, 2]];
    let protos = ["marker", "gate", "marker"];
    let mut seen = Vec::new();
    for mirrored in [false, true] {
        for turns in 0..4 {
            // From the captured layout: M, then R `turns` times.
            capture(&mut tools, world, catalog, [0, 2], [3, 4]);
            if mirrored {
                tools.mirror_stamp(StampView::TopDown).unwrap();
            }
            for _ in 0..turns {
                tools.turn_stamp().unwrap();
            }
            let expected: Vec<StampObject> = copied
                .iter()
                .zip(protos)
                .map(|(&[dx, dy], proto)| {
                    let mut offset = if mirrored { [-dx, dy] } else { [dx, dy] };
                    for _ in 0..turns {
                        offset = [-offset[1], offset[0]];
                    }
                    object(proto, offset[0], offset[1])
                })
                .collect();
            if !seen.contains(&expected) {
                seen.push(expected.clone());
            }
            let action = tools
                .place_action(catalog, &cell(4, 2), [9; 32], false)
                .unwrap();
            assert_eq!(
                action,
                Action::PlaceObjects {
                    plane: "main".into(),
                    layer: 0,
                    x: 4,
                    y: 2,
                    objects: expected,
                    expected: [9; 32],
                },
                "mirrored {mirrored}, {turns} turns"
            );
        }
    }
    assert_eq!(seen.len(), 8);
    // Pending or blocked, nothing is prepared.
    assert!(
        tools
            .place_action(catalog, &cell(4, 2), [9; 32], true)
            .is_err()
    );
}

#[test]
fn copies_get_the_grants_defaults_never_the_originals_fields_or_portals() {
    let (_dir, mut session) = fixture_with(|root| {
        // A stone with its own label and settings.
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"].as_array_mut().unwrap().push(json!({
                "uid": "stone_own", "at": [4, 2], "proto": "stone", "layer": "item",
                "fields": {"label": "My own stone", "settings": {"active": true, "visits": 9}}
            }));
        });
    });
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Place);
    tools.choose_placement(session.catalog(), "crate").unwrap();
    // marker_1 (3, 2) with "Trail marker", stone_own (4, 2), gate_west
    // (0, 3) with its portal.
    let count = {
        let world = session.history().document().world();
        capture(&mut tools, world, session.catalog(), [0, 2], [4, 3])
    };
    assert_eq!(count, 3);
    let action = tools
        .place_action(
            session.catalog(),
            &cell(2, 0),
            session.history().document().revision(),
            false,
        )
        .unwrap();
    let mut records = Records::default();
    session.dispatch(action, &mut records).unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let objects: Vec<&Value> = sealed["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|operation| &operation["object"])
        .collect();
    // Exactly the grant's layer and defaults: the marker and the gate have
    // none, the stone the catalog's "New stone"; no portal, no own uid.
    let strip = |object: &Value| {
        let mut object = object.clone();
        assert!(
            object["uid"].as_str().unwrap().starts_with("o."),
            "{object}"
        );
        object.as_object_mut().unwrap().remove("uid");
        object
    };
    assert_eq!(
        objects
            .iter()
            .map(|object| strip(object))
            .collect::<Vec<_>>(),
        [
            json!({"proto": "marker", "layer": "item"}),
            json!({"proto": "stone", "layer": "item",
                "fields": {"label": "New stone", "settings": {"active": false, "visits": 0}}}),
            json!({"proto": "gate", "layer": "item"}),
        ]
    );
    let tiles: Vec<&Value> = sealed["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|operation| &operation["at"])
        .collect();
    assert_eq!(tiles, [&json!([5, 0]), &json!([6, 0]), &json!([2, 1])]);
}
