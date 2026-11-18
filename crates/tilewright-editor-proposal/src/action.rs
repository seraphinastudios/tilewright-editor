// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The typed JSON form of the edit [`Action`]s: an object with a `kind`
//! and exactly the members of that kind, each always present (`null` for
//! an absent option). Revisions are 64 lowercase hexadecimal digits;
//! coordinates and layers are integers within their types and the World
//! format's integer range. Selection, review, history and reopen actions
//! have no form: they never appear in a proposal.
use serde_json::{Map, Number, Value, json};
use tilewright_editor_ui::{Action, PortalTarget};
use tilewright_jsonc as jsonc;

use crate::{ProposalError, budget, hex, invalid, limits, unhex};

/// The JSON form of an edit action.
///
/// # Errors
/// Refuses, with `proposal.invalid`, an action that is not an edit, a
/// coordinate outside the World format's integer range or a string with
/// NUL, which the World format cannot hold, and, with
/// `proposal.budget`, a string, stroke or field value over its bound.
#[allow(clippy::too_many_lines)]
pub fn encode_action(action: &Action) -> Result<Value, ProposalError> {
    let value = match action {
        Action::Move {
            uid,
            x,
            y,
            expected,
        } => {
            json!({"kind": "move", "uid": text(uid)?, "x": integer(*x)?, "y": integer(*y)?, "expected": hex(expected)})
        }
        Action::SetField {
            uid,
            field,
            value,
            expected,
        } => {
            field_value(value)?;
            json!({"kind": "set_field", "uid": text(uid)?, "field": text(field)?, "value": value, "expected": hex(expected)})
        }
        Action::Paint {
            plane,
            layer,
            grid,
            x,
            y,
            proto,
            expected,
        } => json!({"kind": "paint", "plane": text(plane)?, "layer": layer, "grid": text(grid)?,
            "x": integer(*x)?, "y": integer(*y)?, "proto": text(proto)?, "expected": hex(expected)}),
        Action::PaintStroke {
            plane,
            layer,
            grid,
            cells,
            proto,
            expected,
        } => {
            json!({"kind": "paint_stroke", "plane": text(plane)?, "layer": layer, "grid": text(grid)?,
            "cells": encode_cells(cells)?, "proto": text(proto)?, "expected": hex(expected)})
        }
        Action::FillTerrain {
            plane,
            layer,
            grid,
            x,
            y,
            proto,
            contiguous,
            expected,
        } => {
            json!({"kind": "fill_terrain", "plane": text(plane)?, "layer": layer, "grid": text(grid)?,
            "x": integer(*x)?, "y": integer(*y)?, "proto": text(proto)?, "contiguous": contiguous,
            "expected": hex(expected)})
        }
        Action::ScatterTerrain {
            plane,
            layer,
            grid,
            cells,
            palette,
            seed,
            expected,
        } => {
            json!({"kind": "scatter_terrain", "plane": text(plane)?, "layer": layer, "grid": text(grid)?,
            "cells": encode_cells(cells)?, "palette": text(palette)?, "seed": seed, "expected": hex(expected)})
        }
        Action::AdjustHeights {
            plane,
            layer,
            cells,
            amount,
            expected,
        } => json!({"kind": "adjust_heights", "plane": text(plane)?, "layer": layer,
            "cells": encode_cells(cells)?, "amount": height_amount(i64::from(*amount))?, "expected": hex(expected)}),
        Action::PaintZones {
            plane,
            layer,
            cells,
            zone,
            expected,
        } => json!({"kind": "paint_zones", "plane": text(plane)?, "layer": layer,
            "cells": encode_cells(cells)?, "zone": optional(zone.as_deref())?, "expected": hex(expected)}),
        Action::Remove { uid, expected } => {
            json!({"kind": "remove", "uid": text(uid)?, "expected": hex(expected)})
        }
        Action::Place {
            plane,
            layer,
            x,
            y,
            proto,
            expected,
        } => json!({"kind": "place", "plane": text(plane)?, "layer": layer, "x": integer(*x)?,
            "y": integer(*y)?, "proto": text(proto)?, "expected": hex(expected)}),
        Action::RenameZone {
            plane,
            zone,
            name,
            expected,
        } => json!({"kind": "rename_zone", "plane": text(plane)?, "zone": text(zone)?,
            "name": optional(name.as_deref())?, "expected": hex(expected)}),
        Action::SetZoneParent {
            plane,
            zone,
            parent,
            expected,
        } => json!({"kind": "set_zone_parent", "plane": text(plane)?, "zone": text(zone)?,
            "parent": optional(parent.as_deref())?, "expected": hex(expected)}),
        Action::CreateZone {
            plane,
            zone,
            name,
            parent,
            expected,
        } => json!({"kind": "create_zone", "plane": text(plane)?, "zone": text(zone)?,
            "name": optional(name.as_deref())?, "parent": optional(parent.as_deref())?, "expected": hex(expected)}),
        Action::DeleteZone {
            plane,
            zone,
            expected,
        } => {
            json!({"kind": "delete_zone", "plane": text(plane)?, "zone": text(zone)?, "expected": hex(expected)})
        }
        Action::CreateAnchor {
            plane,
            anchor,
            x,
            y,
            expected,
        } => json!({"kind": "create_anchor", "plane": text(plane)?, "anchor": text(anchor)?,
            "x": integer(*x)?, "y": integer(*y)?, "expected": hex(expected)}),
        Action::MoveAnchor {
            plane,
            anchor,
            x,
            y,
            expected,
        } => json!({"kind": "move_anchor", "plane": text(plane)?, "anchor": text(anchor)?,
            "x": integer(*x)?, "y": integer(*y)?, "expected": hex(expected)}),
        Action::DeleteAnchor {
            plane,
            anchor,
            expected,
        } => {
            json!({"kind": "delete_anchor", "plane": text(plane)?, "anchor": text(anchor)?, "expected": hex(expected)})
        }
        Action::SetPortal {
            uid,
            target,
            expected,
        } => {
            let target = match target {
                None => Value::Null,
                Some(PortalTarget::Anchor { plane, anchor }) => {
                    json!({"kind": "anchor", "plane": text(plane)?, "anchor": text(anchor)?})
                }
                Some(PortalTarget::Location { plane, layer, at }) => {
                    json!({"kind": "location", "plane": text(plane)?, "layer": layer,
                        "at": [integer(at[0])?, integer(at[1])?]})
                }
            };
            json!({"kind": "set_portal", "uid": text(uid)?, "target": target, "expected": hex(expected)})
        }
        Action::Select { .. }
        | Action::ClearSelection
        | Action::Accept
        | Action::Cancel
        | Action::Undo
        | Action::Redo
        | Action::Reopen => {
            return Err(invalid("only edit actions can be recorded in a proposal"));
        }
    };
    Ok(value)
}

/// Reads the JSON form of an edit action from `value`, as the strict
/// reader `tilewright_jsonc` read it.
///
/// # Errors
/// Refuses, with `proposal.invalid`, anything but exactly the form
/// [`encode_action`] writes, and, with `proposal.budget`, a string, stroke
/// or field value over its bound.
#[allow(clippy::too_many_lines)]
pub fn decode_action(value: &jsonc::Value) -> Result<Action, ProposalError> {
    let kind = value
        .get("kind")
        .and_then(jsonc::Value::as_str)
        .ok_or_else(|| invalid("an action needs a string kind"))?;
    let members: &[&str] = match kind {
        "move" => &["uid", "x", "y"],
        "set_field" => &["uid", "field", "value"],
        "paint" => &["plane", "layer", "grid", "x", "y", "proto"],
        "paint_stroke" => &["plane", "layer", "grid", "cells", "proto"],
        "fill_terrain" => &["plane", "layer", "grid", "x", "y", "proto", "contiguous"],
        "scatter_terrain" => &["plane", "layer", "grid", "cells", "palette", "seed"],
        "adjust_heights" => &["plane", "layer", "cells", "amount"],
        "paint_zones" => &["plane", "layer", "cells", "zone"],
        "remove" => &["uid"],
        "place" => &["plane", "layer", "x", "y", "proto"],
        "rename_zone" => &["plane", "zone", "name"],
        "set_zone_parent" => &["plane", "zone", "parent"],
        "create_zone" => &["plane", "zone", "name", "parent"],
        "delete_zone" => &["plane", "zone"],
        "create_anchor" | "move_anchor" => &["plane", "anchor", "x", "y"],
        "delete_anchor" => &["plane", "anchor"],
        "set_portal" => &["uid", "target"],
        _ => return Err(invalid("unknown action kind")),
    };
    let fields = Fields::of(value, members, &["kind", "expected"])?;
    let expected = fields.revision("expected")?;
    let action = match kind {
        "move" => Action::Move {
            uid: fields.text("uid")?,
            x: fields.integer("x")?,
            y: fields.integer("y")?,
            expected,
        },
        "set_field" => {
            let value = to_serde(fields.get("value"), 0)?;
            field_value(&value)?;
            Action::SetField {
                uid: fields.text("uid")?,
                field: fields.text("field")?,
                value,
                expected,
            }
        }
        "paint" => Action::Paint {
            plane: fields.text("plane")?,
            layer: fields.layer("layer")?,
            grid: fields.text("grid")?,
            x: fields.integer("x")?,
            y: fields.integer("y")?,
            proto: fields.text("proto")?,
            expected,
        },
        "paint_stroke" => Action::PaintStroke {
            plane: fields.text("plane")?,
            layer: fields.layer("layer")?,
            grid: fields.text("grid")?,
            cells: fields.cells("cells")?,
            proto: fields.text("proto")?,
            expected,
        },
        "fill_terrain" => Action::FillTerrain {
            plane: fields.text("plane")?,
            layer: fields.layer("layer")?,
            grid: fields.text("grid")?,
            x: fields.integer("x")?,
            y: fields.integer("y")?,
            proto: fields.text("proto")?,
            contiguous: fields.boolean("contiguous")?,
            expected,
        },
        "scatter_terrain" => Action::ScatterTerrain {
            plane: fields.text("plane")?,
            layer: fields.layer("layer")?,
            grid: fields.text("grid")?,
            cells: fields.cells("cells")?,
            palette: fields.text("palette")?,
            seed: fields.seed("seed")?,
            expected,
        },
        "adjust_heights" => Action::AdjustHeights {
            plane: fields.text("plane")?,
            layer: fields.layer("layer")?,
            cells: fields.cells("cells")?,
            amount: height_amount(fields.integer("amount")?)?,
            expected,
        },
        "paint_zones" => Action::PaintZones {
            plane: fields.text("plane")?,
            layer: fields.layer("layer")?,
            cells: fields.cells("cells")?,
            zone: fields.optional("zone")?,
            expected,
        },
        "remove" => Action::Remove {
            uid: fields.text("uid")?,
            expected,
        },
        "place" => Action::Place {
            plane: fields.text("plane")?,
            layer: fields.layer("layer")?,
            x: fields.integer("x")?,
            y: fields.integer("y")?,
            proto: fields.text("proto")?,
            expected,
        },
        "rename_zone" => Action::RenameZone {
            plane: fields.text("plane")?,
            zone: fields.text("zone")?,
            name: fields.optional("name")?,
            expected,
        },
        "set_zone_parent" => Action::SetZoneParent {
            plane: fields.text("plane")?,
            zone: fields.text("zone")?,
            parent: fields.optional("parent")?,
            expected,
        },
        "create_zone" => Action::CreateZone {
            plane: fields.text("plane")?,
            zone: fields.text("zone")?,
            name: fields.optional("name")?,
            parent: fields.optional("parent")?,
            expected,
        },
        "delete_zone" => Action::DeleteZone {
            plane: fields.text("plane")?,
            zone: fields.text("zone")?,
            expected,
        },
        "create_anchor" => Action::CreateAnchor {
            plane: fields.text("plane")?,
            anchor: fields.text("anchor")?,
            x: fields.integer("x")?,
            y: fields.integer("y")?,
            expected,
        },
        "move_anchor" => Action::MoveAnchor {
            plane: fields.text("plane")?,
            anchor: fields.text("anchor")?,
            x: fields.integer("x")?,
            y: fields.integer("y")?,
            expected,
        },
        "delete_anchor" => Action::DeleteAnchor {
            plane: fields.text("plane")?,
            anchor: fields.text("anchor")?,
            expected,
        },
        _ => Action::SetPortal {
            uid: fields.text("uid")?,
            target: portal_target(fields.get("target"))?,
            expected,
        },
    };
    Ok(action)
}

fn portal_target(value: &jsonc::Value) -> Result<Option<PortalTarget>, ProposalError> {
    if matches!(value, jsonc::Value::Null { .. }) {
        return Ok(None);
    }
    let kind = value
        .get("kind")
        .and_then(jsonc::Value::as_str)
        .ok_or_else(|| invalid("a portal target needs a string kind"))?;
    Ok(Some(match kind {
        "anchor" => {
            let fields = Fields::of(value, &["plane", "anchor"], &["kind"])?;
            PortalTarget::Anchor {
                plane: fields.text("plane")?,
                anchor: fields.text("anchor")?,
            }
        }
        "location" => {
            let fields = Fields::of(value, &["plane", "layer", "at"], &["kind"])?;
            let at = cell(fields.get("at"))?;
            PortalTarget::Location {
                plane: fields.text("plane")?,
                layer: fields.layer("layer")?,
                at,
            }
        }
        _ => return Err(invalid("unknown portal target kind")),
    }))
}

/// The members of one object, checked to be exactly `members` and
/// `common`.
struct Fields<'v> {
    members: &'v [jsonc::Member],
}
impl<'v> Fields<'v> {
    fn of(
        value: &'v jsonc::Value,
        members: &[&str],
        common: &[&str],
    ) -> Result<Self, ProposalError> {
        let found = value
            .as_object()
            .ok_or_else(|| invalid("an action is an object"))?;
        // The reader refuses duplicate keys, so equal counts and every
        // member known means exactly these members.
        if found.len() != members.len() + common.len()
            || !found.iter().all(|member| {
                members.contains(&member.key.as_str()) || common.contains(&member.key.as_str())
            })
        {
            return Err(invalid("an action has exactly the members of its kind"));
        }
        Ok(Self { members: found })
    }
    fn get(&self, key: &str) -> &'v jsonc::Value {
        &self
            .members
            .iter()
            .find(|member| member.key == key)
            .expect("members were checked")
            .value
    }
    fn text(&self, key: &str) -> Result<String, ProposalError> {
        let value = self
            .get(key)
            .as_str()
            .ok_or_else(|| invalid(format!("action member {key} is a string")))?;
        text(value).map(str::to_owned)
    }
    fn optional(&self, key: &str) -> Result<Option<String>, ProposalError> {
        match self.get(key) {
            jsonc::Value::Null { .. } => Ok(None),
            _ => self.text(key).map(Some),
        }
    }
    fn integer(&self, key: &str) -> Result<i64, ProposalError> {
        self.get(key)
            .as_i64()
            .ok_or_else(|| invalid(format!("action member {key} is an integer")))
    }
    fn boolean(&self, key: &str) -> Result<bool, ProposalError> {
        match self.get(key) {
            jsonc::Value::Bool { value, .. } => Ok(*value),
            _ => Err(invalid(format!("action member {key} is a boolean"))),
        }
    }
    /// A scatter seed: an integer from 0 to 2^32 - 1.
    fn seed(&self, key: &str) -> Result<u32, ProposalError> {
        u32::try_from(self.integer(key)?)
            .map_err(|_| invalid(format!("action member {key} is outside the seed range")))
    }
    fn layer(&self, key: &str) -> Result<i32, ProposalError> {
        i32::try_from(self.integer(key)?)
            .map_err(|_| invalid(format!("action member {key} is outside the layer range")))
    }
    fn revision(&self, key: &str) -> Result<[u8; 32], ProposalError> {
        self.get(key)
            .as_str()
            .and_then(unhex)
            .ok_or_else(|| invalid(format!("action member {key} is a revision")))
    }
    fn cells(&self, key: &str) -> Result<Vec<[i64; 2]>, ProposalError> {
        let items = self
            .get(key)
            .as_array()
            .ok_or_else(|| invalid(format!("action member {key} is an array of cells")))?;
        let cells = items.iter().map(cell).collect::<Result<Vec<_>, _>>()?;
        stroke_cells(&cells)?;
        Ok(cells)
    }
}

fn cell(value: &jsonc::Value) -> Result<[i64; 2], ProposalError> {
    match value.as_array() {
        Some([x, y]) => match (x.as_i64(), y.as_i64()) {
            (Some(x), Some(y)) => Ok([x, y]),
            _ => Err(invalid("a cell is two integers")),
        },
        _ => Err(invalid("a cell is two integers")),
    }
}

fn text(value: &str) -> Result<&str, ProposalError> {
    if value.len() > limits::TEXT_BYTES {
        return Err(budget("an action string is too long"));
    }
    no_nul(value)?;
    Ok(value)
}
/// The strict reader refuses NUL in strings, escaped or not, as World does.
fn no_nul(value: &str) -> Result<(), ProposalError> {
    if value.contains('\0') {
        return Err(invalid("a string holds NUL"));
    }
    Ok(())
}
fn optional(value: Option<&str>) -> Result<Value, ProposalError> {
    Ok(value
        .map(text)
        .transpose()?
        .map_or(Value::Null, Value::from))
}
fn integer(value: i64) -> Result<i64, ProposalError> {
    if value.unsigned_abs() > limits::INTEGER.unsigned_abs() {
        return Err(invalid("a coordinate is outside the World integer range"));
    }
    Ok(value)
}
/// A height adjustment's amount: an integer, not 0, at most
/// [`limits::HEIGHT_AMOUNT`] either way, the engine's rule.
fn height_amount(amount: i64) -> Result<i32, ProposalError> {
    i32::try_from(amount)
        .ok()
        .filter(|amount| tilewright_editor_ui::sculpt::check_amount(*amount).is_ok())
        .ok_or_else(|| invalid("a height amount is an integer from -1024 to 1024, not 0"))
}

/// The engine's rule for a stroke's cells (`stroke_cells` in the ui
/// crate's session): at least one, at most [`limits::CELLS`], distinct
/// and in increasing order.
fn stroke_cells(cells: &[[i64; 2]]) -> Result<(), ProposalError> {
    if cells.len() > limits::CELLS {
        return Err(budget("a stroke has too many cells"));
    }
    if cells.is_empty() {
        return Err(invalid("a stroke has at least one cell"));
    }
    if !cells.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(invalid(
            "a stroke's cells are distinct and in increasing order",
        ));
    }
    Ok(())
}
fn encode_cells(cells: &[[i64; 2]]) -> Result<Value, ProposalError> {
    stroke_cells(cells)?;
    cells
        .iter()
        .map(|&[x, y]| Ok(json!([integer(x)?, integer(y)?])))
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// A field value within the engine's budget and the World format's
/// numbers: at most [`limits::FIELD_VALUE_DEPTH`] levels of nesting,
/// [`limits::FIELD_VALUE_ENTRIES`] values and
/// [`limits::FIELD_VALUE_BYTES`] encoded, every integer within
/// [`limits::INTEGER`].
fn field_value(value: &Value) -> Result<(), ProposalError> {
    fn walk(value: &Value, depth: usize) -> Result<(), ProposalError> {
        if depth > limits::FIELD_VALUE_DEPTH {
            return Err(budget("a field value is nested too deeply"));
        }
        match value {
            Value::Number(number) if !number.is_f64() => {
                if number.as_i64().is_none_or(|n| integer(n).is_err()) {
                    return Err(invalid(
                        "a field value integer is outside the World integer range",
                    ));
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, depth + 1)?;
                }
            }
            Value::Object(members) => {
                if members.len() > limits::OBJECT_KEYS {
                    return Err(budget("a field value object has too many members"));
                }
                for (key, member) in members {
                    no_nul(key)?;
                    walk(member, depth + 1)?;
                }
            }
            Value::String(text) => no_nul(text)?,
            _ => {}
        }
        Ok(())
    }
    walk(value, 0)?;
    if entries(value) > limits::FIELD_VALUE_ENTRIES {
        return Err(budget("a field value holds too many entries"));
    }
    if value.to_string().len() > limits::FIELD_VALUE_BYTES {
        return Err(budget("a field value is too large"));
    }
    Ok(())
}

/// How many values `value` holds, itself included, as the engine counts.
fn entries(value: &Value) -> usize {
    1 + match value {
        Value::Array(items) => items.iter().map(entries).sum(),
        Value::Object(members) => members.values().map(entries).sum(),
        _ => 0,
    }
}

/// The serde form of a JSON value read by the strict reader. Depth is
/// bounded by the reader's own limit and checked again by
/// [`field_value`].
fn to_serde(value: &jsonc::Value, depth: usize) -> Result<Value, ProposalError> {
    if depth > limits::FIELD_VALUE_DEPTH {
        return Err(budget("a field value is nested too deeply"));
    }
    Ok(match value {
        jsonc::Value::Null { .. } => Value::Null,
        jsonc::Value::Bool { value, .. } => Value::Bool(*value),
        jsonc::Value::Integer { value, .. } => Value::from(*value),
        jsonc::Value::Float { value, .. } => Number::from_f64(*value)
            .map(Value::Number)
            .ok_or_else(|| invalid("a field value number is not finite"))?,
        jsonc::Value::String { value, .. } => Value::String(value.clone()),
        jsonc::Value::Array { items, .. } => Value::Array(
            items
                .iter()
                .map(|item| to_serde(item, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        jsonc::Value::Object { members, .. } => {
            let mut map = Map::new();
            for member in members {
                map.insert(member.key.clone(), to_serde(&member.value, depth + 1)?);
            }
            Value::Object(map)
        }
    })
}
