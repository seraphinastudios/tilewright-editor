// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::RwLock,
};
use tilewright_editor_project::{Error, FileKind, FileStamp, PathProbe, RelativePath};

/// The root of a committed fixture world under this crate's `fixtures`
/// directory, such as `meadow`: a small top-down world whose source holds
/// only painted terrain, with transitions that derive shores, grass edges
/// and grass details.
#[must_use]
pub fn fixture_world(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
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
