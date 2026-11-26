// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A stamp through the editing server: `place_objects` with an inline list
//! of objects prepares one change, the review warns for occupied tiles,
//! `accept` reports the new uids as a list in object order (the output
//! schema allows it), `tile` shows them, and the submitted proposal
//! records the step with that list. Linux only, as the sandbox server
//! tests are.
#![cfg(target_os = "linux")]

mod common;

use common::Layout;
use serde_json::{Value, json};

fn stamp(x: i64, y: i64, objects: Value) -> Value {
    json!({"kind": "place_objects", "plane": "main", "layer": 0, "x": x, "y": y, "objects": objects})
}

#[test]
fn an_agent_places_a_stamp_and_accept_lists_its_uids() {
    let layout = Layout::new(|_| {});
    let mut client = layout.open(None);
    // marker_2 stands on (1, 4); the stamp's second object lands there,
    // its third on the first's tile. Offsets written as whole floats are
    // taken as integers.
    let edited = client.edit(stamp(
        0,
        4,
        json!([
            {"proto": "stone", "offset": [0, 0]},
            {"proto": "marker", "offset": [1.0, 0]},
            {"proto": "bush", "offset": [0, -0.0]},
        ]),
    ));
    let pending = edited.ok()["pending"].clone();
    assert_eq!(pending["files"].as_array().unwrap().len(), 1);
    assert_eq!(
        pending["warnings"][0],
        "Warning: 2 of the 3 new objects land on tiles that already hold an object or another new one; each is added on top."
    );
    let accepted = client.simple("accept");
    let placed = accepted.ok()["applied"]["placed"].clone();
    let uids: Vec<String> = placed
        .as_array()
        .unwrap()
        .iter()
        .map(|uid| uid.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(uids.len(), 3);
    assert!(uids.iter().all(|uid| uid.starts_with("o.")));
    // Each uid is world text in the reply, as a single placement's is.
    assert!(
        uids.iter()
            .all(|uid| accepted.strings.contains(&(uid.clone(), true)))
    );
    // On their tiles, in object order on top of each stack.
    let tile = |client: &mut common::Client<_>, x: i64| {
        client
            .call(
                "tile",
                &json!({"plane": "main", "layer": 0, "x": x, "y": 4}),
            )
            .ok()["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|object| object["uid"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(tile(&mut client, 0), [uids[0].clone(), uids[2].clone()]);
    assert_eq!(
        tile(&mut client, 1),
        ["marker_2".to_owned(), uids[1].clone()]
    );
    // A single placement still reports one string.
    client
        .edit(
            json!({"kind": "place", "plane": "main", "layer": 0, "x": 5, "y": 1, "proto": "lamp"}),
        )
        .ok();
    assert!(client.simple("accept").ok()["applied"]["placed"].is_string());
    // The whole stamp is refused when one object is outside the view or
    // not granted.
    client
        .edit(stamp(
            7,
            1,
            json!([{"proto": "marker", "offset": [0, 0]}, {"proto": "marker", "offset": [1, 0]}]),
        ))
        .refused("scope.outside_view");
    client
        .edit(stamp(
            2,
            1,
            json!([{"proto": "marker", "offset": [0, 0]}, {"proto": "grass", "offset": [1, 0]}]),
        ))
        .refused("grant.missing");
    client
        .edit(stamp(
            2,
            1,
            json!(vec![json!({"proto": "marker", "offset": [0, 0]}); 65]),
        ))
        .refused("proposal.budget");
    let submitted = client.simple("submit");
    assert_eq!(submitted.ok()["proposal"]["steps"], 2);
}

#[test]
fn the_largest_stamp_fits_one_request() {
    // 64 objects whose prototypes are the longest World identifiers (128
    // bytes) at the widest offsets: well inside the 64 KiB of a request.
    let long = format!("p{}", "x".repeat(127));
    let widest: Vec<Value> = (0..64)
        .map(|_| json!({"proto": long, "offset": [-4095, -4095]}))
        .collect();
    let mut action = stamp(
        -9_007_199_254_740_991,
        -9_007_199_254_740_991,
        json!(widest),
    );
    action["expected"] = json!("f".repeat(64));
    let line = common::call_line(1, "edit", &json!({ "action": action }));
    assert!(line.len() < 16 * 1024, "{} bytes", line.len());
    // 64 granted objects over the view's 48 tiles, 16 of them twice:
    // prepared and accepted as one change.
    let layout = Layout::new(|_| {});
    let mut client = layout.open(None);
    let objects: Vec<Value> = (0..64)
        .map(|index| json!({"proto": "marker", "offset": [index % 8, index / 8 % 6]}))
        .collect();
    client.edit(stamp(0, 0, json!(objects))).ok();
    let placed = client.simple("accept").ok()["applied"]["placed"].clone();
    assert_eq!(placed.as_array().unwrap().len(), 64);
}
