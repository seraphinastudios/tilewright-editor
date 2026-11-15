// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `render`: the one command that writes. It writes one new file, the one
//! `--out` names, and refuses any output path it can see is inside the
//! world; the cases it cannot see are listed below and in `docs/CLI.md`.
//! The observe crate draws the pixels; this encodes them as a PNG in
//! memory and writes it through World's confined file access
//! (`tilewright_source`), the only file-writing authority the editor's
//! crates may use. The rules, in `docs/CLI.md`:
//!
//! - the file's directory must exist: none is created;
//! - the file must be outside the world root, compared after resolving
//!   links in both paths, and also with each path component case-folded,
//!   so a link into the world, or a spelling of it in another case, is
//!   inside it (aliases neither comparison sees, such as bind mounts, other
//!   Unicode normalisations or Windows short names, are not detected);
//! - its name must be one every platform can make as written, by one rule
//!   on every platform;
//! - nothing may be at the file's name, a link included;
//! - the image is drawn and encoded completely before the file is made.
//!
//! World's writer creates the file exclusively and does not follow a link
//! at its name, but it first removes whatever is at the name. The name is
//! therefore checked just before the write. On Unix the directory is
//! opened once, after its path is resolved and compared with the world's,
//! and the name checks and the write all go through that handle; a
//! directory swapped in between resolving and opening is not detected, and
//! on Windows, where World opens by path each time, neither is one swapped
//! later. Something put at the name between the last check and the write is
//! removed and replaced; and if the write fails, World removes what is at
//! the name by name. A writer that
//! only creates needs World to offer one: no editor crate may open files
//! with the standard library (`tools/check-architecture.sh`).
use std::ffi::OsString;
use std::path::{Component, Path};
use tilewright_editor_observe::cli::{
    EXIT_REFUSED, Outcome, refused, render_request, write_failed,
};
use tilewright_editor_observe::limits::MAXIMUM_PNG_BYTES;
use tilewright_editor_observe::render;
use tilewright_source::{SourceDirectory, SourceRoot};

/// Where the image goes: an existing directory, resolved and opened once,
/// and a new file name in it.
struct Target {
    root: SourceRoot,
    directory: SourceDirectory,
    name: String,
}

/// Runs `render` with the arguments after the command.
pub fn run(arguments: &[OsString]) -> Outcome {
    match run_checked(arguments) {
        Ok(outcome) | Err(outcome) => outcome,
    }
}

fn run_checked(arguments: &[OsString]) -> Result<Outcome, Outcome> {
    let request = render_request(arguments)?;
    let json = request.json;
    let observed = render::open(&request)?;
    let target = target(Path::new(&request.root), Path::new(&request.out), json)?;
    let rendered = render::render(&observed, &request)?;
    let png = encode(&rendered.frame).ok_or_else(|| {
        refused(
            "output.budget",
            "The PNG would exceed its byte budget.",
            json,
        )
    })?;
    let digest: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(&png).into();
    create_file(&target, &png)
        .map_err(|problem| write_failed(Path::new(&request.out), &problem, json))?;
    Ok(rendered.finish(png.len(), &digest))
}

/// The output refusals: a stable code and this editor's message.
fn output_refused(code: &'static str, json: bool) -> Outcome {
    let message = match code {
        "output.name" => {
            "The output must name a file every platform can make: a UTF-8 name of at most 255 bytes, without control characters or any of \\ : < > \" | ? *, not ending in a dot or space, not a device name such as NUL or COM1, and not ending in a separator."
        }
        "output.directory" => {
            "The output's directory does not exist, is not a directory, or cannot be resolved or searched."
        }
        "output.inside-world" => {
            "The output is inside the world root, or could not be shown to be outside it."
        }
        _ => "Something is already at the output path; nothing is replaced.",
    };
    let outcome = refused(code, message, json);
    debug_assert_eq!(outcome.code, EXIT_REFUSED);
    outcome
}

/// Checks the output path against the rules above, before anything is
/// drawn.
fn target(root: &Path, out: &Path, json: bool) -> Result<Target, Outcome> {
    let name = file_name(out).ok_or_else(|| output_refused("output.name", json))?;
    let parent = match out.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    // The only file system calls here besides World's confined access:
    // resolving two paths, and below whether one is a directory; all
    // read-only.
    let directory = parent
        .canonicalize()
        .map_err(|_| output_refused("output.directory", json))?;
    let world = root
        .canonicalize()
        .map_err(|_| output_refused("output.inside-world", json))?;
    if inside_world(&directory, &world) {
        return Err(output_refused("output.inside-world", json));
    }
    // The resolved parent must be a directory World's access can open.
    // World checks Windows paths when it uses them, not when it opens a
    // root, so a file named as the directory is ruled out here first.
    if !directory.is_dir() {
        return Err(output_refused("output.directory", json));
    }
    let root = SourceRoot::new(&directory);
    let Ok(Some(opened)) = root.directory("", false) else {
        return Err(output_refused("output.directory", json));
    };
    let target = Target {
        root,
        directory: opened,
        name: name.to_owned(),
    };
    if occupied(&target).map_err(|_| output_refused("output.directory", json))? {
        return Err(output_refused("output.exists", json));
    }
    Ok(target)
}

/// The output's file name, if it is one every platform makes as written, by
/// one rule on every platform: a last component that is UTF-8, at most 255
/// bytes, without control characters (U+0000 to U+001F, DEL) or any of
/// `\ : < > " | ? *`, not `.` or `..`, not ending in a dot or a space, not a
/// Windows device name ([`device_name`]), and with no separator after it.
fn file_name(out: &Path) -> Option<&str> {
    // `Path` drops a trailing `.` and separators from its components, so
    // the name must also be what the path's text ends with.
    let name = out.file_name()?;
    let text = out.as_os_str().as_encoded_bytes();
    let before = text.strip_suffix(name.as_encoded_bytes())?.last().copied();
    if before.is_some_and(|byte| byte != b'/' && !(cfg!(windows) && byte == b'\\')) {
        return None;
    }
    name.to_str().filter(|name| {
        name.len() <= 255
            && !name.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
            && !name.chars().any(|character| character.is_ascii_control())
            && !name.ends_with(['.', ' '])
            && !device_name(name)
    })
}

/// Whether `name` is a Windows device name, which Windows opens as the
/// device whatever follows its first dot: CON, PRN, AUX, NUL, COM0 to COM9
/// and LPT0 to LPT9, with the superscript digits ¹ ² ³ too, in any case and
/// with spaces before the dot. Refused on every platform, so one name rule
/// holds everywhere.
fn device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            let mut characters = stem.chars();
            let device: String = characters.by_ref().take(3).collect();
            let digit: Vec<char> = characters.collect();
            (device == "COM" || device == "LPT")
                && matches!(digit[..], [digit] if digit.is_ascii_digit() || matches!(digit, '¹' | '²' | '³'))
        }
    }
}

/// Whether a file in `directory`, both resolved, would be inside the
/// world: in its root directory or beneath it, compared exactly and also
/// with every component case-folded. Linux keeps the case a path was typed
/// in when it resolves one, even on a file system that ignores case (a
/// network share, FAT or exFAT media, a casefolded directory), so
/// `/share/WORLD` can be the world `/share/World`. The fold approximates
/// such file systems' folding and fails closed for case: it also refuses a
/// different directory whose name differs from the world's only in case.
/// It does not detect other aliases: another Unicode normalisation (NFC
/// against NFD), Windows short (8.3) names, or bind mounts.
fn inside_world(directory: &Path, world: &Path) -> bool {
    // Lowercase, uppercase, lowercase: closer to Unicode case folding than
    // lowercasing alone (long s, sharp s, final sigma, ligatures, dotless i).
    let fold = |text: std::borrow::Cow<'_, str>| text.to_lowercase().to_uppercase().to_lowercase();
    let folded = |path: &Path| -> Vec<String> {
        path.components()
            .map(|component| match component {
                Component::Normal(name) => fold(name.to_string_lossy()),
                other => fold(other.as_os_str().to_string_lossy()),
            })
            .collect()
    };
    directory.starts_with(world) || folded(directory).starts_with(&folded(world))
}

/// Whether anything is at the target's name, a link included, as World's
/// access finds it through the target's directory handle (on a file system
/// that ignores case, an entry named otherwise counts too).
fn occupied(target: &Target) -> Result<bool, tilewright_source::AccessError> {
    target.root.exists(&target.name)
}

/// [`encode`] and the PNG's SHA-256, for the MCP server's `render`: the
/// same bytes this command writes.
pub fn encode_with_digest(
    frame: &tilewright_editor_preview::ReferenceFrame,
) -> Option<(Vec<u8>, [u8; 32])> {
    let png = encode(frame)?;
    let digest: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(&png).into();
    Some((png, digest))
}

/// The frame as an RGBA8 PNG with fixed settings and no metadata, so the
/// same pixels always give the same bytes with this encoder; `None` past
/// [`MAXIMUM_PNG_BYTES`].
fn encode(frame: &tilewright_editor_preview::ReferenceFrame) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Balanced);
        encoder
            .write_header()
            .ok()?
            .write_image_data(&frame.rgba8)
            .ok()?;
    }
    within_budget(bytes.len()).then_some(bytes)
}

const fn within_budget(bytes: usize) -> bool {
    bytes <= MAXIMUM_PNG_BYTES
}

/// Writes the new file through the target's directory handle. World's
/// writer creates it exclusively, not following a link at its name, syncs
/// it and removes it again if writing fails; but it first removes whatever
/// is at the name, so the name is checked again just before.
fn create_file(target: &Target, png: &[u8]) -> Result<(), String> {
    if occupied(target).map_err(|error| error.to_string())? {
        return Err("something appeared at the output path; nothing was replaced".into());
    }
    target
        .directory
        .write(&target.name, None, png)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{create_file, file_name, inside_world, target, within_budget};
    use std::path::Path;
    use tilewright_editor_observe::limits::MAXIMUM_PNG_BYTES;
    use tilewright_source::SourceRoot;

    #[test]
    fn only_a_plain_last_name_is_a_file_name() {
        assert_eq!(file_name(Path::new("out.png")), Some("out.png"));
        assert_eq!(file_name(Path::new("a/b/out.png")), Some("out.png"));
        for refused in [
            "out/",
            "out.png/",
            "a/..",
            "a/.",
            "out.png/.",
            ".",
            "..",
            "/",
            "",
            "a:b.png",
        ] {
            assert_eq!(file_name(Path::new(refused)), None, "{refused}");
        }
        // A backslash separates on Windows and is refused in a name elsewhere.
        let expected = cfg!(windows).then_some("b.png");
        assert_eq!(file_name(Path::new("a\\b.png")), expected);
        assert_eq!(file_name(Path::new(&"x".repeat(256))), None);
        assert!(file_name(Path::new(&"x".repeat(255))).is_some());
    }

    #[test]
    fn names_windows_would_alter_or_open_as_devices_are_refused_everywhere() {
        for refused in [
            "NUL",
            "nul",
            "Con.png",
            "aux.tar.gz",
            "PRN",
            "com1.png",
            "COM0",
            "lpt9.png",
            "COM¹.png",
            "lpt³",
            "NUL .png",
            "y.png.",
            "x.png ",
            "a:b",
            "image.png:stream",
            "a<b",
            "a>b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "tab\there",
            "esc\u{1b}[31m",
            "bell\u{7}",
            "nul\u{0}byte",
            "del\u{7f}",
        ] {
            assert_eq!(file_name(Path::new(refused)), None, "{refused}");
        }
        for allowed in [
            "nul2.png",
            "console.png",
            "com10.png",
            "lpt.png",
            "comx.png",
            "auxiliary.png",
            "x.nul",
            "my con.png",
            ".hidden.png",
            "a b.png",
        ] {
            assert_eq!(file_name(Path::new(allowed)), Some(allowed), "{allowed}");
        }
    }

    #[test]
    fn a_file_in_the_root_or_beneath_it_is_inside() {
        let world = Path::new("/w/world");
        assert!(inside_world(Path::new("/w/world"), world));
        assert!(inside_world(Path::new("/w/world/planes"), world));
        assert!(!inside_world(Path::new("/w"), world));
        assert!(!inside_world(Path::new("/w/world2"), world));
        assert!(!inside_world(Path::new("/w/other"), world));
        // Any case of the world's components is inside: fail closed.
        assert!(inside_world(Path::new("/w/WORLD"), world));
        assert!(inside_world(Path::new("/W/World/Planes"), world));
        assert!(!inside_world(Path::new("/w/WORLD2"), world));
        // Spellings that file systems which fold case treat as one name.
        for (spelled, world) in [
            ("/m/aſſets", "/m/assets"),
            ("/m/STRASSE", "/m/Straße"),
            ("/m/ΚΟΣΜΟΣ", "/m/κοσμοσ"),
            ("/m/κοσμος", "/m/ΚΟΣΜΟΣ"),
            ("/m/ﬁle", "/m/FILE"),
            ("/m/tıles", "/m/tiles"),
            ("/m/ẞ", "/m/ss"),
        ] {
            assert!(
                inside_world(Path::new(spelled), Path::new(world)),
                "{spelled} {world}"
            );
            assert!(
                inside_world(Path::new(world), Path::new(spelled)),
                "{world} {spelled}"
            );
        }
        assert!(!inside_world(
            Path::new("/m/assets2"),
            Path::new("/m/aſſets")
        ));
    }

    #[test]
    fn a_name_taken_after_the_checks_is_not_replaced() {
        let scratch = tempfile::tempdir().unwrap();
        let top = SourceRoot::new(scratch.path());
        assert!(top.make_directory("world").unwrap());
        assert!(top.make_directory("out").unwrap());
        let world = scratch.path().join("world");
        let checked = target(&world, &scratch.path().join("out/image.png"), true)
            .unwrap_or_else(|outcome| panic!("{outcome:?}"));
        // Another writer takes the name between the checks and the write.
        let out = top.directory("out", false).unwrap().unwrap();
        out.write("image.png", None, b"keep me").unwrap();
        let problem = create_file(&checked, b"\x89PNG").unwrap_err();
        assert!(problem.contains("nothing was replaced"), "{problem}");
        assert_eq!(
            top.read("out/image.png", 64).unwrap().as_deref(),
            Some(&b"keep me"[..])
        );
        // The output directory gained nothing else.
        let (names, _) = top.names("out", 8).unwrap().unwrap();
        assert_eq!(names.into_iter().collect::<Vec<_>>(), ["image.png"]);
    }

    #[test]
    fn the_image_bounds_are_the_reference_renderers_target_limits() {
        use tilewright_editor_observe::limits::{MAXIMUM_RENDER_PIXELS, MAXIMUM_RENDER_SIDE};
        let target = tilewright_render_api::TargetLimits::default();
        assert_eq!(target.maximum_dimension, MAXIMUM_RENDER_SIDE);
        assert_eq!(target.maximum_pixels, MAXIMUM_RENDER_PIXELS);
    }

    #[test]
    fn the_png_budget_is_inclusive() {
        assert!(within_budget(MAXIMUM_PNG_BYTES));
        assert!(!within_budget(MAXIMUM_PNG_BYTES + 1));
    }
}
