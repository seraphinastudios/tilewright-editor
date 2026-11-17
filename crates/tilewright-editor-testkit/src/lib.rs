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

/// Labels the resources `ids` of the world copy at `world` as project art:
/// its catalog becomes format 2 and those resources' `license` `project`.
/// Their pixels are unchanged. Tests never use project art: they label the
/// MIT neutral sample, in a temporary copy, and nothing labelled `project`
/// is ever committed (`tools/check-prototype-assets.py` refuses it).
///
/// # Panics
/// When the copy's catalog cannot be read or written, or an id is not one
/// of its MIT resources.
pub fn label_project_art(world: &Path, ids: &[&str]) {
    edit_catalog(world, |catalog| {
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
    });
}

/// Replaces resource `id` of the world copy at `world` with a generated
/// `width` × `height` ATX1 image of one `color`, and its catalog entry's
/// digest and dimensions with the new file's.
///
/// # Panics
/// When the copy cannot be read or written, or `id` is not a resource.
pub fn replace_resource(world: &Path, id: &str, width: u32, height: u32, color: [u8; 4]) {
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
    edit_catalog(world, |catalog| {
        let resource = catalog["resources"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|resource| resource["id"] == id)
            .unwrap();
        let path = resource["path"].as_str().unwrap().to_owned();
        std::fs::write(world.join(path), &bytes).unwrap();
        resource["sha256"] = digest.clone().into();
        resource["width"] = width.into();
        resource["height"] = height.into();
    });
}

fn edit_catalog(world: &Path, change: impl FnOnce(&mut serde_json::Value)) {
    let path = world.join("preview.jsonc");
    let mut catalog: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut catalog);
    std::fs::write(path, serde_json::to_string_pretty(&catalog).unwrap()).unwrap();
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
