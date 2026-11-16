// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `render` through the binary: the PNG against committed goldens, its
//! report, the one file it writes and every output-path refusal, with the
//! world tree and everything beside it left exactly as they were.
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tilewright_editor_testkit::fixture_world;

fn render(arguments: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tilewright-editor"))
        .arg("render")
        .args(arguments)
        .output()
        .unwrap()
}

/// Renders `root` to `out` with `extra` arguments and `--json`; returns
/// the exit code, the report or refusal (null when stdout is empty) and
/// stderr.
fn run(root: &Path, out: &Path, extra: &[&str]) -> (i32, Value, String) {
    let mut arguments: Vec<&OsStr> = vec![
        OsStr::new("--json"),
        OsStr::new("--out"),
        out.as_os_str(),
        root.as_os_str(),
    ];
    arguments.extend(extra.iter().map(OsStr::new));
    let output = render(&arguments);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value = if stdout.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&stdout).expect("strict JSON")
    };
    (
        output.status.code().unwrap(),
        value,
        String::from_utf8(output.stderr).unwrap(),
    )
}

/// Expects `render` to refuse with `code` (exit 4) and leave `out` absent.
fn refused(root: &Path, out: &Path, extra: &[&str], code: &str) {
    let (exit, value, stderr) = run(root, out, extra);
    assert_eq!(exit, 4, "{code}: {stderr}");
    assert_eq!(value["code"], code, "{stderr}");
    assert!(stderr.contains(code));
}

/// Width, height and RGBA8 pixels of a PNG.
fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(
        (info.color_type, info.bit_depth),
        (png::ColorType::Rgba, png::BitDepth::Eight)
    );
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

/// Lowercase hexadecimal SHA-256 of `bytes`.
fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    tilewright_editor_document::revision_hex(&sha2::Sha256::digest(bytes).into())
}

/// Compares the decoded pixels of the PNG `actual` with
/// `tests/golden/<name>.txt`: its size and the SHA-256 digest of its RGBA8
/// pixels, rows top to bottom. (PNG files in this repository go to Git
/// LFS, which CI does not fetch, so the golden is the digest.)
/// `TILEWRIGHT_UPDATE_GOLDEN=1` rewrites the golden instead.
fn golden(name: &str, actual: &[u8]) {
    let (width, height, pixels) = decode(actual);
    let digest = sha256(&pixels);
    let actual = format!("width {width}\nheight {height}\nrgba8-sha256 {digest}\n");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("TILEWRIGHT_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "golden {name} differs");
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A scratch directory holding a copy of the neutral sample as `world` and
/// an empty `out` directory beside it.
fn scratch() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    let out = directory.path().join("out");
    std::fs::create_dir(&out).unwrap();
    (directory, root, out)
}

type Entry = (String, Vec<u8>, std::fs::Permissions, std::time::SystemTime);

/// Every entry under `top`, not following links: its kind (with a link's
/// target), bytes, permissions and modification time.
fn snapshot(top: &Path) -> BTreeMap<PathBuf, Entry> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![top.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = std::fs::symlink_metadata(&path).unwrap();
            let (kind, bytes) = if metadata.is_symlink() {
                (
                    format!("link {}", std::fs::read_link(&path).unwrap().display()),
                    Vec::new(),
                )
            } else if metadata.is_dir() {
                pending.push(path.clone());
                ("directory".to_owned(), Vec::new())
            } else {
                ("file".to_owned(), std::fs::read(&path).unwrap())
            };
            entries.insert(
                path.strip_prefix(top).unwrap().to_path_buf(),
                (
                    kind,
                    bytes,
                    metadata.permissions(),
                    metadata.modified().unwrap(),
                ),
            );
        }
    }
    entries
}

/// Whether everything under `top` is as `before` recorded it. A bool, so a
/// failure does not print every file's bytes.
fn unchanged(top: &Path, before: &BTreeMap<PathBuf, Entry>) -> bool {
    snapshot(top) == *before
}

#[test]
fn render_writes_the_golden_png_and_reports_it() {
    let (_directory, root, out) = scratch();
    for (name, extra) in [
        ("render-neutral.png", &[][..]),
        (
            "render-neutral-region.png",
            &["--region", "2", "1", "4", "3"][..],
        ),
    ] {
        let file = out.join(name);
        let (exit, report, stderr) = run(&root, &file, extra);
        assert_eq!(exit, 0, "{stderr}");
        assert!(stderr.is_empty());
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        golden(name, &bytes);
        let (width, height, _) = decode(&bytes);
        assert_eq!(report["format"], "tilewright.editor-render/1");
        assert_eq!(report["image"]["width"], width);
        assert_eq!(report["image"]["height"], height);
        assert_eq!(report["png"]["bytes"], bytes.len());
        assert_eq!(report["png"]["sha256"], sha256(&bytes));
    }
    // The text form prints the same report.
    let file = out.join("text.png");
    let output = render(&[OsStr::new("--out"), file.as_os_str(), root.as_os_str()]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.starts_with("format: tilewright.editor-render/1\n"),
        "{text}"
    );
    assert!(text.contains("  width: 256\n  height: 192\n"), "{text}");
}

#[test]
fn render_is_byte_identical_run_to_run() {
    let (_directory, root, out) = scratch();
    let [first, second] = ["first.png", "second.png"].map(|name| out.join(name));
    for file in [&first, &second] {
        assert_eq!(run(&root, file, &["--scale", "3"]).0, 0);
    }
    assert_eq!(
        std::fs::read(first).unwrap(),
        std::fs::read(second).unwrap()
    );
}

#[test]
fn render_writes_one_new_file_and_leaves_the_world_untouched() {
    let (directory, root, out) = scratch();
    let before = snapshot(directory.path());
    let file = out.join("image.png");
    assert_eq!(run(&root, &file, &[]).0, 0);
    let mut after = snapshot(directory.path());
    let written = after.remove(Path::new("out/image.png")).unwrap();
    assert_eq!(written.0, "file");
    // The PNG gets the platform's default access, as a file the standard
    // library creates beside it does (on Unix 0o666 less the umask).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let probe = directory.path().join("probe");
        std::fs::write(&probe, b"").unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode(&file), mode(&probe));
        std::fs::remove_file(&probe).unwrap();
    }
    // The output directory's modification time changes with its new entry;
    // nothing else does, and nothing else appears.
    after.remove(Path::new("out"));
    let mut expected = before;
    expected.remove(Path::new("out"));
    assert!(
        after == expected,
        "the tree changed beyond the one new file"
    );
}

/// Applies `change` to the JSON document at `path` beneath `root`.
fn edit(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut value);
    std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
}

const MAIN_CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

/// Copies of the neutral sample beside `world` that `render` refuses for
/// what they are: no catalog, a catalog that does not load, a view the
/// preview cannot prepare, a 64 × 64 view (too many pixels at scale 3),
/// and 4,400 stacked markers (too much raster work at scale 8).
fn refused_worlds(parent: &Path) -> [(PathBuf, &'static [&'static str], &'static str); 5] {
    let neutral = fixture_world("prototype-neutral");
    let copy = |name: &str| {
        let root = parent.join(name);
        copy_tree(&neutral, &root);
        root
    };
    let absent = copy("absent");
    std::fs::remove_file(absent.join("preview.jsonc")).unwrap();
    let invalid = copy("invalid");
    std::fs::write(invalid.join("preview.jsonc"), "{\"format\": \"other\"}").unwrap();
    let unprepared = copy("unprepared");
    std::fs::write(
        unprepared.join("defs/proto/orphan.jsonc"),
        r#"{"format": "tilewright.def/1", "type": "proto", "id": "orphan", "fields": {}}"#,
    )
    .unwrap();
    edit(&unprepared, MAIN_CHUNK, |chunk| {
        chunk["objects"].as_array_mut().unwrap().push(
            serde_json::json!({"uid": "orphan_1", "at": [2, 2], "proto": "orphan", "layer": "item"}),
        );
    });
    let large = copy("large");
    edit(&large, "preview.jsonc", |catalog| {
        catalog["view"]["bounds"] = serde_json::json!([0, 0, 64, 64]);
    });
    let stacked = copy("stacked");
    edit(&stacked, MAIN_CHUNK, |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        for index in 0..4_400 {
            objects.push(serde_json::json!({
                "uid": format!("stack_{index}"),
                "at": [index % 8, (index / 8) % 6],
                "proto": "marker",
                "layer": "item",
            }));
        }
    });
    [
        (absent, &[], "catalog.absent"),
        (invalid, &[], "catalog.invalid"),
        (unprepared, &[], "preview.unprepared"),
        (large, &["--scale", "3"], "output.budget"),
        (stacked, &["--scale", "8"], "output.budget"),
    ]
}

#[test]
fn every_refusal_leaves_everything_as_it_was() {
    let (directory, root, out) = scratch();
    std::fs::write(out.join("existing.png"), b"keep me").unwrap();
    let worlds = refused_worlds(directory.path());
    // A directory beside the world whose name differs from it only in
    // case: on a file system that ignores case it is the world itself.
    let upper = directory.path().join("WORLD");
    let _ = std::fs::create_dir(&upper);
    let before = snapshot(directory.path());
    let cases: Vec<(PathBuf, &[&str], &str)> = vec![
        // In the world root, beneath it, and back into it through `..`.
        (root.join("image.png"), &[], "output.inside-world"),
        (root.join("planes/image.png"), &[], "output.inside-world"),
        (out.join("../world/image.png"), &[], "output.inside-world"),
        // A spelling of the world in another case, whether or not the
        // file system tells it apart.
        (upper.join("image.png"), &[], "output.inside-world"),
        // A file is never replaced.
        (out.join("existing.png"), &[], "output.exists"),
        // A directory is never created.
        (out.join("missing/image.png"), &[], "output.directory"),
        (out.join("existing.png/image.png"), &[], "output.directory"),
        // A name it cannot make, or one Windows would alter.
        (out.join("image.png/"), &[], "output.name"),
        (out.join(".."), &[], "output.name"),
        (out.join("a:b.png"), &[], "output.name"),
        (out.join("NUL.png"), &[], "output.name"),
        (out.join("image.png."), &[], "output.name"),
        (out.join("image.png "), &[], "output.name"),
        // The view and the budgets.
        (
            out.join("image.png"),
            &["--region", "8", "0", "1", "1"],
            "target.unknown",
        ),
        (
            out.join("image.png"),
            &["--region", "0", "0", "4097", "1"],
            "output.budget",
        ),
    ];
    for (file, extra, code) in &cases {
        refused(&root, file, extra, code);
        assert!(
            unchanged(directory.path(), &before),
            "{code} changed the tree"
        );
    }
    // Worlds without a view to draw, or too much to draw.
    for (world, extra, code) in &worlds {
        refused(world, &out.join("image.png"), extra, code);
        assert!(
            unchanged(directory.path(), &before),
            "{code} changed the tree"
        );
    }
    // A usage error and a world that does not load write nothing either.
    let (exit, _, _) = run(&root, &out.join("image.png"), &["--scale", "0"]);
    assert_eq!(exit, 2);
    let (exit, value, _) = run(&out, &out.join("image.png"), &[]);
    assert_eq!(
        (exit, &value["format"]),
        (3, &Value::from("tilewright.editor-load-failure/1"))
    );
    assert!(unchanged(directory.path(), &before));
    // render.failed is not here: the reference renderer cannot be made to
    // fail on a world that prepares without injecting a fault.
}

#[test]
fn the_world_loads_first_then_the_output_is_checked_then_the_view() {
    let (directory, root, out) = scratch();
    let [_, _, (unprepared, _, _), ..] = refused_worlds(directory.path());
    let before = snapshot(directory.path());
    // A bad output and a view that cannot be drawn: the output refusal
    // wins, and nothing is drawn or written.
    refused(
        &unprepared,
        &unprepared.join("image.png"),
        &[],
        "output.inside-world",
    );
    refused(
        &unprepared,
        &out.join("missing/image.png"),
        &[],
        "output.directory",
    );
    refused(
        &root,
        &out.join("missing/image.png"),
        &["--region", "9", "9", "1", "1"],
        "output.directory",
    );
    // A bad output and a world that does not load: the load failure wins,
    // since the world's root must resolve before the output is compared
    // with it.
    let (exit, value, _) = run(&out.join("no-world"), &out.join("missing/image.png"), &[]);
    assert_eq!(
        (exit, &value["format"]),
        (3, &Value::from("tilewright.editor-load-failure/1"))
    );
    assert!(unchanged(directory.path(), &before));
}

#[test]
fn a_sibling_whose_name_only_starts_like_the_world_is_outside() {
    let (directory, root, _) = scratch();
    for name in ["world2", "worlds", "WORLD-images", "other"] {
        let sibling = directory.path().join(name);
        std::fs::create_dir(&sibling).unwrap();
        let (exit, _, stderr) = run(&root, &sibling.join("image.png"), &[]);
        assert_eq!(exit, 0, "{name}: {stderr}");
        assert!(sibling.join("image.png").is_file());
    }
}

#[test]
fn the_png_is_8_bit_rgba_with_no_ancillary_chunks() {
    let (_directory, root, out) = scratch();
    let file = out.join("image.png");
    assert_eq!(run(&root, &file, &[]).0, 0);
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut chunks = Vec::new();
    let mut at = 8;
    while at < bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = String::from_utf8(bytes[at + 4..at + 8].to_vec()).unwrap();
        if kind == "IHDR" {
            let header = &bytes[at + 8..at + 8 + length];
            // Width, height, then bit depth 8, colour type 6 (RGBA),
            // deflate, adaptive filtering, no interlace.
            assert_eq!(&header[8..], [8, 6, 0, 0, 0]);
        }
        chunks.push(kind);
        at += 12 + length;
    }
    chunks.dedup();
    assert_eq!(chunks, ["IHDR", "IDAT", "IEND"]);
}

#[cfg(unix)]
#[test]
fn links_never_lead_a_render_into_the_world_or_through_a_name() {
    use std::os::unix::fs::symlink;
    let (directory, root, out) = scratch();
    std::fs::write(out.join("target.png"), b"keep me").unwrap();
    // A directory link into the world, and the world reached through it.
    symlink(&root, out.join("world-link")).unwrap();
    symlink(root.join("planes"), out.join("planes-link")).unwrap();
    // Links at the file's name: to a file, to nothing, and into the world.
    symlink(out.join("target.png"), out.join("to-file.png")).unwrap();
    symlink(out.join("nothing.png"), out.join("dangling.png")).unwrap();
    symlink(root.join("new.png"), out.join("into-world.png")).unwrap();
    let before = snapshot(directory.path());
    for (file, code) in [
        (out.join("world-link/image.png"), "output.inside-world"),
        (out.join("planes-link/image.png"), "output.inside-world"),
        (out.join("to-file.png"), "output.exists"),
        (out.join("dangling.png"), "output.exists"),
        (out.join("into-world.png"), "output.exists"),
    ] {
        refused(&root, &file, &[], code);
        assert!(
            unchanged(directory.path(), &before),
            "{code} changed the tree"
        );
    }
    assert!(!out.join("nothing.png").exists() && !root.join("new.png").exists());
    // A world root given through a link is the same world.
    let linked_root = out.join("world-link");
    refused(
        &linked_root,
        &root.join("image.png"),
        &[],
        "output.inside-world",
    );
    // The output's directory may be a link to a directory outside the
    // world: links are resolved before the comparison, not refused.
    let elsewhere = directory.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, out.join("elsewhere-link")).unwrap();
    assert_eq!(run(&root, &out.join("elsewhere-link/image.png"), &[]).0, 0);
    assert!(elsewhere.join("image.png").is_file());
}

#[cfg(unix)]
#[test]
fn a_failed_write_exits_5_names_the_path_escaped_and_leaves_no_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_directory, root, out) = scratch();
    let locked = out.join("locked\u{1b}[31m");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Where permissions do not bind (as root), there is nothing to test.
    if std::fs::write(locked.join("probe"), b"").is_ok() {
        std::fs::remove_file(locked.join("probe")).unwrap();
        return;
    }
    let (exit, value, stderr) = run(&root, &locked.join("image.png"), &[]);
    assert_eq!(exit, 5, "{stderr}");
    assert_eq!(value["code"], "output.write");
    assert!(
        stderr.starts_with("tilewright-editor: output.write: "),
        "{stderr}"
    );
    assert!(stderr.contains(r"locked\u{1b}[31m/image.png"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'));
    assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 0);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(windows)]
#[test]
fn the_world_is_found_whatever_case_or_separators_name_it() {
    let (_directory, root, out) = scratch();
    let upper = root
        .parent()
        .unwrap()
        .join(root.file_name().unwrap().to_str().unwrap().to_uppercase());
    for file in [
        upper.join("image.png"),
        PathBuf::from(format!("{}\\planes\\image.png", upper.display())),
        PathBuf::from(format!("{}/../WORLD/image.png", out.display())),
    ] {
        refused(&root, &file, &[], "output.inside-world");
    }
    assert_eq!(run(&root, &out.join("image.png"), &[]).0, 0);
}

#[cfg(unix)]
#[test]
fn a_hostile_output_directory_is_written_into_and_never_echoed() {
    let (_directory, root, out) = scratch();
    // Terminal control characters may be in the directory's name, never in
    // the file's. The report never names the output path, so they cannot
    // reach stdout; stderr, which names it only when a write fails, escapes
    // them (a_failed_write_exits_5_names_the_path_escaped_and_leaves_no_file).
    let hostile = out.join("hostile\u{1b}]0;title\u{7}");
    std::fs::create_dir(&hostile).unwrap();
    let file = hostile.join("image.png");
    let (exit, report, stderr) = run(&root, &file, &[]);
    assert_eq!((exit, stderr.as_str()), (0, ""));
    assert!(!report.to_string().contains("hostile"));
    assert!(file.is_file());
    // The same characters in the file's name are refused.
    refused(&root, &hostile.join("x\u{1b}[31m.png"), &[], "output.name");
}
