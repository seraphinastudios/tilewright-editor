// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The tools' output schemas, written once as values: `tools/list` writes
//! them as JSON Schema (2020-12, the revision's default), and
//! [`Schema::check`] checks a result against them, which the tests do for
//! every result they see.
//!
//! A string the world wrote, or that may quote it, has the schema keyword
//! `"x-world-text": true`. Unknown keywords are annotations in JSON
//! Schema, so validators ignore it; it tells a client, mechanically, which
//! values are world data. Every other string is this editor's own: a
//! format tag, a code, a kind, a cursor or a revision.

use std::fmt::Write as _;
use tilewright_editor_observe::node::json_string;
use tilewright_jsonc::Value;

/// One schema.
#[derive(Clone, Debug, PartialEq)]
pub enum Schema {
    /// A string this editor wrote.
    Text,
    /// A string the world wrote, or that may quote it.
    World,
    Integer,
    Boolean,
    /// 64 lowercase hex digits.
    Revision,
    /// One of these strings, this editor's.
    Enum(&'static [&'static str]),
    /// Exactly this string.
    Const(&'static str),
    /// A field value: a world string, a number, a boolean or null.
    Scalar,
    Nullable(Box<Schema>),
    List(Box<Schema>),
    /// An object with exactly these properties, the required ones always.
    Object(Vec<Property>),
    /// An object that matches exactly one of these.
    OneOf(Vec<Schema>),
}

/// A property of an [`Schema::Object`].
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub name: String,
    pub schema: Schema,
    pub required: bool,
}

/// Builds an [`Schema::Object`] member by member, in the order the reports
/// write them.
#[derive(Default)]
pub struct Shape(Vec<Property>);

impl Shape {
    pub fn new() -> Self {
        Self::default()
    }

    fn property(mut self, name: &str, schema: Schema, required: bool) -> Self {
        self.0.push(Property {
            name: name.to_owned(),
            schema,
            required,
        });
        self
    }

    /// A required member.
    #[must_use]
    pub fn with(self, name: &str, schema: Schema) -> Self {
        self.property(name, schema, true)
    }

    /// A member that may be left out.
    #[must_use]
    pub fn maybe(self, name: &str, schema: Schema) -> Self {
        self.property(name, schema, false)
    }

    /// A required integer.
    #[must_use]
    pub fn integer(self, name: &str) -> Self {
        self.with(name, Schema::Integer)
    }

    /// World text as `Map::text` writes it: the value, then
    /// `<name>_clipped: true` when it was cut.
    #[must_use]
    pub fn world(self, name: &str) -> Self {
        self.with(name, Schema::World)
            .maybe(&format!("{name}_clipped"), Schema::Boolean)
    }

    /// World text or null, as `Map::optional_text` writes it.
    #[must_use]
    pub fn optional_world(self, name: &str) -> Self {
        self.with(name, Schema::World.nullable())
            .maybe(&format!("{name}_clipped"), Schema::Boolean)
    }

    /// World text that may be left out.
    #[must_use]
    pub fn maybe_world(self, name: &str) -> Self {
        self.maybe(name, Schema::World)
            .maybe(&format!("{name}_clipped"), Schema::Boolean)
    }

    /// A list cut at its printed bound: the items and `<name>_more`.
    #[must_use]
    pub fn list(self, name: &str, item: Schema) -> Self {
        self.with(name, Schema::List(Box::new(item)))
            .integer(&format!("{name}_more"))
    }

    /// A paged list of a report: the items, `<name>_more` and
    /// `<name>_cursor`.
    #[must_use]
    pub fn paged(self, name: &str, item: Schema) -> Self {
        self.list(name, item)
            .with(&format!("{name}_cursor"), Schema::Text.nullable())
    }

    pub fn build(self) -> Schema {
        Schema::Object(self.0)
    }
}

impl Schema {
    #[must_use]
    pub fn nullable(self) -> Self {
        Self::Nullable(Box::new(self))
    }

    #[must_use]
    pub fn list(self) -> Self {
        Self::List(Box::new(self))
    }

    /// The schema as JSON Schema.
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        self.write_json(&mut output);
        output
    }

    fn write_json(&self, output: &mut String) {
        match self {
            Self::Text => output.push_str(r#"{"type":"string"}"#),
            Self::World => output.push_str(r#"{"type":"string","x-world-text":true}"#),
            Self::Integer => output.push_str(r#"{"type":"integer"}"#),
            Self::Boolean => output.push_str(r#"{"type":"boolean"}"#),
            Self::Revision => output.push_str(r#"{"type":"string","pattern":"^[0-9a-f]{64}$"}"#),
            Self::Enum(values) => {
                output.push_str(r#"{"type":"string","enum":["#);
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    json_string(value, output);
                }
                output.push_str("]}");
            }
            Self::Const(value) => {
                output.push_str(r#"{"const":"#);
                json_string(value, output);
                output.push('}');
            }
            Self::Scalar => output
                .push_str(r#"{"type":["string","number","boolean","null"],"x-world-text":true}"#),
            Self::Nullable(inner) => {
                output.push_str(r#"{"anyOf":["#);
                inner.write_json(output);
                output.push_str(r#",{"type":"null"}]}"#);
            }
            Self::List(item) => {
                output.push_str(r#"{"type":"array","items":"#);
                item.write_json(output);
                output.push('}');
            }
            Self::Object(properties) => {
                output.push_str(r#"{"type":"object","properties":{"#);
                for (index, property) in properties.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    json_string(&property.name, output);
                    output.push(':');
                    property.schema.write_json(output);
                }
                output.push_str(r#"},"required":["#);
                let mut first = true;
                for property in properties.iter().filter(|property| property.required) {
                    if !first {
                        output.push(',');
                    }
                    first = false;
                    json_string(&property.name, output);
                }
                output.push_str(r#"],"additionalProperties":false}"#);
            }
            Self::OneOf(choices) => {
                output.push_str(r#"{"type":"object","oneOf":["#);
                for (index, choice) in choices.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    choice.write_json(output);
                }
                output.push_str("]}");
            }
        }
    }

    /// Checks `value` against the schema. On success, every string value
    /// in it, in document order, and whether the schema marks it as world
    /// text.
    ///
    /// # Errors
    /// Where `value` does not conform, and how.
    pub fn check(&self, value: &Value) -> Result<Vec<(String, bool)>, String> {
        let mut strings = Vec::new();
        self.check_at(value, "$", &mut strings)?;
        Ok(strings)
    }

    fn check_at(
        &self,
        value: &Value,
        at: &str,
        strings: &mut Vec<(String, bool)>,
    ) -> Result<(), String> {
        let wrong = || {
            Err(format!(
                "{at}: {} does not match {}",
                value.kind(),
                self.to_json()
            ))
        };
        let mut string = |world: bool| match value.as_str() {
            Some(text) => {
                strings.push((text.to_owned(), world));
                Ok(())
            }
            None => wrong(),
        };
        match self {
            Self::Text => string(false),
            Self::World => string(true),
            Self::Revision => match value.as_str() {
                Some(text) if crate::budget::is_revision(text) => string(false),
                _ => wrong(),
            },
            Self::Enum(values) => match value.as_str() {
                Some(text) if values.contains(&text) => string(false),
                _ => wrong(),
            },
            Self::Const(expected) => match value.as_str() {
                Some(text) if text == *expected => string(false),
                _ => wrong(),
            },
            Self::Integer => value.as_i64().map_or_else(wrong, |_| Ok(())),
            Self::Boolean => value.as_bool().map_or_else(wrong, |_| Ok(())),
            Self::Scalar => match value {
                Value::String { .. } => string(true),
                Value::Null { .. }
                | Value::Bool { .. }
                | Value::Integer { .. }
                | Value::Float { .. } => Ok(()),
                _ => wrong(),
            },
            Self::Nullable(inner) => match value {
                Value::Null { .. } => Ok(()),
                _ => inner.check_at(value, at, strings),
            },
            Self::List(item) => {
                let Some(items) = value.as_array() else {
                    return wrong();
                };
                for (index, element) in items.iter().enumerate() {
                    item.check_at(element, &format!("{at}[{index}]"), strings)?;
                }
                Ok(())
            }
            Self::Object(properties) => {
                let Some(members) = value.as_object() else {
                    return wrong();
                };
                for member in members {
                    let property = properties
                        .iter()
                        .find(|property| property.name == member.key)
                        .ok_or_else(|| format!("{at}: unexpected member {}", member.key))?;
                    property.schema.check_at(
                        &member.value,
                        &format!("{at}.{}", member.key),
                        strings,
                    )?;
                }
                for property in properties.iter().filter(|property| property.required) {
                    if !members.iter().any(|member| member.key == property.name) {
                        return Err(format!("{at}: missing member {}", property.name));
                    }
                }
                Ok(())
            }
            Self::OneOf(choices) => {
                let mut matched = None;
                let mut reasons = String::new();
                for choice in choices {
                    let mut found = Vec::new();
                    match choice.check_at(value, at, &mut found) {
                        Ok(()) if matched.is_some() => {
                            return Err(format!("{at}: matches more than one choice"));
                        }
                        Ok(()) => matched = Some(found),
                        Err(reason) => {
                            let _ = write!(reasons, "; {reason}");
                        }
                    }
                }
                let found = matched.ok_or_else(|| format!("{at}: matches no choice{reasons}"))?;
                strings.extend(found);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Schema, Shape};
    use tilewright_jsonc::{Document, Limits};

    fn value(text: &str) -> tilewright_jsonc::Value {
        Document::parse(text, &Limits::default())
            .unwrap()
            .root()
            .clone()
    }

    #[test]
    fn objects_are_closed_and_strings_are_marked() {
        let schema = Shape::new()
            .with("format", Schema::Const("f"))
            .world("name")
            .paged("items", Schema::Integer)
            .maybe("extra", Schema::Text)
            .build();
        let good = value(
            r#"{"format":"f","name":"n","name_clipped":true,"items":[1],"items_more":0,"items_cursor":null}"#,
        );
        assert_eq!(
            schema.check(&good),
            Ok(vec![("f".to_owned(), false), ("n".to_owned(), true)])
        );
        for bad in [
            r#"{"format":"g","name":"n","items":[],"items_more":0,"items_cursor":null}"#,
            r#"{"format":"f","items":[],"items_more":0,"items_cursor":null}"#,
            r#"{"format":"f","name":"n","items":["x"],"items_more":0,"items_cursor":null}"#,
            r#"{"format":"f","name":"n","items":[],"items_more":0,"items_cursor":null,"other":1}"#,
            r#"{"format":"f","name":1,"items":[],"items_more":0,"items_cursor":null}"#,
            "[]",
        ] {
            assert!(schema.check(&value(bad)).is_err(), "{bad}");
        }
        assert_eq!(
            Schema::World.nullable().to_json(),
            r#"{"anyOf":[{"type":"string","x-world-text":true},{"type":"null"}]}"#
        );
    }

    #[test]
    fn one_of_needs_exactly_one_match() {
        let a = Shape::new().with("k", Schema::Const("a")).build();
        let b = Shape::new().with("k", Schema::Const("b")).build();
        let schema = Schema::OneOf(vec![a.clone(), b]);
        assert!(schema.check(&value(r#"{"k":"a"}"#)).is_ok());
        assert!(schema.check(&value(r#"{"k":"c"}"#)).is_err());
        assert!(
            Schema::OneOf(vec![a.clone(), a])
                .check(&value(r#"{"k":"a"}"#))
                .is_err()
        );
    }
}
