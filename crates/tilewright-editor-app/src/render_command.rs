// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `render`: the one command that writes, and only the file `--out`
//! names. The observe crate draws the pixels; this encodes them as a PNG in
//! memory and writes it through World's confined file access
//! (`tilewright_source`), which removes nothing here and follows no link at
//! the file's name. The rules, in `docs/CLI.md`:
//!
//! - the file's directory must exist: none is created;
//! - the file must be outside the world root, compared after resolving
//!   links in both paths, so a link into the world is inside it;
//! - nothing may be at the file's name, a link included: no file is
//!   replaced and no link is followed;
//! - the image is drawn and encoded completely before the file is made,
//!   and a failed write leaves no file behind.
//!
//! The checks and the write are separate steps: a local process that
//! changes the paths between them can defeat them, as with any command
//! that writes where its caller says.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{
    EXIT_REFUSED, Outcome, refused, render_request, write_failed,
};
use tilewright_editor_observe::limits::MAXIMUM_PNG_BYTES;
use tilewright_editor_observe::render;
use tilewright_source::SourceRoot;

/// Where the image goes: an existing directory, resolved, and a new file
/// name in it.
struct Target {
    directory: PathBuf,
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
            "The output must name a file: a UTF-8 name without \\, : or NUL, not ending in a separator."
        }
        "output.directory" => "The output's directory does not exist or cannot be resolved.",
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
    // resolving two paths. tools/check-architecture.sh allows them in this
    // file alone.
    let directory = parent
        .canonicalize()
        .map_err(|_| output_refused("output.directory", json))?;
    let world = root
        .canonicalize()
        .map_err(|_| output_refused("output.inside-world", json))?;
    if inside_world(&directory, &world) {
        return Err(output_refused("output.inside-world", json));
    }
    let target = Target {
        directory,
        name: name.to_owned(),
    };
    // The resolved parent must be a directory World's access can open.
    if !matches!(
        SourceRoot::new(&target.directory).directory("", false),
        Ok(Some(_))
    ) {
        return Err(output_refused("output.directory", json));
    }
    if occupied(&target).map_err(|_| output_refused("output.directory", json))? {
        return Err(output_refused("output.exists", json));
    }
    Ok(target)
}

/// The output's file name, if it has one World's confined access can make:
/// a last component that is a plain UTF-8 name (no `\`, `:` or NUL, at most
/// 255 bytes), and no separator after it.
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
        name.len() <= 255 && !name.contains(['\\', ':', '\0']) && *name != "." && *name != ".."
    })
}

/// Whether a file in `directory`, both resolved, would be inside the
/// world: in its root directory or beneath it.
fn inside_world(directory: &Path, world: &Path) -> bool {
    directory.starts_with(world)
}

/// Whether anything is at the target's name, a link included, as World's
/// access finds it (on a file system that ignores case, an entry named
/// otherwise counts too).
fn occupied(target: &Target) -> Result<bool, tilewright_source::AccessError> {
    SourceRoot::new(&target.directory).exists(&target.name)
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

/// Writes the new file. World's access makes it with exclusive creation,
/// not following a link at its name, syncs it and removes it again if the
/// write fails. Its write first removes whatever is at the name, so the
/// name is checked again just before.
fn create_file(target: &Target, png: &[u8]) -> Result<(), String> {
    if occupied(target).map_err(|error| error.to_string())? {
        return Err("something appeared at the output path; nothing was replaced".into());
    }
    let directory = SourceRoot::new(&target.directory)
        .directory("", false)
        .map_err(|error| error.to_string())?
        .ok_or("the output's directory is gone")?;
    directory
        .write(&target.name, None, png)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{file_name, inside_world, within_budget};
    use std::path::Path;
    use tilewright_editor_observe::limits::MAXIMUM_PNG_BYTES;

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
    fn a_file_in_the_root_or_beneath_it_is_inside() {
        let world = Path::new("/w/world");
        assert!(inside_world(Path::new("/w/world"), world));
        assert!(inside_world(Path::new("/w/world/planes"), world));
        assert!(!inside_world(Path::new("/w"), world));
        assert!(!inside_world(Path::new("/w/world2"), world));
        assert!(!inside_world(Path::new("/w/other"), world));
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
