// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The `edit` tool's input schema, as `tools/list` sends it, against the
//! server: for every action kind, at and past each bound, a missing or
//! extra member and an unknown kind, the schema and the server agree on
//! whether the action has the proposal format's form. The schema is
//! evaluated by a small JSON Schema evaluator of the keywords the schema
//! uses, which refuses any other keyword, so the schema cannot grow a
//! keyword the test does not check. It knows no `oneOf` and no
//! `prefixItems`: the schema uses only keywords draft-07 and 2020-12 read
//! alike.
#![cfg(target_os = "linux")]

mod common;

use common::{Client, Layout, call_line, checked_call, request};
use serde_json::{Value, json};
use tilewright_editor_recovery::DurableRecovery;

/// Whether `value` matches `schema`, for the keywords the editing
/// server's schemas use.
#[allow(clippy::too_many_lines)]
fn matches(schema: &Value, value: &Value) -> bool {
    let Some(keywords) = schema.as_object() else {
        return schema
            .as_bool()
            .expect("a schema is an object or a boolean");
    };
    keywords
        .iter()
        .all(|(keyword, argument)| match keyword.as_str() {
            "description" | "x-world-text" => true,
            "type" => match argument {
                Value::String(name) => has_type(name, value),
                Value::Array(names) => names
                    .iter()
                    .any(|name| has_type(name.as_str().unwrap(), value)),
                _ => panic!("type {argument}"),
            },
            "const" => value == argument,
            "enum" => argument.as_array().unwrap().contains(value),
            "properties" => value.as_object().is_none_or(|members| {
                argument
                    .as_object()
                    .unwrap()
                    .iter()
                    .all(|(name, property)| {
                        members
                            .get(name)
                            .is_none_or(|member| matches(property, member))
                    })
            }),
            "required" => value.as_object().is_none_or(|members| {
                argument
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|name| members.contains_key(name.as_str().unwrap()))
            }),
            "additionalProperties" => {
                assert_eq!(argument, &json!(false));
                let known = keywords["properties"].as_object().unwrap();
                value
                    .as_object()
                    .is_none_or(|members| members.keys().all(|name| known.contains_key(name)))
            }
            "anyOf" => argument
                .as_array()
                .unwrap()
                .iter()
                .any(|choice| matches(choice, value)),
            "items" => value
                .as_array()
                .is_none_or(|items| items.iter().all(|item| matches(argument, item))),
            "minItems" => value
                .as_array()
                .is_none_or(|items| items.len() as u64 >= argument.as_u64().unwrap()),
            "maxItems" => value
                .as_array()
                .is_none_or(|items| items.len() as u64 <= argument.as_u64().unwrap()),
            "uniqueItems" => value.as_array().is_none_or(|items| {
                items
                    .iter()
                    .enumerate()
                    .all(|(index, item)| !items[..index].contains(item))
            }),
            "minimum" => value
                .as_f64()
                .is_none_or(|number| number >= argument.as_f64().unwrap()),
            "maximum" => value
                .as_f64()
                .is_none_or(|number| number <= argument.as_f64().unwrap()),
            "maxLength" => value
                .as_str()
                .is_none_or(|text| text.chars().count() as u64 <= argument.as_u64().unwrap()),
            "pattern" => value
                .as_str()
                .is_none_or(|text| match argument.as_str().unwrap() {
                    "^[0-9a-f]{64}$" => {
                        text.len() == 64
                            && text
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    }
                    "^[^\\u0000]*$" => !text.contains('\0'),
                    other => panic!("pattern {other}"),
                }),
            other => panic!("the evaluator does not know {other}"),
        })
}

fn has_type(name: &str, value: &Value) -> bool {
    match name {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "number" => value.is_number(),
        // JSON Schema's integer: any number with no fraction.
        "integer" => value.as_f64().is_some_and(|number| number.fract() == 0.0),
        other => panic!("type {other}"),
    }
}

/// Whether the server takes `action` as an action (the engine may refuse
/// it afterwards); a pending change it prepared is cancelled.
fn server_takes(client: &mut Client<DurableRecovery>, action: &Value) -> bool {
    let line = call_line(1, "edit", &json!({ "action": action }));
    let replies = client.session(&[line]);
    let (line, reply) = replies.into_iter().next().unwrap();
    if reply.get("error").is_some() {
        // The parser's refusal: an integer past 2^53 - 1, NUL, too many
        // items.
        assert_eq!(reply["error"]["code"], -32700, "{line}");
        return false;
    }
    let called = checked_call("edit", line, reply);
    let structured = called.structured();
    if structured["ok"] == true {
        client.simple("cancel").ok();
        return true;
    }
    let code = structured["code"].as_str().unwrap();
    !["proposal.invalid", "proposal.budget", "request.invalid"].contains(&code)
}

/// One action of each kind, in the decoder's order, every member at a
/// plausible value.
fn bases(revision: &str) -> Vec<Value> {
    let anchor = json!({"kind": "anchor", "plane": "main", "anchor": "a"});
    [
        json!({"kind": "move", "uid": "marker_1", "x": 4, "y": 2}),
        json!({"kind": "set_field", "uid": "marker_1", "field": "label", "value": {"a": [1, 2.5, null, true]}}),
        json!({"kind": "paint", "plane": "main", "layer": 0, "grid": "floor", "x": 0, "y": 0, "proto": "grass"}),
        json!({"kind": "paint_stroke", "plane": "main", "layer": 0, "grid": "floor", "cells": [[0, 0], [0, 1]], "proto": "grass"}),
        json!({"kind": "fill_terrain", "plane": "main", "layer": 0, "grid": "floor", "x": 0, "y": 0, "proto": "sand", "contiguous": true}),
        json!({"kind": "scatter_terrain", "plane": "main", "layer": 0, "grid": "floor", "cells": [[0, 0], [0, 1]], "palette": "speckle", "seed": 1234}),
        json!({"kind": "paint_zones", "plane": "main", "layer": 0, "cells": [[0, 0]], "zone": null}),
        json!({"kind": "remove", "uid": "marker_1"}),
        json!({"kind": "place", "plane": "main", "layer": 0, "x": 1, "y": 1, "proto": "marker"}),
        json!({"kind": "rename_zone", "plane": "main", "zone": "z", "name": null}),
        json!({"kind": "set_zone_parent", "plane": "main", "zone": "z", "parent": "y"}),
        json!({"kind": "create_zone", "plane": "main", "zone": "z", "name": "Z", "parent": null}),
        json!({"kind": "delete_zone", "plane": "main", "zone": "z"}),
        json!({"kind": "create_anchor", "plane": "main", "anchor": "a", "x": 1, "y": 1}),
        json!({"kind": "move_anchor", "plane": "main", "anchor": "a", "x": 2, "y": 1}),
        json!({"kind": "delete_anchor", "plane": "main", "anchor": "a"}),
        json!({"kind": "set_portal", "uid": "marker_1", "target": anchor}),
    ]
    .into_iter()
    .map(|mut action| {
        action["expected"] = json!(revision);
        action
    })
    .collect()
}

/// Variations of `base` at and past the bounds of each of its members.
fn variations(base: &Value) -> Vec<Value> {
    const MOST: i64 = (1 << 53) - 1;
    let with = |name: &str, value: Value| {
        let mut action = base.clone();
        action[name] = value;
        action
    };
    let mut found = Vec::new();
    for (name, value) in base.as_object().unwrap() {
        let mut missing = base.clone();
        missing.as_object_mut().unwrap().remove(name);
        found.push(missing);
        match name.as_str() {
            "kind" => {
                found.push(with(name, json!("teleport")));
                found.push(with(name, json!("Move")));
            }
            "expected" => {
                found.push(with(name, json!("A".repeat(64))));
                found.push(with(name, json!(value.as_str().unwrap()[..63])));
            }
            "x" | "y" => {
                for number in [json!(MOST), json!(-MOST), json!(3.0), json!(1e3)] {
                    found.push(with(name, number));
                }
                found.push(with(name, json!(3.5)));
                found.push(with(name, json!("3")));
                found.push(with(name, json!(9.1e15)));
            }
            "layer" => {
                for number in [i64::from(i32::MAX), i64::from(i32::MIN)] {
                    found.push(with(name, json!(number)));
                    found.push(with(name, json!(number + number.signum())));
                }
                found.push(with(name, json!(0.0)));
            }
            "cells" => {
                let row = |count: i64| json!((0..count).map(|y| [0, y]).collect::<Vec<_>>());
                found.push(with(name, row(1)));
                found.push(with(name, row(4096)));
                found.push(with(name, row(4097)));
                found.push(with(name, json!([])));
                found.push(with(name, json!([[0, 0], [0, 0]])));
                found.push(with(name, json!([[0, 0, 1]])));
                found.push(with(name, json!([[0.0, 1.0]])));
                found.push(with(name, json!([[0, 0.5]])));
            }
            "target" => {
                found.push(with(name, Value::Null));
                found.push(with(
                    name,
                    json!({"kind": "location", "plane": "main", "layer": 0, "at": [1.0, 2]}),
                ));
                found.push(with(
                    name,
                    json!({"kind": "location", "plane": "main", "layer": 0, "at": [1]}),
                ));
                found.push(with(name, json!({"kind": "anchor", "plane": "main"})));
                found.push(with(
                    name,
                    json!({"kind": "zone", "plane": "main", "anchor": "a"}),
                ));
            }
            "contiguous" => {
                found.push(with(name, json!(false)));
                found.push(with(name, Value::Null));
                found.push(with(name, json!(1)));
                found.push(with(name, json!(0)));
                found.push(with(name, json!("true")));
            }
            "seed" => {
                for number in [0, i64::from(u32::MAX)] {
                    found.push(with(name, json!(number)));
                }
                found.push(with(name, json!(i64::from(u32::MAX) + 1)));
                found.push(with(name, json!(-1)));
                found.push(with(name, json!(7.0)));
                found.push(with(name, json!(7.5)));
                found.push(with(name, json!("7")));
                found.push(with(name, Value::Null));
            }
            "value" => {
                found.push(with(name, Value::Null));
                found.push(with(name, json!("text")));
            }
            _ => {
                // A string member: at its bound, past it, null, another type.
                found.push(with(name, json!("u".repeat(4096))));
                found.push(with(name, json!("u".repeat(4097))));
                found.push(with(name, Value::Null));
                found.push(with(name, json!(7)));
            }
        }
    }
    found.push(with("extra", json!(1)));
    found.push(json!([base]));
    found
}

#[test]
fn the_action_schema_and_the_server_agree_at_every_bound() {
    let layout = Layout::new(|_| {});
    let mut client = layout.open(None);
    let replies = client.session(&[request(1, "tools/list", json!({}))]);
    let tools = replies[0].1["result"]["tools"].as_array().unwrap();
    let edit = tools.iter().find(|tool| tool["name"] == "edit").unwrap();
    let input = &edit["inputSchema"];
    let schema = &input["properties"]["action"];
    let revision = client.revision();
    let mut checked = 0;
    for base in bases(&revision) {
        let kind = base["kind"].as_str().unwrap().to_owned();
        assert!(matches(schema, &base), "{kind}");
        assert!(server_takes(&mut client, &base), "{kind}");
        for action in variations(&base) {
            let schema_takes = matches(schema, &action);
            assert_eq!(
                schema_takes,
                server_takes(&mut client, &action),
                "{kind}: {}",
                action.to_string().get(..400).unwrap_or(&action.to_string())
            );
            assert_eq!(matches(input, &json!({ "action": action })), schema_takes);
            checked += 1;
        }
    }
    assert!(checked > 300, "{checked}");
    assert!(!matches(input, &json!({})));
    assert!(!matches(
        input,
        &json!({"action": bases(&revision)[0], "x": 1})
    ));
}

/// What JSON Schema cannot say, each refused by the server: the decoder
/// stays the authority, and docs/MCP-EDITING.md names each of these.
#[test]
fn what_the_schema_cannot_express_the_server_still_refuses() {
    let layout = Layout::new(|_| {});
    let mut client = layout.open(None);
    let replies = client.session(&[request(1, "tools/list", json!({}))]);
    let tools = replies[0].1["result"]["tools"].as_array().unwrap();
    let schema = tools.iter().find(|tool| tool["name"] == "edit").unwrap()["inputSchema"]
        ["properties"]["action"]
        .clone();
    let revision = client.revision();
    let base = |action: Value| {
        let mut action = action;
        action["expected"] = json!(revision);
        action
    };
    let deep = (0..9).fold(json!(1), |value, _| json!([value]));
    for action in [
        // Cells out of order.
        base(
            json!({"kind": "paint_zones", "plane": "main", "layer": 0, "cells": [[0, 1], [0, 0]], "zone": null}),
        ),
        // A string of 4,096 characters but more bytes.
        base(json!({"kind": "remove", "uid": "\u{e9}".repeat(4096)})),
        // A field value past the engine's budget.
        base(json!({"kind": "set_field", "uid": "marker_1", "field": "label", "value": deep})),
    ] {
        assert!(matches(&schema, &action), "{action}");
        assert!(!server_takes(&mut client, &action), "{action}");
    }
}
