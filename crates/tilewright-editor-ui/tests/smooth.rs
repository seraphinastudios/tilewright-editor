// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Smooth (`Action::SmoothHeights`): every covered cell of an existing
//! chunk set to kernel 1's rounded weighted mean of the committed heights
//! around it, as one reviewed `terrain.set` on the `height` grid. The
//! fixture is a temporary copy of the neutral sample with a grant and
//! generated heights; the committed sample has neither. The rule is
//! compared, over random heights, ranges, views and strokes, with a
//! reference written from ADR 0008's text.
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::sculpt::{self, HeightChange, SMOOTH_KERNEL, SmoothCounts};
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";
const WEST: &str = "planes/main/layers/0/-1_0.jsonc";

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
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
fn grant(root: &Path, min: i32, max: i32) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = json!({"min": min, "max": max});
    });
}
fn view(root: &Path, bounds: [i64; 4]) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["view"]["bounds"] = json!(bounds);
    });
}
/// Write `height` of each plane cell into the chunk at `path`, whose first
/// column is plane x `origin`.
fn set_chunk(root: &Path, path: &str, origin: i64, height: impl Fn([i64; 2]) -> i32) {
    let rows: Vec<Vec<i32>> = (0..32)
        .map(|y| (0..32).map(|x| height([origin + x, y])).collect())
        .collect();
    edit_json(root, path, |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
}
fn set_heights(root: &Path, height: impl Fn([i64; 2]) -> i32 + Copy) {
    set_chunk(root, CHUNK, 0, height);
    set_chunk(root, WEST, -32, height);
}
/// A generated height from -12 to 12.
fn generated([x, y]: [i64; 2]) -> i32 {
    i32::try_from((x * 7 + y * 3).rem_euclid(25)).unwrap() - 12
}
fn open(root: &Path) -> Session {
    let document = WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    Session::new(
        History::new(document),
        catalog,
        resources,
        Actor::new("agent:smooth").unwrap(),
    )
}
/// The sample with the range -6 to 6 and generated heights in both
/// chunks, then `configure`.
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    grant(dir.path(), -6, 6);
    set_heights(dir.path(), generated);
    configure(dir.path());
    let session = open(dir.path());
    (dir, session)
}
#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}
fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}
fn smooth(session: &Session, cells: Vec<[i64; 2]>) -> Action {
    Action::SmoothHeights {
        plane: "main".into(),
        layer: 0,
        cells,
        kernel: SMOOTH_KERNEL,
        expected: revision(session),
    }
}
type Planned = Result<(Vec<HeightChange>, SmoothCounts), RefusalCode>;
fn plan(session: &Session, cells: &[[i64; 2]]) -> Planned {
    sculpt::smooth_plan(
        session.history().document().world(),
        session.catalog(),
        "main",
        0,
        cells,
        SMOOTH_KERNEL,
    )
    .map_err(|error| error.code())
}
/// The cells of the pending `terrain.set` on `height`, in order: `at`,
/// expect, value.
fn pending_cells(session: &Session) -> Vec<HeightChange> {
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let operations = sealed["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["op"], "terrain.set");
    assert_eq!(operations[0]["grid"], "height");
    let number = |value: &Value| i32::try_from(value.as_i64().unwrap()).unwrap();
    operations[0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let at = [
                cell["at"][0].as_i64().unwrap(),
                cell["at"][1].as_i64().unwrap(),
            ];
            (at, number(&cell["expect"]), number(&cell["value"]))
        })
        .collect()
}
/// The committed height of plane cell `[x, y]` of chunk `0_0` or `-1_0`.
fn height(session: &Session, [x, y]: [i64; 2]) -> i32 {
    let chunk = &session.world().planes()["main"].chunks[&(0, x.div_euclid(32), 0)];
    chunk.heights[usize::try_from(y * 32 + x.rem_euclid(32)).unwrap()]
}

/// ADR 0008's kernel 1, written from its text and sharing no code with
/// the engine. Per covered cell with a chunk: the sum `S` of weight ×
/// clamped height over the 5 × 5 cells centred on it (weight
/// `b[dx] × b[dy]` with `b` = 1, 4, 6, 4, 1; a cell other than the centre
/// read when inside `bounds` and with a chunk), and the weight `W` read;
/// the new height is the integer nearest `S / W`, an exact half going to
/// the one nearer the cell's clamped height. Cells whose new height is
/// their stored one are left out.
fn reference(
    cells: &[[i64; 2]],
    (min, max): (i32, i32),
    [bx, by, width, tall]: [i64; 4],
    stored: impl Fn([i64; 2]) -> Option<i32>,
) -> Planned {
    let inside = |[x, y]: [i64; 2]| x >= bx && x < bx + width && y >= by && y < by + tall;
    let mut changes = Vec::new();
    let mut counts = SmoothCounts::default();
    for &[x, y] in cells {
        let Some(own) = stored([x, y]) else {
            counts.missing += 1;
            continue;
        };
        let new = reference_cell(own, (min, max), |dx, dy| {
            let at = [x + dx, y + dy];
            if inside(at) { stored(at) } else { None }
        });
        if new == own {
            counts.even += 1;
        } else {
            changes.push(([x, y], own, new));
        }
    }
    if changes.is_empty() {
        Err(RefusalCode::ChangeNone)
    } else {
        Ok((changes, counts))
    }
}

/// The reference's new height for one cell of height `own` whose cell
/// `(dx, dy)` away reads `read(dx, dy)` (`None`: not read).
fn reference_cell(own: i32, (min, max): (i32, i32), read: impl Fn(i64, i64) -> Option<i32>) -> i32 {
    let clamp = |value: i32| i64::from(value.max(min).min(max));
    let binomial = |offset: i64| [1, 4, 6, 4, 1][usize::try_from(offset + 2).unwrap()];
    let (mut sum, mut weight) = (36 * clamp(own), 36);
    for dy in -2..=2_i64 {
        for dx in -2..=2_i64 {
            if (dx, dy) == (0, 0) {
                continue;
            }
            if let Some(value) = read(dx, dy) {
                let w = binomial(dx) * binomial(dy);
                sum += w * clamp(value);
                weight += w;
            }
        }
    }
    let low = sum.div_euclid(weight);
    // Below the midpoint of low and low + 1, or on it with the cell's own
    // height at or below low: low; otherwise low + 1.
    let new = match (2 * sum).cmp(&((2 * low + 1) * weight)) {
        std::cmp::Ordering::Less => low,
        std::cmp::Ordering::Equal if clamp(own) <= low => low,
        std::cmp::Ordering::Equal | std::cmp::Ordering::Greater => low + 1,
    };
    i32::try_from(new).unwrap()
}

#[test]
fn single_cells_match_the_reference_exhaustively_on_a_small_domain() {
    // Every own height -1 to 1 and every assignment of the 8 cells one
    // tile away from {not read, -1, 0, 1}, under the cells two tiles away
    // not read, all at 1, or alternately -2 and not read, and under the
    // ranges -1 to 1 and 0 to 1 (so -1 and -2 are clamped).
    let outer: [fn(i64, i64) -> Option<i32>; 3] = [
        |_, _| None,
        |_, _| Some(1),
        |dx, dy| ((dx + dy).rem_euclid(2) == 0).then_some(-2),
    ];
    let mut checked = 0_u64;
    for range in [(-1, 1), (0, 1)] {
        let grant = tilewright_editor_preview::HeightGrant {
            min: range.0,
            max: range.1,
        };
        for far in outer {
            for own in -1..=1 {
                for code in 0..4_u32.pow(8) {
                    let inner = |dx: i64, dy: i64| -> Option<i32> {
                        let index = [
                            (-1, -1),
                            (0, -1),
                            (1, -1),
                            (-1, 0),
                            (1, 0),
                            (-1, 1),
                            (0, 1),
                            (1, 1),
                        ]
                        .iter()
                        .position(|&offset| offset == (dx, dy))
                        .unwrap();
                        match code / 4_u32.pow(u32::try_from(index).unwrap()) % 4 {
                            0 => None,
                            digit => Some(i32::try_from(digit).unwrap() - 2),
                        }
                    };
                    let at = |dx: i64, dy: i64| {
                        if dx.abs() == 2 || dy.abs() == 2 {
                            far(dx, dy)
                        } else {
                            inner(dx, dy)
                        }
                    };
                    let engine = sculpt::smoothed(
                        own,
                        sculpt::SMOOTH_NEIGHBOURS.map(|([dx, dy], _)| at(dx, dy)),
                        grant,
                    );
                    assert_eq!(
                        engine,
                        reference_cell(own, range, at),
                        "{own} {code} {range:?}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 2 * 3 * 3 * 65_536);
}

#[test]
fn single_cells_match_the_reference_at_the_extremes() {
    // Random heights from small to the 32-bit extremes, random ranges up to
    // the whole 32-bit range, and random cells not read.
    let mut random = Random(0x0123_4567_89ab_cdef);
    let pick = |random: &mut Random| -> i32 {
        match random.below(6) {
            0 => i32::MAX,
            1 => i32::MIN,
            2 => i32::try_from(random.below(2001) - 1000).unwrap(),
            3 => i32::try_from(random.below(1 << 32) - (1 << 31)).unwrap(),
            _ => i32::try_from(random.below(7) - 3).unwrap(),
        }
    };
    for _ in 0..200_000 {
        let (a, b) = (pick(&mut random), pick(&mut random));
        let grant = tilewright_editor_preview::HeightGrant {
            min: a.min(b),
            max: a.max(b),
        };
        let own = pick(&mut random);
        let around: Vec<Option<i32>> = (0..24)
            .map(|_| (random.below(4) != 0).then(|| pick(&mut random)))
            .collect();
        let at = |dx: i64, dy: i64| {
            let index = usize::try_from((dy + 2) * 5 + dx + 2).unwrap();
            around[if index > 12 { index - 1 } else { index }]
        };
        let engine = sculpt::smoothed(
            own,
            sculpt::SMOOTH_NEIGHBOURS.map(|([dx, dy], _)| at(dx, dy)),
            grant,
        );
        assert_eq!(
            engine,
            reference_cell(own, (grant.min, grant.max), at),
            "{own} {around:?} {grant:?}"
        );
        assert!(grant.contains(engine));
    }
}

/// A small deterministic generator (xorshift), for heights and strokes.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, bound: u64) -> i64 {
        i64::try_from(self.next() % bound).unwrap()
    }
}

/// The view over chunk corners `-1_0`, `0_0` (heights) and `-1_1`, `0_1`
/// (no chunks).
const CORNER: [i64; 4] = [-3, 27, 7, 7];
fn corner_cells() -> Vec<[i64; 2]> {
    (-3..4)
        .flat_map(|x| (27..34).map(move |y| [x, y]))
        .collect()
}

#[test]
fn the_rule_matches_the_reference_over_random_heights_ranges_and_strokes() {
    let ranges = [(-6, 6), (-1, 1), (0, 3), (-20, -15), (5, 40), (-1024, 1024)];
    let mut random = Random(0x5eed_cafe_f00d);
    let mut records = Records::default();
    let mut checked = 0;
    let mut changed = 0;
    for seed in 0..6_u64 {
        // Heights: small ones (many ties), wide ones, and a few at the
        // 32-bit extremes, each seed differently.
        let table: Vec<i32> = (0..64 * 32)
            .map(|index| match (seed, index % 17) {
                (0, _) => index % 5 - 2,
                (_, 3) if seed > 3 => i32::MAX,
                (_, 5) if seed > 3 => i32::MIN,
                _ => {
                    let spread: i64 = [4, 9, 30, 2000, 60, 7][usize::try_from(seed).unwrap()];
                    i32::try_from(random.below((2 * spread + 1).unsigned_abs()) - spread).unwrap()
                }
            })
            .collect();
        let table = &table;
        let at = move |[x, y]: [i64; 2]| table[usize::try_from((x + 32) + 64 * y).unwrap()];
        let stored = |cell: [i64; 2]| (cell[1] < 32).then(|| at(cell));
        for (min, max) in ranges {
            let (_dir, mut session) = fixture_with(|root| {
                grant(root, min, max);
                set_heights(root, at);
                view(root, CORNER);
            });
            let cells = corner_cells();
            let mut strokes = vec![
                cells.clone(),
                cells.iter().copied().filter(|c| c[1] == 30).collect(),
                cells.iter().copied().filter(|c| c[1] >= 32).collect(),
                vec![[-3, 27]],
                vec![[3, 33]],
            ];
            for _ in 0..20 {
                let stroke: Vec<[i64; 2]> = cells
                    .iter()
                    .copied()
                    .filter(|_| random.below(3) == 0)
                    .collect();
                if !stroke.is_empty() {
                    strokes.push(stroke);
                }
            }
            for stroke in &strokes {
                let expected = reference(stroke, (min, max), CORNER, stored);
                assert_eq!(
                    plan(&session, stroke),
                    expected,
                    "seed {seed}, range {min} to {max}"
                );
                if let Ok((changes, _)) = &expected {
                    changed += changes.len();
                    session
                        .dispatch(smooth(&session, stroke.clone()), &mut records)
                        .unwrap();
                    assert_eq!(&pending_cells(&session), changes);
                    session.dispatch(Action::Cancel, &mut records).unwrap();
                }
                checked += 1;
            }
        }
    }
    assert!(checked >= 6 * 6 * 5, "{checked}");
    assert!(changed > 1000, "{changed}");
    assert!(records.0.is_empty());
}

#[test]
fn each_cell_reads_committed_heights_only_so_the_stroke_does_not_matter() {
    // A Jacobi pass: a cell's new height is the same whether it is
    // smoothed alone or with every other cell around it, and smoothing
    // twice is not the same as once.
    let (_dir, session) = fixture_with(|root| view(root, [0, 0, 8, 8]));
    let cells: Vec<[i64; 2]> = (0..8).flat_map(|x| (0..8).map(move |y| [x, y])).collect();
    let (together, _) = plan(&session, &cells).unwrap();
    for &(at, before, after) in &together {
        let (alone, _) = plan(&session, &[at]).unwrap();
        assert_eq!(alone, [(at, before, after)]);
    }
    // Cells left out alone are left out together.
    let changed: Vec<[i64; 2]> = together.iter().map(|change| change.0).collect();
    for &at in &cells {
        if !changed.contains(&at) {
            assert_eq!(plan(&session, &[at]), Err(RefusalCode::ChangeNone));
        }
    }
}

#[test]
fn a_hand_computed_patch() {
    // Range -8 to 8, view 0..7 × 0..7 over chunk 0_0; heights 0 but for
    // a spike of 8 at (1, 1) and a pit of -8 at (4, 4).
    let (_dir, session) = fixture_with(|root| {
        grant(root, -8, 8);
        set_heights(root, |at| match at {
            [1, 1] => 8,
            [4, 4] => -8,
            _ => 0,
        });
        view(root, [0, 0, 7, 7]);
    });
    let cells: Vec<[i64; 2]> = (0..7).flat_map(|x| (0..7).map(move |y| [x, y])).collect();
    let (changes, counts) = plan(&session, &cells).unwrap();
    // The pit, all 25 read: (36 × -8) / 256 = -1.125, so -1. Its edge
    // neighbours: (24 × -8) / 256 = -0.75, so -1; its diagonal ones,
    // (16 × -8) / 256 = -0.5 exactly, stay at 0 (a half goes toward the
    // cell's own height); two away, (6 × -8) / 256, stay too.
    // The spike near the view's corner reads fewer cells: (1, 1) itself
    // loses row and column -1, weight 256 - 16 - 16 + 1 = 225: (-189 × 8)
    // / 225 = -6.72 below it, so 8 - 7 = 1. Its diagonal (0, 0) reads
    // weight 121: (16 × 8) / 121 = 1.06, so 1, where (2, 2), with the pit
    // two away, reads (16 × 8 - 1 × 8) / 256 = 0.47 and stays. The pit's
    // diagonal (5, 5) loses column 7, weight 225: (16 × -8) / 225 = -0.57,
    // so -1; (3, 3), like (2, 2), stays.
    assert_eq!(
        changes,
        [
            ([0, 0], 0, 1),
            ([0, 1], 0, 1),
            ([0, 2], 0, 1),
            ([1, 0], 0, 1),
            ([1, 1], 8, 1),
            ([1, 2], 0, 1),
            ([2, 0], 0, 1),
            ([2, 1], 0, 1),
            ([3, 4], 0, -1),
            ([3, 5], 0, -1),
            ([4, 3], 0, -1),
            ([4, 4], -8, -1),
            ([4, 5], 0, -1),
            ([5, 3], 0, -1),
            ([5, 4], 0, -1),
            ([5, 5], 0, -1),
        ]
    );
    assert_eq!(
        counts,
        SmoothCounts {
            even: 33,
            missing: 0
        }
    );
}

#[test]
fn cells_outside_the_view_and_in_missing_chunks_are_left_out() {
    // (0, 31) at 0; west of it, x -1 and -2, at 12 in chunk -1_0; south
    // of it, rows 32 and 33, no chunk; range wide enough to read 12.
    let heights = |at: [i64; 2]| if at[0] < 0 { 12 } else { 0 };
    let in_view = |bounds| {
        let (_dir, session) = fixture_with(|root| {
            grant(root, -20, 20);
            set_heights(root, heights);
            view(root, bounds);
        });
        plan(&session, &[[0, 31]])
    };
    // The view starts at x 0: the 12s are not read, and the missing chunk
    // below is not read as 0 either, so the cell is even.
    assert_eq!(in_view([0, 28, 4, 6]), Err(RefusalCode::ChangeNone));
    // The view takes in x -1, one tile away: rows 29 to 31 of that column
    // weigh 4 × (1 + 4 + 6) = 44, so 528 over the weight read, 165 (rows
    // 32 and 33 have no chunk): 3.2, so 3.
    assert_eq!(
        in_view([-1, 28, 4, 6]),
        Ok((vec![([0, 31], 0, 3)], SmoothCounts::default()))
    );
    // It takes in x -2 as well, two tiles away: 1 × 11 more, 660 / 176 =
    // 3.75, so 4. Counted as 0, the missing rows would give 660 / 256,
    // rounded to 3.
    assert_eq!(
        in_view([-2, 28, 5, 6]),
        Ok((vec![([0, 31], 0, 4)], SmoothCounts::default()))
    );
}

#[test]
fn every_height_read_is_clamped_and_cells_outside_the_range_come_in() {
    let (_dir, session) = fixture_with(|root| {
        set_heights(root, |at| match at {
            [0, 0] => i32::MAX,
            [1, 0] => i32::MIN,
            [5, 3] => 1000,
            [6, 3] => -30,
            _ => 0,
        });
        view(root, [0, 0, 8, 6]);
    });
    let (changes, counts) = plan(&session, &[[0, 0], [1, 0], [4, 3], [5, 3], [6, 3]]).unwrap();
    // Range -6 to 6. (0, 0) reads as 6, (1, 0) beside it as -6: (0, 0)
    // gives 6 - 654 / 121 = 0.6, so 1; (1, 0) gives -6 + 918 / 165 =
    // -0.44, so 0. (5, 3) and (6, 3) come down to 0 among zeros. (4, 3)
    // reads 1000 as 6 and -30 as -6: 108 / 256, so it stays at 0, where
    // reading 1000 as stored would raise it by 90.
    assert_eq!(
        changes,
        [
            ([0, 0], i32::MAX, 1),
            ([1, 0], i32::MIN, 0),
            ([5, 3], 1000, 0),
            ([6, 3], -30, 0),
        ]
    );
    assert_eq!(counts.even, 1);
}

#[test]
fn nothing_to_change_says_why() {
    let (_dir, mut session) = fixture_with(|root| {
        set_heights(root, |_| 2);
        view(root, [0, 28, 8, 6]);
    });
    let mut records = Records::default();
    let error = session
        .dispatch(
            smooth(&session, vec![[0, 30], [0, 32], [0, 33], [1, 30], [2, 31]]),
            &mut records,
        )
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ChangeNone);
    assert_eq!(
        error.message(),
        "smooth changes no cell: 3 covered cells already even, 2 in map chunks that do not exist (painting terrain there creates them)"
    );
    assert!(session.history().pending().is_none());
}

#[test]
fn refusals_carry_their_codes_and_messages_and_leave_nothing_pending() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let expected = revision(&session);
    let with = |plane: &str, layer: i32, cells: Vec<[i64; 2]>, kernel: u32| Action::SmoothHeights {
        plane: plane.into(),
        layer,
        cells,
        kernel,
        expected,
    };
    let view_only = "height editing is limited to the active view";
    let cases = [
        (
            with("main", 0, vec![[8, 0]], 1),
            RefusalCode::ScopeOutsideView,
            view_only,
        ),
        (
            with("main", 1, vec![[0, 0]], 1),
            RefusalCode::ScopeOutsideView,
            view_only,
        ),
        (
            with("other", 0, vec![[0, 0]], 1),
            RefusalCode::ScopeOutsideView,
            view_only,
        ),
        (with("main", 0, vec![], 1), RefusalCode::InputBudget, ""),
        (
            with("main", 0, (0..4097).map(|y| [100, y]).collect(), 1),
            RefusalCode::InputBudget,
            "",
        ),
        (
            with("main", 0, vec![[1, 0], [0, 0]], 1),
            RefusalCode::InputInvalid,
            "",
        ),
        (
            with("main", 0, vec![[0, 0], [0, 0]], 1),
            RefusalCode::InputInvalid,
            "",
        ),
        // An unknown kernel, before the grant and the view.
        (
            with("main", 0, vec![[100, 0]], 2),
            RefusalCode::InputInvalid,
            "unknown smooth kernel 2; this editor knows kernel 1",
        ),
        (
            with("main", 0, vec![[0, 0]], 0),
            RefusalCode::InputInvalid,
            "unknown smooth kernel 0; this editor knows kernel 1",
        ),
        (
            Action::SmoothHeights {
                plane: "main".into(),
                layer: 0,
                cells: vec![[0, 1]],
                kernel: 1,
                expected: [9; 32],
            },
            RefusalCode::RevisionStale,
            "",
        ),
    ];
    for (action, code, message) in cases {
        let name = format!("{action:?}");
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert_eq!(error.code(), code, "{}", &name[..name.len().min(200)]);
        if !message.is_empty() {
            assert_eq!(error.message(), message);
        }
        assert!(session.history().pending().is_none());
    }
    // A pending change fences a smooth.
    session
        .dispatch(with("main", 0, vec![[0, 1]], 1), &mut records)
        .unwrap();
    let error = session
        .dispatch(with("main", 0, vec![[0, 2]], 1), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
}

#[test]
fn without_the_grant_heights_cannot_be_smoothed_anywhere() {
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]
                .as_object_mut()
                .unwrap()
                .remove("heights");
        });
    });
    let mut records = Records::default();
    for cells in [vec![[0, 0]], vec![[100, 0]]] {
        let error = session
            .dispatch(smooth(&session, cells), &mut records)
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::GrantMissing);
    }
    assert!(session.history().pending().is_none());
}

#[test]
fn a_height_that_would_stack_past_32_bits_on_a_far_layer_is_refused() {
    // layer_height 3 on layer 715,827,882: base 2,147,483,646, so a new
    // height 1 stacks to i32::MAX and 2 past it.
    let far = 715_827_882;
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, "profile/profile.jsonc", |profile| {
            profile["layer_height"] = json!(3);
        });
        let directory = root.join(format!("planes/main/layers/{far}"));
        fs::create_dir_all(&directory).unwrap();
        let mut chunk: Value =
            serde_json::from_slice(&fs::read(root.join(CHUNK)).unwrap()).unwrap();
        chunk["layer"] = json!(far);
        chunk["objects"] = json!([]);
        // A 4 at (1, 0) beside zeros: (0, 0) becomes (24 × 4) / 121 =
        // 0.79, so 1. Rows 3 to 5, columns 3 to 7 at 8.
        let mut rows = vec![vec![0; 32]; 32];
        rows[0][1] = 4;
        for row in &mut rows[3..6] {
            row[3..8].fill(8);
        }
        chunk["terrain"]["height"] = json!({ "rows": rows });
        fs::write(
            directory.join("0_0.jsonc"),
            serde_json::to_vec_pretty(&chunk).unwrap(),
        )
        .unwrap();
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["layer"] = json!(far);
        });
    });
    let mut records = Records::default();
    let expected = revision(&session);
    let on = |cells: Vec<[i64; 2]>| Action::SmoothHeights {
        plane: "main".into(),
        layer: far,
        cells,
        kernel: 1,
        expected,
    };
    session.dispatch(on(vec![[0, 0]]), &mut records).unwrap();
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // (5, 5) at 8 reads as 6, the range's end, and so does every cell it
    // reads (rows 6 and 7 are outside the view): it becomes 6.
    let error = session
        .dispatch(on(vec![[5, 5]]), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::TargetUnsupported);
    assert_eq!(
        error.message(),
        format!(
            "cell (5, 5): height 6 on layer {far} would stack past the 32-bit range the preview draws"
        )
    );
    assert!(session.history().pending().is_none());
}

#[test]
fn the_same_action_prepares_the_same_bytes() {
    let (dir, mut session) = fixture_with(|_| {});
    let mut other = open(dir.path());
    let mut records = Records::default();
    let cells: Vec<[i64; 2]> = (0..8).flat_map(|x| (0..6).map(move |y| [x, y])).collect();
    let action = smooth(&session, cells);
    session.dispatch(action.clone(), &mut records).unwrap();
    other.dispatch(action.clone(), &mut records).unwrap();
    let first = session.history().pending().unwrap().sealed().to_owned();
    assert_eq!(other.history().pending().unwrap().sealed(), first);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    session.dispatch(action, &mut records).unwrap();
    assert_eq!(session.history().pending().unwrap().sealed(), first);
}

// Publication is checked, which only Linux provides (see `session.rs`).
#[cfg(target_os = "linux")]
#[test]
fn an_accepted_smooth_undoes_and_redoes_exactly_and_smooths_further_when_repeated() {
    let (dir, mut session) = fixture_with(|root| view(root, [-2, 0, 8, 6]));
    let mut records = Records::default();
    let original = [
        fs::read(dir.path().join(CHUNK)).unwrap(),
        fs::read(dir.path().join(WEST)).unwrap(),
    ];
    let cells: Vec<[i64; 2]> = (-2..6).flat_map(|x| (0..6).map(move |y| [x, y])).collect();
    let (planned, _) = plan(&session, &cells).unwrap();
    session
        .dispatch(smooth(&session, cells.clone()), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    for &(at, _, after) in &planned {
        assert_eq!(height(&session, at), after, "{at:?}");
    }
    let accepted = fs::read(dir.path().join(CHUNK)).unwrap();
    let after = revision(&session);
    // Again: the ground is smoother, so fewer or smaller changes remain.
    let (again, _) = plan(&session, &cells).unwrap();
    let spread = |heights: &dyn Fn([i64; 2]) -> i32| {
        cells
            .iter()
            .map(|&[x, y]| (heights([x, y]) - heights([x + 1, y])).unsigned_abs())
            .sum::<u32>()
    };
    let now = spread(&|at| height(&session, at));
    assert!(now < spread(&generated), "{now}");
    assert!(!again.is_empty());
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original[0]);
    assert_eq!(fs::read(dir.path().join(WEST)).unwrap(), original[1]);
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(revision(&session), after);
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), accepted);
}

#[cfg(target_os = "linux")]
#[test]
fn a_smooth_to_all_zeros_removes_the_grid_and_undo_restores_it() {
    // One height of 1 at (3, 3): (36 × 1) / 256 = 0.14, so 0; (2, 2)
    // beside it reads (16 × 1) / 256 and stays: every height is 0 again.
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    grant(dir.path(), -8, 8);
    let plain: Value = serde_json::from_slice(&fs::read(dir.path().join(CHUNK)).unwrap()).unwrap();
    set_chunk(dir.path(), CHUNK, 0, |at| i32::from(at == [3, 3]));
    let mut session = open(dir.path());
    let mut records = Records::default();
    session
        .dispatch(smooth(&session, vec![[2, 2], [3, 3]]), &mut records)
        .unwrap();
    assert_eq!(pending_cells(&session), [([3, 3], 1, 0)]);
    session.dispatch(Action::Accept, &mut records).unwrap();
    let chunk: Value = serde_json::from_slice(&fs::read(dir.path().join(CHUNK)).unwrap()).unwrap();
    assert_eq!(chunk, plain);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(height(&session, [3, 3]), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn a_cell_read_changed_on_disk_after_review_refuses_the_accept() {
    // The smooth writes only chunk 0_0, and reads cells one and two tiles
    // west in chunk -1_0, which the transaction does not touch. A change
    // there between Review and Accept is caught by the whole-World check.
    // Column 0 reads x -1 and -2; column 1 reads only x -1, two away.
    for column in [0, 1] {
        let (dir, mut session) = fixture_with(|root| {
            grant(root, -64, 64);
            set_heights(root, |at| if at[0] < 0 { 40 } else { 0 });
            view(root, [-2, 0, 8, 6]);
        });
        let mut records = Records::default();
        let cells: Vec<[i64; 2]> = (0..6).map(|y| [column, y]).collect();
        session
            .dispatch(smooth(&session, cells), &mut records)
            .unwrap();
        let pending = pending_cells(&session);
        assert_eq!(pending.len(), 6, "{column}");
        assert!(
            pending
                .iter()
                .all(|(at, _, after)| at[0] == column && *after > 0)
        );
        let west = fs::read(dir.path().join(WEST)).unwrap();
        set_chunk(
            dir.path(),
            WEST,
            -32,
            |at| if at[0] == -1 { -40 } else { 40 },
        );
        let chunk = fs::read(dir.path().join(CHUNK)).unwrap();
        let error = session.dispatch(Action::Accept, &mut records).unwrap_err();
        assert_eq!(error.code(), RefusalCode::SourceChanged, "{error}");
        // Nothing was written.
        assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), chunk);
        assert_ne!(fs::read(dir.path().join(WEST)).unwrap(), west);
        assert!(records.0.is_empty());
    }
}
