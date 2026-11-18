// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Scatter (`Action::ScatterTerrain`): a catalog palette drawn by weight for
//! each cell of a brush stroke, from a recorded seed, as one reviewed
//! `terrain.set`. The draw is pinned by golden values computed outside this
//! repository from the definition in `src/scatter.rs`.
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{Palette, PreviewLimits, SelectionKey, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::tools::{TerrainRule, ToolMode, ToolState};
use tilewright_editor_ui::uid::SeededUidSource;
use tilewright_editor_ui::{Action, Actor, RefusalCode, Session, inspector, scatter};

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
/// Give the catalog the palettes `palettes` (the `editing.palettes` form).
fn set_palettes(root: &Path, palettes: &Value) {
    edit_json(root, "preview.jsonc", |catalog| {
        catalog["editing"]["palettes"] = palettes.clone();
    });
}
/// `speckle`: grass 3, sand 1, on `floor`.
fn speckle() -> Value {
    json!([{"name": "speckle", "grid": "floor", "entries": [
        {"value": "sand", "weight": 1}, {"value": "grass", "weight": 3}]}])
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
    set_palettes(dir.path(), &speckle());
    configure(dir.path());
    let session = open(dir.path(), "editor:prototype");
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
fn scatter_action(session: &Session, cells: Vec<[i64; 2]>, palette: &str, seed: u32) -> Action {
    Action::ScatterTerrain {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        cells,
        palette: palette.into(),
        seed,
        expected: revision(session),
    }
}
fn view_cells() -> Vec<[i64; 2]> {
    // The fixture's view is [0, 0, 8, 6]; stroke order is x, then y.
    (0..8).flat_map(|x| (0..6).map(move |y| [x, y])).collect()
}
/// The cells of the pending `terrain.set`: `at` to (value, expect).
fn pending_cells(session: &Session) -> BTreeMap<[i64; 2], (String, Value)> {
    let sealed: Value =
        serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap();
    let operations = sealed["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["op"], "terrain.set");
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
                    cell["value"].as_str().unwrap().to_owned(),
                    cell["expect"].clone(),
                ),
            )
        })
        .collect()
}
fn sealed(session: &Session) -> String {
    session.history().pending().unwrap().sealed().to_owned()
}

/// Seed 1234, layer 0, `speckle` (grass 3, sand 1) over the fixture's
/// view, row y = 0 first, column x = 0 first: `G` grass, `s` sand. From the
/// definition alone, by a separate implementation.
const GOLDEN_1234: [&str; 6] = [
    "GsGGGGGG", "GGGGsGGG", "GGGGGsGs", "GsGGGGGG", "GGGGssGG", "GssGGsGG",
];
/// The fixture's committed `floor` rows inside the view (`.` empty).
const COMMITTED: [&str; 6] = [
    "ggggssss", "ggggssss", "ggggssss", "ggggssss", "ggggss.s", "ggggssss",
];
fn letter(value: Option<&str>) -> char {
    match value {
        Some("grass") => 'G',
        Some("sand") => 's',
        None => '.',
        Some(other) => panic!("unexpected {other}"),
    }
}

#[test]
fn a_scatter_writes_the_golden_draw_and_leaves_out_cells_that_hold_it() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    session
        .dispatch(
            scatter_action(&session, view_cells(), "speckle", 1234),
            &mut records,
        )
        .unwrap();
    let cells = pending_cells(&session);
    for (y, (golden, committed)) in GOLDEN_1234.iter().zip(COMMITTED).enumerate() {
        for (x, (want, held)) in golden.chars().zip(committed.chars()).enumerate() {
            let at = [i64::try_from(x).unwrap(), i64::try_from(y).unwrap()];
            let held_upper = match held {
                'g' => 'G',
                other => other,
            };
            match cells.get(&at) {
                // A cell already holding its draw is left out.
                None => assert_eq!(held_upper, want, "{at:?} left out"),
                Some((value, expect)) => {
                    assert_ne!(held_upper, want, "{at:?} changed");
                    assert_eq!(letter(Some(value)), want, "{at:?}");
                    let expected_before = match held {
                        'g' => json!("grass"),
                        's' => json!("sand"),
                        _ => Value::Null,
                    };
                    assert_eq!(expect, &expected_before, "{at:?}");
                }
            }
        }
    }
    // 48 cells; the draw and the committed world agree on 26 of them.
    assert_eq!(cells.len(), 22);
}

// Publication is checked, which only Linux provides (see `session.rs`).
#[cfg(target_os = "linux")]
#[test]
fn an_accepted_scatter_holds_the_golden_draw_and_undoes_in_one_step() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    session
        .dispatch(
            scatter_action(&session, view_cells(), "speckle", 1234),
            &mut records,
        )
        .unwrap();
    // Accept, then undo and redo, each in one step.
    session.dispatch(Action::Accept, &mut records).unwrap();
    for (y, golden) in GOLDEN_1234.iter().enumerate() {
        for (x, want) in golden.chars().enumerate() {
            let at = [i64::try_from(x).unwrap(), i64::try_from(y).unwrap()];
            let world = session.world();
            let chunk = &world.planes()["main"].chunks[&(0, 0, 0)];
            let local = usize::try_from(at[1] * 32 + at[0]).unwrap();
            assert_eq!(letter(chunk.ground[0].value(local)), want, "{at:?}");
        }
    }
    let after = revision(&session);
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(revision(&session), after);
}

/// The definition again, written differently: 128-bit products and a
/// running total taken from the weights in value order.
fn reference(palette: &Palette, seed: u32, layer: i32, [x, y]: [i64; 2]) -> String {
    const G: u64 = 0x9e37_79b9_7f4a_7c15;
    let mix = |z: u64| {
        let z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    let mut h = mix(u64::from(seed).wrapping_add(G));
    for word in [i64::from(layer), x, y] {
        h = mix((h ^ u64::from_ne_bytes(word.to_ne_bytes())).wrapping_add(G));
    }
    let total: u128 = palette.entries.values().map(|w| u128::from(*w)).sum();
    let r = (u128::from(h >> 32) * total) >> 32;
    let mut below = 0u128;
    palette
        .entries
        .iter()
        .find(|(_, weight)| {
            below += u128::from(**weight);
            r < below
        })
        .map(|(value, _)| value.clone())
        .unwrap()
}

#[test]
fn the_draw_agrees_with_a_second_implementation_and_follows_the_weights() {
    let palette = Palette {
        grid: "floor".into(),
        entries: [
            ("a".to_owned(), 1),
            ("b".to_owned(), 10_000),
            ("c".to_owned(), 37),
            ("d".to_owned(), 9_999),
        ]
        .into(),
    };
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for seed in [0, 1, 1234, u32::MAX] {
        for layer in [-3, 0, 7] {
            for x in -20..20 {
                for y in -20..20 {
                    let got = scatter::value(&palette, seed, layer, [x, y]).unwrap();
                    assert_eq!(got, reference(&palette, seed, layer, [x, y]));
                    *counts.entry(got.to_owned()).or_default() += 1;
                }
            }
        }
    }
    // 19,200 draws: b and d about half each, c a few dozen, a about one.
    let total: u32 = counts.values().sum();
    assert_eq!(total, 19_200);
    for (value, low, high) in [("b", 9_100, 10_100), ("d", 9_100, 10_100), ("c", 10, 120)] {
        let count = counts.get(value).copied().unwrap_or(0);
        assert!((low..=high).contains(&count), "{value}: {count}");
    }
    assert!(counts.get("a").copied().unwrap_or(0) <= 10);
}

#[test]
fn the_same_seed_gives_the_same_bytes_and_a_cell_keeps_its_value_in_any_stroke() {
    let (dir, mut first) = fixture_with(|_| {});
    let mut second = open(dir.path(), "editor:prototype");
    let mut records = Records::default();
    for session in [&mut first, &mut second] {
        let action = scatter_action(session, view_cells(), "speckle", 99);
        session.dispatch(action, &mut records).unwrap();
    }
    assert_eq!(sealed(&first), sealed(&second));
    let whole = pending_cells(&first);
    second.dispatch(Action::Cancel, &mut records).unwrap();
    // Another seed scatters differently.
    let action = scatter_action(&second, view_cells(), "speckle", 100);
    second.dispatch(action, &mut records).unwrap();
    assert_ne!(sealed(&first), sealed(&second));
    second.dispatch(Action::Cancel, &mut records).unwrap();
    // A partial stroke with the same seed gives each of its cells the value
    // the whole stroke gave it: the draw is per cell.
    let part: Vec<[i64; 2]> = view_cells()
        .into_iter()
        .filter(|[x, y]| (x + y) % 3 == 0)
        .collect();
    let action = scatter_action(&second, part.clone(), "speckle", 99);
    second.dispatch(action, &mut records).unwrap();
    let partial = pending_cells(&second);
    assert!(!partial.is_empty());
    for at in &part {
        assert_eq!(partial.get(at), whole.get(at), "{at:?}");
    }
}

#[test]
fn the_palette_is_read_from_the_catalog_when_prepared() {
    // The same action under a catalog whose speckle has other weights.
    let (dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let action = scatter_action(&session, view_cells(), "speckle", 1234);
    session.dispatch(action.clone(), &mut records).unwrap();
    let before = sealed(&session);
    set_palettes(
        dir.path(),
        &json!([{"name": "speckle", "grid": "floor", "entries": [
            {"value": "sand", "weight": 3}, {"value": "grass", "weight": 1}]}]),
    );
    let mut changed = open(dir.path(), "editor:prototype");
    changed.dispatch(action, &mut records).unwrap();
    assert_ne!(sealed(&changed), before);
}

#[test]
fn refusals_carry_the_codes_of_a_brush_stroke() {
    let (_dir, mut session) = fixture_with(|root| {
        let mut palettes = speckle();
        palettes.as_array_mut().unwrap().push(
            json!({"name": "sandy", "grid": "floor", "entries": [{"value": "sand", "weight": 5}]}),
        );
        set_palettes(root, &palettes);
    });
    let mut records = Records::default();
    let expected = revision(&session);
    let with = |cells: Vec<[i64; 2]>, grid: &str, palette: &str| Action::ScatterTerrain {
        plane: "main".into(),
        layer: 0,
        grid: grid.into(),
        cells,
        palette: palette.into(),
        seed: 5,
        expected,
    };
    let cases = [
        (
            with(vec![[0, 0]], "floor", "nothing"),
            RefusalCode::GrantMissing,
        ),
        (
            with(vec![[0, 0]], "edge", "speckle"),
            RefusalCode::GrantMissing,
        ),
        (
            with(vec![[100, 0]], "floor", "speckle"),
            RefusalCode::ScopeOutsideView,
        ),
        (
            with(vec![[5, 0], [5, 1]], "floor", "sandy"),
            RefusalCode::ChangeNone,
        ),
        (with(vec![], "floor", "speckle"), RefusalCode::InputBudget),
        (
            with(vec![[1, 0], [0, 0]], "floor", "speckle"),
            RefusalCode::InputInvalid,
        ),
        (
            with(vec![[0, 0]; 4097], "floor", "speckle"),
            RefusalCode::InputBudget,
        ),
        (
            Action::ScatterTerrain {
                plane: "main".into(),
                layer: 0,
                grid: "floor".into(),
                cells: vec![[0, 0]],
                palette: "sandy".into(),
                seed: 5,
                expected: [9; 32],
            },
            RefusalCode::RevisionStale,
        ),
    ];
    for (action, code) in cases {
        let error = session.dispatch(action.clone(), &mut records).unwrap_err();
        assert_eq!(error.code(), code, "{action:?}");
        assert!(session.history().pending().is_none());
    }
    // A pending change fences a scatter.
    session
        .dispatch(with(vec![[0, 0]], "floor", "sandy"), &mut records)
        .unwrap();
    let error = session
        .dispatch(with(vec![[1, 0]], "floor", "sandy"), &mut records)
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
}

fn cell(x: i64, y: i64) -> SelectionKey {
    SelectionKey::Cell {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        x,
        y,
    }
}
/// Select `start` and begin a seeded stroke there, as a native press does.
fn press(
    session: &mut Session,
    tools: &mut ToolState,
    records: &mut Records,
    start: [i64; 2],
    seed: u32,
) -> Result<(), tilewright_editor_ui::tools::ToolError> {
    session
        .dispatch(
            Action::Select {
                generation: session.generation(),
                key: cell(start[0], start[1]),
            },
            records,
        )
        .unwrap();
    let key = session.selection().unwrap().clone();
    let inspected = inspector::inspect(
        session.world(),
        session.catalog(),
        session.selection(),
        session.generation(),
        session.history().document().revision(),
    );
    tools.begin_seeded_stroke(&inspected, &key, !session.can_prepare(), seed)
}

#[test]
fn a_scatter_stroke_previews_exactly_what_its_release_prepares() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Terrain);
    tools.choose_scatter(session.catalog(), "speckle").unwrap();
    tools.set_brush(tilewright_editor_ui::brush::Brush {
        shape: tilewright_editor_ui::brush::BrushShape::Square,
        radius: 2,
    });
    press(&mut session, &mut tools, &mut records, [2, 2], 1234).unwrap();
    tools
        .extend_stroke(&cell(5, 3), session.generation())
        .unwrap();
    let preview = session.stroke_changes(tools.stroke().unwrap()).unwrap();
    let action = tools
        .finish_stroke(session.generation(), false)
        .unwrap()
        .unwrap();
    let Action::ScatterTerrain {
        ref palette, seed, ..
    } = action
    else {
        panic!("not a scatter: {action:?}");
    };
    assert_eq!((palette.as_str(), seed), ("speckle", 1234));
    session.dispatch(action, &mut records).unwrap();
    let prepared: Vec<[i64; 2]> = pending_cells(&session).into_keys().collect();
    assert_eq!(preview, prepared);
    // The values are the golden draw's.
    for (at, (value, _)) in pending_cells(&session) {
        let golden = GOLDEN_1234[usize::try_from(at[1]).unwrap()]
            .chars()
            .nth(usize::try_from(at[0]).unwrap())
            .unwrap();
        assert_eq!(letter(Some(&value)), golden, "{at:?}");
    }
}

#[test]
fn a_palette_choice_and_a_terrain_choice_exclude_each_other() {
    let (_dir, mut session) = fixture_with(|_| {});
    let mut records = Records::default();
    let mut tools = ToolState::default();
    tools.set_mode(ToolMode::Terrain);
    assert!(tools.choose_scatter(session.catalog(), "nothing").is_err());
    tools
        .choose_terrain(session.catalog(), "floor", "sand")
        .unwrap();
    tools.choose_scatter(session.catalog(), "speckle").unwrap();
    assert_eq!(tools.scatter_choice(), Some("speckle"));
    assert!(tools.terrain_choice().is_none());
    // Fill and replace paint one terrain: with a palette chosen they ask
    // for one.
    tools.set_terrain_rule(TerrainRule::Fill);
    assert!(
        tools
            .fill_action(session.catalog(), &cell(1, 1), revision(&session), false)
            .is_err()
    );
    assert!(press(&mut session, &mut tools, &mut records, [1, 1], 3).is_err());
    tools.set_terrain_rule(TerrainRule::Brush);
    press(&mut session, &mut tools, &mut records, [1, 1], 3).unwrap();
    tools.cancel_stroke();
    tools
        .choose_terrain(session.catalog(), "floor", "grass")
        .unwrap();
    assert!(tools.scatter_choice().is_none());
    press(&mut session, &mut tools, &mut records, [1, 1], 3).unwrap();
    assert!(matches!(
        tools.finish_stroke(session.generation(), false).unwrap(),
        Some(Action::PaintStroke { .. })
    ));
}

#[test]
fn a_seed_drawn_from_the_session_follows_its_uid_source() {
    let (dir, mut first) = fixture_with(|_| {});
    let mut second = open(dir.path(), "editor:prototype");
    first.set_uid_source(Box::new(SeededUidSource::new(3)));
    second.set_uid_source(Box::new(SeededUidSource::new(3)));
    let seeds: Vec<u32> = (0..4).map(|_| first.draw_seed().unwrap()).collect();
    assert_eq!(
        seeds,
        (0..4)
            .map(|_| second.draw_seed().unwrap())
            .collect::<Vec<_>>()
    );
    assert!(seeds.windows(2).all(|pair| pair[0] != pair[1]));
    // Drawing a seed changes nothing in the session.
    assert!(first.history().pending().is_none());
    let _ = fs::read(dir.path().join(CHUNK)).unwrap();
}

#[test]
fn a_palette_scatters_only_on_its_own_grid() {
    // `edge` grants the same values as `floor`, but speckle is floor's.
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["terrain"]
                .as_array_mut()
                .unwrap()
                .push(json!({"grid": "edge", "values": ["grass", "sand"]}));
        });
    });
    let mut records = Records::default();
    let mut on = |grid: &str| {
        let action = Action::ScatterTerrain {
            plane: "main".into(),
            layer: 0,
            grid: grid.into(),
            cells: vec![[0, 0], [1, 0]],
            palette: "speckle".into(),
            seed: 77,
            expected: revision(&session),
        };
        let result = session.dispatch(action, &mut records);
        if result.is_ok() {
            session.dispatch(Action::Cancel, &mut records).unwrap();
        }
        result.map_err(|error| error.code())
    };
    assert_eq!(on("edge"), Err(RefusalCode::GrantMissing));
    assert_eq!(on("floor"), Ok(()));
}
