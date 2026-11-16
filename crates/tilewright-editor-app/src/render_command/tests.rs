// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use super::{
    Target, create_file, file_name, inside_world, target, within_budget, write_outcome,
    write_refusal,
};
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{EXIT_REFUSED, EXIT_WRITE_FAILED};
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

/// What can be at the output's name when the write starts: made by `put`
/// in the output directory as `image.png`, then checked by `kept`.
struct Occupant {
    kind: &'static str,
    put: fn(&Path),
    kept: fn(&Path) -> bool,
}

fn occupants() -> Vec<Occupant> {
    let mut occupants = vec![
        Occupant {
            kind: "file",
            put: |name| std::fs::write(name, b"keep me").unwrap(),
            kept: |name| std::fs::read(name).is_ok_and(|bytes| bytes == b"keep me"),
        },
        Occupant {
            kind: "directory",
            put: |name| std::fs::create_dir(name).unwrap(),
            kept: |name| {
                name.symlink_metadata().is_ok_and(|entry| entry.is_dir())
                    && std::fs::read_dir(name).is_ok_and(|mut entries| entries.next().is_none())
            },
        },
    ];
    #[cfg(unix)]
    occupants.extend([
        Occupant {
            kind: "link to a file",
            put: |name| {
                std::fs::write(name.with_file_name("linked"), b"keep me").unwrap();
                std::os::unix::fs::symlink("linked", name).unwrap();
            },
            kept: |name| {
                std::fs::read_link(name).is_ok_and(|to| to == Path::new("linked"))
                    && std::fs::read(name.with_file_name("linked"))
                        .is_ok_and(|bytes| bytes == b"keep me")
            },
        },
        Occupant {
            kind: "dangling link",
            put: |name| std::os::unix::fs::symlink("missing", name).unwrap(),
            kept: |name| {
                std::fs::read_link(name).is_ok_and(|to| to == Path::new("missing"))
                    && !name.with_file_name("missing").exists()
            },
        },
    ]);
    occupants
}

/// A world and an empty output directory beside it, and the checked target
/// `out/image.png`.
fn checked_target() -> (tempfile::TempDir, PathBuf, Target) {
    let scratch = tempfile::tempdir().unwrap();
    let top = SourceRoot::new(scratch.path());
    assert!(top.make_directory("world").unwrap());
    assert!(top.make_directory("out").unwrap());
    let checked = target(
        &scratch.path().join("world"),
        &scratch.path().join("out/image.png"),
        true,
    )
    .unwrap_or_else(|outcome| panic!("{outcome:?}"));
    let out = scratch.path().join("out");
    (scratch, out, checked)
}

#[test]
fn a_name_taken_after_the_checks_is_refused_by_the_write_and_left_as_it_was() {
    for occupant in occupants() {
        let (_scratch, out, checked) = checked_target();
        // Another writer takes the name between the checks and the write.
        let name = out.join("image.png");
        (occupant.put)(&name);
        let before: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let error = create_file(&checked, b"\x89PNG").unwrap_err();
        assert!(error.already_exists(), "{}: {error}", occupant.kind);
        assert!(!error.may_be_left(), "{}: {error}", occupant.kind);
        assert!((occupant.kept)(&name), "{} was changed", occupant.kind);
        let after: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(after, before, "{}", occupant.kind);
        // The command reports it as the refusal the checks would have given.
        let outcome = write_refusal(&name, &error, true);
        assert_eq!(outcome.code, EXIT_REFUSED, "{}", occupant.kind);
        assert!(
            outcome.stdout.contains("\"output.exists\""),
            "{}",
            outcome.stdout
        );
    }
}

#[test]
fn a_name_taken_before_the_checks_is_refused_before_anything_is_drawn() {
    for occupant in occupants() {
        let (scratch, out, _) = checked_target();
        (occupant.put)(&out.join("image.png"));
        let outcome = target(&scratch.path().join("world"), &out.join("image.png"), true)
            .err()
            .unwrap_or_else(|| panic!("{} was not refused", occupant.kind));
        assert_eq!(outcome.code, EXIT_REFUSED, "{}", occupant.kind);
        assert!(
            outcome.stdout.contains("\"output.exists\""),
            "{}",
            outcome.stdout
        );
        assert!((occupant.kept)(&out.join("image.png")), "{}", occupant.kind);
    }
}

#[test]
fn a_free_name_is_written_once_and_then_refused() {
    let (_scratch, out, checked) = checked_target();
    create_file(&checked, b"\x89PNG").unwrap();
    assert_eq!(std::fs::read(out.join("image.png")).unwrap(), b"\x89PNG");
    let error = create_file(&checked, b"other").unwrap_err();
    assert!(error.already_exists(), "{error}");
    assert_eq!(std::fs::read(out.join("image.png")).unwrap(), b"\x89PNG");
}

#[test]
fn a_failed_write_says_whether_its_file_may_be_left() {
    let out = Path::new("out\u{1b}[31m.png");
    for json in [false, true] {
        // Taken since the checks: a refusal, whatever else is said.
        for left in [false, true] {
            let outcome = write_outcome(out, true, left, "taken", json);
            assert_eq!(outcome.code, EXIT_REFUSED);
            assert!(
                outcome
                    .stderr
                    .starts_with("tilewright-editor: output.exists: ")
            );
            assert_eq!(outcome.stdout.contains("\"output.exists\""), json);
        }
        let gone = write_outcome(out, false, false, "denied", json);
        assert_eq!(gone.code, EXIT_WRITE_FAILED);
        assert!(gone.stderr.starts_with("tilewright-editor: output.write: "));
        assert!(
            gone.stderr.contains(r"out\u{1b}[31m.png: denied"),
            "{}",
            gone.stderr
        );
        assert!(!gone.stderr.contains("partly written"), "{}", gone.stderr);
        assert_eq!(gone.stdout.contains("left no file of its own"), json);
        let left = write_outcome(out, false, true, "disk full", json);
        assert_eq!(left.code, EXIT_WRITE_FAILED);
        assert!(
            left.stderr.ends_with(
                ": disk full; the file this command made may be left there, partly written\n"
            ),
            "{}",
            left.stderr
        );
        assert!(!left.stderr.contains('\u{1b}'));
        assert_eq!(left.stdout.contains("may be left at the output path"), json);
        assert!(!left.stdout.contains("left no file"));
    }
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
