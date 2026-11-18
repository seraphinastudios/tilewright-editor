// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Raise and lower (`Action::AdjustHeights`): each covered cell's height
//! plus a flat amount, kept within the catalog's `editing.heights` range,
//! as one reviewed `terrain.set` on the `height` grid. The fixture is a
//! temporary copy of the neutral sample with a grant and generated heights;
//! the committed sample has neither.
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session, sculpt};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";
const MIN: i32 = -6;
const MAX: i32 = 6;

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
fn grant(root: &Path, heights: &Value) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["heights"] = heights.clone();
    });
}
/// The generated height of plane cell `[x, y]` of chunk `0_0`: from -6 to 6.
fn generated([x, y]: [i64; 2]) -> i32 {
    i32::try_from((x * 5 + y * 3) % 13).unwrap() - 6
}
/// Write `height` into every cell of chunk `0_0`.
fn set_heights(root: &Path, height: impl Fn([i64; 2]) -> i32) {
    let rows: Vec<Vec<i32>> = (0..32)
        .map(|y| (0..32).map(|x| height([x, y])).collect())
        .collect();
    edit_json(root, CHUNK, |chunk| {
        chunk["terrain"]["height"] = json!({ "rows": rows });
    });
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
        Actor::new("agent:heights").unwrap(),
    )
}
/// The sample with the range -6 to 6 and generated heights, then
/// `configure`.
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    grant(dir.path(), &json!({"min": MIN, "max": MAX}));
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
fn adjust(session: &Session, cells: Vec<[i64; 2]>, amount: i32) -> Action {
    Action::AdjustHeights {
        plane: "main".into(),
        layer: 0,
        cells,
        amount,
        expected: revision(session),
    }
}
fn view_cells() -> Vec<[i64; 2]> {
    // The fixture's view is [0, 0, 8, 6]; stroke order is x, then y.
    (0..8).flat_map(|x| (0..6).map(move |y| [x, y])).collect()
}
/// The cells of the pending `terrain.set` on `height`: `at` to (value,
/// expect).
fn pending_cells(session: &Session) -> BTreeMap<[i64; 2], (i64, i64)> {
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let operations = sealed["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["op"], "terrain.set");
    assert_eq!(operations[0]["grid"], "height");
    operations[0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let at = [
                cell["at"][0].as_i64().unwrap(),
                cell["at"][1].as_i64().unwrap(),
            ];
            (
                at,
                (
                    cell["value"].as_i64().unwrap(),
                    cell["expect"].as_i64().unwrap(),
                ),
            )
        })
        .collect()
}
/// The committed height of plane cell `at` of chunk `0_0`.
fn height(session: &Session, [x, y]: [i64; 2]) -> i32 {
    let chunk = &session.world().planes()["main"].chunks[&(0, 0, 0)];
    chunk.heights[usize::try_from(y * 32 + x).unwrap()]
}
/// What the rule says, written independently: add, then keep in range.
fn expected_changes(
    cells: &[[i64; 2]],
    amount: i32,
    before: impl Fn([i64; 2]) -> i32,
) -> BTreeMap<[i64; 2], (i64, i64)> {
    cells
        .iter()
        .filter_map(|&at| {
            let old = before(at);
            // The median of the ends and the sum, written without
            // `clamp`, so the test does not share the engine's arithmetic.
            let mut three = [MIN, old + amount, MAX];
            three.sort_unstable();
            let new = three[1];
            (new != old).then_some((at, (i64::from(new), i64::from(old))))
        })
        .collect()
}

#[test]
fn raising_and_lowering_add_the_amount_once_and_stop_at_the_range_ends() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    for amount in [4, -4, 1, -1, 12, -12] {
        let expected = expected_changes(&view_cells(), amount, generated);
        // Some cells are already at the end, so they are left out.
        assert!(expected.len() < view_cells().len(), "{amount}");
        session
            .dispatch(adjust(&session, view_cells(), amount), &mut records)
            .unwrap();
        assert_eq!(pending_cells(&session), expected, "{amount}");
        // The words' count is the engine's own plan.
        let plan = sculpt::plan(
            session.history().document().world(),
            session.catalog(),
            "main",
            0,
            &view_cells(),
            amount,
        )
        .unwrap();
        assert_eq!(plan.len(), expected.len());
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
    assert!(records.0.is_empty());
}

#[test]
fn the_same_action_prepares_the_same_bytes() {
    let (dir, mut session) = fixture_with(|_| {});
    let mut other = open(dir.path());
    let mut records = Records::default();
    let action = adjust(&session, view_cells(), 3);
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
fn an_accepted_adjustment_holds_its_heights_and_undoes_in_one_step() {
    let (dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    session
        .dispatch(adjust(&session, view_cells(), 5), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    for at in view_cells() {
        assert_eq!(height(&session, at), (generated(at) + 5).min(MAX), "{at:?}");
    }
    // Cells outside the stroke keep their heights.
    assert_eq!(height(&session, [8, 0]), generated([8, 0]));
    let accepted = fs::read(dir.path().join(CHUNK)).unwrap();
    let after = revision(&session);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(revision(&session), after);
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), accepted);
}

#[cfg(target_os = "linux")]
#[test]
fn a_chunk_without_a_height_grid_gets_one_and_loses_it_back_at_zero() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    grant(dir.path(), &json!({"min": -8, "max": 8}));
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    let mut session = open(dir.path());
    let mut records = Records::default();
    // No grid: every height is 0, so every cell expects 0.
    session
        .dispatch(adjust(&session, vec![[0, 0], [3, 2]], 3), &mut records)
        .unwrap();
    assert_eq!(
        pending_cells(&session),
        BTreeMap::from([([0, 0], (3, 0)), ([3, 2], (3, 0))])
    );
    session.dispatch(Action::Accept, &mut records).unwrap();
    let chunk: Value = serde_json::from_slice(&fs::read(dir.path().join(CHUNK)).unwrap()).unwrap();
    assert!(chunk["terrain"].get("height").is_some());
    assert_eq!(height(&session, [3, 2]), 3);
    // Back to 0 everywhere: World removes the grid again.
    session
        .dispatch(adjust(&session, vec![[0, 0], [3, 2]], -3), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    let chunk: Value = serde_json::from_slice(&fs::read(dir.path().join(CHUNK)).unwrap()).unwrap();
    assert!(chunk["terrain"].get("height").is_none());
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
}

#[test]
fn cells_of_chunks_that_do_not_exist_are_left_out_and_never_created() {
    // A view over the corner where chunks -1_0 and 0_0 exist and -1_1 and
    // 0_1 do not.
    let (dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([-2, 28, 8, 6]);
        });
    });
    let mut records = Records::default();
    let layers = dir.path().join("planes/main/layers/0");
    let files = fs::read_dir(&layers).unwrap().count();
    let cells: Vec<[i64; 2]> = (-2..6)
        .flat_map(|x| (28..34).map(move |y| [x, y]))
        .collect();
    session
        .dispatch(adjust(&session, cells.clone(), 2), &mut records)
        .unwrap();
    let pending = pending_cells(&session);
    assert!(!pending.is_empty());
    assert!(pending.keys().all(|&[_, y]| y < 32), "{pending:?}");
    // Chunk -1_0 has no height grid: its cells expect 0.
    assert_eq!(pending[&[-1, 30]], (2, 0));
    assert_eq!(
        pending[&[2, 30]],
        (
            i64::from(generated([2, 30]) + 2).min(6),
            i64::from(generated([2, 30]))
        )
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Only cells of chunks that do not exist: nothing to change.
    let missing: Vec<[i64; 2]> = cells.into_iter().filter(|&[_, y]| y >= 32).collect();
    let error = session
        .dispatch(adjust(&session, missing, 2), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ChangeNone);
    assert!(session.history().pending().is_none());
    assert_eq!(fs::read_dir(&layers).unwrap().count(), files);
}

#[test]
fn cells_outside_the_range_are_left_out_moved_in_by_the_amount_or_refused() {
    // Range -6 to 6; (2, 2) far above it, (3, 3) far below, (4, 4) just
    // above, (5, 5) just below.
    let outside = |at: [i64; 2]| match at {
        [2, 2] => 100,
        [3, 3] => -100,
        [4, 4] => 8,
        [5, 5] => -9,
        _ => generated(at),
    };
    let (_dir, mut session) = fixture_with(|root| set_heights(root, outside));
    let mut records = Records::default();
    let mut prepared = |cells: Vec<[i64; 2]>, amount: i32| {
        let result = session.dispatch(adjust(&session, cells, amount), &mut records);
        let found = result.map(|()| pending_cells(&session));
        if session.history().pending().is_some() {
            session.dispatch(Action::Cancel, &mut records).unwrap();
        }
        found
    };
    // At or beyond the end the amount moves toward: left out, like a cell
    // at that end.
    let raised = prepared(vec![[0, 1], [2, 2], [4, 4]], 1).unwrap();
    assert_eq!(raised.keys().collect::<Vec<_>>(), [&[0, 1]]);
    let lowered = prepared(vec![[0, 1], [3, 3], [5, 5]], -1).unwrap();
    assert_eq!(lowered.keys().collect::<Vec<_>>(), [&[0, 1]]);
    // Beyond the other end: moved by the amount, then kept in the range,
    // never further than the amount.
    assert_eq!(prepared(vec![[4, 4]], -2).unwrap()[&[4, 4]], (6, 8));
    assert_eq!(prepared(vec![[4, 4]], -9).unwrap()[&[4, 4]], (-1, 8));
    assert_eq!(prepared(vec![[4, 4]], -1024).unwrap()[&[4, 4]], (-6, 8));
    assert_eq!(prepared(vec![[5, 5]], 3).unwrap()[&[5, 5]], (-6, -9));
    assert_eq!(prepared(vec![[5, 5]], 1024).unwrap()[&[5, 5]], (6, -9));
    assert_eq!(prepared(vec![[2, 2]], -100).unwrap()[&[2, 2]], (0, 100));
    // Still outside after moving by the amount: the whole action is
    // refused, naming the cell, its height and the amount it needs.
    for (cells, amount, message) in [
        (
            vec![[0, 1], [4, 4]],
            -1,
            "cell (4, 4) has height 8, above the granted range -6 to 6: lowering it into the range needs an amount of at least 2",
        ),
        (
            vec![[2, 2]],
            -93,
            "cell (2, 2) has height 100, above the granted range -6 to 6: lowering it into the range needs an amount of at least 94",
        ),
        (
            vec![[3, 3], [5, 5]],
            2,
            "cell (3, 3) has height -100, below the granted range -6 to 6: raising it into the range needs an amount of at least 94",
        ),
        (
            vec![[5, 5]],
            2,
            "cell (5, 5) has height -9, below the granted range -6 to 6: raising it into the range needs an amount of at least 3",
        ),
    ] {
        let error = prepared(cells.clone(), amount).unwrap_err();
        assert_eq!(error.code(), RefusalCode::TargetUnsupported, "{cells:?}");
        assert_eq!(error.message(), message);
    }
    // The words' count is the plan's.
    let plan = sculpt::plan(
        session.history().document().world(),
        session.catalog(),
        "main",
        0,
        &[[0, 1], [2, 2], [4, 4], [5, 5]],
        3,
    )
    .unwrap();
    assert_eq!(plan, [([0, 1], -3, 0), ([5, 5], -9, -6)]);
}

#[test]
fn cells_far_outside_the_range_at_the_integer_extremes() {
    let (_dir, mut session) = fixture_with(|root| {
        set_heights(root, |at| match at {
            [0, 0] => i32::MAX,
            [1, 0] => i32::MIN,
            _ => 0,
        });
    });
    let mut records = Records::default();
    // Left out: beyond the end each amount moves toward.
    session
        .dispatch(adjust(&session, vec![[0, 0], [0, 1]], 1024), &mut records)
        .unwrap();
    assert_eq!(pending_cells(&session), BTreeMap::from([([0, 1], (6, 0))]));
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Too far to bring in: refused, with the exact amount needed.
    let error = session
        .dispatch(adjust(&session, vec![[1, 0]], 1024), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::TargetUnsupported);
    assert!(
        error.message().ends_with(
            "it is 2147483642 from the range, more than one adjustment can move (1024), so this tool cannot bring it in; leave it out of the stroke"
        ),
        "{error}"
    );
    let error = session
        .dispatch(adjust(&session, vec![[0, 0]], -1024), &mut records)
        .unwrap_err();
    assert!(
        error.message().ends_with("it is 2147483641 from the range, more than one adjustment can move (1024), so this tool cannot bring it in; leave it out of the stroke"),
        "{error}"
    );
}

#[test]
fn a_cell_more_than_one_adjustment_from_the_range_cannot_be_brought_in() {
    // Range -6 to 6: (0, 0) is exactly 1,024 above it, (1, 0) 1,025 above.
    let (_dir, mut session) = fixture_with(|root| {
        set_heights(root, |at| match at {
            [0, 0] => 1030,
            [1, 0] => 1031,
            _ => 0,
        });
    });
    let mut records = Records::default();
    let error = session
        .dispatch(adjust(&session, vec![[0, 0]], -1023), &mut records)
        .unwrap_err();
    assert_eq!(
        error.message(),
        "cell (0, 0) has height 1030, above the granted range -6 to 6: lowering it into the range needs an amount of at least 1024"
    );
    session
        .dispatch(adjust(&session, vec![[0, 0]], -1024), &mut records)
        .unwrap();
    assert_eq!(pending_cells(&session)[&[0, 0]], (6, 1030));
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let error = session
        .dispatch(adjust(&session, vec![[1, 0]], -1024), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::TargetUnsupported);
    assert_eq!(
        error.message(),
        "cell (1, 0) has height 1031, above the granted range -6 to 6: it is 1025 from the range, more than one adjustment can move (1024), so this tool cannot bring it in; leave it out of the stroke"
    );
    assert!(session.history().pending().is_none());
}

#[test]
fn the_hard_bound_ends_and_the_largest_amounts() {
    let (_dir, mut session) = fixture_with(|root| {
        grant(root, &json!({"min": -1024, "max": 1024}));
        set_heights(root, |at| match at {
            [0, 0] => 1024,
            [1, 0] => -1024,
            _ => 0,
        });
    });
    let mut records = Records::default();
    let cells = vec![[0, 0], [1, 0], [2, 0]];
    session
        .dispatch(adjust(&session, cells.clone(), 1024), &mut records)
        .unwrap();
    assert_eq!(
        pending_cells(&session),
        BTreeMap::from([([1, 0], (0, -1024)), ([2, 0], (1024, 0))])
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    session
        .dispatch(adjust(&session, cells, -1024), &mut records)
        .unwrap();
    assert_eq!(
        pending_cells(&session),
        BTreeMap::from([([0, 0], (0, 1024)), ([2, 0], (-1024, 0))])
    );
}

#[test]
fn nothing_to_change_says_why() {
    // A view over chunk 0_0 (heights at the top in row y = 30) and chunk
    // 0_1, which does not exist.
    let (_dir, mut session) = fixture_with(|root| {
        set_heights(root, |at| if at[1] == 30 { MAX } else { 0 });
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([0, 28, 8, 6]);
        });
    });
    let mut records = Records::default();
    let error = session
        .dispatch(
            adjust(
                &session,
                vec![[0, 30], [0, 32], [0, 33], [1, 30], [2, 30]],
                1,
            ),
            &mut records,
        )
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ChangeNone);
    assert_eq!(
        error.message(),
        "height adjustment changes no cell: 3 covered cells already at or above the top of the range, 2 in map chunks that do not exist (painting terrain there creates them)"
    );
}

#[test]
fn a_height_that_would_stack_past_32_bits_on_a_far_layer_is_refused() {
    // layer_height 3 on layer 715,827,882: base 2,147,483,646, so height 1
    // stacks to i32::MAX and height 2 past it.
    far_layer(715_827_882, 1, 2);
    // On layer -715,827,882 the base is -2,147,483,646: -2 stacks to
    // i32::MIN, -3 past it, and a raise is far from the edge.
    far_layer(-715_827_882, -2, -3);
    far_layer(-715_827_882, 8, -3);
}

/// On layer `far` (`layer_height` 3), raising or lowering a cell at 0 to
/// `fits` is prepared and to `past` is refused as stacking past 32 bits.
fn far_layer(far: i32, fits: i32, past: i32) {
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
        chunk["terrain"]["height"] = json!({"fill": 0});
        fs::write(
            directory.join("0_0.jsonc"),
            serde_json::to_vec_pretty(&chunk).unwrap(),
        )
        .unwrap();
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["layer"] = json!(far);
        });
    });
    // The fixture's range is -6 to 6: a raise by 8 stops at 6.
    let mut records = Records::default();
    let expected = revision(&session);
    let to = |height: i32| Action::AdjustHeights {
        plane: "main".into(),
        layer: far,
        cells: vec![[0, 0]],
        amount: height,
        expected,
    };
    session.dispatch(to(fits), &mut records).unwrap();
    session.dispatch(Action::Cancel, &mut records).unwrap();
    let error = session.dispatch(to(past), &mut records).unwrap_err();
    assert_eq!(error.code(), RefusalCode::TargetUnsupported);
    assert_eq!(
        error.message(),
        format!(
            "cell (0, 0): height {past} on layer {far} would stack past the 32-bit range the preview draws"
        )
    );
    assert!(session.history().pending().is_none());
}

#[test]
#[allow(clippy::too_many_lines)]
fn refusals_carry_their_codes_and_leave_nothing_pending() {
    let (_dir, mut session) = fixture_with(|root| {
        // Row y = 0 at the top of the range.
        set_heights(root, |at| if at[1] == 0 { MAX } else { generated(at) });
    });
    let mut records = Records::default();
    let expected = revision(&session);
    let with = |plane: &str, layer: i32, cells: Vec<[i64; 2]>, amount: i32| Action::AdjustHeights {
        plane: plane.into(),
        layer,
        cells,
        amount,
        expected,
    };
    let outside: Vec<[i64; 2]> = (0..4096).map(|y| [100, y]).collect();
    let cases = [
        (
            with("main", 0, vec![[8, 0]], 1),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with("main", 0, vec![[0, 6]], 1),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with("main", 0, vec![[-1, 0]], 1),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with("main", 1, vec![[0, 0]], 1),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with("other", 0, vec![[0, 0]], 1),
            RefusalCode::ScopeOutsideView,
        ),
        // At the cap the cells are taken; past it they are not.
        (
            with("main", 0, outside.clone(), 1),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with("main", 0, (0..4097).map(|y| [100, y]).collect(), 1),
            RefusalCode::InputBudget,
        ),
        (with("main", 0, vec![], 1), RefusalCode::InputBudget),
        (
            with("main", 0, vec![[0, 0], [0, 0]], 1),
            RefusalCode::InputInvalid,
        ),
        (
            with("main", 0, vec![[1, 0], [0, 0]], 1),
            RefusalCode::InputInvalid,
        ),
        (with("main", 0, vec![[0, 1]], 0), RefusalCode::InputInvalid),
        (
            with("main", 0, vec![[0, 1]], 1025),
            RefusalCode::InputBudget,
        ),
        (
            with("main", 0, vec![[0, 1]], -1025),
            RefusalCode::InputBudget,
        ),
        (
            with("main", 0, vec![[0, 1]], i32::MIN),
            RefusalCode::InputBudget,
        ),
        // Already at the top: a raise changes nothing.
        (
            with("main", 0, vec![[0, 0], [1, 0], [7, 0]], 1),
            RefusalCode::ChangeNone,
        ),
        (
            Action::AdjustHeights {
                plane: "main".into(),
                layer: 0,
                cells: vec![[0, 1]],
                amount: 1,
                expected: [9; 32],
            },
            RefusalCode::RevisionStale,
        ),
    ];
    for (action, code) in cases {
        let name = format!("{action:?}");
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert_eq!(error.code(), code, "{}", &name[..name.len().min(200)]);
        assert!(session.history().pending().is_none());
    }
    // The largest amounts are taken.
    for amount in [1024, -1024] {
        session
            .dispatch(with("main", 0, vec![[0, 1]], amount), &mut records)
            .unwrap();
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
    // The same row lowers.
    session
        .dispatch(with("main", 0, vec![[0, 0], [1, 0]], -1), &mut records)
        .unwrap();
    assert_eq!(
        pending_cells(&session),
        BTreeMap::from([([0, 0], (5, 6)), ([1, 0], (5, 6))])
    );
    // A pending change fences an adjustment.
    let error = session
        .dispatch(with("main", 0, vec![[0, 1]], 1), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
}

#[test]
fn without_the_grant_heights_cannot_be_edited_anywhere() {
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]
                .as_object_mut()
                .unwrap()
                .remove("heights");
        });
    });
    let mut records = Records::default();
    // The grant is checked before the view, as for a paint.
    for cells in [vec![[0, 0]], vec![[100, 0]]] {
        let error = session
            .dispatch(adjust(&session, cells, 1), &mut records)
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::GrantMissing);
    }
    assert!(session.history().pending().is_none());
}
