// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! `render`'s arguments, framing, bounds and refusals, without writing:
//! the binary's tests cover the PNG, the file and the goldens.
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tilewright_editor_observe::cli::{self, Outcome};
use tilewright_editor_observe::render::{self, Rendered, Request};
use tilewright_editor_preview::{PreviewLimits, ReferenceFrame, loader};
use tilewright_editor_testkit::fixture_world;

fn request(arguments: &[&str]) -> Result<Request, Outcome> {
    let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
    cli::render_request(&arguments)
}

/// Draws `root` with `extra` arguments, or returns the outcome to print.
fn draw(root: &Path, extra: &[&str]) -> Result<Rendered, Outcome> {
    let root = root.to_str().unwrap();
    let mut arguments = vec!["--json", "--out", "unused.png", root];
    arguments.extend_from_slice(extra);
    let request = request(&arguments)?;
    let observed = render::open(&request)?;
    render::render(&observed, &request)
}

fn refusal(root: &Path, extra: &[&str]) -> Value {
    let outcome = draw(root, extra).err().expect("refused");
    assert_eq!(outcome.code, cli::EXIT_REFUSED, "{outcome:?}");
    let value: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(value["format"], "tilewright.editor-refusal/1");
    assert!(outcome.stderr.contains(value["code"].as_str().unwrap()));
    value
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

fn neutral_copy() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    (directory, root)
}

/// Rewrites the catalog view's bounds of the world at `root`.
fn set_view(root: &Path, bounds: [i64; 4]) {
    let path = root.join("preview.jsonc");
    let mut catalog: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    catalog["view"]["bounds"] = json!(bounds);
    std::fs::write(path, serde_json::to_string_pretty(&catalog).unwrap()).unwrap();
}

/// The `width` × `height` pixels of `frame` from `left`, `top`.
fn crop(frame: &ReferenceFrame, left: u32, top: u32, width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::new();
    for row in top..top + height {
        let start = ((row * frame.width + left) * 4) as usize;
        pixels.extend_from_slice(&frame.rgba8[start..start + (width * 4) as usize]);
    }
    pixels
}

/// The World frame the desktop editor shows when it opens `root`: its
/// default 1280 × 800 view, prepared and drawn as the editor does.
fn desktop_frame(root: &Path) -> ReferenceFrame {
    let document = tilewright_editor_document::WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let view = catalog.default_view(document.world(), 1280, 800, 1);
    tilewright_editor_preview::prepare(
        document.world(),
        &catalog,
        &resources,
        &view,
        PreviewLimits::default(),
    )
    .unwrap()
    .reference_frame()
    .unwrap()
}

fn report(rendered: Rendered) -> Value {
    let outcome = rendered.finish(1234, &[0xab; 32]);
    assert_eq!(outcome.code, cli::EXIT_SUCCESS);
    serde_json::from_str(&outcome.stdout).unwrap()
}

#[test]
fn the_whole_view_is_the_desktop_view_uncut() {
    let root = fixture_world("prototype-neutral");
    let rendered = draw(&root, &[]).unwrap();
    let frame = rendered.frame.clone();
    // 8 × 6 tiles of 32 pixels: nothing in the sample reaches past them.
    assert_eq!([frame.width, frame.height], [256, 192]);
    assert_eq!(frame.rgba8.len(), 256 * 192 * 4);
    // The same camera and renderer as the desktop's default view, so the
    // image is the middle of what the desktop shows.
    let desktop = desktop_frame(&root);
    assert_eq!(
        frame.rgba8,
        crop(&desktop, (1280 - 256) / 2, (800 - 192) / 2, 256, 192)
    );
    let value = report(rendered);
    assert_eq!(value["format"], "tilewright.editor-render/1");
    assert_eq!(value["catalog"]["status"], "loaded");
    assert_eq!(
        value["image"],
        json!({"plane": "main", "layer": 0, "region": [0, 0, 8, 6], "tiles": 48, "scale": 1, "width": 256, "height": 192})
    );
    assert_eq!(value["diagnostics"][0]["code"], "preview.effects-neutral");
    assert_eq!(value["diagnostics_more"], 0);
    assert_eq!(
        value["png"],
        json!({"bytes": 1234, "sha256": "ab".repeat(32)})
    );
}

#[test]
fn a_region_draws_only_its_tiles_and_scale_multiplies_pixels() {
    let root = fixture_world("prototype-neutral");
    let whole = draw(&root, &[]).unwrap().frame;
    let region = draw(&root, &["--region", "2", "1", "4", "3"]).unwrap();
    let frame = region.frame.clone();
    assert_eq!([frame.width, frame.height], [128, 96]);
    // In this top-down sample no art reaches past its tile, so the region
    // is exactly that part of the whole view. That is not so in general:
    // art that reaches across a tile edge (tall objects, raised ground,
    // isometric neighbours) is drawn only for tiles inside the region.
    assert_eq!(frame.rgba8, crop(&whole, 64, 32, 128, 96));
    assert_eq!(report(region)["image"]["region"], json!([2, 1, 4, 3]));
    // Each art pixel becomes a scale × scale block.
    let double = draw(&root, &["--scale", "2"]).unwrap().frame;
    assert_eq!([double.width, double.height], [512, 384]);
    for (y, x) in [(0, 0), (37, 101), (191, 255), (100, 200)] {
        let original = &crop(&whole, x, y, 1, 1);
        assert_eq!(&crop(&double, 2 * x, 2 * y, 2, 1)[..4], original);
        assert_eq!(&crop(&double, 2 * x, 2 * y, 2, 2)[12..], original);
    }
}

#[test]
fn rendering_is_deterministic() {
    let root = fixture_world("prototype-neutral");
    let first = draw(&root, &["--region", "1", "1", "5", "4"]).unwrap();
    let second = draw(&root, &["--region", "1", "1", "5", "4"]).unwrap();
    assert_eq!(first.frame, second.frame);
    assert_eq!(report(first), report(second));
}

#[test]
fn the_tile_bound_is_inclusive_at_4096() {
    let (_directory, root) = neutral_copy();
    set_view(&root, [0, 0, 64, 64]);
    // 4,096 tiles at scale 2 is also exactly the pixel bound: 4,096 × 4,096.
    let rendered = draw(&root, &["--scale", "2"]).unwrap();
    assert_eq!([rendered.frame.width, rendered.frame.height], [4096, 4096]);
    assert_eq!(report(rendered)["image"]["tiles"], 4096);
    // One tile more is refused before the view is consulted, so the region
    // need not fit the view to be refused for its size.
    for region in [
        ["0", "0", "4097", "1"],
        ["0", "0", "1", "4097"],
        ["0", "0", "65", "64"],
    ] {
        let mut extra = vec!["--region"];
        extra.extend_from_slice(&region);
        let value = refusal(&root, &extra);
        assert_eq!(value["code"], "output.budget", "{region:?}");
        assert_eq!(value["message"], "The region has more than 4,096 tiles.");
    }
    // Past the pixel bound: 6,144 pixels a side at scale 3.
    let value = refusal(&root, &["--scale", "3"]);
    assert_eq!(value["code"], "output.budget");
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("16,777,216 pixels")
    );
    // Past the side bound with few pixels: a 4,096 × 1 view is 131,072
    // pixels wide.
    set_view(&root, [0, 0, 4096, 1]);
    assert_eq!(refusal(&root, &[])["code"], "output.budget");
    let narrow = draw(&root, &["--region", "0", "0", "256", "1"])
        .unwrap()
        .frame;
    assert_eq!([narrow.width, narrow.height], [8192, 32]);
    assert_eq!(
        refusal(&root, &["--region", "0", "0", "257", "1"])["code"],
        "output.budget"
    );
}

#[test]
fn a_region_must_lie_inside_the_view() {
    let root = fixture_world("prototype-neutral");
    for region in [
        ["-1", "0", "1", "1"],
        ["0", "-1", "1", "1"],
        ["8", "0", "1", "1"],
        ["0", "6", "1", "1"],
        ["7", "0", "2", "1"],
        ["0", "5", "1", "2"],
        ["9223372036854775807", "0", "1", "1"],
    ] {
        let mut extra = vec!["--region"];
        extra.extend_from_slice(&region);
        let value = refusal(&root, &extra);
        assert_eq!(value["code"], "target.unknown", "{region:?}");
        assert_eq!(
            value["message"],
            "The region is not inside the catalog's view."
        );
    }
    // Its last tile may be the view's last.
    let corner = draw(&root, &["--region", "7", "5", "1", "1"]).unwrap();
    assert_eq!([corner.frame.width, corner.frame.height], [32, 32]);
}

#[test]
fn a_world_without_a_usable_view_is_refused_with_its_reason() {
    // No catalog.
    let value = refusal(&fixture_world("meadow"), &[]);
    assert_eq!(value["code"], "catalog.absent");
    // A catalog that does not load.
    let (_directory, root) = neutral_copy();
    std::fs::write(root.join("preview.jsonc"), "{\"format\": \"other\"}").unwrap();
    assert_eq!(refusal(&root, &[])["code"], "catalog.invalid");
    // A view the preview cannot prepare: an object the catalog cannot
    // draw. The refusal names no world text; lint gives the reason.
    let (_directory, root) = neutral_copy();
    std::fs::write(
        root.join("defs/proto/orphan.jsonc"),
        r#"{"format": "tilewright.def/1", "type": "proto", "id": "orphan", "fields": {}}"#,
    )
    .unwrap();
    let chunk = root.join("planes/main/layers/0/0_0.jsonc");
    let mut value: Value = serde_json::from_str(&std::fs::read_to_string(&chunk).unwrap()).unwrap();
    value["objects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"uid": "orphan_1", "at": [2, 2], "proto": "orphan", "layer": "item"}));
    std::fs::write(&chunk, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let value = refusal(&root, &[]);
    assert_eq!(value["code"], "preview.unprepared");
    assert!(!value.to_string().contains("orphan"));
    // The object is outside this region, so the region still draws.
    assert!(draw(&root, &["--region", "0", "0", "2", "2"]).is_ok());
    // A world that does not load prints its load failure (exit 3).
    let outcome = draw(Path::new("no/such/world"), &[]).err().unwrap();
    assert_eq!(outcome.code, cli::EXIT_NOT_LOADED);
    let value: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(value["format"], "tilewright.editor-load-failure/1");
}

#[test]
fn render_arguments_are_checked() {
    let parsed = request(&[
        "--out", "a.png", "--region", "-3", "4", "5", "6", "--scale", "8", "--json", "ROOT",
    ])
    .unwrap();
    assert_eq!(
        parsed,
        Request {
            root: "ROOT".into(),
            out: "a.png".into(),
            region: Some([-3, 4, 5, 6]),
            scale: 8,
            json: true,
        }
    );
    let parsed = request(&["ROOT", "--out", "-a.png"]).unwrap();
    assert_eq!((parsed.scale, parsed.region, parsed.json), (1, None, false));
    assert_eq!(parsed.out, OsString::from("-a.png"));
    for (arguments, problem) in [
        (&["ROOT"][..], "render needs --out FILE"),
        (&["--out"][..], "--out needs a FILE"),
        (
            &["--out", "--json", "ROOT"][..],
            "--out FILE may not start with --",
        ),
        (&["--out", "", "ROOT"][..], "--out FILE may not be empty"),
        (
            &["--out", "a", "--out", "b", "ROOT"][..],
            "unknown or repeated option --out",
        ),
        (&["--out", "a"][..], "render takes 1 argument(s)"),
        (
            &["--out", "a", "R1", "R2"][..],
            "render takes 1 argument(s)",
        ),
        (
            &["--out", "a", "--region", "0", "0", "1", "ROOT"][..],
            "--region takes four integers",
        ),
        (
            &["--out", "a", "--region", "0", "0", "1"][..],
            "--region takes four integers",
        ),
        (
            &["--out", "a", "--region", "0", "0", "0", "1", "ROOT"][..],
            "W and H must be at least 1",
        ),
        (
            &["--out", "a", "--region", "0", "0", "1", "-1", "ROOT"][..],
            "W and H must be at least 1",
        ),
        (
            &[
                "--out", "a", "--region", "0", "0", "1", "1", "--region", "0", "0", "1", "1",
                "ROOT",
            ][..],
            "repeated option --region",
        ),
        (
            &["--out", "a", "--scale", "0", "ROOT"][..],
            "--scale takes an integer from 1 to 8",
        ),
        (
            &["--out", "a", "--scale", "9", "ROOT"][..],
            "--scale takes an integer from 1 to 8",
        ),
        (
            &["--out", "a", "--scale", "1.5", "ROOT"][..],
            "--scale takes an integer from 1 to 8",
        ),
        (
            &["--out", "a", "--scale", "2", "--scale", "2", "ROOT"][..],
            "repeated option --scale",
        ),
        (
            &["--out", "a", "--json", "--json", "ROOT"][..],
            "repeated option --json",
        ),
        (
            &["--out", "a", "--match", "x", "ROOT"][..],
            "unknown or repeated option --match",
        ),
    ] {
        let outcome = request(arguments).expect_err("usage error");
        assert_eq!(outcome.code, cli::EXIT_USAGE, "{arguments:?}");
        assert!(outcome.stdout.is_empty());
        assert!(
            outcome.stderr.contains(problem),
            "{arguments:?}: {}",
            outcome.stderr
        );
        assert!(outcome.stderr.contains("render [--json] --out FILE"));
    }
    // `render` is not one of the printing commands `run` runs.
    let outcome = cli::run("render", &[OsString::from("ROOT")]);
    assert_eq!(outcome.code, cli::EXIT_USAGE);
}

#[test]
fn a_write_failure_names_the_path_escaped_and_exits_5() {
    for json in [false, true] {
        let outcome = cli::write_failed(Path::new("out\u{1b}[31m.png"), "denied\u{7}", json);
        assert_eq!(outcome.code, cli::EXIT_WRITE_FAILED);
        assert!(!outcome.stderr.contains('\u{1b}') && !outcome.stderr.contains('\u{7}'));
        assert!(outcome.stderr.contains("output.write"));
        assert!(
            outcome.stderr.contains(r"out\u{1b}[31m.png"),
            "{}",
            outcome.stderr
        );
        assert_eq!(outcome.stdout.contains("\"output.write\""), json);
        assert!(!outcome.stdout.contains("31m") && !outcome.stdout.contains("denied"));
    }
}
