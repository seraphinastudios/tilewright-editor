// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Raise, lower, flatten and smooth through the editing server: on a temporary copy of the
//! neutral sample with a height range and generated heights, `catalog`
//! reports the range, `adjust_heights` prepares one change, `tile` shows the
//! new heights after Accept, and the submitted proposal records the step
//! as intent. Linux only, as the sandbox server tests are.
#![cfg(target_os = "linux")]

mod common;

use common::{Layout, edit_json};
use serde_json::json;

/// Height `(x + 2y) % 7 - 3` in chunk `0_0` and the range -4 to 4.
fn hilly(world: &std::path::Path) {
    let rows: Vec<Vec<i64>> = (0..32_i64)
        .map(|y| (0..32_i64).map(|x| (x + 2 * y) % 7 - 3).collect())
        .collect();
    edit_json(world, "planes/main/layers/0/0_0.jsonc", |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
    edit_json(world, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = json!({"min": -4, "max": 4});
    });
}

fn tile_height(
    client: &mut common::Client<tilewright_editor_recovery::DurableRecovery>,
    x: i64,
    y: i64,
) -> i64 {
    client
        .call(
            "tile",
            &json!({"plane": "main", "layer": 0, "x": x, "y": y}),
        )
        .ok()["height"]
        .as_i64()
        .unwrap()
}

#[test]
fn an_agent_raises_and_lowers_heights_and_tile_shows_them() {
    let layout = Layout::new(hilly);
    let mut client = layout.open(None);
    assert_eq!(
        client.simple("catalog").ok()["heights"],
        json!({"min": -4, "max": 4})
    );
    // (0, 0) holds -3, (2, 0) holds -1, (3, 0) holds 0, (6, 0) holds 3.
    for (x, before) in [(0, -3), (2, -1), (3, 0), (6, 3)] {
        assert_eq!(tile_height(&mut client, x, 0), before);
    }
    let cells = json!([[0, 0], [2, 0], [3, 0], [6, 0]]);
    let edited = client.edit(
        json!({"kind": "adjust_heights", "plane": "main", "layer": 0, "cells": cells, "amount": 3}),
    );
    edited.ok();
    client.simple("accept").ok();
    // Each plus 3, kept at most 4.
    for (x, after) in [(0, 0), (2, 2), (3, 3), (6, 4)] {
        assert_eq!(tile_height(&mut client, x, 0), after);
    }
    // A cell already at the top: a raise there changes nothing.
    let none = client.edit(
        json!({"kind": "adjust_heights", "plane": "main", "layer": 0, "cells": [[6, 0]], "amount": 1}),
    );
    none.refused("change.none");
    // Lower, written as a whole number in floating point: the server takes
    // it as the integer.
    client
        .edit(json!({"kind": "adjust_heights", "plane": "main", "layer": 0, "cells": cells, "amount": -8.0}))
        .ok();
    client.simple("accept").ok();
    for x in [0, 2, 3, 6] {
        assert_eq!(tile_height(&mut client, x, 0), -4);
    }
    // 0 is not an amount.
    client
        .edit(json!({"kind": "adjust_heights", "plane": "main", "layer": 0, "cells": cells, "amount": 0}))
        .refused("proposal.invalid");
    let submitted = client.simple("submit");
    assert_eq!(submitted.ok()["proposal"]["steps"], 2);
}

#[test]
fn without_the_grant_the_catalog_reports_no_range_and_the_edit_is_refused() {
    let layout = Layout::new(|_| {});
    let mut client = layout.open(None);
    assert!(client.simple("catalog").ok().get("heights").is_none());
    client
        .edit(json!({"kind": "adjust_heights", "plane": "main", "layer": 0, "cells": [[0, 0]], "amount": 1}))
        .refused("grant.missing");
}

#[test]
fn an_agent_flattens_heights_to_a_target_in_the_range() {
    let layout = Layout::new(hilly);
    let mut client = layout.open(None);
    // (0, 0) holds -3, (2, 0) -1, (3, 0) 0, (6, 0) 3; flatten to 2.
    let cells = json!([[0, 0], [2, 0], [3, 0], [6, 0]]);
    client
        .edit(json!({"kind": "flatten_heights", "plane": "main", "layer": 0, "cells": cells, "target": 2}))
        .ok();
    client.simple("accept").ok();
    for x in [0, 2, 3, 6] {
        assert_eq!(tile_height(&mut client, x, 0), 2);
    }
    // Every cell already at the target: nothing to change.
    client
        .edit(json!({"kind": "flatten_heights", "plane": "main", "layer": 0, "cells": cells, "target": 2.0}))
        .refused("change.none");
    // Outside the range -4 to 4: not granted; past 1,024: invalid.
    client
        .edit(json!({"kind": "flatten_heights", "plane": "main", "layer": 0, "cells": cells, "target": 5}))
        .refused("grant.missing");
    client
        .edit(json!({"kind": "flatten_heights", "plane": "main", "layer": 0, "cells": cells, "target": 1025}))
        .refused("proposal.invalid");
    // The range's end, written as a whole number in floating point.
    client
        .edit(json!({"kind": "flatten_heights", "plane": "main", "layer": 0, "cells": [[0, 0], [1, 0]], "target": -4.0}))
        .ok();
    client.simple("accept").ok();
    assert_eq!(tile_height(&mut client, 0, 0), -4);
    assert_eq!(tile_height(&mut client, 1, 0), -4);
    let submitted = client.simple("submit");
    assert_eq!(submitted.ok()["proposal"]["steps"], 2);
}

#[test]
fn an_agent_smooths_heights_toward_their_neighbours() {
    let layout = Layout::new(hilly);
    let mut client = layout.open(None);
    // (3, 1) holds 2, its edge neighbours 0, 1, 3, -3 and its corners -1,
    // 1, 3, -2: (4 × 2 + 2 × 1 + 1) / 16 = 0.69, so 1. (3, 2) holds -3 and
    // reads (3, 1)'s committed 2, not its new 1: edges 2, 3, -2, -1,
    // corners 1, 3, -2, 0: (4 × -3 + 2 × 2 + 2) / 16 = -0.375, so 0.
    let cells = json!([[3, 1], [3, 2]]);
    client
        .edit(json!({"kind": "smooth_heights", "plane": "main", "layer": 0, "cells": cells, "kernel": 1}))
        .ok();
    client.simple("accept").ok();
    assert_eq!(tile_height(&mut client, 3, 1), 1);
    assert_eq!(tile_height(&mut client, 3, 2), 0);
    // Only kernel 1 exists; 1.0 is 1.
    for kernel in [json!(2), json!(0), json!("1"), json!(null)] {
        client
            .edit(json!({"kind": "smooth_heights", "plane": "main", "layer": 0, "cells": cells, "kernel": kernel}))
            .refused("proposal.invalid");
    }
    client
        .edit(json!({"kind": "smooth_heights", "plane": "main", "layer": 0, "cells": [[3, 2], [3, 3]], "kernel": 1.0}))
        .ok();
    client.simple("cancel").ok();
    // Outside the view: refused.
    client
        .edit(json!({"kind": "smooth_heights", "plane": "main", "layer": 0, "cells": [[3, 6]], "kernel": 1}))
        .refused("scope.outside_view");
    let submitted = client.simple("submit");
    assert_eq!(submitted.ok()["proposal"]["steps"], 1);
}
