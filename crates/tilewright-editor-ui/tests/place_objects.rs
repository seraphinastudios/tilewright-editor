// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Stamps (`Action::PlaceObjects`, ADR 0009): one reviewed transaction of
//! one `object.insert` per member, in member order, each exactly what
//! Place writes for one object; refused whole before anything is
//! prepared; a warning, never a refusal, for occupied tiles; and an exact
//! inverse on Undo.
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::WorldDocument;
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{
    Action, RefusalCode, Session, SessionPhase,
    stamp::{MAXIMUM_STAMP_OBJECTS, MAXIMUM_STAMP_OFFSET, StampObject},
    uid::{SeededUidSource, UidSource, format_uid},
};

const CHUNK: &str = "planes/main/layers/0/0_0.jsonc";
const SEED: u64 = 42;

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
/// Every file under `root`, by relative path, with its bytes.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else {
                let path = entry.path();
                let relative = path.strip_prefix(root).unwrap().display().to_string();
                files.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    files
}
fn open(root: &Path) -> Session {
    let document = WorldDocument::open(root).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    let mut session = Session::new(
        History::new(document),
        catalog,
        resources,
        tilewright_editor_ui::Actor::new("editor:prototype").unwrap(),
    );
    session.set_uid_source(Box::new(SeededUidSource::new(SEED)));
    session
}
/// A temporary copy of the neutral sample, which grants marker, stone,
/// bush, crate, gate and lamp in the view `[0, 0, 8, 6]`.
fn fixture_with(configure: impl FnOnce(&Path)) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_world("prototype-neutral"), dir.path());
    configure(dir.path());
    let session = open(dir.path());
    (dir, session)
}
fn fixture() -> (tempfile::TempDir, Session) {
    fixture_with(|_| {})
}
fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}
#[derive(Default)]
struct Records(Vec<String>);
impl RecoveryStore for Records {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.0.push(inverse.into());
        Ok(())
    }
}
fn object(proto: &str, dx: i64, dy: i64) -> StampObject {
    StampObject {
        proto: proto.into(),
        offset: [dx, dy],
    }
}
fn stamp(session: &Session, x: i64, y: i64, objects: Vec<StampObject>) -> Action {
    Action::PlaceObjects {
        plane: "main".into(),
        layer: 0,
        x,
        y,
        objects,
        expected: session.history().document().revision(),
    }
}
/// The uids a fresh `SeededUidSource::new(SEED)` gives, in order.
fn seeded(count: usize) -> Vec<String> {
    let mut source = SeededUidSource::new(SEED);
    (0..count)
        .map(|_| {
            let mut bytes = [0; 16];
            source.fill(&mut bytes).unwrap();
            format_uid(bytes)
        })
        .collect()
}
/// An action, or how to build one from the session (for its revision).
trait BuildAction {
    fn build(self, session: &Session) -> Action;
}
impl BuildAction for Action {
    fn build(self, _: &Session) -> Action {
        self
    }
}
impl<F: FnOnce(&Session) -> Action> BuildAction for F {
    fn build(self, session: &Session) -> Action {
        self(session)
    }
}
fn sealed(session: &Session) -> Value {
    serde_json::from_str(session.history().pending().unwrap().sealed()).unwrap()
}
fn code(session: &mut Session, action: impl BuildAction) -> RefusalCode {
    let action = action.build(session);
    let error = session
        .dispatch(action, &mut Records::default())
        .unwrap_err();
    assert!(session.history().pending().is_none(), "{error}");
    assert_eq!(session.status().phase, SessionPhase::Ready, "{error}");
    error.code()
}

#[test]
fn a_stamp_is_one_transaction_of_inserts_in_member_order() {
    let (dir, mut session) = fixture();
    let original = tree(dir.path());
    let mut records = Records::default();
    let uids = seeded(3);
    // Two markers on one tile and a stone beside them.
    session
        .dispatch(
            stamp(
                &session,
                5,
                2,
                vec![
                    object("marker", 0, 0),
                    object("stone", 1, -1),
                    object("marker", 0, 0),
                ],
            ),
            &mut records,
        )
        .unwrap();
    assert_eq!(session.status().phase, SessionPhase::Pending);
    let transaction = sealed(&session);
    assert_eq!(
        transaction["operations"],
        json!([
            {"op": "object.insert", "plane": "main", "layer": 0, "at": [5, 2],
                "object": {"uid": uids[0], "proto": "marker", "layer": "item"}},
            {"op": "object.insert", "plane": "main", "layer": 0, "at": [6, 1],
                "object": {"uid": uids[1], "proto": "stone", "layer": "item",
                    "fields": {"label": "New stone", "settings": {"active": false, "visits": 0}}}},
            {"op": "object.insert", "plane": "main", "layer": 0, "at": [5, 2],
                "object": {"uid": uids[2], "proto": "marker", "layer": "item"}},
        ])
    );
    // One review: the occupied-tile warning first, then each insertion.
    assert_eq!(
        session.pending_warnings(),
        [
            "Warning: 1 of the 3 new objects lands on a tile that already holds an object or another new one; each is added on top.".to_owned(),
            format!("Adds marker at tile (5, 2) of main layer 0, with uid {}.", uids[0]),
            format!("Adds stone at tile (6, 1) of main layer 0, with uid {}.", uids[1]),
            format!("Adds marker at tile (5, 2) of main layer 0, with uid {}.", uids[2]),
        ]
    );
    assert!(session.review().complete);
    assert_eq!(session.history().pending().unwrap().changed().count(), 1);
    // Nothing is written before Accept, and Cancel writes nothing.
    assert_eq!(tree(dir.path()), original);
    session.dispatch(Action::Cancel, &mut records).unwrap();
    assert!(session.pending_warnings().is_empty());
    assert_eq!(tree(dir.path()), original);
    assert!(records.0.is_empty());

    // Publication is checked on Linux only; other platforms refuse it.
    if !cfg!(target_os = "linux") {
        return;
    }
    let uids = seeded(6)[3..].to_vec();
    session
        .dispatch(
            stamp(
                &session,
                5,
                2,
                vec![
                    object("marker", 0, 0),
                    object("stone", 1, -1),
                    object("marker", 0, 0),
                ],
            ),
            &mut records,
        )
        .unwrap();
    let selection = session.selection().cloned();
    session.dispatch(Action::Accept, &mut records).unwrap();
    // One recovery record, one undo step; the selection stays as it was.
    assert_eq!(records.0.len(), 1);
    assert_eq!(session.history().depths(), (1, 0));
    assert_eq!(session.selection().cloned(), selection);
    let step = session.applied_steps().last().unwrap();
    let origin = step.origin.unwrap();
    assert_eq!(origin.placed_objects(), uids.as_slice());
    assert_eq!(origin.placed(), None);
    let placed = tree(dir.path());
    assert_ne!(placed, original);
    // Stacked in member order on (5, 2), after anything already there.
    let chunk = &session.world().planes()["main"].chunks[&(0, 0, 0)];
    let on_tile: Vec<_> = chunk
        .objects
        .iter()
        .filter(|object| object.at == Some([5, 2]))
        .filter_map(|object| object.uid.as_deref())
        .collect();
    assert_eq!(on_tile, [uids[0].as_str(), uids[2].as_str()]);
    assert_eq!(session.world().object(&uids[1]).unwrap().at, Some([6, 1]));

    // Undo is the exact inverse: every byte of the world as before.
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(tree(dir.path()), original);
    for uid in &uids {
        assert!(session.world().object(uid).is_none());
    }
    session.dispatch(Action::Redo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert_eq!(tree(dir.path()), placed);
    assert_eq!(records.0.len(), 3);
    // A redone stamp keeps its origin and uids.
    let origin = session.applied_steps().last().unwrap().origin.unwrap();
    assert_eq!(origin.placed_objects(), uids.as_slice());
}

#[test]
fn a_stamp_across_chunks_creates_the_missing_chunk_and_undo_removes_it() {
    let new = "planes/main/layers/0/1_0.jsonc";
    let (dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["view"]["bounds"] = json!([0, 0, 40, 6]);
        });
    });
    let original = tree(dir.path());
    let mut records = Records::default();
    session
        .dispatch(
            stamp(
                &session,
                31,
                1,
                vec![
                    object("marker", 0, 0),
                    object("lamp", 1, 0),
                    object("bush", 2, 1),
                ],
            ),
            &mut records,
        )
        .unwrap();
    let pending = session.history().pending().unwrap();
    let changed: Vec<_> = pending
        .changed()
        .map(|change| change.path.clone())
        .collect();
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert!(changed.iter().any(|path| path == new));
    assert!(changed.iter().any(|path| path == CHUNK));
    if !cfg!(target_os = "linux") {
        return;
    }
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(dir.path().join(new).exists());
    session.dispatch(Action::Undo, &mut records).unwrap();
    session.dispatch(Action::Accept, &mut records).unwrap();
    assert!(!dir.path().join(new).exists());
    assert_eq!(tree(dir.path()), original);
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_stamp_is_refused_whole_before_anything_is_prepared() {
    let (dir, mut session) = fixture();
    let original = tree(dir.path());
    // No member, and one too many.
    assert_eq!(
        code(&mut session, |s: &Session| stamp(s, 1, 1, Vec::new())),
        RefusalCode::InputInvalid
    );
    assert_eq!(
        code(&mut session, |s: &Session| stamp(
            s,
            1,
            1,
            vec![object("marker", 0, 0); MAXIMUM_STAMP_OBJECTS + 1]
        )),
        RefusalCode::InputBudget
    );
    // 64 members on one tile are a valid stamp.
    session
        .dispatch(
            stamp(
                &session,
                1,
                1,
                vec![object("marker", 0, 0); MAXIMUM_STAMP_OBJECTS],
            ),
            &mut Records::default(),
        )
        .unwrap();
    assert_eq!(sealed(&session)["operations"].as_array().unwrap().len(), 64);
    session
        .dispatch(Action::Cancel, &mut Records::default())
        .unwrap();
    // An offset past the bound, even far outside any view.
    let far = MAXIMUM_STAMP_OFFSET + 1;
    assert_eq!(
        code(&mut session, |s: &Session| stamp(
            s,
            1,
            1,
            vec![object("marker", 0, 0), object("marker", far, 0)]
        )),
        RefusalCode::InputBudget
    );
    // One ungranted member (mapped or not) refuses every member, and comes
    // before the view: this stamp is also outside it.
    for proto in ["grass", "unknown"] {
        assert_eq!(
            code(&mut session, |s: &Session| stamp(
                s,
                7,
                1,
                vec![object("marker", 0, 0), object(proto, 1, 0)]
            )),
            RefusalCode::GrantMissing,
            "{proto}"
        );
    }
    // One member outside the view refuses the stamp; it is never clipped.
    // The view is x 0..=7, y 0..=5.
    for (x, y, offset) in [
        (7, 1, [1, 0]),
        (0, 1, [-1, 0]),
        (1, 5, [0, 1]),
        (1, 0, [0, -1]),
        (i64::MAX, 0, [1, 0]),
        (0, i64::MIN, [0, -1]),
    ] {
        assert_eq!(
            code(&mut session, |s: &Session| stamp(
                s,
                x,
                y,
                vec![
                    object("marker", 0, 0),
                    object("marker", offset[0], offset[1])
                ]
            )),
            RefusalCode::ScopeOutsideView,
            "{x} {y} {offset:?}"
        );
    }
    // The anchor itself need not be a member's tile, nor in the view.
    session
        .dispatch(
            stamp(
                &session,
                -3,
                9,
                vec![object("marker", 3, -9), object("marker", 10, -4)],
            ),
            &mut Records::default(),
        )
        .unwrap();
    let operations = sealed(&session)["operations"].clone();
    assert_eq!(operations[0]["at"], json!([0, 0]));
    assert_eq!(operations[1]["at"], json!([7, 5]));
    session
        .dispatch(Action::Cancel, &mut Records::default())
        .unwrap();
    // Another plane or spatial layer.
    for (plane, layer) in [("other", 0), ("main", 1)] {
        let action = Action::PlaceObjects {
            plane: plane.into(),
            layer,
            x: 1,
            y: 1,
            objects: vec![object("marker", 0, 0)],
            expected: session.history().document().revision(),
        };
        assert_eq!(
            code(&mut session, action),
            RefusalCode::ScopeOutsideView,
            "{plane} {layer}"
        );
    }
    // A stale revision, after the members' checks.
    let action = Action::PlaceObjects {
        plane: "main".into(),
        layer: 0,
        x: 1,
        y: 1,
        objects: vec![object("marker", 0, 0)],
        expected: [7; 32],
    };
    assert_eq!(code(&mut session, action), RefusalCode::RevisionStale);
    // A pending change fences a stamp, and a stamp fences the next edit.
    session
        .dispatch(
            stamp(&session, 1, 1, vec![object("marker", 0, 0)]),
            &mut Records::default(),
        )
        .unwrap();
    let error = session
        .dispatch(
            stamp(&session, 2, 1, vec![object("marker", 0, 0)]),
            &mut Records::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::PendingExists);
    session
        .dispatch(Action::Cancel, &mut Records::default())
        .unwrap();
    assert_eq!(tree(dir.path()), original);
}

#[test]
fn world_refusing_a_member_refuses_the_stamp_with_nothing_pending() {
    // The catalog checks that `settings` is an object; World checks inside.
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, "preview.jsonc", |catalog| {
            catalog["editing"]["objects"][1]["defaults"] = json!({"settings": {"active": true}});
        });
    });
    let error = session
        .dispatch(
            stamp(
                &session,
                1,
                1,
                vec![object("marker", 0, 0), object("stone", 1, 0)],
            ),
            &mut Records::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::WorldRefused);
    assert!(
        error.to_string().starts_with("placement refused"),
        "{error}"
    );
    assert!(session.history().pending().is_none());
    assert_eq!(session.status().phase, SessionPhase::Ready);
}

#[test]
fn occupied_tiles_warn_and_never_refuse() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    // marker_2 stands on (1, 4); nothing stands on (2, 4) or (3, 4).
    session
        .dispatch(
            stamp(
                &session,
                1,
                4,
                vec![object("stone", 0, 0), object("bush", 1, 0)],
            ),
            &mut records,
        )
        .unwrap();
    assert_eq!(
        session.pending_warnings()[0],
        "Warning: 1 of the 2 new objects lands on a tile that already holds an object or another new one; each is added on top."
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // Three on one empty tile: the second and third land on the first.
    session
        .dispatch(
            stamp(&session, 3, 4, vec![object("bush", 0, 0); 3]),
            &mut records,
        )
        .unwrap();
    assert_eq!(
        session.pending_warnings()[0],
        "Warning: 2 of the 3 new objects land on tiles that already hold an object or another new one; each is added on top."
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // A single member on an occupied tile.
    session
        .dispatch(
            stamp(&session, 1, 4, vec![object("marker", 0, 0)]),
            &mut records,
        )
        .unwrap();
    assert_eq!(
        session.pending_warnings()[0],
        "Warning: 1 of the 1 new object lands on a tile that already holds an object or another new one; each is added on top."
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // No occupied tile: no stamp warning, only the insertion notes.
    session
        .dispatch(
            stamp(
                &session,
                2,
                4,
                vec![object("stone", 0, 0), object("bush", 1, 0)],
            ),
            &mut records,
        )
        .unwrap();
    assert!(
        session
            .pending_warnings()
            .iter()
            .all(|line| !line.starts_with("Warning")),
        "{:?}",
        session.pending_warnings()
    );
    // A single Place keeps its own notes, with no stamp warning.
    session.dispatch(Action::Cancel, &mut records).unwrap();
    session
        .dispatch(
            Action::Place {
                plane: "main".into(),
                layer: 0,
                x: 1,
                y: 4,
                proto: "stone".into(),
                expected: session.history().document().revision(),
            },
            &mut records,
        )
        .unwrap();
    assert_eq!(session.pending_warnings().len(), 1);
    assert!(session.pending_warnings()[0].starts_with("Adds stone"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn a_recorded_stamp_assigns_exactly_its_uids_or_is_refused() {
    let (_dir, mut session) = fixture();
    let mut records = Records::default();
    let recorded = vec![format_uid([1; 16]), format_uid([2; 16])];
    let objects = vec![object("marker", 0, 0), object("stone", 1, 0)];
    session
        .dispatch_placing_objects(
            stamp(&session, 2, 2, objects.clone()),
            &recorded,
            &mut records,
        )
        .unwrap();
    let operations = sealed(&session)["operations"].clone();
    assert_eq!(operations[0]["object"]["uid"], json!(recorded[0]));
    assert_eq!(operations[1]["object"]["uid"], json!(recorded[1]));
    session.dispatch(Action::Cancel, &mut records).unwrap();
    // The session's own source is untouched: the next stamp draws from it.
    session
        .dispatch(
            stamp(&session, 2, 2, vec![object("marker", 0, 0)]),
            &mut records,
        )
        .unwrap();
    assert_eq!(
        sealed(&session)["operations"][0]["object"]["uid"],
        json!(seeded(1)[0])
    );
    session.dispatch(Action::Cancel, &mut records).unwrap();

    let refused = |session: &mut Session, action: &dyn Fn(&Session) -> Action, uids: &[String]| {
        let action = action(session);
        let error = session
            .dispatch_placing_objects(action, uids, &mut Records::default())
            .unwrap_err();
        assert!(session.history().pending().is_none());
        error.code()
    };
    let place = Action::Place {
        plane: "main".into(),
        layer: 0,
        x: 2,
        y: 2,
        proto: "marker".into(),
        expected: session.history().document().revision(),
    };
    assert_eq!(
        refused(&mut session, &|_: &Session| place.clone(), &recorded[..1]),
        RefusalCode::InputInvalid
    );
    for (case, uids) in [
        recorded[..1].to_vec(),
        vec![
            recorded[0].clone(),
            recorded[1].clone(),
            format_uid([3; 16]),
        ],
        vec![recorded[0].clone(), "marker_9".into()],
        vec![recorded[0].clone(), recorded[0].clone()],
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            refused(
                &mut session,
                &|s: &Session| stamp(s, 2, 2, objects.clone()),
                uids
            ),
            RefusalCode::InputInvalid,
            "case {case}"
        );
    }
    // A taken uid is uid.unavailable, never drawn again, and only after
    // the grants and the view.
    let taken = vec![recorded[0].clone(), "o.aaaaaaaaaaaaaaaaaaaaaaaaaa".into()];
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"].as_array_mut().unwrap().push(
                json!({"uid": "o.aaaaaaaaaaaaaaaaaaaaaaaaaa", "at": [0, 0], "proto": "marker", "layer": "item"}),
            );
        });
    });
    assert_eq!(
        refused(
            &mut session,
            &|s: &Session| stamp(s, 2, 2, objects.clone()),
            &taken
        ),
        RefusalCode::UidUnavailable
    );
    assert_eq!(
        refused(
            &mut session,
            &|s: &Session| stamp(s, 2, 2, vec![object("marker", 0, 0), object("grass", 1, 0)]),
            &taken
        ),
        RefusalCode::GrantMissing
    );
    assert_eq!(
        refused(
            &mut session,
            &|s: &Session| stamp(s, 7, 2, objects.clone()),
            &taken
        ),
        RefusalCode::ScopeOutsideView
    );
}

#[test]
fn every_draw_is_unused_in_the_world_and_among_the_stamps_own() {
    /// Material that repeats: zeros twice, then ones, then zeros again,
    /// then twos.
    struct Repeating(Vec<[u8; 16]>);
    impl UidSource for Repeating {
        fn fill(
            &mut self,
            bytes: &mut [u8; 16],
        ) -> Result<(), tilewright_editor_ui::uid::UidError> {
            *bytes = self.0.remove(0);
            Ok(())
        }
    }
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"].as_array_mut().unwrap().push(
                json!({"uid": format_uid([0; 16]), "at": [0, 0], "proto": "marker", "layer": "item"}),
            );
        });
    });
    session.set_uid_source(Box::new(Repeating(vec![
        [0; 16], [1; 16], [0; 16], [1; 16], [2; 16],
    ])));
    session
        .dispatch(
            stamp(
                &session,
                2,
                2,
                vec![object("marker", 0, 0), object("marker", 1, 0)],
            ),
            &mut Records::default(),
        )
        .unwrap();
    let operations = sealed(&session)["operations"].clone();
    // The world's uid is skipped, and so is the one the first member took.
    assert_eq!(operations[0]["object"]["uid"], json!(format_uid([1; 16])));
    assert_eq!(operations[1]["object"]["uid"], json!(format_uid([2; 16])));
}

/// A deterministic sequence for the property test (`SplitMix64`).
struct Draws(u64);
impl Draws {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// An integer from `low` to `high`, both included.
    fn within(&mut self, low: i64, high: i64) -> i64 {
        let span = u64::try_from(high - low + 1).unwrap();
        low + i64::try_from(self.next() % span).unwrap()
    }
}

/// Random stamps: 1 to 64 members, offsets that overlap, touch the view's
/// edges and sometimes leave it, of every granted prototype and some that
/// are not. A refused stamp changes nothing; an accepted one, undone,
/// restores every byte of the world, and the recorded step, prepared again
/// with its uids on the world it started from, gives the same transaction
/// byte for byte.
#[cfg(target_os = "linux")]
#[test]
fn random_stamps_undo_exactly_and_replay_identically() {
    const GRANTED: [&str; 6] = ["bush", "crate", "gate", "lamp", "marker", "stone"];
    let (dir, mut session) = fixture();
    let original = tree(dir.path());
    let mut draws = Draws(0x5eed);
    let mut records = Records::default();
    let (mut accepted, mut outside, mut ungranted) = (0, 0, 0);
    for case in 0..64 {
        let count = usize::try_from(draws.within(1, 64)).unwrap();
        // The view is x 0..=7, y 0..=5. Most stamps keep every member in
        // it (each member a random view tile, the anchor anywhere near),
        // so members overlap and touch the edges; the rest spread freely
        // and mostly leave it.
        let anchor = [draws.within(-2, 9), draws.within(-2, 7)];
        let inside = draws.within(0, 9) < 7;
        let spread = draws.within(0, 4);
        let objects: Vec<StampObject> = (0..count)
            .map(|_| {
                let proto = if draws.within(0, 199) == 0 {
                    "grass"
                } else {
                    GRANTED[usize::try_from(draws.within(0, 5)).unwrap()]
                };
                let offset = if inside {
                    [
                        draws.within(0, 7) - anchor[0],
                        draws.within(0, 5) - anchor[1],
                    ]
                } else {
                    [draws.within(-spread, spread), draws.within(-spread, spread)]
                };
                object(proto, offset[0], offset[1])
            })
            .collect();
        let in_view = objects.iter().all(|object| {
            let [x, y] = [anchor[0] + object.offset[0], anchor[1] + object.offset[1]];
            (0..8).contains(&x) && (0..6).contains(&y)
        });
        let granted = objects.iter().all(|object| object.proto != "grass");
        let action = stamp(&session, anchor[0], anchor[1], objects.clone());
        let result = session.dispatch(action.clone(), &mut records);
        if !granted {
            assert_eq!(
                result.unwrap_err().code(),
                RefusalCode::GrantMissing,
                "case {case}"
            );
            ungranted += 1;
            continue;
        }
        if !in_view {
            assert_eq!(
                result.unwrap_err().code(),
                RefusalCode::ScopeOutsideView,
                "case {case}"
            );
            outside += 1;
            continue;
        }
        result.unwrap_or_else(|error| panic!("case {case}: {error}"));
        let prepared = session.history().pending().unwrap().sealed().to_owned();
        let operations = sealed(&session)["operations"].as_array().unwrap().len();
        assert_eq!(operations, count, "case {case}");
        session.dispatch(Action::Accept, &mut records).unwrap();
        let step = session.applied_steps().last().unwrap();
        assert_eq!(step.transaction, prepared, "case {case}");
        let uids = step.origin.unwrap().placed_objects().to_vec();
        assert_eq!(uids.len(), count, "case {case}");
        assert_ne!(tree(dir.path()), original, "case {case}");
        session.dispatch(Action::Undo, &mut records).unwrap();
        session.dispatch(Action::Accept, &mut records).unwrap();
        assert_eq!(tree(dir.path()), original, "case {case}");
        // Prepared again with its recorded uids on the same world: the
        // same transaction, byte for byte (the same actor here).
        let mut replay = open(dir.path());
        replay
            .dispatch_placing_objects(action, &uids, &mut Records::default())
            .unwrap_or_else(|error| panic!("case {case}: {error}"));
        assert_eq!(
            replay.history().pending().unwrap().sealed(),
            prepared,
            "case {case}"
        );
        accepted += 1;
    }
    assert_eq!(tree(dir.path()), original);
    // The draws reach every outcome.
    assert!(
        accepted >= 8 && outside >= 8 && ungranted >= 1,
        "{accepted} {outside} {ungranted}"
    );
}

#[test]
fn the_revision_is_checked_after_the_grants_and_view_and_before_the_uids() {
    let (_dir, mut session) = fixture();
    let stale = |objects: Vec<StampObject>, x: i64| Action::PlaceObjects {
        plane: "main".into(),
        layer: 0,
        x,
        y: 1,
        objects,
        expected: [7; 32],
    };
    // A stale stamp with an ungranted object, or an object outside the
    // view: the grant and the view are judged first.
    assert_eq!(
        code(
            &mut session,
            stale(vec![object("marker", 0, 0), object("grass", 1, 0)], 1)
        ),
        RefusalCode::GrantMissing
    );
    assert_eq!(
        code(
            &mut session,
            stale(vec![object("marker", 0, 0), object("marker", 1, 0)], 7)
        ),
        RefusalCode::ScopeOutsideView
    );
    // A stale replay whose recorded uid the World already holds: the
    // revision is judged before any uid.
    let taken = "o.aaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
    let (_dir, mut session) = fixture_with(|root| {
        edit_json(root, CHUNK, |chunk| {
            chunk["objects"].as_array_mut().unwrap().push(
                json!({"uid": "o.aaaaaaaaaaaaaaaaaaaaaaaaaa", "at": [0, 0], "proto": "marker", "layer": "item"}),
            );
        });
    });
    let uids = vec![format_uid([1; 16]), taken];
    let objects = vec![object("marker", 0, 0), object("stone", 1, 0)];
    let error = session
        .dispatch_placing_objects(stale(objects.clone(), 2), &uids, &mut Records::default())
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::RevisionStale);
    assert!(session.history().pending().is_none());
    // Current, the same replay reaches the uids.
    let error = session
        .dispatch_placing_objects(
            stamp(&session, 2, 1, objects),
            &uids,
            &mut Records::default(),
        )
        .unwrap_err();
    assert_eq!(error.code(), RefusalCode::UidUnavailable);
}

#[test]
fn only_objects_on_the_stamps_spatial_layer_occupy_its_tiles() {
    // An object on spatial layer 1 at (3, 4), inside the view's x/y
    // bounds; nothing stands on (3, 4) of layer 0.
    let (_dir, mut session) = fixture_with(|root| {
        let upper = json!({"format": "tilewright.chunk/1", "plane": "main", "layer": 1, "chunk": [0, 0],
            "objects": [{"uid": "upper_1", "at": [3, 4], "proto": "marker", "layer": "item"}]});
        fs::create_dir_all(root.join("planes/main/layers/1")).unwrap();
        fs::write(
            root.join("planes/main/layers/1/0_0.jsonc"),
            serde_json::to_vec_pretty(&upper).unwrap(),
        )
        .unwrap();
    });
    let objects = vec![object("bush", 0, 0)];
    let world = session.world();
    assert_eq!(
        tilewright_editor_ui::stamp::occupied(world, "main", 0, [3, 4], &objects),
        0
    );
    assert_eq!(
        tilewright_editor_ui::stamp::occupied(world, "main", 1, [3, 4], &objects),
        1
    );
    session
        .dispatch(stamp(&session, 3, 4, objects), &mut Records::default())
        .unwrap();
    assert!(
        session
            .pending_warnings()
            .iter()
            .all(|line| !line.starts_with("Warning")),
        "{:?}",
        session.pending_warnings()
    );
}
