// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A person's favourites: short ordered lists of values the catalog already
//! grants (terrain values, catalog scatter palettes and Place prototypes),
//! read from the file the provisioning helper copies into the session
//! (`--palettes FILE`, docs/OFFLINE-PROTOTYPE.md#favourites).
//!
//! Choosing a favourite is choosing the same value in the catalog's own
//! list, so it prepares exactly the action that choice prepares: nothing
//! new is recorded, and proposals, MCP and replay are unchanged. The file's
//! shape is strict, closed and bounded; a file that breaks it is refused as
//! a whole, with a reason that never quotes its contents. An entry the
//! current catalog does not grant still loads; it cannot be chosen, only
//! removed.
use std::fmt;
use tilewright_editor_preview::PreviewCatalog;
use tilewright_jsonc as jsonc;

use crate::tools::{self, TerrainChoice};

/// The file's format name and version.
pub const FORMAT: &str = "tilewright.editor-favourites/1";
/// Most bytes of a favourites file.
pub const MAXIMUM_BYTES: usize = 64 * 1024;
/// Most entries of each list.
pub const MAXIMUM_ENTRIES: usize = 32;
/// Most bytes of a grid, terrain value or prototype id.
pub const MAXIMUM_VALUE_BYTES: usize = 128;
/// Most bytes of a scatter palette name, as the catalog bounds it.
pub const MAXIMUM_SCATTER_BYTES: usize = tilewright_editor_preview::MAXIMUM_PALETTE_NAME_BYTES;

/// One favourite: a value of one of the catalog's lists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Favourite {
    /// A terrain value on its grid (`editing.terrain`).
    Terrain(TerrainChoice),
    /// A catalog scatter palette, by name (`editing.palettes`).
    Scatter(String),
    /// A prototype Place puts down (`editing.objects`).
    Object(String),
}

impl Favourite {
    /// Whether the catalog grants this favourite: whether choosing it in
    /// the catalog's own list would be accepted.
    pub fn granted(&self, catalog: &PreviewCatalog) -> bool {
        match self {
            Self::Terrain(choice) => tools::terrain_palette(catalog).contains(choice),
            Self::Scatter(name) => catalog.editing().palettes.contains_key(name),
            Self::Object(prototype) => tools::object_palette(catalog).contains(&prototype.as_str()),
        }
    }
}

/// A file that breaks the format; the reason never quotes the file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Malformed(pub String);
impl fmt::Display for Malformed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for Malformed {}

/// The person's three lists, each in their order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Favourites {
    pub terrain: Vec<TerrainChoice>,
    pub scatter: Vec<String>,
    pub objects: Vec<String>,
}

fn malformed(reason: impl Into<String>) -> Malformed {
    Malformed(reason.into())
}

fn reader_limits() -> jsonc::Limits {
    jsonc::Limits {
        max_input_bytes: MAXIMUM_BYTES,
        // The document, a list, a terrain entry.
        max_depth: 3,
        max_values: 1 + 3 * (1 + MAXIMUM_ENTRIES * 3) + 1,
        max_array_items: MAXIMUM_ENTRIES,
        max_object_keys: 4,
        max_string_bytes: MAXIMUM_VALUE_BYTES.max(FORMAT.len()),
        max_comments: 0,
        ..jsonc::Limits::default()
    }
}

/// A grid, value, palette name or prototype id: 1 to `limit` bytes, no
/// control character.
fn text<'v>(value: &'v jsonc::Value, what: &str, limit: usize) -> Result<&'v str, Malformed> {
    let text = value
        .as_str()
        .ok_or_else(|| malformed(format!("{what} is not a string")))?;
    if text.is_empty() || text.len() > limit {
        return Err(malformed(format!("{what} is not 1 to {limit} bytes")));
    }
    if text.chars().any(char::is_control) {
        return Err(malformed(format!("{what} contains a control character")));
    }
    Ok(text)
}

/// The items of list `name`, or none when it is absent.
fn list<'v>(root: &'v jsonc::Value, name: &str) -> Result<&'v [jsonc::Value], Malformed> {
    match root.get(name) {
        None => Ok(&[]),
        Some(value) => value
            .as_array()
            .ok_or_else(|| malformed(format!("{name} is not a list"))),
    }
}

/// Refuses a list that names one entry twice.
fn unique<T: PartialEq>(items: &[T], name: &str) -> Result<(), Malformed> {
    for (index, item) in items.iter().enumerate() {
        if items[..index].contains(item) {
            return Err(malformed(format!(
                "{name} entry {} repeats an earlier entry",
                index + 1
            )));
        }
    }
    Ok(())
}

impl Favourites {
    /// Reads a favourites file: strict JSON (no comments, no trailing
    /// commas, no duplicate keys) of at most [`MAXIMUM_BYTES`], an object
    /// with `format` equal to [`FORMAT`] and optional `terrain`
    /// (`[{"grid", "value"}]`), `scatter` and `objects` (`[string]`) lists,
    /// each of at most [`MAXIMUM_ENTRIES`] distinct entries, and no other
    /// member. Whether the catalog grants an entry is not checked here.
    ///
    /// # Errors
    /// [`Malformed`] for anything else, with a reason that does not quote
    /// the file.
    pub fn decode(bytes: &[u8]) -> Result<Self, Malformed> {
        if bytes.len() > MAXIMUM_BYTES {
            return Err(malformed(format!(
                "the file is larger than {} KiB",
                MAXIMUM_BYTES / 1024
            )));
        }
        let source = std::str::from_utf8(bytes).map_err(|_| malformed("the file is not UTF-8"))?;
        let document = jsonc::Document::parse(source, &reader_limits()).map_err(|error| {
            use jsonc::ErrorCode::{ArrayLimit, StringTooLong};
            match error.code {
                ArrayLimit => malformed(format!("a list has more than {MAXIMUM_ENTRIES} entries")),
                StringTooLong => malformed(format!(
                    "a string is longer than {MAXIMUM_VALUE_BYTES} bytes"
                )),
                code => malformed(format!(
                    "the file is not strict JSON within its bounds ({}, line {})",
                    code.as_str(),
                    error.line
                )),
            }
        })?;
        let root = document.root();
        let members = root
            .as_object()
            .ok_or_else(|| malformed("the file is not a JSON object"))?;
        if root.get("format").and_then(jsonc::Value::as_str) != Some(FORMAT) {
            return Err(malformed(format!("the format is not {FORMAT}")));
        }
        // A key is the person's text: say only that there is one.
        if members.iter().any(|member| {
            !["format", "terrain", "scatter", "objects"].contains(&member.key.as_str())
        }) {
            return Err(malformed(
                "the file has a member other than format, terrain, scatter and objects",
            ));
        }
        let mut terrain = Vec::new();
        for (index, item) in list(root, "terrain")?.iter().enumerate() {
            let entry = format!("terrain entry {}", index + 1);
            let fields = item
                .as_object()
                .ok_or_else(|| malformed(format!("{entry} is not an object")))?;
            let (Some(grid), Some(value), 2) = (item.get("grid"), item.get("value"), fields.len())
            else {
                return Err(malformed(format!(
                    "{entry} does not have exactly grid and value"
                )));
            };
            terrain.push(TerrainChoice {
                grid: text(grid, &format!("{entry}'s grid"), MAXIMUM_VALUE_BYTES)?.into(),
                prototype: text(value, &format!("{entry}'s value"), MAXIMUM_VALUE_BYTES)?.into(),
            });
        }
        let names = |name: &str, limit: usize| -> Result<Vec<String>, Malformed> {
            list(root, name)?
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    text(item, &format!("{name} entry {}", index + 1), limit).map(str::to_owned)
                })
                .collect()
        };
        let favourites = Self {
            terrain,
            scatter: names("scatter", MAXIMUM_SCATTER_BYTES)?,
            objects: names("objects", MAXIMUM_VALUE_BYTES)?,
        };
        unique(&favourites.terrain, "terrain")?;
        unique(&favourites.scatter, "scatter")?;
        unique(&favourites.objects, "objects")?;
        Ok(favourites)
    }

    /// The file's bytes, which [`Self::decode`] reads back as these lists:
    /// every member, in the order format, terrain, scatter, objects.
    pub fn encode(&self) -> Vec<u8> {
        fn strings(output: &mut String, items: impl Iterator<Item = String>) {
            let items: Vec<String> = items.collect();
            if items.is_empty() {
                output.push_str("[]");
                return;
            }
            output.push_str("[\n");
            output.push_str(&items.join(",\n"));
            output.push_str("\n  ]");
        }
        let quoted = |value: &str| {
            let mut text = String::new();
            jsonc::write_json_string(value, &mut text);
            text
        };
        let mut output = format!("{{\n  \"format\": {},\n  \"terrain\": ", quoted(FORMAT));
        strings(
            &mut output,
            self.terrain.iter().map(|choice| {
                format!(
                    "    {{\"grid\": {}, \"value\": {}}}",
                    quoted(&choice.grid),
                    quoted(&choice.prototype)
                )
            }),
        );
        output.push_str(",\n  \"scatter\": ");
        strings(
            &mut output,
            self.scatter
                .iter()
                .map(|name| format!("    {}", quoted(name))),
        );
        output.push_str(",\n  \"objects\": ");
        strings(
            &mut output,
            self.objects
                .iter()
                .map(|name| format!("    {}", quoted(name))),
        );
        output.push_str("\n}\n");
        output.into_bytes()
    }

    /// The favourites a mode shows, in order: Terrain mode's terrain values
    /// then scatter palettes, or with `place` Place mode's prototypes.
    pub fn entries(&self, place: bool) -> Vec<Favourite> {
        if place {
            return self
                .objects
                .iter()
                .cloned()
                .map(Favourite::Object)
                .collect();
        }
        self.terrain
            .iter()
            .cloned()
            .map(Favourite::Terrain)
            .chain(self.scatter.iter().cloned().map(Favourite::Scatter))
            .collect()
    }

    /// Where `favourite` is in its list, if it is there.
    pub fn position(&self, favourite: &Favourite) -> Option<usize> {
        match favourite {
            Favourite::Terrain(choice) => self.terrain.iter().position(|item| item == choice),
            Favourite::Scatter(name) => self.scatter.iter().position(|item| item == name),
            Favourite::Object(name) => self.objects.iter().position(|item| item == name),
        }
    }

    /// Stars `favourite` (at the end of its list) or unstars it. Returns
    /// whether it is starred now.
    ///
    /// # Errors
    /// Refuses to star a value its list has no room for.
    pub fn toggle(&mut self, favourite: &Favourite) -> Result<bool, Malformed> {
        fn flip<T: PartialEq + Clone>(items: &mut Vec<T>, item: &T) -> Result<bool, Malformed> {
            if let Some(index) = items.iter().position(|value| value == item) {
                items.remove(index);
                return Ok(false);
            }
            if items.len() >= MAXIMUM_ENTRIES {
                return Err(malformed(format!(
                    "a favourites list holds at most {MAXIMUM_ENTRIES} entries"
                )));
            }
            items.push(item.clone());
            Ok(true)
        }
        match favourite {
            Favourite::Terrain(choice) => flip(&mut self.terrain, choice),
            Favourite::Scatter(name) => flip(&mut self.scatter, name),
            Favourite::Object(name) => flip(&mut self.objects, name),
        }
    }

    /// Moves `favourite` one place up its list. Returns whether it moved:
    /// not when it is first or not starred.
    pub fn raise(&mut self, favourite: &Favourite) -> bool {
        fn up<T: PartialEq>(items: &mut [T], item: &T) -> bool {
            match items.iter().position(|value| value == item) {
                Some(index) if index > 0 => {
                    items.swap(index - 1, index);
                    true
                }
                _ => false,
            }
        }
        match favourite {
            Favourite::Terrain(choice) => up(&mut self.terrain, choice),
            Favourite::Scatter(name) => up(&mut self.scatter, name),
            Favourite::Object(name) => up(&mut self.objects, name),
        }
    }
}
