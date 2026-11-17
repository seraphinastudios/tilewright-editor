// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The version string this editor build reports for itself: in `version`,
//! as both MCP servers' `serverInfo` version, as the sandbox's `editor`,
//! and as the `editor` member of every proposal it records.
//!
//! A packaged build reports its release version: `tools/package-linux.sh`
//! and `tools/package-windows.ps1` set `TILEWRIGHT_EDITOR_RELEASE_VERSION`
//! to the version they name the archive with while they build. Any other
//! build reports the crate version with `-dev` appended: a development
//! build, which does not tell one development build from another. The value
//! is checked when the editor is compiled, so an unusable one fails the
//! package build: 1 to [`MAXIMUM_BYTES`] printable ASCII characters, no
//! space, as a proposal's `editor` member allows.

/// The longest version string a proposal's `editor` member holds.
const MAXIMUM_BYTES: usize = tilewright_editor_proposal::limits::EDITOR_BYTES;

/// The version string a build without a release version reports.
const DEVELOPMENT: &str = concat!(env!("CARGO_PKG_VERSION"), "-dev");

/// This build's version string; the one every place reports.
pub(crate) const EDITOR_VERSION: &str = reported(option_env!("TILEWRIGHT_EDITOR_RELEASE_VERSION"));

/// The version string for a build given `release`, or the development
/// form without one.
///
/// # Panics
/// For a release version that is empty, longer than [`MAXIMUM_BYTES`], or
/// has a byte that is not printable ASCII or is a space; evaluated for
/// [`EDITOR_VERSION`], that is a compile error.
const fn reported(release: Option<&'static str>) -> &'static str {
    let Some(version) = release else {
        return DEVELOPMENT;
    };
    let bytes = version.as_bytes();
    assert!(
        !bytes.is_empty() && bytes.len() <= MAXIMUM_BYTES,
        "TILEWRIGHT_EDITOR_RELEASE_VERSION must be 1 to 64 bytes"
    );
    let mut index = 0;
    while index < bytes.len() {
        assert!(
            bytes[index].is_ascii_graphic(),
            "TILEWRIGHT_EDITOR_RELEASE_VERSION must be printable ASCII without spaces"
        );
        index += 1;
    }
    version
}

#[cfg(test)]
mod tests {
    use super::{DEVELOPMENT, EDITOR_VERSION, MAXIMUM_BYTES, reported};

    #[test]
    fn a_build_without_a_release_version_is_a_development_build() {
        assert_eq!(DEVELOPMENT, "0.1.0-dev");
        assert_eq!(reported(None), DEVELOPMENT);
        // The test build reports what the binary built beside it reports.
        assert_eq!(
            EDITOR_VERSION,
            option_env!("TILEWRIGHT_EDITOR_RELEASE_VERSION").unwrap_or(DEVELOPMENT)
        );
    }

    #[test]
    fn a_release_version_is_reported_as_given() {
        for version in [
            "1.47.0",
            "0.1.0-test.1",
            "2.0.0+build.7",
            &"9".repeat(MAXIMUM_BYTES),
        ] {
            let version: &'static str = Box::leak(version.to_owned().into_boxed_str());
            assert_eq!(reported(Some(version)), version);
        }
    }

    #[test]
    fn an_unusable_release_version_is_refused() {
        let too_long = "9".repeat(MAXIMUM_BYTES + 1);
        for version in [
            "",
            "1.47.0 beta",
            "1.47.0\t",
            "1.47.0\n",
            "1.47.\u{e9}",
            "\u{7f}",
            too_long.as_str(),
        ] {
            let version: &'static str = Box::leak(version.to_owned().into_boxed_str());
            assert!(
                std::panic::catch_unwind(|| reported(Some(version))).is_err(),
                "{version:?} was accepted"
            );
        }
    }
}
