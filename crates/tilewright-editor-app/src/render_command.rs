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
//! The file is made with World's create-only write
//! (`SourceDirectory::create`), which makes it by an exclusive create that
//! follows no link and never removes or replaces anything that was at the
//! name before the call, on success or failure. Something put at the name
//! after the checks is therefore refused as `output.exists` and left as it
//! is. If the write fails after the file was made, on Unix World removes
//! its own file only while the name is still that file (a window of about a
//! microsecond remains, in which a file renamed over the name would be
//! removed); on Windows it never removes it, so a partly written file may be
//! left, and the failure says so. On Unix the directory is opened once,
//! after its path is resolved and compared with the world's, and the name
//! check and the write go through that handle; a directory swapped in
//! between resolving and opening is not detected, and on Windows, where
//! World opens by path each time, neither is one swapped later. No editor
//! crate may open files with the standard library
//! (`tools/check-architecture.sh`).
use std::ffi::OsString;
use std::path::{Component, Path};
use tilewright_editor_observe::cli::{
    EXIT_REFUSED, Outcome, refused, render_request, write_failed, write_failed_leaving_file,
};
use tilewright_editor_observe::limits::MAXIMUM_PNG_BYTES;
use tilewright_editor_observe::render;
use tilewright_source::{AccessError, SourceDirectory, SourceRoot};

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
        .map_err(|error| write_refusal(Path::new(&request.out), &error, json))?;
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

/// Writes the new file through the target's directory handle with World's
/// create-only write: an exclusive create that follows no link, syncs the
/// file, and never removes or replaces what was at the name. The name was
/// checked in [`target`] for an early refusal; something put there since is
/// refused by the create itself.
fn create_file(target: &Target, png: &[u8]) -> Result<(), AccessError> {
    target.directory.create(&target.name, None, png)
}

/// What a failed [`create_file`] means for the command.
fn write_refusal(out: &Path, error: &AccessError, json: bool) -> Outcome {
    write_outcome(
        out,
        error.already_exists(),
        error.may_be_left(),
        &error.to_string(),
        json,
    )
}

/// A name taken since the checks is `output.exists` (exit 4): nothing was
/// touched. Any other failure is `output.write` (exit 5), saying so when
/// the file the write made may be left at the name.
fn write_outcome(out: &Path, taken: bool, left: bool, problem: &str, json: bool) -> Outcome {
    if taken {
        output_refused("output.exists", json)
    } else if left {
        write_failed_leaving_file(out, problem, json)
    } else {
        write_failed(out, problem, json)
    }
}

#[cfg(test)]
mod tests;
