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

/// The CPU reference rendering of the scene the desktop editor prepares
/// when it opens `root`: its default 1280 × 800 view. The desktop draws the
/// same prepared scene on the GPU; this is not that GPU frame.
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
    // The same camera and scene as the desktop's default view, so the
    // image is the middle of that view's reference rendering.
    let desktop = desktop_frame(&root);
    assert_eq!(
        frame.rgba8,
        crop(&desktop, (1280 - 256) / 2, (800 - 192) / 2, 256, 192)
    );
    // It is the World scene, not the cell scene (which covers every tile
    // with opaque white): the sample's one empty tile, (6, 4), shows the
    // clear colour, and grass at (0, 0) is green.
    assert_eq!(
        crop(&frame, 6 * 32 + 16, 4 * 32 + 16, 1, 1),
        [10, 13, 18, 255]
    );
    let grass = crop(&frame, 16, 16, 1, 1);
    assert!(grass[1] > grass[0] && grass[1] > grass[2], "{grass:?}");
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
    // This region holds marker_1, whose art reaches 8 pixels above its
    // tile, whole, and no art from outside reaches into it, so it is
    // exactly that part of the whole view.
    assert_eq!(frame.rgba8, crop(&whole, 64, 32, 128, 96));
    assert_eq!(report(region)["image"]["region"], json!([2, 1, 4, 3]));
    // With marker_1's tile on its top row, the art reaches past the region:
    // the frame grows by 8 pixels above and, being centred, 8 below, where
    // nothing is drawn. The tiles themselves are that part of the view.
    let reaching = draw(&root, &["--region", "2", "2", "4", "2"])
        .unwrap()
        .frame;
    assert_eq!([reaching.width, reaching.height], [128, 80]);
    assert_eq!(
        crop(&reaching, 0, 8, 128, 64),
        crop(&whole, 64, 64, 128, 64)
    );
    assert_ne!(
        crop(&reaching, 0, 0, 128, 8),
        [10, 13, 18, 255].repeat(128 * 8)
    );
    assert_eq!(
        crop(&reaching, 0, 72, 128, 8),
        [10, 13, 18, 255].repeat(128 * 8)
    );
    // Each art pixel becomes a scale × scale block.
    let double = draw(&root, &["--scale", "2"]).unwrap().frame;
    assert_eq!([double.width, double.height], [512, 384]);
    for (y, x) in [(0, 0), (37, 101), (191, 255), (100, 200)] {
        let original = &crop(&whole, x, y, 1, 1);
        assert_eq!(&crop(&double, 2 * x, 2 * y, 2, 1)[..4], original);
        assert_eq!(&crop(&double, 2 * x, 2 * y, 2, 2)[12..], original);
    }
}

/// Sets the profile's projection of the world at `root`.
fn set_projection(root: &Path, projection: Value) {
    let path = root.join("profile/profile.jsonc");
    let mut profile: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    profile["projection"] = projection;
    std::fs::write(path, serde_json::to_string_pretty(&profile).unwrap()).unwrap();
}

/// Applies `change` to the neutral copy's main chunk.
fn edit_chunk(root: &Path, change: impl FnOnce(&mut Value)) {
    let path = root.join("planes/main/layers/0/0_0.jsonc");
    let mut chunk: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    change(&mut chunk);
    std::fs::write(path, serde_json::to_string_pretty(&chunk).unwrap()).unwrap();
}

#[test]
fn an_isometric_view_is_framed_like_the_desktop_view() {
    let (_directory, root) = neutral_copy();
    set_projection(
        &root,
        json!({"kind": "isometric", "tile_width": 64, "tile_height": 32}),
    );
    let frame = draw(&root, &[]).unwrap().frame;
    // 8 × 6 tiles: a diamond 14 half-tiles wide and high, at least.
    assert!(
        frame.width >= 14 * 32 && frame.height >= 14 * 16,
        "{}x{}",
        frame.width,
        frame.height
    );
    assert_eq!((frame.width % 2, frame.height % 2), (0, 0));
    let desktop = desktop_frame(&root);
    assert_eq!(
        frame.rgba8,
        crop(
            &desktop,
            (1280 - frame.width) / 2,
            (800 - frame.height) / 2,
            frame.width,
            frame.height
        )
    );
    // Nothing the desktop view draws lies outside the frame.
    let clear = [10, 13, 18, 255];
    let outside = (0..800).any(|y| {
        (0..1280).any(|x| {
            let inside_x = (1280 - frame.width) / 2 <= x && x < u32::midpoint(1280, frame.width);
            let inside_y = (800 - frame.height) / 2 <= y && y < u32::midpoint(800, frame.height);
            !(inside_x && inside_y) && crop(&desktop, x, y, 1, 1) != clear
        })
    });
    assert!(!outside);
    // A region at scale 3 is framed the same way.
    let region = draw(&root, &["--region", "1", "1", "3", "2", "--scale", "3"])
        .unwrap()
        .frame;
    assert!(region.width >= 5 * 32 * 3 && region.height >= 5 * 16 * 3);
}

#[test]
fn ground_lowered_below_the_region_widens_the_frame_downwards() {
    let (_directory, root) = neutral_copy();
    // Tile (2, 5), on the bottom row, 40 pixels lower than the rest.
    edit_chunk(&root, |chunk| {
        let mut rows = vec![vec![0; 32]; 32];
        rows[5][2] = -40;
        chunk["terrain"]["height"] = json!({"rows": rows});
    });
    let frame = draw(&root, &[]).unwrap().frame;
    // 96 pixels below the centre to the bottom row's edge, and 40 more.
    assert_eq!([frame.width, frame.height], [256, 2 * (96 + 40)]);
}

#[test]
fn work_only_this_render_exceeds_is_a_budget_refusal() {
    let (_directory, root) = neutral_copy();
    // 4,400 markers stacked on the view's tiles: at scale 8 each charges
    // about 62,000 raster samples, past the scene's 268,435,456 in all,
    // while the desktop's default view at scale 1 charges about 1,000.
    edit_chunk(&root, |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        for index in 0..4_400 {
            objects.push(json!({
                "uid": format!("stack_{index}"),
                "at": [index % 8, (index / 8) % 6],
                "proto": "marker",
                "layer": "item",
            }));
        }
    });
    let value = refusal(&root, &["--scale", "8"]);
    assert_eq!(value["code"], "output.budget");
    assert_eq!(
        value["message"],
        "The region at this scale exceeds the renderer's scene budget; narrow the region or lower the scale."
    );
    assert!(draw(&root, &[]).is_ok());
    assert!(draw(&root, &["--scale", "8", "--region", "0", "0", "1", "1"]).is_ok());
}

#[test]
fn the_measuring_compile_never_refuses_what_the_final_compile_fits() {
    let (_directory, root) = neutral_copy();
    // A standing prototype with tile-sized art, stacked on one tile.
    std::fs::write(
        root.join("defs/proto/slab.jsonc"),
        r#"{"format": "tilewright.def/1", "type": "proto", "id": "slab", "fields": {}}"#,
    )
    .unwrap();
    let path = root.join("preview.jsonc");
    let mut catalog: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    catalog["prototypes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"proto": "slab", "resource": "grass", "anchor": "standing"}));
    std::fs::write(&path, serde_json::to_string_pretty(&catalog).unwrap()).unwrap();
    // At scale 8 tile (0, 0) is a 256 × 256 frame. In it the final compile
    // charges each quad 256 × 256 raster samples: 4,090 slabs and the
    // tile's ground charge 268,107,776, within the scene's 268,435,456. The
    // measuring viewport charges each 257 × 257, about 270 million in all,
    // so without headroom the measuring compile would refuse it.
    edit_chunk(&root, |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        for index in 0..4_090 {
            objects.push(json!({
                "uid": format!("slab_{index}"),
                "at": [0, 0],
                "proto": "slab",
                "layer": "item",
            }));
        }
    });
    // Framed and prepared, not drawn: drawing it takes seconds in a debug
    // build, and the final compile at the default limits is the check.
    let arguments: Vec<OsString> = [
        "--out",
        "unused.png",
        root.to_str().unwrap(),
        "--region",
        "0",
        "0",
        "1",
        "1",
        "--scale",
        "8",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let parsed = cli::render_request(&arguments).unwrap();
    let observed = render::open(&parsed).unwrap();
    let framed =
        render::frame_region(&observed, &parsed).unwrap_or_else(|outcome| panic!("{outcome:?}"));
    assert_eq!(framed.size(), [256, 256]);
    // Twice as many is past the final compile's budget as well.
    edit_chunk(&root, |chunk| {
        let objects = chunk["objects"].as_array_mut().unwrap();
        for index in 4_090..8_180 {
            objects.push(json!({
                "uid": format!("slab_{index}"),
                "at": [0, 0],
                "proto": "slab",
                "layer": "item",
            }));
        }
    });
    let value = refusal(&root, &["--region", "0", "0", "1", "1", "--scale", "8"]);
    assert_eq!(value["code"], "output.budget");
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

#[test]
fn a_write_failure_that_may_leave_its_file_says_so() {
    for json in [false, true] {
        let outcome =
            cli::write_failed_leaving_file(Path::new("out\u{1b}[31m.png"), "full\u{7}", json);
        assert_eq!(outcome.code, cli::EXIT_WRITE_FAILED);
        assert!(!outcome.stderr.contains('\u{1b}') && !outcome.stderr.contains('\u{7}'));
        assert!(
            outcome
                .stderr
                .starts_with(r"tilewright-editor: output.write: the image could not be written to out\u{1b}[31m.png: "),
            "{}",
            outcome.stderr
        );
        assert!(
            outcome
                .stderr
                .ends_with("; the file this command made may be left there, partly written\n")
        );
        assert_eq!(outcome.stdout.contains("\"output.write\""), json);
        assert_eq!(outcome.stdout.contains("partly written"), json);
        assert!(!outcome.stdout.contains("left no file") && !outcome.stdout.contains("full"));
    }
}
