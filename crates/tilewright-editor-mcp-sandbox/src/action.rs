// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The `edit` tool's `action`: the proposal format's Action form
//! (`docs/PROPOSALS.md`) as a JSON Schema, and the one rewrite the server
//! makes before the proposal crate's decoder, which stays the authority on
//! what an action is.

use std::fmt::Write as _;
use tilewright_editor_proposal::limits;
use tilewright_jsonc::Value;

/// Every action kind and its members besides `kind` and `expected`, in
/// the decoder's order. Each member's schema is [`member`]'s.
pub const KINDS: [(&str, &[&str]); 18] = [
    ("move", &["uid", "x", "y"]),
    ("set_field", &["uid", "field", "value"]),
    ("paint", &["plane", "layer", "grid", "x", "y", "proto"]),
    (
        "paint_stroke",
        &["plane", "layer", "grid", "cells", "proto"],
    ),
    (
        "fill_terrain",
        &["plane", "layer", "grid", "x", "y", "proto", "contiguous"],
    ),
    (
        "scatter_terrain",
        &["plane", "layer", "grid", "cells", "palette", "seed"],
    ),
    ("adjust_heights", &["plane", "layer", "cells", "amount"]),
    ("paint_zones", &["plane", "layer", "cells", "zone"]),
    ("remove", &["uid"]),
    ("place", &["plane", "layer", "x", "y", "proto"]),
    ("rename_zone", &["plane", "zone", "name"]),
    ("set_zone_parent", &["plane", "zone", "parent"]),
    ("create_zone", &["plane", "zone", "name", "parent"]),
    ("delete_zone", &["plane", "zone"]),
    ("create_anchor", &["plane", "anchor", "x", "y"]),
    ("move_anchor", &["plane", "anchor", "x", "y"]),
    ("delete_anchor", &["plane", "anchor"]),
    ("set_portal", &["uid", "target"]),
];

/// A string of an action: at most [`limits::TEXT_BYTES`] bytes (the
/// schema can only count characters) and no NUL.
fn text() -> String {
    format!(
        r#"{{"type":"string","maxLength":{},"pattern":"^[^\\u0000]*$"}}"#,
        limits::TEXT_BYTES
    )
}

fn optional_text() -> String {
    format!(r#"{{"anyOf":[{},{{"type":"null"}}]}}"#, text())
}

fn integer() -> String {
    format!(
        r#"{{"type":"integer","minimum":-{0},"maximum":{0}}}"#,
        limits::INTEGER
    )
}

fn layer() -> String {
    format!(
        r#"{{"type":"integer","minimum":{},"maximum":{}}}"#,
        i32::MIN,
        i32::MAX
    )
}

fn cell() -> String {
    format!(
        r#"{{"type":"array","items":{0},"minItems":2,"maxItems":2}}"#,
        integer()
    )
}

/// The schema of member `name` of an action of `kind`.
fn member(kind: &str, name: &str) -> String {
    match name {
        "x" | "y" => integer(),
        "layer" => layer(),
        "cells" => format!(
            r#"{{"type":"array","items":{},"minItems":1,"maxItems":{},"uniqueItems":true,"description":"Cells [x, y], distinct and in increasing order of x, then y."}}"#,
            cell(),
            limits::CELLS
        ),
        "value" => format!(
            r#"{{"description":"The field's new value: any JSON value, at most {} levels deep, {} values, {} members in an object and {} bytes encoded; integers within the World range."}}"#,
            limits::FIELD_VALUE_DEPTH,
            limits::FIELD_VALUE_ENTRIES,
            limits::OBJECT_KEYS,
            limits::FIELD_VALUE_BYTES
        ),
        "target" => format!(
            r#"{{"anyOf":[{{"type":"null"}},{{"type":"object","properties":{{"kind":{{"const":"anchor"}},"plane":{0},"anchor":{0}}},"required":["kind","plane","anchor"],"additionalProperties":false}},{{"type":"object","properties":{{"kind":{{"const":"location"}},"plane":{0},"layer":{1},"at":{2}}},"required":["kind","plane","layer","at"],"additionalProperties":false}}],"description":"Where the portal leads, or null to clear it."}}"#,
            text(),
            layer(),
            cell()
        ),
        "contiguous" => r#"{"type":"boolean","description":"The fill rule. true: set proto on the cells joined to the start cell (x, y) through edge neighbours (not diagonals) that hold the same terrain as the start cell (an empty start cell matches empty cells), within the catalog's active view. false: set proto on every cell of the active view that holds the same terrain as the start cell (an empty start cell matches empty cells). The start cell must be in the active view; cells already holding proto are left out."}"#.to_owned(),
        "palette" => format!(
            r#"{{"type":"string","maxLength":{},"pattern":"^[^\\u0000]*$","description":"The name of a palette in the catalog's editing.palettes (the catalog tool lists them) for this grid. The palette is read from the catalog when the step is prepared: each cell gets one of its values, drawn by weight from the seed and the cell alone. Cells already holding their drawn value are left out."}}"#,
            limits::TEXT_BYTES
        ),
        "seed" => format!(
            r#"{{"type":"integer","minimum":0,"maximum":{},"description":"The scatter seed. The same seed, palette and cell always draw the same value; choose a new one for a different pattern."}}"#,
            u32::MAX
        ),
        "amount" => format!(
            r#"{{"anyOf":[{{"type":"integer","minimum":-{0},"maximum":-1}},{{"type":"integer","minimum":1,"maximum":{0}}}],"description":"How much to raise (positive) or lower (negative) the tile height of each cell, once each, never 0. Needs the catalog's editing.heights range (the catalog tool reports it as heights): each cell becomes its height plus amount, kept from min to max. Cells at or beyond the end of the range the amount moves toward, and cells of map chunks that do not exist, are left out. A cell beyond the other end moves by amount and is kept in the range; if amount does not bring it into the range, the whole action is refused, naming the amount it needs."}}"#,
            limits::HEIGHT_AMOUNT
        ),
        "name" | "parent" => optional_text(),
        "zone" if kind == "paint_zones" => optional_text(),
        _ => text(),
    }
}

/// The schema of the `action` argument: one choice per kind, each with
/// exactly its members, all required (`null` for an absent option). The
/// choices are an `anyOf`, which more hosts take than `oneOf`; their `kind`
/// consts already exclude each other. Fixed-length arrays are `items` with
/// `minItems` and `maxItems`, which draft-07 and 2020-12 read alike.
pub fn action_schema() -> String {
    let revision = r#"{"type":"string","pattern":"^[0-9a-f]{64}$","description":"The sandbox revision the action was made for: state.revision."}"#;
    let mut kinds = String::new();
    let mut choices = String::new();
    for (index, (kind, members)) in KINDS.iter().enumerate() {
        if index > 0 {
            kinds.push(',');
            choices.push(',');
        }
        let _ = write!(kinds, "\"{kind}\"");
        let mut properties = format!(r#""kind":{{"const":"{kind}"}}"#);
        let mut required = String::from(r#""kind""#);
        for name in *members {
            let _ = write!(properties, r#","{name}":{}"#, member(kind, name));
            let _ = write!(required, r#","{name}""#);
        }
        let _ = write!(properties, r#","expected":{revision}"#);
        required.push_str(r#","expected""#);
        let _ = write!(
            choices,
            r#"{{"type":"object","properties":{{{properties}}},"required":[{required}],"additionalProperties":false}}"#
        );
    }
    format!(
        r#"{{"type":"object","properties":{{"kind":{{"enum":[{kinds}]}}}},"required":["kind","expected"],"anyOf":[{choices}],"description":"One Action in the proposal format's form (docs/PROPOSALS.md): kind and exactly that kind's members, every one present, null for an absent option. Strings count UTF-8 bytes on the server: at most {} bytes."}}"#,
        limits::TEXT_BYTES
    )
}

/// `action` with the numbers its integer members hold written as
/// integers when they have no fraction (`3.0`, `3e0`), as JSON Schema's
/// `integer` admits them, so that the decoder, which reads integers as
/// written, takes what the schema takes. Only the members that are
/// integers in every action are rewritten: `x`, `y`, `layer`, `seed`,
/// `amount`, the cells of `cells`, and a portal target's `layer` and `at`; a field `value`
/// is never touched, so it is recorded as written.
pub fn normalized(action: &Value) -> Value {
    let mut action = action.clone();
    if let Value::Object { members, .. } = &mut action {
        for member in members {
            match member.key.as_str() {
                "x" | "y" | "layer" | "seed" | "amount" => whole(&mut member.value),
                "cells" => {
                    if let Value::Array { items, .. } = &mut member.value {
                        items.iter_mut().for_each(pair);
                    }
                }
                "target" => {
                    if let Value::Object { members, .. } = &mut member.value {
                        for member in members {
                            match member.key.as_str() {
                                "layer" => whole(&mut member.value),
                                "at" => pair(&mut member.value),
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    action
}

/// The items of an array, each made [`whole`].
fn pair(value: &mut Value) {
    if let Value::Array { items, .. } = value {
        items.iter_mut().for_each(whole);
    }
}

/// A number with no fraction, of magnitude within the World range,
/// written as an integer.
fn whole(value: &mut Value) {
    if let Value::Float {
        value: number,
        span,
    } = *value
        && number.fract() == 0.0
        && number.abs() <= 9_007_199_254_740_991.0
    {
        #[allow(clippy::cast_possible_truncation)]
        let integer = number as i64;
        *value = Value::Integer {
            value: integer,
            span,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{KINDS, action_schema, normalized};
    use tilewright_jsonc::{Document, Limits, Segment};

    #[test]
    fn the_kinds_are_the_decoders_and_the_schema_is_json() {
        let schema: serde_json::Value = serde_json::from_str(&action_schema()).unwrap();
        let choices = schema["anyOf"].as_array().unwrap();
        assert_eq!(choices.len(), KINDS.len());
        for ((kind, members), choice) in KINDS.iter().zip(choices) {
            assert_eq!(choice["properties"]["kind"]["const"], *kind);
            let required: Vec<&str> = choice["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap())
                .collect();
            assert_eq!(required.len(), members.len() + 2, "{kind}");
            assert!(members.iter().all(|name| required.contains(name)), "{kind}");
        }
    }

    #[test]
    fn only_integer_members_are_rewritten() {
        let parse = |text: &str| Document::parse(text, &Limits::default()).unwrap();
        let action = parse(
            r#"{"x":3.0,"y":-0.0,"layer":1e0,"cells":[[1.0,2]],"target":{"layer":2.0,"at":[4.0,5.0],"plane":1.0},"value":7.0,"uid":1.5,"other":2.0}"#,
        );
        let rewritten = normalized(action.root());
        for (pointer, integer) in [
            ("/x", Some(3)),
            ("/y", Some(0)),
            ("/layer", Some(1)),
            ("/cells/0/0", Some(1)),
            ("/target/layer", Some(2)),
            ("/target/at/1", Some(5)),
            ("/target/plane", None),
            ("/value", None),
            ("/uid", None),
            ("/other", None),
        ] {
            let path: Vec<Segment<'_>> = pointer[1..]
                .split('/')
                .map(|part| {
                    part.parse::<usize>()
                        .map_or(Segment::Key(part), Segment::Index)
                })
                .collect();
            let value = rewritten.pointer(&path).unwrap();
            assert_eq!(value.as_i64(), integer, "{pointer}");
        }
        // A fraction, or a magnitude past the World range, stays a number.
        let kept = normalized(parse(r#"{"x":3.5,"y":1e300}"#).root());
        assert_eq!(kept.get("x").unwrap().as_i64(), None);
        assert_eq!(kept.get("y").unwrap().as_i64(), None);
    }
}
