// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Fill and replace (`Action::FillTerrain`): the region the engine works out
//! from the committed World, checked against an independent reference, and
//! the single reviewed `terrain.set` it becomes through the brush stroke's
//! planner.
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::review::{MAXIMUM_REVIEW_BYTES, MAXIMUM_REVIEW_FILES};
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session, fill};

const EAST: &str = "planes/main/layers/0/0_0.jsonc";
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
fn open(root: &Path, actor: &str) -> Session {
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
        Actor::new(actor).unwrap(),
    )
}
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    configure(dir.path());
    let session = open(dir.path(), "editor:prototype");
    (dir, session)
}
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
fn set_view(root: &Path, bounds: [i64; 4]) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["view"]["bounds"] = json!(bounds);
    });
}
/// Replace the first rows of a chunk's `floor` grid (`g` grass, `s` sand,
/// `.` empty), each padded with empty cells to the chunk's 32 columns.
fn set_rows(root: &Path, chunk: &str, rows: &[&str]) {
    edit_json(root, chunk, |chunk| {
        let grid = &mut chunk["terrain"]["floor"]["rows"];
        for (index, row) in rows.iter().enumerate() {
            grid[index] = json!(format!("{row:.<32}"));
        }
    });
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
fn fill_action(session: &Session, at: [i64; 2], proto: &str, contiguous: bool) -> Action {
    Action::FillTerrain {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x: at[0],
        y: at[1],
        proto: proto.into(),
        contiguous,
        expected: revision(session),
    }
}
/// The committed `floor` value of plane cell `[x, y]`.
fn value(session: &Session, [x, y]: [i64; 2]) -> Option<String> {
    let chunk = session.history().document().world().planes()["main"]
        .chunks
        .get(&(0, x.div_euclid(32), y.div_euclid(32)))?;
    let local = usize::try_from(y.rem_euclid(32) * 32 + x.rem_euclid(32)).unwrap();
    chunk.ground[0].value(local).map(str::to_owned)
}
fn view_cells(session: &Session) -> Vec<[i64; 2]> {
    let (_, _, [min_x, min_y, width, height]) = session.catalog().view();
    (min_y..min_y + height)
        .flat_map(|y| (min_x..min_x + width).map(move |x| [x, y]))
        .collect()
}
/// The reference region, by another method than the engine's: start from
/// `at` and add, pass after pass over the view in row order, every matching
/// cell next to one already in, until a pass adds nothing. Without
/// `contiguous`, every matching cell of the view.
fn reference(session: &Session, at: [i64; 2], contiguous: bool) -> BTreeSet<[i64; 2]> {
    let start = value(session, at);
    let matching: Vec<[i64; 2]> = view_cells(session)
        .into_iter()
        .filter(|&cell| value(session, cell) == start)
        .collect();
    if !contiguous {
        return matching.into_iter().collect();
    }
    let mut region = BTreeSet::from([at]);
    loop {
        let before = region.len();
        for &[x, y] in &matching {
            let touching = [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]]
                .iter()
                .any(|next| region.contains(next));
            if touching {
                region.insert([x, y]);
            }
        }
        if region.len() == before {
            return region;
        }
    }
}
/// The prepared transaction, parsed.
fn sealed(session: &Session) -> Value {
    serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap()
}
/// The `at` of every cell of the prepared single `terrain.set`.
fn prepared_cells(session: &Session) -> Vec<[i64; 2]> {
    let sealed = sealed(session);
    let operations = sealed["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1, "{sealed}");
    operations[0]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let at = cell["at"].as_array().unwrap();
            [at[0].as_i64().unwrap(), at[1].as_i64().unwrap()]
        })
        .collect()
}
/// A small deterministic generator, so the property cases are the same on
/// every run.
struct Random(u64);
impl Random {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % u64::try_from(bound).unwrap()).unwrap()
    }
}

#[test]
fn edge_neighbours_connect_a_fill_and_diagonals_do_not() {
    let (_dir, session) = fixture_with(|root| {
        set_rows(
            root,
            EAST,
            &[
                "sgggggss", "gsggggsg", "ggsggggs", "gggggggg", "ggggsggg", "gggsgsgg",
            ],
        );
    });
    let world = session.history().document().world();
    let region = |at, contiguous| {
        fill::region(world, session.catalog(), "main", 0, "floor", at, contiguous).unwrap()
    };
    // The sand diagonal from the corner: each cell alone.
    for at in [[0, 0], [1, 1], [2, 2]] {
        let found = region(at, true);
        assert_eq!(found.value, Some("sand"));
        assert_eq!(found.cells, [at], "{at:?}");
    }
    // Three sand cells joined by edges, and a sand cross arm that only
    // touches (4, 4) at corners.
    assert_eq!(region([6, 0], true).cells, [[6, 0], [6, 1], [7, 0]]);
    assert_eq!(region([4, 4], true).cells, [[4, 4]]);
    // Replace takes every sand cell of the view, joined or not.
    let all = region([4, 4], false);
    assert_eq!(all.cells.len(), 10);
    assert!(all.cells.contains(&[0, 0]) && all.cells.contains(&[5, 5]));
    // The grass around them is one region: it flows between the diagonals,
    // but not into the two grass cells that sand and the view's edge box in.
    let grass = region([1, 0], true);
    assert_eq!(grass.value, Some("grass"));
    assert_eq!(grass.cells.len(), 48 - 10 - 2);
    for boxed in [[7, 1], [4, 5]] {
        assert!(!grass.cells.contains(&boxed), "{boxed:?}");
        assert_eq!(region(boxed, true).cells, [boxed]);
    }
}

#[test]
fn a_fill_never_leaves_the_view_even_to_come_back() {
    // Two grass columns joined only below the view (row 6).
    let (_dir, session) = fixture_with(|root| {
        set_rows(
            root,
            EAST,
            &[
                "gsgsssss", "gsgsssss", "gsgsssss", "gsgsssss", "gsgsssss", "gsgsssss", "ggg",
            ],
        );
    });
    let world = session.history().document().world();
    let found = fill::region(world, session.catalog(), "main", 0, "floor", [0, 0], true).unwrap();
    assert_eq!(found.cells, (0..6).map(|y| [0, y]).collect::<Vec<_>>());
    // The cells just outside the view hold grass too and are never covered.
    assert_eq!(value(&session, [1, 6]).as_deref(), Some("grass"));
    let replaced =
        fill::region(world, session.catalog(), "main", 0, "floor", [0, 0], false).unwrap();
    assert_eq!(replaced.cells.len(), 12);
    assert!(replaced.cells.iter().all(|&[_, y]| (0..6).contains(&y)));
}

#[test]
fn a_view_clipped_fill_covers_only_its_bounds() {
    let (_dir, session) = fixture_with(|root| set_view(root, [1, 1, 3, 2]));
    let world = session.history().document().world();
    let found = fill::region(world, session.catalog(), "main", 0, "floor", [2, 1], true).unwrap();
    assert_eq!(
        found.cells,
        [[1, 1], [1, 2], [2, 1], [2, 2], [3, 1], [3, 2]]
    );
    let outside = fill::region(world, session.catalog(), "main", 0, "floor", [0, 1], true);
    assert_eq!(outside.unwrap_err().code(), RefusalCode::ScopeOutsideView);
}

#[test]
fn every_start_cell_of_random_worlds_matches_the_reference() {
    let mut random = Random(0x0f11_5eed_0000_0001);
    let mut compared = 0;
    for round in 0..12 {
        let rows: Vec<String> = (0..6)
            .map(|_| {
                (0..12)
                    .map(|_| ['g', 's', '.'][random.below(if round % 3 == 0 { 2 } else { 3 })])
                    .collect()
            })
            .collect();
        // The widened view spans the chunk west of the default view: four
        // columns of it, then eight of the default chunk.
        let (_dir, mut session) = fixture_with(|root| {
            set_view(root, [-4, 0, 12, 6]);
            let west: Vec<String> = rows
                .iter()
                .map(|row| format!("{}{}", ".".repeat(28), &row[..4]))
                .collect();
            let west: Vec<&str> = west.iter().map(String::as_str).collect();
            set_rows(root, WEST, &west);
            let east: Vec<&str> = rows.iter().map(|row| &row[4..]).collect();
            set_rows(root, EAST, &east);
        });
        let mut records = Records::default();
        for at in view_cells(&session) {
            for contiguous in [true, false] {
                let expected = reference(&session, at, contiguous);
                let world = session.history().document().world();
                let found =
                    fill::region(world, session.catalog(), "main", 0, "floor", at, contiguous)
                        .unwrap();
                assert_eq!(found.value, value(&session, at).as_deref());
                assert_eq!(
                    found.cells.iter().copied().collect::<BTreeSet<_>>(),
                    expected,
                    "round {round} at {at:?} contiguous {contiguous}"
                );
                assert!(found.cells.windows(2).all(|pair| pair[0] < pair[1]));
                // What the engine would change, and what it prepares, are
                // the region less the cells that already hold the terrain.
                let proto = if value(&session, at).as_deref() == Some("grass") {
                    "sand"
                } else {
                    "grass"
                };
                let action = fill_action(&session, at, proto, contiguous);
                let changes = session.fill_changes(&action).unwrap();
                assert_eq!(changes, found.cells);
                session.dispatch(action, &mut records).unwrap();
                assert_eq!(prepared_cells(&session), changes);
                session.dispatch(Action::Cancel, &mut records).unwrap();
                compared += 1;
            }
        }
        assert!(records.0.is_empty());
    }
    assert_eq!(compared, 12 * 72 * 2);
}

#[test]
fn a_fill_is_the_stroke_planners_transaction_for_its_region() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let expected = revision(&session);
    for (at, proto, contiguous) in [
        ([1, 1], "sand", true),
        ([5, 0], "grass", true),
        ([5, 0], "grass", false),
        ([6, 4], "sand", true),
    ] {
        let region = reference(&session, at, contiguous);
        session
            .dispatch(fill_action(&session, at, proto, contiguous), &mut records)
            .unwrap();
        let filled = session.history().pending().unwrap().sealed().to_owned();
        session.dispatch(Action::Cancel, &mut records).unwrap();
        session
            .dispatch(
                Action::PaintStroke {
                    plane: "main".into(),
                    layer: 0,
                    grid: "floor".into(),
                    cells: region.into_iter().collect(),
                    proto: proto.into(),
                    expected,
                },
                &mut records,
            )
            .unwrap();
        let stroked = session.history().pending().unwrap().sealed().to_owned();
        session.dispatch(Action::Cancel, &mut records).unwrap();
        assert_eq!(filled, stroked, "{at:?} {contiguous}");
    }
    // Each cell expects its committed value: grass, sand, or empty (null).
    session
        .dispatch(fill_action(&session, [6, 4], "sand", true), &mut records)
        .unwrap();
    let operation = &sealed(&session)["operations"][0];
    assert_eq!(operation["op"], "terrain.set");
    assert_eq!(operation["plane"], "main");
    assert_eq!(operation["layer"], 0);
    assert_eq!(operation["grid"], "floor");
    assert_eq!(
        operation["cells"],
        json!([{"at": [6, 4], "value": "sand", "expect": null}])
    );
}

#[test]
fn the_same_world_gives_the_same_bytes_and_another_actor_only_its_name() {
    let (dir, mut first) = fixture_with(|root| set_view(root, [-4, 0, 12, 6]));
    let mut second = open(dir.path(), "editor:prototype");
    let mut agent = open(dir.path(), "agent:fill \"tests\"");
    let mut records = Records::default();
    for (at, contiguous) in [
        ([0, 0], true),
        ([0, 0], false),
        ([-2, 0], true),
        ([-4, 5], false),
    ] {
        let mut texts = Vec::new();
        for session in [&mut first, &mut second, &mut agent] {
            let action = fill_action(session, at, "sand", contiguous);
            // Twice in one session, and in each other session.
            for _ in 0..2 {
                session.dispatch(action.clone(), &mut records).unwrap();
                texts.push(session.history().pending().unwrap().sealed().to_owned());
                session.dispatch(Action::Cancel, &mut records).unwrap();
            }
        }
        assert!(texts[..4].iter().all(|text| text == &texts[0]), "{at:?}");
        assert_eq!(texts[4], texts[5]);
        assert_ne!(texts[4], texts[0]);
        assert_eq!(
            texts[4].replace(
                &Value::String("agent:fill \"tests\"".into()).to_string(),
                "\"editor:prototype\""
            ),
            texts[0]
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_fill_across_chunks_is_one_step_with_one_inverse_and_undoes_whole() {
    let (dir, mut session) = fixture_with(|root| set_view(root, [-4, 0, 12, 6]));
    let east = dir.path().join(EAST);
    let west = dir.path().join(WEST);
    let zones = dir.path().join("planes/main/zones.jsonc");
    let original = (fs::read(&east).unwrap(), fs::read(&west).unwrap());
    let zones_before = fs::read(&zones).unwrap();
    let mut records = Records::default();
    // Every grass cell of the widened view, on both sides of x = 0.
    let action = fill_action(&session, [-2, 0], "sand", false);
    let changes = session.fill_changes(&action).unwrap();
    assert!(changes.iter().any(|&[x, _]| x < 0) && changes.iter().any(|&[x, _]| x >= 0));
    // Nothing is written or prepared by working out the changes.
    assert!(session.history().pending().is_none());
    session.dispatch(action, &mut records).unwrap();
    assert_eq!(session.history().pending().unwrap().changed().count(), 2);
    assert!(session.review().complete);
    assert_eq!(
        (fs::read(&east).unwrap(), fs::read(&west).unwrap()),
        original
    );
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(records.0.len(), 1, "one fill retains one exact inverse");
    assert_eq!(session.history().depths(), (1, 0));
    for &cell in &changes {
        assert_eq!(value(&session, cell).as_deref(), Some("sand"), "{cell:?}");
    }
    // The empty cell, the other grid and the zones are untouched.
    assert_eq!(value(&session, [6, 4]), None);
    assert_eq!(fs::read(&zones).unwrap(), zones_before);
    let edge = |session: &Session| {
        let chunk = &session.history().document().world().planes()["main"].chunks[&(0, 0, 0)];
        chunk.ground.get(1).map(|grid| {
            (0..1024)
                .map(|cell| grid.value(cell).map(str::to_owned))
                .collect::<Vec<_>>()
        })
    };
    let painted = (fs::read(&east).unwrap(), fs::read(&west).unwrap());
    let edges = edge(&session);
    session.dispatch(Action::Undo, &mut records).unwrap();
    assert_eq!(session.history().pending().unwrap().changed().count(), 2);
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(
        (fs::read(&east).unwrap(), fs::read(&west).unwrap()),
        original
    );
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(
        (fs::read(&east).unwrap(), fs::read(&west).unwrap()),
        painted
    );
    assert_eq!(edge(&session), edges);
    assert_eq!(records.0.len(), 3);
    // The step keeps the fill as it was dispatched.
    let step = session.applied_steps().last().unwrap();
    assert!(matches!(
        step.origin.unwrap().action(),
        Action::FillTerrain {
            contiguous: false,
            ..
        }
    ));
}

#[test]
fn fills_are_refused_with_the_codes_of_a_paint() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let base = |session: &Session| fill_action(session, [1, 1], "sand", true);
    let with = |session: &Session, change: &dyn Fn(&mut Action)| {
        let mut action = base(session);
        change(&mut action);
        action
    };
    let cases: Vec<(Action, RefusalCode)> = vec![
        (
            with(&session, &|action| {
                if let Action::FillTerrain { expected, .. } = action {
                    *expected = [0; 32];
                }
            }),
            RefusalCode::RevisionStale,
        ),
        (
            fill_action(&session, [8, 0], "sand", true),
            RefusalCode::ScopeOutsideView,
        ),
        (
            fill_action(&session, [0, -1], "sand", false),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with(&session, &|action| {
                if let Action::FillTerrain { layer, .. } = action {
                    *layer = 1;
                }
            }),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with(&session, &|action| {
                if let Action::FillTerrain { plane, .. } = action {
                    *plane = "other".into();
                }
            }),
            RefusalCode::ScopeOutsideView,
        ),
        // Not granted, wherever it starts.
        (
            fill_action(&session, [1, 1], "stone", true),
            RefusalCode::GrantMissing,
        ),
        (
            fill_action(&session, [100, 1], "stone", false),
            RefusalCode::GrantMissing,
        ),
        (
            with(&session, &|action| {
                if let Action::FillTerrain { grid, .. } = action {
                    *grid = "edge".into();
                }
            }),
            RefusalCode::GrantMissing,
        ),
        // Already this terrain: no change, fill or replace.
        (
            fill_action(&session, [0, 0], "grass", true),
            RefusalCode::ChangeNone,
        ),
        (
            fill_action(&session, [7, 5], "sand", false),
            RefusalCode::ChangeNone,
        ),
    ];
    for (action, code) in cases {
        let name = format!("{action:?}");
        // The preview refuses exactly as dispatch does.
        let preview = session.fill_changes(&action).unwrap_err();
        assert_eq!(preview.code(), code, "{name}");
        let message = preview.message().to_owned();
        let error = session.dispatch(action, &mut records).unwrap_err();
        assert_eq!(error.code(), code, "{name}");
        assert_eq!(error.message(), message, "{name}");
        assert!(session.history().pending().is_none(), "{name}");
    }
    assert_eq!(
        session.fill_changes(&Action::Accept).unwrap_err().code(),
        RefusalCode::InputInvalid
    );
    assert!(records.0.is_empty());
}

#[test]
fn a_preview_needs_a_ready_session_and_the_fills_own_revision() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let action = fill_action(&session, [1, 1], "sand", true);
    assert_eq!(session.fill_changes(&action).unwrap().len(), 24);
    session.dispatch(action.clone(), &mut records).unwrap();
    // A pending change: no preview, and no second fill.
    assert_eq!(
        session.fill_changes(&action).unwrap_err().code(),
        RefusalCode::PendingExists
    );
    assert_eq!(
        session
            .dispatch(action.clone(), &mut records)
            .unwrap_err()
            .code(),
        RefusalCode::PendingExists
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert_eq!(session.fill_changes(&action).unwrap().len(), 24);
}

#[cfg(target_os = "linux")]
#[test]
fn each_cell_expects_its_committed_value_so_a_changed_cell_refuses_the_fill() {
    let (dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    session
        .dispatch(fill_action(&session, [1, 1], "sand", true), &mut records)
        .unwrap();
    let text = session.history().pending().unwrap().sealed().to_owned();
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // The same transaction prepares on an identical copy...
    let same = tempfile::tempdir().unwrap();
    copy_tree(dir.path(), same.path());
    let mut history = History::new(WorldDocument::open(same.path()).unwrap());
    let base = history.document().revision();
    history.prepare(base, "editor:action", &text).unwrap();
    // ...and is refused by World where one covered cell changed, even with
    // that copy's own revision: the cell's `expect` no longer holds.
    let changed = tempfile::tempdir().unwrap();
    copy_tree(dir.path(), changed.path());
    set_rows(changed.path(), EAST, &["gggsssss"]);
    let mut history = History::new(WorldDocument::open(changed.path()).unwrap());
    let base = history.document().revision();
    assert!(matches!(
        history.prepare(base, "editor:action", &text),
        Err(tilewright_editor_commands::Error::Rejected(_))
    ));
}

/// The neutral fixture's plane with a 64 x 64 view over four full chunks of
/// grass: the largest view the catalog allows.
fn full_view(root: &Path) {
    set_view(root, [0, 0, 64, 64]);
    let grass: Vec<String> = (0..32).map(|_| "g".repeat(32)).collect();
    let grass: Vec<&str> = grass.iter().map(String::as_str).collect();
    set_rows(root, EAST, &grass);
    for [x, y] in [[1, 0], [0, 1], [1, 1]] {
        let chunk = json!({
            "format": "tilewright.chunk/1",
            "plane": "main",
            "layer": 0,
            "chunk": [x, y],
            "terrain": {"floor": {"palette": {"g": "grass"}, "rows": grass}},
            "objects": [],
        });
        fs::write(
            root.join(format!("planes/main/layers/0/{x}_{y}.jsonc")),
            serde_json::to_vec_pretty(&chunk).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn a_full_view_replace_fits_the_stroke_cap_and_the_review() {
    let (_dir, mut session) = fixture_with(full_view);
    let mut records = Records::default();
    for contiguous in [true, false] {
        let action = fill_action(&session, [63, 63], "sand", contiguous);
        let changes = session.fill_changes(&action).unwrap();
        assert_eq!(
            changes.len(),
            tilewright_editor_ui::brush::MAXIMUM_STROKE_CELLS
        );
        session.dispatch(action, &mut records).unwrap();
        assert_eq!(prepared_cells(&session), changes);
        let pending = session.history().pending().unwrap();
        assert_eq!(pending.changed().count(), 4);
        // Measured: four changed chunk files, each excerpt well inside the
        // review's budget, so the review is complete and Accept is offered.
        assert!(pending.changed().count() <= MAXIMUM_REVIEW_FILES);
        let shown: usize = session.review().lines.iter().map(String::len).sum();
        assert!(shown < MAXIMUM_REVIEW_BYTES, "{shown}");
        assert!(session.review().complete);
        session.dispatch(Action::Cancel, &mut records).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_fill_of_cells_with_no_chunk_creates_the_chunk_as_a_stroke_does() {
    // The view reaches 8 columns into chunk (1, 0), which has no document.
    let (dir, mut session) = fixture_with(|root| set_view(root, [28, 0, 12, 6]));
    let missing = dir.path().join("planes/main/layers/0/1_0.jsonc");
    assert!(!missing.exists());
    let mut records = Records::default();
    let action = fill_action(&session, [35, 5], "grass", true);
    let changes = session.fill_changes(&action).unwrap();
    // Every empty cell of the view joins: the empty right half of chunk
    // (0, 0) and all of chunk (1, 0) in view.
    assert_eq!(changes.len(), 12 * 6);
    session.dispatch(action, &mut records).unwrap();
    let filled = session.history().pending().unwrap().sealed().to_owned();
    session.dispatch(Action::Cancel, &mut records).unwrap();
    session
        .dispatch(
            Action::PaintStroke {
                plane: "main".into(),
                layer: 0,
                grid: "floor".into(),
                cells: changes.clone(),
                proto: "grass".into(),
                expected: revision(&session),
            },
            &mut records,
        )
        .unwrap();
    assert_eq!(session.history().pending().unwrap().sealed(), filled);
    assert_eq!(session.history().pending().unwrap().changed().count(), 2);
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(missing.exists());
    assert_eq!(value(&session, [39, 0]).as_deref(), Some("grass"));
}
