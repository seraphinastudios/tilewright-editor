// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The map highlight of an open proposal's current step: which tiles, from
//! the decoded Action and the person's committed world, how they are drawn
//! and what the frame evidence says, on the CPU scene path without a window.
use super::proposal::{
    AGENT, KINDS, RECEIPT, Twin, accept, edit_json, evidence, open, open_session,
    person_application, prepare_and_accept, press_control, revision, scratch, state, twin,
};
use super::zone_manage::enter;
use super::zones::{button, key, route};
use super::{acknowledge_ui, cell_pixel, copy_private};
use crate::prototype::Application;
use crate::prototype::proposal_view::{
    Concerns, FRAME_TILES, HIGHLIGHT_FROM_TINT, HIGHLIGHT_TILES, HIGHLIGHT_TINT, Highlight, State,
};
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use tilewright_editor_preview::SelectionKey;
use tilewright_editor_proposal::Proposal;
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, PortalTarget};
use tilewright_render_api::{RenderRequest, Renderer};
use tilewright_render_testkit::{ReferenceRenderer, offscreen_target};
use tilewright_scene::SemanticId;

/// A highlighted tile as the tests compare it: role, plane, layer, tile and
/// whether it is in view.
type Seen = (&'static str, String, i32, [i64; 2], bool);

/// Whether `(plane, layer, at)` is in the neutral fixture's view: plane
/// `main`, layer 0, tiles 0..8 by 0..6 (its `preview.jsonc`), worked out
/// here rather than through the catalog.
fn fixture_view(plane: &str, layer: i32, [x, y]: [i64; 2]) -> bool {
    plane == "main" && layer == 0 && (0..8).contains(&x) && (0..6).contains(&y)
}

fn tiles(highlight: &Highlight) -> Vec<Seen> {
    highlight
        .tiles
        .iter()
        .map(|tile| {
            (
                tile.role.key(),
                tile.plane.clone(),
                tile.layer,
                tile.at,
                tile.in_view,
            )
        })
        .collect()
}

/// `(role, tile)` on main/0, with the view worked out independently.
fn main0(expected: &[(&'static str, [i64; 2])]) -> Vec<Seen> {
    expected
        .iter()
        .map(|(role, at)| {
            (
                *role,
                "main".to_owned(),
                0,
                *at,
                fixture_view("main", 0, *at),
            )
        })
        .collect()
}

/// The tints the highlight should draw: its in-view tiles, a `from` tile
/// lighter.
fn expected_tints(seen: &[Seen]) -> BTreeMap<[i64; 2], [f32; 4]> {
    seen.iter()
        .filter(|tile| tile.4)
        .map(|(role, _, _, at, _)| {
            (
                *at,
                if *role == "from" {
                    HIGHLIGHT_FROM_TINT
                } else {
                    HIGHLIGHT_TINT
                },
            )
        })
        .collect()
}

/// What the overlay draws now, by tile: each copy's cell and colour, read
/// back through the prepared cell picks. Every copy is unpickable.
fn drawn(app: &Application) -> BTreeMap<[i64; 2], [f32; 4]> {
    let mut found = BTreeMap::new();
    for quad in crate::prototype::proposal_overlay(app) {
        assert!(!quad.pickable);
        let Some(SelectionKey::Cell { x, y, .. }) = app.prepared.cell_picks.get(&quad.semantic_id)
        else {
            panic!("a highlight copy of no cell");
        };
        let previous = found.insert([*x, *y], quad.color);
        assert!(
            previous.is_none_or(|color| color.map(f32::to_bits) == quad.color.map(f32::to_bits))
        );
    }
    found
}

/// The highlight-tinted quads of the frame the native loop would present.
fn presented(app: &mut Application) -> usize {
    app.refresh_ui();
    crate::prototype::workspace_presentation(app)
        .unwrap()
        .scene
        .quads()
        .iter()
        .filter(|quad| {
            !quad.pickable && (quad.color == HIGHLIGHT_TINT || quad.color == HIGHLIGHT_FROM_TINT)
        })
        .count()
}

/// The stored highlight equals one derived afresh from the state now.
fn assert_fresh(app: &mut Application) {
    let Some(open) = app.proposal.as_mut() else {
        return;
    };
    let kept = open.highlight.take();
    app.refresh_highlight();
    assert_eq!(app.proposal.as_ref().unwrap().highlight, kept);
}

/// The step, what it concerns and its tiles, of the highlight shown now.
fn shown(app: &mut Application) -> Option<(usize, Concerns, Vec<Seen>)> {
    assert_fresh(app);
    let highlight = app.proposal_highlight()?;
    Some((highlight.step, highlight.concerns, tiles(highlight)))
}

/// What step `step` (from 0) of the 15 kinds highlights on the person's
/// world as each step finds it.
fn expected_kind(step: usize) -> (Concerns, Vec<Seen>) {
    let tiles = match step {
        0 => main0(&[("to", [4, 2]), ("from", [3, 2])]),
        // marker_1 is at (4, 2) after the move.
        1 => main0(&[("object", [4, 2])]),
        2 => main0(&[("cell", [0, 0])]),
        3 => main0(&[("cell", [0, 1]), ("cell", [1, 1])]),
        4 => main0(&[("cell", [6, 0])]),
        5 => main0(&[("object", [1, 4])]),
        6 => main0(&[("target", [2, 2])]),
        7..=10 => return (Concerns::NoTiles, Vec::new()),
        11 => main0(&[("anchor", [2, 2])]),
        12 => main0(&[("to", [6, 3]), ("from", [7, 3])]),
        // Outside the view: listed, not drawn.
        13 => main0(&[("anchor", [-2, 0])]),
        // gate_west's tile, and arrival where step 12 moved it.
        _ => main0(&[("object", [0, 3]), ("target", [6, 3])]),
    };
    (Concerns::Tiles, tiles)
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_kind_highlights_its_tiles_before_prepare_and_while_pending() {
    let mut twin = twin(KINDS, |_| {}, |_| {}, None);
    // Without a proposal: nothing.
    assert!(twin.app.proposal_highlight().is_none());
    assert!(drawn(&twin.app).is_empty());
    assert_eq!(presented(&mut twin.app), 0);
    open(&mut twin);
    let mut drawn_somewhere = 0;
    for step in 0..KINDS {
        let (concerns, expected) = expected_kind(step);
        let tints = expected_tints(&expected);
        assert_eq!(state(&twin.app), State::Ready { step });
        assert_eq!(
            shown(&mut twin.app),
            Some((step, concerns, expected.clone())),
            "step {} before prepare",
            step + 1
        );
        assert_eq!(drawn(&twin.app), tints, "step {} drawn", step + 1);
        // Composed into the presented frame: one copy per in-view tile on
        // this flat top-down fixture.
        assert_eq!(presented(&mut twin.app), tints.len());
        if step == 0 {
            twin.app.capture("highlight-ready").unwrap();
            assert_eq!(
                evidence(&twin, "highlight-ready.json")["proposal"]["highlight"],
                json!({"step": 1, "concerns": "tiles", "in_view": 2, "outside_view": 0,
                "tiles": [
                    {"role": "to", "plane": "main", "layer": 0, "x": 4, "y": 2, "in_view": true},
                    {"role": "from", "plane": "main", "layer": 0, "x": 3, "y": 2, "in_view": true},
                ]})
            );
        }
        if step == 13 {
            twin.app.capture("highlight-outside").unwrap();
            assert_eq!(
                evidence(&twin, "highlight-outside.json")["proposal"]["highlight"],
                json!({"step": 14, "concerns": "tiles", "in_view": 0, "outside_view": 1,
                    "tiles": [{"role": "anchor", "plane": "main", "layer": 0, "x": -2, "y": 0,
                        "in_view": false}]})
            );
        }
        if step == 7 {
            twin.app.capture("highlight-zone").unwrap();
            assert_eq!(
                evidence(&twin, "highlight-zone.json")["proposal"]["highlight"],
                json!({"step": 8, "concerns": "no tiles", "in_view": 0, "outside_view": 0,
                    "tiles": []})
            );
        }
        press_control(&mut twin.app);
        assert_eq!(state(&twin.app), State::Pending { step });
        // While pending the pending World is drawn, and the highlight marks
        // the same tiles on it.
        assert_eq!(
            shown(&mut twin.app),
            Some((step, concerns, expected)),
            "step {} pending",
            step + 1
        );
        assert_eq!(drawn(&twin.app), tints, "step {} drawn pending", step + 1);
        drawn_somewhere += tints.len();
        enter(&mut twin.app);
        assert_eq!(
            revision(&twin.app.session),
            twin.proposal.steps()[step].after()
        );
    }
    assert!(drawn_somewhere >= 12);
    assert_eq!(state(&twin.app), State::Finished);
    assert_eq!(shown(&mut twin.app), None);
    assert!(drawn(&twin.app).is_empty());
    assert_eq!(presented(&mut twin.app), 0);
}

#[test]
#[allow(clippy::too_many_lines)]
fn tiles_on_another_layer_outside_the_view_or_not_placed_are_not_drawn() {
    let (_person, app) = person_application(|root| {
        edit_json(root, "planes/main/anchors.jsonc", |anchors| {
            anchors["anchors"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": "far", "layer": 0, "at": [400, 3]}));
        });
    });
    let expected = revision(&app.session);
    let of = |action: Action| {
        let highlight = Highlight::of(4, &action, &app.session);
        assert_eq!(highlight.step, 4);
        assert_eq!(highlight.revision, expected);
        // The tints are exactly the in-view tiles', by role.
        assert_eq!(highlight.tints, expected_tints(&tiles(&highlight)));
        (highlight.concerns, tiles(&highlight))
    };
    let main = || String::from("main");
    let not_placed = (Concerns::NotPlaced, Vec::new());
    assert_eq!(
        of(Action::Move {
            uid: "missing".into(),
            x: 1,
            y: 1,
            expected
        }),
        not_placed
    );
    assert_eq!(
        of(Action::Remove {
            uid: "missing".into(),
            expected
        }),
        not_placed
    );
    assert_eq!(
        of(Action::MoveAnchor {
            plane: main(),
            anchor: "gone".into(),
            x: 1,
            y: 1,
            expected
        }),
        not_placed
    );
    assert_eq!(
        of(Action::SetPortal {
            uid: "missing".into(),
            target: None,
            expected
        }),
        not_placed
    );
    // A move onto its own tile names the tile once, as its destination.
    assert_eq!(
        of(Action::Move {
            uid: "marker_1".into(),
            x: 3,
            y: 2,
            expected
        }),
        (Concerns::Tiles, main0(&[("to", [3, 2])]))
    );
    assert_eq!(
        of(Action::DeleteAnchor {
            plane: main(),
            anchor: "far".into(),
            expected
        }),
        (Concerns::Tiles, main0(&[("anchor", [400, 3])]))
    );
    assert_eq!(
        of(Action::MoveAnchor {
            plane: main(),
            anchor: "far".into(),
            x: 2,
            y: 1,
            expected
        }),
        (
            Concerns::Tiles,
            main0(&[("to", [2, 1]), ("from", [400, 3])])
        )
    );
    // A portal leading to another layer: its target is listed, not drawn.
    assert_eq!(
        of(Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Location {
                plane: main(),
                layer: 1,
                at: [2, 3]
            }),
            expected
        }),
        (
            Concerns::Tiles,
            vec![
                ("object", main(), 0, [0, 3], true),
                ("target", main(), 1, [2, 3], false),
            ]
        )
    );
    assert_eq!(
        of(Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Location {
                plane: "other".into(),
                layer: 0,
                at: [2, 3]
            }),
            expected
        }),
        (
            Concerns::Tiles,
            vec![
                ("object", main(), 0, [0, 3], true),
                ("target", "other".into(), 0, [2, 3], false),
            ]
        )
    );
    assert_eq!(
        of(Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Anchor {
                plane: main(),
                anchor: "far".into()
            }),
            expected
        }),
        (
            Concerns::Tiles,
            main0(&[("object", [0, 3]), ("target", [400, 3])])
        )
    );
    // An anchor target that is not in this world: the object only.
    assert_eq!(
        of(Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Anchor {
                plane: main(),
                anchor: "gone".into()
            }),
            expected
        }),
        (Concerns::Tiles, main0(&[("object", [0, 3])]))
    );
    assert_eq!(
        of(Action::Paint {
            plane: main(),
            layer: 1,
            grid: "floor".into(),
            x: 2,
            y: 2,
            proto: "sand".into(),
            expected
        }),
        (Concerns::Tiles, vec![("cell", main(), 1, [2, 2], false)])
    );
    // Repeated cells count once; the first role is kept.
    assert_eq!(
        of(Action::PaintZones {
            plane: main(),
            layer: 0,
            cells: vec![[1, 1], [900, 1], [1, 1], [7, 5], [8, 5], [0, -1]],
            zone: None,
            expected
        }),
        (
            Concerns::Tiles,
            main0(&[
                ("cell", [1, 1]),
                ("cell", [900, 1]),
                ("cell", [7, 5]),
                ("cell", [8, 5]),
                ("cell", [0, -1]),
            ])
        )
    );
    assert_eq!(
        of(Action::Place {
            plane: main(),
            layer: 0,
            x: 7,
            y: 6,
            proto: "marker".into(),
            expected
        }),
        (Concerns::Tiles, main0(&[("target", [7, 6])]))
    );
    assert_eq!(
        of(Action::CreateAnchor {
            plane: main(),
            anchor: "new".into(),
            x: -1,
            y: 5,
            expected
        }),
        (Concerns::Tiles, main0(&[("anchor", [-1, 5])]))
    );
    for action in [
        Action::RenameZone {
            plane: main(),
            zone: "path".into(),
            name: None,
            expected,
        },
        Action::SetZoneParent {
            plane: main(),
            zone: "path".into(),
            parent: None,
            expected,
        },
        Action::CreateZone {
            plane: main(),
            zone: "camp".into(),
            name: None,
            parent: None,
            expected,
        },
        Action::DeleteZone {
            plane: main(),
            zone: "path".into(),
            expected,
        },
    ] {
        assert_eq!(of(action), (Concerns::NoTiles, Vec::new()));
    }
}

#[test]
fn the_highlight_is_bounded_and_the_frame_lists_the_first_few() {
    let (_person, app) = person_application(|_| {});
    let expected = revision(&app.session);
    // More cells than the bound, the first 48 of them in view.
    let cells: Vec<[i64; 2]> = (0..64).flat_map(|y| (0..80).map(move |x| [x, y])).collect();
    assert!(cells.len() > HIGHLIGHT_TILES);
    let highlight = Highlight::of(
        0,
        &Action::PaintStroke {
            plane: "main".into(),
            layer: 0,
            grid: "floor".into(),
            cells: cells.clone(),
            proto: "sand".into(),
            expected,
        },
        &app.session,
    );
    assert_eq!(highlight.tiles.len(), HIGHLIGHT_TILES);
    let kept: Vec<[i64; 2]> = highlight.tiles.iter().map(|tile| tile.at).collect();
    assert_eq!(kept, cells[..HIGHLIGHT_TILES]);
    let in_view = cells[..HIGHLIGHT_TILES]
        .iter()
        .filter(|at| fixture_view("main", 0, **at))
        .count();
    assert_eq!(in_view, 48);
    assert_eq!(highlight.tints.len(), in_view);
    let frame = highlight.frame();
    assert_eq!(frame["in_view"], in_view);
    assert_eq!(frame["outside_view"], HIGHLIGHT_TILES - in_view);
    let listed = frame["tiles"].as_array().unwrap();
    assert_eq!(listed.len(), FRAME_TILES);
    for (index, tile) in listed.iter().enumerate() {
        let index = i64::try_from(index).unwrap();
        assert_eq!(
            tile,
            &json!({"role": "cell", "plane": "main", "layer": 0, "x": index, "y": 0,
                "in_view": index < 8})
        );
    }
}

/// The agent's twin world with `actions` accepted, as a proposal, and the
/// person's application on the neutral fixture with it open.
fn proposal_of(actions: &[fn([u8; 32]) -> Action]) -> Twin {
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    let mut session = open_session(&world, AGENT);
    for action in actions {
        let expected = revision(&session);
        accept(&mut session, action(expected));
    }
    let proposal = Proposal::from_session(&session, "0.1.0", RECEIPT, None).unwrap();
    let (person, app) = person_application(|_| {});
    let mut twin = Twin {
        person,
        agent: agent_root,
        app,
        proposal,
    };
    open(&mut twin);
    twin
}

fn stroke(expected: [u8; 32]) -> Action {
    Action::PaintStroke {
        plane: "main".into(),
        layer: 0,
        grid: "floor".into(),
        cells: vec![[0, 1], [1, 1], [2, 1]],
        proto: "sand".into(),
        expected,
    }
}

#[test]
fn the_persons_own_brush_preview_wins_on_a_shared_cell() {
    let mut twin = proposal_of(&[stroke]);
    let app = &mut twin.app;
    let all = expected_tints(&main0(&[
        ("cell", [0, 1]),
        ("cell", [1, 1]),
        ("cell", [2, 1]),
    ]));
    assert_eq!(drawn(app), all);
    app.control(Control::Terrain).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    acknowledge_ui(app);
    // A one-cell stroke on (0, 1), held down: its release would change it.
    let at = cell_pixel(app, 0, 1);
    route(app, &button(true, at)).unwrap();
    assert_eq!(
        app.session
            .stroke_changes(app.tools.stroke().unwrap())
            .unwrap(),
        [[0, 1]]
    );
    assert_eq!(state(app), State::Ready { step: 0 });
    let mut rest = all.clone();
    rest.remove(&[0, 1]);
    assert_eq!(drawn(app), rest, "the preview's cell is left to it");
    app.refresh_ui();
    let composed = crate::prototype::workspace_presentation(app).unwrap();
    let white = composed
        .scene
        .quads()
        .iter()
        .filter(|quad| quad.color == crate::workspace_render::STROKE_PREVIEW_TINT)
        .count();
    assert_eq!(white, 1, "the brush preview is drawn on (0, 1)");
    assert_eq!(presented(app), 2);
    // Esc ends the stroke; the highlight takes the cell again.
    route(app, &key(Keycode::Escape)).unwrap();
    assert!(app.tools.stroke().is_none());
    assert_eq!(drawn(app), all);
}

#[test]
#[allow(clippy::too_many_lines)]
fn the_highlight_follows_the_state() {
    let mut twin = twin(3, |_| {}, |_| {}, None);
    open(&mut twin);
    let step = |app: &mut Application| shown(app).map(|(step, _, _)| step);
    assert_eq!(step(&mut twin.app), Some(0));
    prepare_and_accept(&mut twin, 0);
    // The next step after an Accept.
    assert_eq!(step(&mut twin.app), Some(1));
    assert_eq!(drawn(&twin.app), expected_tints(&expected_kind(1).1));
    // A selection changes nothing.
    let at = cell_pixel(&mut twin.app, 5, 5);
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &button(true, at)).unwrap();
    route(&mut twin.app, &button(false, at)).unwrap();
    assert_eq!(step(&mut twin.app), Some(1));
    // Cancel stops the proposal: nothing is highlighted until it resumes.
    press_control(&mut twin.app);
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::Escape)).unwrap();
    assert_eq!(state(&twin.app), State::Stopped { resume: Some(1) });
    assert_eq!(step(&mut twin.app), None);
    assert!(drawn(&twin.app).is_empty());
    press_control(&mut twin.app);
    assert_eq!(state(&twin.app), State::Ready { step: 1 });
    assert_eq!(step(&mut twin.app), Some(1));
    // An undone step is highlighted again.
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::Z)).unwrap();
    enter(&mut twin.app);
    assert_eq!(state(&twin.app), State::Undone { step: 0 });
    assert_eq!(step(&mut twin.app), Some(0));
    assert_eq!(drawn(&twin.app), expected_tints(&expected_kind(0).1));
    // Redo continues; then the person's own edit holds the proposal.
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::Y)).unwrap();
    enter(&mut twin.app);
    assert_eq!(step(&mut twin.app), Some(1));
    twin.app
        .action(Action::Move {
            uid: "marker_2".into(),
            x: 2,
            y: 5,
            expected: revision(&twin.app.session),
        })
        .unwrap();
    assert_eq!(state(&twin.app), State::OtherPending);
    assert_eq!(step(&mut twin.app), None);
    assert!(drawn(&twin.app).is_empty());
    enter(&mut twin.app);
    assert!(matches!(state(&twin.app), State::Moved { .. }));
    assert_eq!(step(&mut twin.app), None);
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::Z)).unwrap();
    enter(&mut twin.app);
    assert_eq!(step(&mut twin.app), Some(1));
    // A blocked session shows nothing, though the stored one is kept.
    twin.app.blocked = Some("test".into());
    assert!(twin.app.proposal_highlight().is_none());
    assert!(drawn(&twin.app).is_empty());
    twin.app.blocked = None;
    assert_eq!(step(&mut twin.app), Some(1));
    // A proposal that does not apply highlights nothing.
    let mut other = super::proposal::twin(
        1,
        |root| {
            edit_json(root, "planes/main/zones.jsonc", |zones| {
                zones["zones"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"id": "elsewhere"}));
            });
        },
        |_| {},
        None,
    );
    open(&mut other);
    assert_eq!(state(&other.app), State::DoesNotApply);
    assert_eq!(step(&mut other.app), None);
    assert!(drawn(&other.app).is_empty());
    other.app.capture("does-not-apply").unwrap();
    assert_eq!(
        evidence(&other, "does-not-apply.json")["proposal"]["highlight"],
        json!(null)
    );
}

fn render(
    presentation: crate::workspace_render::Presentation,
) -> tilewright_render_api::FrameOutput {
    ReferenceRenderer::new(offscreen_target(
        presentation.scene.viewport().width,
        presentation.scene.viewport().height,
    ))
    .unwrap()
    .render(RenderRequest {
        scene: &presentation.scene,
        resources: presentation.resources,
    })
    .unwrap()
}

#[test]
fn the_highlight_changes_only_its_tiles_pixels_and_never_what_a_click_picks() {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    let app = &mut twin.app;
    // Zones and badges shown, so the highlight is checked over their tints.
    acknowledge_ui(app);
    route(app, &key(Keycode::O)).unwrap();
    assert!(app.tools.zones_visible());
    for pending in [false, true] {
        app.refresh_ui();
        let with = render(crate::prototype::workspace_presentation(app).unwrap());
        // The same frame with the highlight taken out: what main draws.
        let kept = app.proposal.as_mut().unwrap().highlight.take();
        assert!(crate::prototype::proposal_overlay(app).is_empty());
        let without = render(crate::prototype::workspace_presentation(app).unwrap());
        app.proposal.as_mut().unwrap().highlight = kept;
        let marked: BTreeSet<u32> = app
            .prepared
            .cell_picks
            .iter()
            .filter(
                |(_, key)| matches!(key, SelectionKey::Cell { x, y: 2, .. } if *x == 3 || *x == 4),
            )
            .map(|(id, _)| id.0)
            .collect();
        assert_eq!(marked.len(), 2);
        let cells = app.frame(true).unwrap();
        let mask = app.prepared.mask(app.view.frame_revision, true).unwrap();
        let mut changed_cells = BTreeSet::new();
        for (index, id) in cells.semantic_ids.iter().enumerate() {
            let changed =
                with.rgba8[index * 4..index * 4 + 4] != without.rgba8[index * 4..index * 4 + 4];
            let (x, y) = (
                u32::try_from(index % 1280).unwrap(),
                u32::try_from(index / 1280).unwrap(),
            );
            if !marked.contains(id) {
                assert!(!changed, "pixel {x},{y} outside the highlight");
                continue;
            }
            assert_eq!(
                mask.pick(x, y).unwrap(),
                app.prepared.cell_picks.get(&SemanticId(*id)).cloned()
            );
            if changed {
                changed_cells.insert(*id);
            }
        }
        assert_eq!(changed_cells, marked, "pending: {pending}");
        assert_eq!(with.semantic_ids, without.semantic_ids);
        assert_eq!(with.depth, without.depth);
        assert_eq!(with.coverage, without.coverage);
        if !pending {
            // A click on a highlighted tile selects what it holds: the
            // destination is an empty cell, the source holds marker_1.
            for (x, expected) in [
                (
                    4,
                    SelectionKey::Cell {
                        plane: "main".into(),
                        layer: 0,
                        grid: "floor".into(),
                        x: 4,
                        y: 2,
                    },
                ),
                (
                    3,
                    SelectionKey::Object {
                        uid: "marker_1".into(),
                    },
                ),
            ] {
                let picked = app
                    .frame(false)
                    .unwrap()
                    .semantic_ids
                    .iter()
                    .enumerate()
                    .find_map(|(index, id)| {
                        let key = app.prepared.picks.get(&SemanticId(*id))?;
                        (key == &expected).then_some(index)
                    })
                    .unwrap();
                let at = [
                    f32::from(u16::try_from(picked % 1280).unwrap()),
                    f32::from(u16::try_from(picked / 1280).unwrap()),
                ];
                assert!(
                    app.proposal_highlight()
                        .unwrap()
                        .tints
                        .contains_key(&[x, 2])
                );
                acknowledge_ui(app);
                route(app, &button(true, at)).unwrap();
                route(app, &button(false, at)).unwrap();
                assert_eq!(app.session.selection(), Some(&expected));
            }
            acknowledge_ui(app);
            route(app, &key(Keycode::Escape)).unwrap();
            press_control(app);
            assert_eq!(state(app), State::Pending { step: 0 });
        }
    }
}
