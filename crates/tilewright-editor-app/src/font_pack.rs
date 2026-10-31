// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Explicit read-only, digest-pinned application fonts. Toolkit confinement
//! rejects symlink/traversal/special-file inputs; renderer validates all bytes.
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use tilewright_render_ui::{
    unicode::{FontBlob, FontPack, TextLimits, TextSystem},
    unicode_fonts::FONTS,
};
use tilewright_source::SourceRoot;

pub fn load(path: &Path, world: &Path) -> Result<TextSystem, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("font pack: {error}"))?;
    let world = world
        .canonicalize()
        .map_err(|error| format!("source root: {error}"))?;
    if canonical.starts_with(&world) || world.starts_with(&canonical) {
        return Err("font pack and project source roots must be separate".into());
    }
    let root = SourceRoot::new(&canonical);
    let manifest = root
        .read("manifest.json", 1024 * 1024)
        .map_err(|error| error.to_string())?
        .ok_or("font pack manifest missing")?;
    let digest: [u8; 32] = Sha256::digest(&manifest).into();
    if digest != tilewright_render_ui::unicode_fonts::FONT_PACK_MANIFEST_SHA256 {
        return Err("font pack manifest digest mismatch".into());
    }
    let mut fonts = Vec::with_capacity(FONTS.len());
    for font in FONTS {
        let bytes = root
            .read(
                font.path,
                u64::try_from(font.bytes).map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("font pack {}: {error}", font.path))?
            .ok_or_else(|| format!("font pack file missing: {}", font.path))?;
        if bytes.len() != font.bytes {
            return Err(format!("font pack byte length mismatch: {}", font.path));
        }
        fonts.push(FontBlob {
            family: font.family.into(),
            bytes: Arc::from(bytes),
            sha256: font.sha256,
        });
    }
    let limits = TextLimits::default();
    TextSystem::new(
        FontPack::new(fonts, limits).map_err(|error| error.to_string())?,
        limits,
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
pub fn test_system() -> TextSystem {
    let path = std::env::var_os("TILEWRIGHT_UNICODE_FONT_ROOT")
        .expect("tests require explicit trusted font fixture TILEWRIGHT_UNICODE_FONT_ROOT");
    load(Path::new(&path), Path::new("/tmp")).expect("verified test font pack")
}
