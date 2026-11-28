// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::RwLock,
};
use tilewright_editor_project::{Error, FileKind, FileStamp, PathProbe, RelativePath};

/// The root of a committed fixture world under this crate's `fixtures`
/// directory. `meadow` exercises painted terrain transitions;
/// `prototype-neutral` adds authored MIT visual resources, a typed editable
/// marker, an empty paintable cell and negative/cross-chunk ground.
#[must_use]
pub fn fixture_world(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// The editor catalog text `catalog` with the resources `ids` labelled as
/// project art: format 2, and those resources' `license` `project`. Their
/// files are unchanged. Tests never use project art: they label the MIT
/// neutral sample in a temporary copy, and nothing labelled `project` is
/// ever committed (`tools/check-prototype-assets.py` refuses it). Pure: the
/// caller reads and writes the copy.
///
/// # Panics
/// When `catalog` is not JSON, or an id is not one of its MIT resources.
#[must_use]
pub fn project_art_catalog(catalog: &str, ids: &[&str]) -> String {
    edited(catalog, |catalog| {
        catalog["format"] = "tilewright.editor-preview/2".into();
        let mut found = 0;
        for resource in catalog["resources"].as_array_mut().unwrap() {
            assert_eq!(resource["license"], "MIT");
            if ids.contains(&resource["id"].as_str().unwrap()) {
                resource["license"] = "project".into();
                found += 1;
            }
        }
        assert_eq!(found, ids.len(), "unknown resource id in {ids:?}");
    })
}

/// A generated `width` × `height` ATX1 image of one `color`, and
/// `catalog` with resource `id`'s digest and dimensions set to the image's.
/// The caller writes the image at the resource's path. Pure.
///
/// # Panics
/// When `catalog` is not JSON or `id` is not one of its resources.
#[must_use]
pub fn generated_resource(
    catalog: &str,
    id: &str,
    width: u32,
    height: u32,
    color: [u8; 4],
) -> (Vec<u8>, String) {
    let mut bytes = b"ATX1".to_vec();
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    for _ in 0..u64::from(width) * u64::from(height) {
        bytes.extend(color);
    }
    let mut digest = String::with_capacity(64);
    for byte in <sha2::Sha256 as sha2::Digest>::digest(&bytes) {
        write!(digest, "{byte:02x}").unwrap();
    }
    let catalog = edited(catalog, |catalog| {
        let resource = catalog["resources"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|resource| resource["id"] == id)
            .unwrap();
        resource["sha256"] = digest.clone().into();
        resource["width"] = width.into();
        resource["height"] = height.into();
    });
    (bytes, catalog)
}

/// The prototypes a generated large world places, in turn: the neutral
/// sample's standing objects.
const LARGE_WORLD_OBJECTS: [&str; 4] = ["stone", "bush", "crate", "lamp"];

/// The files that grow a copy of `fixture_world("prototype-neutral")` into a
/// large world: `side` × `side` chunks of plane `main`, layer 0, chunk
/// coordinates `0..side` on both axes, each 32 × 32 floor tiles of the
/// sample's `grass` and `sand` in a fixed diagonal pattern and holding
/// `objects_per_chunk` (at most 32) of the sample's standing objects on
/// distinct tiles. The sample's own chunk `[0, 0]` is kept, so it is not
/// among the files; its other chunks stay as they are. Deterministic: the
/// same arguments always give the same paths and bytes. Pure: the caller
/// copies the sample and writes each `(path, text)` under it, and sets the
/// view with [`large_world_catalog`]. The art is the sample's MIT art.
///
/// # Panics
/// When `objects_per_chunk` is over 32.
#[must_use]
pub fn large_world_chunks(side: u32, objects_per_chunk: u32) -> Vec<(String, String)> {
    assert!(objects_per_chunk <= 32, "at most one object per row");
    let mut files = Vec::new();
    for cy in 0..side {
        for cx in 0..side {
            if (cx, cy) == (0, 0) {
                continue;
            }
            let rows: Vec<String> = (0..32)
                .map(|y| {
                    (0..32)
                        .map(|x| {
                            if (x / 4 + y / 4 + cx + cy) % 5 == 0 {
                                's'
                            } else {
                                'g'
                            }
                        })
                        .collect()
                })
                .collect();
            // 7 and 13 are odd, so the tiles of one chunk's objects differ.
            let objects: Vec<serde_json::Value> = (0..objects_per_chunk)
                .map(|n| {
                    serde_json::json!({
                        "uid": format!("o_{cx}_{cy}_{n}"),
                        "at": [(n * 7 + cx) % 32, (n * 13 + cy) % 32],
                        "proto": LARGE_WORLD_OBJECTS[n as usize % LARGE_WORLD_OBJECTS.len()],
                        "layer": "item",
                    })
                })
                .collect();
            let chunk = serde_json::json!({
                "format": "tilewright.chunk/1",
                "plane": "main",
                "layer": 0,
                "chunk": [cx, cy],
                "terrain": {"floor": {"palette": {"g": "grass", "s": "sand"}, "rows": rows}},
                "objects": objects,
            });
            files.push((
                format!("planes/main/layers/0/{cx}_{cy}.jsonc"),
                serde_json::to_string(&chunk).unwrap(),
            ));
        }
    }
    files
}

/// The sample's editor catalog text `catalog` with its view's bounds set to
/// `bounds` (`[x, y, width, height]`), for a world grown by
/// [`large_world_chunks`]. Pure.
///
/// # Panics
/// When `catalog` is not JSON.
#[must_use]
pub fn large_world_catalog(catalog: &str, bounds: [i64; 4]) -> String {
    edited(catalog, |catalog| {
        catalog["view"]["bounds"] = bounds.into();
    })
}

fn edited(catalog: &str, change: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut catalog: serde_json::Value = serde_json::from_str(catalog).unwrap();
    change(&mut catalog);
    serde_json::to_string_pretty(&catalog).unwrap()
}

#[derive(Default)]
pub struct MemoryPathProbe {
    files: RwLock<BTreeMap<RelativePath, FileStamp>>,
}

impl MemoryPathProbe {
    pub fn insert(&self, path: RelativePath, stamp: FileStamp) -> Result<(), Error> {
        self.files
            .write()
            .map_err(|_| Error::Probe("poisoned fake".into()))?
            .insert(path, stamp);
        Ok(())
    }

    pub fn regular(root: &str, relative: &RelativePath, identity: u128, revision: u8) -> FileStamp {
        FileStamp {
            canonical: format!("{}/{}", root.trim_end_matches('/'), relative.as_str()),
            identity,
            revision: [revision; 32],
            kind: FileKind::Regular,
        }
    }
}

impl PathProbe for MemoryPathProbe {
    fn inspect(&self, relative: &RelativePath) -> Result<FileStamp, Error> {
        self.files
            .read()
            .map_err(|_| Error::Probe("poisoned fake".into()))?
            .get(relative)
            .cloned()
            .ok_or_else(|| Error::Probe("missing fake path".into()))
    }
}
