// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Flatten (`Action::FlattenHeights`): every covered cell of an existing
//! chunk set to one recorded target height inside the catalog's
//! `editing.heights` range, as one reviewed `terrain.set` on the `height`
//! grid. The fixture is a temporary copy of the neutral sample with a grant
//! and generated heights; the committed sample has neither. The rule is
//! compared, exhaustively over small ranges, with a reference written from
//! ADR 0007's text.
use serde_json::{Value, json};
use std::{fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::sculpt::{self, HeightChange};
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";

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
/// The generated height of plane cell `[x, y]` of chunk `0_0`: from -12
/// to 12.
fn generated([x, y]: [i64; 2]) -> i32 {
    i32::try_from((x * 7 + y * 3) % 25).unwrap() - 12
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
        Actor::new("agent:flatten").unwrap(),
    )
}
/// The sample with the range -6 to 6 and generated heights, then
/// `configure`.
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
fn flatten(session: &Session, cells: Vec<[i64; 2]>, target: i32) -> Action {
    Action::FlattenHeights {
        plane: "main".into(),
        layer: 0,
        cells,
        target,
        expected: revision(session),
    }
}
fn plan(
    session: &Session,
    cells: &[[i64; 2]],
    target: i32,
) -> Result<Vec<HeightChange>, RefusalCode> {
    sculpt::flatten_plan(
        session.history().document().world(),
        session.catalog(),
        "main",
        0,
        cells,
        target,
    )
    .map_err(|error| error.code())
}
/// The cells of the pending `terrain.set` on `height`, in order: `at`,
/// value, expect.
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
/// The committed height of plane cell `at` of chunk `0_0`.
fn height(session: &Session, [x, y]: [i64; 2]) -> i32 {
    let chunk = &session.world().planes()["main"].chunks[&(0, 0, 0)];
    chunk.heights[usize::try_from(y * 32 + x).unwrap()]
}

/// ADR 0007's rule, written from its text and sharing no code with the
/// engine: a target past 1,024 either way is input.budget; a target the
/// range does not hold is grant.missing; otherwise every covered cell with
/// a chunk whose height is not the target becomes the target, in stroke
/// order, and none left is change.none.
fn reference(
    cells: &[[i64; 2]],
    target: i32,
    (min, max): (i32, i32),
    before: impl Fn([i64; 2]) -> Option<i32>,
) -> Result<Vec<HeightChange>, RefusalCode> {
    if !(-1024..=1024).contains(&target) {
        return Err(RefusalCode::InputBudget);
    }
    if !(min..=max).contains(&target) {
        return Err(RefusalCode::GrantMissing);
    }
    let mut changes = Vec::new();
    for &at in cells {
        if let Some(old) = before(at)
            && old != target
        {
            changes.push((at, old, target));
        }
    }
    if changes.is_empty() {
        Err(RefusalCode::ChangeNone)
    } else {
        Ok(changes)
    }
}

/// The view over chunk corners `-1_0` (no height grid: 0), `0_0`
/// (generated) and `-1_1`, `0_1` (no chunks).
const CORNER: [i64; 4] = [-2, 28, 8, 6];
fn corner_cells() -> Vec<[i64; 2]> {
    (-2..6)
        .flat_map(|x| (28..34).map(move |y| [x, y]))
        .collect()
}
fn corner_height(at: [i64; 2]) -> Option<i32> {
    match at {
        [_, y] if y >= 32 => None,
        [x, _] if x < 0 => Some(0),
        at => Some(generated(at)),
    }
}

#[test]
fn the_rule_matches_the_reference_for_every_small_range_and_target() {
    let cells = corner_cells();
    // Ranges inside, around and away from the generated -12 to 12, and
    // the hard bound; targets across and past each, and past 1,024.
    let ranges = [(-6, 6), (-1, 1), (0, 3), (-20, -15), (5, 40), (-1024, 1024)];
    let targets: Vec<i32> = (-22..=42).chain([-1025, -1024, 1024, 1025]).collect();
    let mut records = Records::default();
    let mut checked = 0;
    for (min, max) in ranges {
        let (_dir, mut session) = fixture_with(|root| {
            grant(root, min, max);
            edit_json(root, "preview.jsonc", |catalog| {
                catalog["view"]["bounds"] = json!(CORNER);
            });
        });
        // Every subset shape the reference must agree on: the whole view,
        // one row, and only cells without a chunk.
        let row: Vec<[i64; 2]> = cells.iter().copied().filter(|at| at[1] == 30).collect();
        let missing: Vec<[i64; 2]> = cells.iter().copied().filter(|at| at[1] >= 32).collect();
        for stroke in [&cells, &row, &missing] {
            for &target in &targets {
                let expected = reference(stroke, target, (min, max), corner_height);
                assert_eq!(
                    plan(&session, stroke, target),
                    expected,
                    "range {min} to {max}, target {target}"
                );
                // What dispatch prepares is the plan, cell for cell.
                if let Ok(changes) = &expected {
                    session
                        .dispatch(flatten(&session, stroke.clone(), target), &mut records)
                        .unwrap();
                    assert_eq!(&pending_cells(&session), changes);
                    session.dispatch(Action::Cancel, &mut records).unwrap();
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 6 * 3 * 69);
    assert!(records.0.is_empty());
}

#[test]
fn cells_outside_the_range_are_flattened_into_it_whatever_their_distance() {
    // Range -6 to 6; (0, 0) and (1, 0) at the integer extremes, (2, 0)
    // more than one adjustment (1,024) above, (3, 0) just below.
    let (_dir, mut session) = fixture_with(|root| {
        set_heights(root, |at| match at {
            [0, 0] => i32::MAX,
            [1, 0] => i32::MIN,
            [2, 0] => 1031,
            [3, 0] => -7,
            [4, 0] => 6,
            _ => 0,
        });
    });
    let mut records = Records::default();
    let cells = vec![[0, 0], [1, 0], [2, 0], [3, 0], [4, 0], [5, 0]];
    // A raise or lower cannot bring (2, 0) in; a flatten can.
    let error = session
        .dispatch(
            Action::AdjustHeights {
                plane: "main".into(),
                layer: 0,
                cells: vec![[2, 0]],
                amount: -1024,
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap_err();
    assert!(
        error
            .message()
            .ends_with("or flatten it to a height in the range"),
        "{error}"
    );
    session
        .dispatch(flatten(&session, cells.clone(), 6), &mut records)
        .unwrap();
    assert_eq!(
        pending_cells(&session),
        [
            ([0, 0], i32::MAX, 6),
            ([1, 0], i32::MIN, 6),
            ([2, 0], 1031, 6),
            ([3, 0], -7, 6),
            ([5, 0], 0, 6),
        ]
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    session
        .dispatch(flatten(&session, cells, -6), &mut records)
        .unwrap();
    assert_eq!(pending_cells(&session).len(), 6);
}

#[test]
fn the_same_action_prepares_the_same_bytes() {
    let (dir, mut session) = fixture_with(|_| {});
    let mut other = open(dir.path());
    let mut records = Records::default();
    let cells: Vec<[i64; 2]> = (0..8).flat_map(|x| (0..6).map(move |y| [x, y])).collect();
    let action = flatten(&session, cells, 2);
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
fn an_accepted_flatten_holds_its_target_and_undoes_in_one_step() {
    let (dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let original = fs::read(dir.path().join(CHUNK)).unwrap();
    let cells: Vec<[i64; 2]> = (0..8).flat_map(|x| (0..6).map(move |y| [x, y])).collect();
    session
        .dispatch(flatten(&session, cells.clone(), -4), &mut records)
        .unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    for &at in &cells {
        assert_eq!(height(&session, at), -4, "{at:?}");
    }
    assert_eq!(height(&session, [8, 0]), generated([8, 0]));
    let accepted = fs::read(dir.path().join(CHUNK)).unwrap();
    let after = revision(&session);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), original);
    for &at in &cells {
        assert_eq!(height(&session, at), generated(at), "{at:?}");
    }
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(revision(&session), after);
    assert_eq!(fs::read(dir.path().join(CHUNK)).unwrap(), accepted);
}

#[cfg(target_os = "linux")]
#[test]
fn a_flatten_to_zero_of_every_height_removes_the_grid_and_undo_restores_it() {
    // Heights only at (0, 0) and (3, 2): flattening both to 0 returns every
    // height of the chunk to 0, and World removes the grid.
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    grant(dir.path(), -8, 8);
    let plain: Value = serde_json::from_slice(&fs::read(dir.path().join(CHUNK)).unwrap()).unwrap();
    set_heights(dir.path(), |at| match at {
        [0, 0] => 5,
        [3, 2] => -2,
        _ => 0,
    });
    let mut session = open(dir.path());
    let mut records = Records::default();
    session
        .dispatch(
            flatten(&session, vec![[0, 0], [1, 1], [3, 2]], 0),
            &mut records,
        )
        .unwrap();
    assert_eq!(pending_cells(&session), [([0, 0], 5, 0), ([3, 2], -2, 0)]);
    session.dispatch(Action::Accept, &mut records).unwrap();
    let chunk: Value = serde_json::from_slice(&fs::read(dir.path().join(CHUNK)).unwrap()).unwrap();
    assert!(chunk["terrain"].get("height").is_none());
    // The chunk is the sample's own again, which had no grid.
    assert_eq!(chunk, plain);
    // Undo brings the grid back, exactly.
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(height(&session, [0, 0]), 5);
    assert_eq!(height(&session, [3, 2]), -2);
    let chunk: Value = serde_json::from_slice(&fs::read(dir.path().join(CHUNK)).unwrap()).unwrap();
    assert!(chunk["terrain"].get("height").is_some());
}

#[test]
fn nothing_to_change_says_why() {
    let (_dir, mut session) = fixture_with(|root| {
        set_heights(root, |at| if at[1] == 30 { 4 } else { 0 });
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([0, 28, 8, 6]);
        });
    });
    let mut records = Records::default();
    let error = session
        .dispatch(
            flatten(
                &session,
                vec![[0, 30], [0, 32], [0, 33], [1, 30], [2, 30]],
                4,
            ),
            &mut records,
        )
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::ChangeNone);
    assert_eq!(
        error.message(),
        "flatten changes no cell: 3 covered cells already at the target 4, 2 in map chunks that do not exist (painting terrain there creates them)"
    );
    assert!(session.history().pending().is_none());
}

#[test]
fn a_target_that_would_stack_past_32_bits_on_a_far_layer_is_refused() {
    // layer_height 3 on layer 715,827,882: base 2,147,483,646, so target 1
    // stacks to i32::MAX and 2 past it; mirrored below.
    far_layer(715_827_882, 1, 2);
    far_layer(-715_827_882, -2, -3);
}

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
    let mut records = Records::default();
    let expected = revision(&session);
    let to = |target: i32| Action::FlattenHeights {
        plane: "main".into(),
        layer: far,
        cells: vec![[0, 0]],
        target,
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
fn refusals_carry_their_codes_and_messages_and_leave_nothing_pending() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let expected = revision(&session);
    let with =
        |plane: &str, layer: i32, cells: Vec<[i64; 2]>, target: i32| Action::FlattenHeights {
            plane: plane.into(),
            layer,
            cells,
            target,
            expected,
        };
    let cases = [
        (
            with("main", 0, vec![[8, 0]], 1),
            RefusalCode::ScopeOutsideView,
            "height editing is limited to the active view",
        ),
        (
            with("main", 1, vec![[0, 0]], 1),
            RefusalCode::ScopeOutsideView,
            "height editing is limited to the active view",
        ),
        (
            with("other", 0, vec![[0, 0]], 1),
            RefusalCode::ScopeOutsideView,
            "height editing is limited to the active view",
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
        (
            with("main", 0, vec![[0, 0]], 1025),
            RefusalCode::InputBudget,
            "a flatten target is a height from -1024 to 1024",
        ),
        (
            with("main", 0, vec![[0, 0]], i32::MIN),
            RefusalCode::InputBudget,
            "a flatten target is a height from -1024 to 1024",
        ),
        // In the hard bound but not in the range: not granted, before the
        // view, naming the range.
        (
            with("main", 0, vec![[0, 0]], 7),
            RefusalCode::GrantMissing,
            "flatten target 7 is outside the granted height range -6 to 6",
        ),
        (
            with("main", 0, vec![[100, 0]], -7),
            RefusalCode::GrantMissing,
            "flatten target -7 is outside the granted height range -6 to 6",
        ),
        (
            Action::FlattenHeights {
                plane: "main".into(),
                layer: 0,
                cells: vec![[0, 1]],
                target: 1,
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
    // The ends of the range are targets.
    for target in [-6, 6] {
        session
            .dispatch(with("main", 0, vec![[0, 1]], target), &mut records)
            .unwrap();
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
    // A pending change fences a flatten.
    session
        .dispatch(with("main", 0, vec![[0, 1]], 6), &mut records)
        .unwrap();
    let error = session
        .dispatch(with("main", 0, vec![[0, 2]], 6), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
}

#[test]
fn without_the_grant_heights_cannot_be_flattened_anywhere() {
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
            .dispatch(flatten(&session, cells, 0), &mut records)
            .unwrap_err();
        assert_eq!(error.code(), RefusalCode::GrantMissing);
    }
    // The hard bound is checked first.
    let error = session
        .dispatch(flatten(&session, vec![[0, 0]], 2000), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::InputBudget);
    assert!(session.history().pending().is_none());
}
