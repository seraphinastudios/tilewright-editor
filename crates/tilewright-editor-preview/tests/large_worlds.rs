// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Large generated worlds (`docs/LARGE-WORLDS.md`): a check that the native
//! app's load budget opens a world past its old file cap, and a benchmark,
//! ignored by default, of the editor's world path at several sizes.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use tilewright_edit::PublishOptions;
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreparedPreview, PreviewLimits, ViewSpec, loader, prepare};
use tilewright_editor_testkit::{fixture_world, large_world_catalog, large_world_chunks};

/// The largest view the default preview limits allow: 4,096 cells.
const VIEW: [i64; 4] = [0, 0, 64, 64];

/// One floor tile of the sample's own chunk `[0, 0]`, grass to sand.
const PAINT: &str = r#"{"format":"tilewright.transaction/1","operations":[{"op":"terrain.set","plane":"main","layer":0,"grid":"floor","cells":[{"at":[2,2],"value":"sand","expect":"grass"}]}]}"#;

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The neutral sample grown to `side` × `side` chunks in a temporary
/// directory, with the view at [`VIEW`].
fn generated(side: u32, objects_per_chunk: u32) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("world");
    copy_tree(&fixture_world("prototype-neutral"), &root);
    for (path, text) in large_world_chunks(side, objects_per_chunk) {
        fs::write(root.join(path), text).unwrap();
    }
    let catalog = fs::read_to_string(root.join("preview.jsonc")).unwrap();
    fs::write(
        root.join("preview.jsonc"),
        large_world_catalog(&catalog, VIEW),
    )
    .unwrap();
    (directory, root)
}

fn files(root: &Path) -> usize {
    let mut count = 0;
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            count += files(&entry.path());
        } else {
            count += 1;
        }
    }
    count
}

fn view(document: &WorldDocument, width: u32, height: u32) -> (ViewSpec, PreparedPreview) {
    let limits = PreviewLimits::default();
    let (catalog, resources) =
        loader::load(document.source(), document.world(), limits).expect("catalog");
    let view = catalog.default_view(document.world(), width, height, 1);
    let prepared = prepare(document.world(), &catalog, &resources, &view, limits).expect("prepare");
    (view, prepared)
}

#[test]
fn the_generator_is_deterministic() {
    assert_eq!(large_world_chunks(3, 5), large_world_chunks(3, 5));
    let chunks = large_world_chunks(3, 5);
    // Every chunk but the sample's own [0, 0].
    assert_eq!(chunks.len(), 8);
    assert_eq!(chunks[0].0, "planes/main/layers/0/1_0.jsonc");
}

#[test]
fn the_native_app_opens_and_shows_a_world_past_its_old_file_cap() {
    // 16 × 16 chunks, one file each, and the sample's own files: more than
    // the 256 files the app's budget allowed before.
    let (_directory, root) = generated(16, 4);
    assert!(files(&root) > 256, "{}", files(&root));
    let document = WorldDocument::open_prototype(&root).unwrap();
    // The generated chunks and the sample's chunk [-1, 0].
    assert_eq!(document.world().planes()["main"].chunks.len(), 16 * 16 + 1);
    let (view, prepared) = view(&document, 1280, 800);
    assert_eq!(view.bounds, VIEW);
    assert!(!prepared.scene.quads().is_empty());
}

fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

/// Accepts every inverse: the benchmark's worlds are disposable.
struct Discard;
impl RecoveryStore for Discard {
    fn retain(&mut self, _inverse: &str) -> Result<(), String> {
        Ok(())
    }
}

fn environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// Times the editor's world path on generated worlds and prints one line per
/// size. `docs/LARGE-WORLDS.md` says how to run it and records its numbers.
///
/// - `TILEWRIGHT_BENCH_SIDES`: chunks per side, comma separated (default
///   `13,45,90`: about 170, 2,000 and 8,100 chunks);
/// - `TILEWRIGHT_BENCH_OBJECTS`: objects per chunk (default 32);
/// - `TILEWRIGHT_BENCH_GPU`: when set, also draws the prepared World scene
///   offscreen with the shared renderer and names its adapter;
/// - `TILEWRIGHT_BENCH_SIZE`: the camera's `WIDTHxHEIGHT` (default
///   `2048x2048`, which shows the whole view at zoom 1).
#[test]
#[ignore = "a benchmark: run it in release with --ignored (docs/LARGE-WORLDS.md)"]
#[allow(clippy::too_many_lines)]
fn large_world_benchmark() {
    let sides: Vec<u32> = environment("TILEWRIGHT_BENCH_SIDES")
        .unwrap_or_else(|| "13,45,90".into())
        .split(',')
        .map(|side| side.trim().parse().unwrap())
        .collect();
    let objects: u32 = environment("TILEWRIGHT_BENCH_OBJECTS").map_or(32, |n| n.parse().unwrap());
    let (width, height) = environment("TILEWRIGHT_BENCH_SIZE").map_or((2048, 2048), |size| {
        let (width, height) = size.split_once('x').unwrap();
        (width.parse().unwrap(), height.parse().unwrap())
    });
    let gpu = environment("TILEWRIGHT_BENCH_GPU").is_some();
    let mut renderer = gpu.then(|| {
        tilewright_render_wgpu::WgpuRenderer::new(tilewright_render_api::TargetDescriptor {
            kind: tilewright_render_api::TargetKind::Offscreen,
            width,
            height,
            backend: tilewright_render_api::BackendPreference::Automatic,
        })
        .expect("renderer")
    });
    if let Some(renderer) = &renderer {
        let adapter = renderer.adapter();
        println!(
            "adapter: {} ({}, {}), driver {}",
            adapter.name, adapter.backend, adapter.device_type, adapter.driver
        );
    }
    println!("camera {width}x{height}, view {VIEW:?}, {objects} objects per chunk");
    for side in sides {
        let (_directory, root) = generated(side, objects);
        let source_files = files(&root);
        let mut document = None;
        let open = median(
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    document = Some(WorldDocument::open_prototype(&root).expect("open"));
                    milliseconds(start)
                })
                .collect(),
        );
        let document = document.unwrap();
        let chunks = document.world().planes()["main"].chunks.len();
        let edit = median(
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    tilewright_editor_commands::preview(&document, "bench", PAINT)
                        .expect("edit preview");
                    milliseconds(start)
                })
                .collect(),
        );
        // Accept the edit, then undo it untimed, so each round starts from
        // the same tree.
        let mut history = History::new(document.clone());
        let options = PublishOptions {
            unchecked_access: true,
        };
        let commit = median(
            (0..3)
                .map(|_| {
                    let revision = history.document().revision();
                    history.prepare(revision, "bench", PAINT).expect("prepare");
                    let start = Instant::now();
                    history.commit(&mut Discard, &options).expect("commit");
                    let elapsed = milliseconds(start);
                    history.prepare_undo().expect("undo");
                    history.commit(&mut Discard, &options).expect("undo commit");
                    elapsed
                })
                .collect(),
        );
        let limits = PreviewLimits::default();
        let mut loaded = None;
        let catalog_load = median(
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    loaded = Some(
                        loader::load(document.source(), document.world(), limits).expect("catalog"),
                    );
                    milliseconds(start)
                })
                .collect(),
        );
        let (catalog, resources) = loaded.unwrap();
        let view = catalog.default_view(document.world(), width, height, 1);
        let mut prepared = None;
        let build = median(
            (0..9)
                .map(|_| {
                    let start = Instant::now();
                    prepared = Some(
                        prepare(document.world(), &catalog, &resources, &view, limits)
                            .expect("prepare"),
                    );
                    milliseconds(start)
                })
                .collect(),
        );
        let prepared = prepared.unwrap();
        print!(
            "chunks={chunks} files={source_files} open_ms={open:.1} edit_preview_ms={edit:.1} commit_ms={commit:.1} catalog_load_ms={catalog_load:.2} scene_build_ms={build:.2} quads={} cell_quads={}",
            prepared.scene.quads().len(),
            prepared.cell_scene.quads().len(),
        );
        if let Some(renderer) = &mut renderer {
            let request = tilewright_render_api::RenderRequest {
                scene: &prepared.scene,
                resources: prepared.resources.clone(),
            };
            let mut submitted = Vec::new();
            let mut finished = Vec::new();
            // Five warm-up frames upload the textures.
            for frame in 0..14 {
                let start = Instant::now();
                renderer.draw(&request).expect("draw");
                let draw = milliseconds(start);
                pollster::block_on(renderer.finish()).expect("finish");
                if frame >= 5 {
                    submitted.push(draw);
                    finished.push(milliseconds(start));
                }
            }
            print!(
                " draw_submit_ms={:.2} draw_finish_ms={:.2}",
                median(submitted),
                median(finished)
            );
        }
        println!();
    }
}
