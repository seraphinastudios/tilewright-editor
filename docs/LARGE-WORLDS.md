# Large worlds

How the editor's world path scales with world size, how to measure it, and
why the native app's load budget is what it is.

## The generator

`tilewright_editor_testkit::large_world_chunks(side, objects_per_chunk)` grows
a copy of the neutral sample (`fixture_world("prototype-neutral")`) to
`side` × `side` chunks of plane `main`, layer 0: 32 × 32 floor tiles of the
sample's `grass` and `sand` in a fixed pattern, and up to 32 of the sample's
standing objects (`stone`, `bush`, `crate`, `lamp`) per chunk on distinct
tiles. The sample's own chunks stay. `large_world_catalog` sets the
catalog's view. Both are pure and deterministic: the caller copies the
sample and writes the files. The art is the sample's MIT art; nothing else
enters the world.

## The benchmark

`crates/tilewright-editor-preview/tests/large_worlds.rs` holds a check that
runs with the workspace's tests (the app's budget opens and shows a
16 × 16-chunk world, past the old 256-file cap) and a benchmark that is
ignored by default. Run the benchmark in release:

```sh
cargo test --release -p tilewright-editor-preview --test large_worlds \
  -- --ignored --nocapture large_world_benchmark
```

For each size it generates a world in a temporary directory and prints:

| Field | What is timed |
|---|---|
| `open_ms` | `WorldDocument::open_prototype`, the app's open (median of 3) |
| `edit_preview_ms` | `tilewright_editor_commands::preview` of a one-tile paint: plan plus baseline reload (median of 3) |
| `commit_ms` | `History::commit` of that paint: reload, publish, reopen (median of 3; each is undone untimed) |
| `scene_build_ms` | catalog load and `prepare` of the catalog's 64 × 64 view, the World and cell-pick scenes (median of 9) |
| `quads`, `cell_quads` | the World scene's and the cell-pick scene's quads |
| `draw_submit_ms`, `draw_finish_ms` | with `TILEWRIGHT_BENCH_GPU`: the shared renderer's offscreen `draw` of the World scene, and `draw` plus `finish` (median of 9 after 5 warm-up frames; no UI or overlays) |

`TILEWRIGHT_BENCH_SIDES` (default `13,45,90`), `TILEWRIGHT_BENCH_OBJECTS`
(default 32) and `TILEWRIGHT_BENCH_SIZE` (default `2048x2048`, which shows the
whole view at zoom 1) change the sizes. `VK_ICD_FILENAMES` picks the Vulkan
driver; the benchmark prints the adapter it got.

## Measurements

2026-11-28, release build, renderer 0.7.2, World 0.2.2, 32 objects per chunk,
64 × 64 view, 2048 × 2048 target, Linux 7.0, Ryzen 9 9950X3D.

| Chunks | Files | Open | Edit preview | Commit | Scene build | Quads |
|---|---|---|---|---|---|---|
| 170 | 228 | 10 ms | 18 ms | 32 ms | 4.9 ms | 3,661 |
| 2,026 | 2,084 | 131 ms | 263 ms | 424 ms | 7.0 ms | 3,661 |
| 8,101 | 8,159 | 579 ms | 1,180 ms | 1,860 ms | 8.0 ms | 3,661 |

These are the RADV run; the other two drivers' runs are within about 20%
except the scene build on lavapipe (up to 10 ms). Draw of the same scene:

| Adapter | Backend, driver | Draw + submit | Draw + finish |
|---|---|---|---|
| NVIDIA GeForce RTX 3090 | Vulkan, NVIDIA 595.99.02 | 0.9-2.4 ms | 1.0-3.6 ms |
| AMD Ryzen 9 9950X3D iGPU (RADV RAPHAEL_MENDOCINO) | Vulkan, Mesa 26.0.8 RADV | 0.8 ms | 3.0 ms |
| llvmpipe (LLVM 21.1.8, 256 bits) | Vulkan, Mesa 26.0.8 lavapipe | 0.8-1.4 ms | 5.9-8.0 ms |

What this says:

- Open, edit preview and commit grow with the whole world, about 70 µs,
  145 µs and 230 µs a chunk: every one loads the whole tree, an edit preview
  twice and a commit three times. Accepting a one-tile paint at 8,100 chunks
  takes about 3 s. That is World's work (seraphinastudios/tilewright-world#48),
  not the editor's or the renderer's.
- Scene build and draw do not grow with the world: the view is at most
  4,096 cells. Scene build grows slowly (a scan of every chunk for objects).

## The native app's load budget

`tilewright_editor_document::prototype_load_options`, used by the native app
(`prototype`), the sandbox runner and lint's budget check, allows 16,384
files, 32,768 directory entries and 256 MiB (1 MiB, 65,536 values and 65,536
array items per document). One chunk is one file, so the old 256 files
refused worlds over about 200 chunks. The new budget:

- holds two planes at the preview's plane budget of 8,192 chunks, or one and
  its definitions;
- opens such a world in about 1.2 s (twice the 8,100-chunk row) and keeps
  the worst edit within a few seconds;
- reads at most 256 MiB, 16 KiB a file on average; the 8,100-chunk world is
  27 MB;
- is still a bound: past it, World refuses the load with `source.budget`,
  and `lint` reports `world.over_editor_budget`.

The provisioning helper (`tools/provision-prototype.py`) copies a world with
room for that budget plus the catalog's resources (64 MiB): 65,536 entries
and files, 512 MiB.

The preview's plane chunk budget (`PreviewLimits::maximum_chunks`, 8,192) is
unchanged: the scene build stays under 10 ms at 8,100 chunks, and a plane
past it is refused before any work.
