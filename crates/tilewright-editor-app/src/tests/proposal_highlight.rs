// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The map highlight of an open proposal's current step: which tiles, from
//! the decoded Action and the person's committed world, how they are drawn
//! and what the frame evidence says, on the CPU scene path without a window.
use super::proposal::{
    AGENT, KINDS, RECEIPT, Twin, accept, edit_json, evidence, forged, open, open_session,
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

/// What step `step` (from 0) of the 17 kinds highlights on the person's
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
        14 => main0(&[("object", [0, 3]), ("target", [6, 3])]),
        // The start cell, then the sand block east of x = 4 in x, then y
        // order, less the view's empty cell.
        15 => {
            let mut cells = vec![("cell", [5, 0])];
            for x in 4..8 {
                for y in 0..6 {
                    if [x, y] != [5, 0] && [x, y] != [6, 4] {
                        cells.push(("cell", [x, y]));
                    }
                }
            }
            main0(&cells)
        }
        _ => main0(&[("cell", [6, 4])]),
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
                json!({"step": 1, "concerns": "tiles", "in_view": 2, "outside_view": 0, "left_out": false,
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
                json!({"step": 14, "concerns": "tiles", "in_view": 0, "outside_view": 1, "left_out": false,
                    "tiles": [{"role": "anchor", "plane": "main", "layer": 0, "x": -2, "y": 0,
                        "in_view": false}]})
            );
        }
        if step == 7 {
            twin.app.capture("highlight-zone").unwrap();
            assert_eq!(
                evidence(&twin, "highlight-zone.json")["proposal"]["highlight"],
                json!({"step": 8, "concerns": "no tiles", "in_view": 0, "outside_view": 0, "left_out": false,
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
    // A fill that would change nothing, or starts outside the view, names
    // only its start cell; one that would names the start cell first.
    let fill = |x: i64, proto: &str, contiguous: bool| Action::FillTerrain {
        plane: main(),
        layer: 0,
        grid: "floor".into(),
        x,
        y: 0,
        proto: proto.into(),
        contiguous,
        expected,
    };
    assert_eq!(
        of(fill(0, "grass", true)),
        (Concerns::Tiles, main0(&[("cell", [0, 0])]))
    );
    assert_eq!(
        of(fill(100, "sand", false)),
        (Concerns::Tiles, main0(&[("cell", [100, 0])]))
    );
    let (_, replaced) = of(fill(3, "sand", false));
    assert_eq!(replaced.len(), 24);
    assert_eq!(replaced[0].3, [3, 0]);
    assert_eq!(replaced[1].3, [0, 0]);
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
            y: 3,
            proto: "sand".into(),
            expected
        }),
        (Concerns::Tiles, vec![("cell", main(), 1, [2, 3], false)])
    );
    // x and y in their places: an asymmetric in-view tile.
    assert_eq!(
        of(Action::Paint {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            x: 2,
            y: 3,
            proto: "sand".into(),
            expected
        }),
        (Concerns::Tiles, main0(&[("cell", [2, 3])]))
    );
    assert_eq!(
        of(Action::Place {
            plane: main(),
            layer: 0,
            x: 1,
            y: 5,
            proto: "marker".into(),
            expected
        }),
        (Concerns::Tiles, main0(&[("target", [1, 5])]))
    );
    assert_eq!(
        of(Action::CreateAnchor {
            plane: main(),
            anchor: "new".into(),
            x: 6,
            y: 1,
            expected
        }),
        (Concerns::Tiles, main0(&[("anchor", [6, 1])]))
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
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
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
    // Nothing is drawn on a prepared scene of another frame.
    app.view.frame_revision += 1;
    assert!(drawn(app).is_empty());
    app.view.frame_revision -= 1;
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
    twin.app.blocked = Some(crate::prototype::Blocked::new(
        crate::prototype::BlockCause::View,
        "test",
    ));
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
#[allow(clippy::too_many_lines)]
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
        let composed = crate::prototype::workspace_presentation(app).unwrap();
        // A highlight copy and a zone tint in one World slot: the highlight
        // draws after (copies in a slot draw in identity order).
        let is_highlight = |quad: &tilewright_scene::Quad| {
            !quad.pickable && (quad.color == HIGHLIGHT_TINT || quad.color == HIGHLIGHT_FROM_TINT)
        };
        let is_zone = |quad: &tilewright_scene::Quad| {
            !quad.pickable
                && (0..tilewright_editor_ui::zones::TINTS.len()).any(|tint| {
                    quad.color.map(f32::to_bits)
                        == tilewright_editor_ui::zones::tint_color(tint).map(f32::to_bits)
                })
        };
        let mut shared = 0;
        for over in composed
            .scene
            .quads()
            .iter()
            .filter(|quad| is_highlight(quad))
        {
            for under in composed.scene.quads().iter().filter(|quad| {
                is_zone(quad)
                    && (quad.depth, quad.painter_order) == (over.depth, over.painter_order)
            }) {
                assert!(over.semantic_id.0 > under.semantic_id.0);
                shared += 1;
            }
        }
        assert!(shared > 0, "a highlighted tile has a zone tint");
        let with = render(composed);
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

#[test]
fn a_refused_step_stays_highlighted() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    twin.proposal = forged(&twin.proposal, |body| {
        body["steps"][1]["action"]["value"] = json!("Other");
    });
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    acknowledge_ui(&mut twin.app);
    assert!(route(&mut twin.app, &key(Keycode::G)).is_err());
    assert!(matches!(state(&twin.app), State::Refused { step: 1, .. }));
    let (concerns, expected) = expected_kind(1);
    assert_eq!(shown(&mut twin.app), Some((1, concerns, expected.clone())));
    assert_eq!(drawn(&twin.app), expected_tints(&expected));
}

/// The person's own overlays now: zone tints, badges and the brush preview.
fn person_overlay(app: &Application) -> Vec<tilewright_scene::Quad> {
    let mut overlay = crate::prototype::zone_overlay(app);
    overlay.extend(crate::prototype::badge_overlay(app));
    overlay.extend(crate::prototype::stroke_overlay(app));
    overlay
}

/// Frame limits with room for the World, the interface as laid out with
/// the highlight reported left out, and the person's own overlays, but not
/// for the highlight too.
fn limits_without_highlight(app: &mut Application) {
    app.highlight_omitted = true;
    app.refresh_ui();
    let person = person_overlay(app);
    let fits = crate::workspace_render::compose_with_overlay(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
        &person,
        tilewright_scene::SceneLimits::default(),
    )
    .unwrap();
    app.frame_limits = tilewright_scene::SceneLimits {
        maximum_quads: fits.scene.quads().len(),
        ..tilewright_scene::SceneLimits::default()
    };
}

fn count(app: &mut Application, wanted: impl Fn(&tilewright_scene::Quad) -> bool) -> usize {
    app.refresh_ui();
    crate::prototype::workspace_presentation(app)
        .unwrap()
        .scene
        .quads()
        .iter()
        .filter(|quad| !quad.pickable && wanted(quad))
        .count()
}

fn zone_tinted(quad: &tilewright_scene::Quad) -> bool {
    (0..tilewright_editor_ui::zones::TINTS.len()).any(|tint| {
        quad.color.map(f32::to_bits)
            == tilewright_editor_ui::zones::tint_color(tint).map(f32::to_bits)
    })
}

#[test]
fn the_highlight_gives_way_first_and_never_takes_the_review_status() {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    press_control(&mut twin.app);
    let app = &mut twin.app;
    assert_eq!(state(app), State::Pending { step: 0 });
    acknowledge_ui(app);
    route(app, &key(Keycode::O)).unwrap();
    let status = app.status_label();
    assert!(status.contains("matches its recording"), "{status}");
    let zones = count(app, zone_tinted);
    let badges = count(app, |quad| {
        quad.color == crate::workspace_render::BADGE_LIGHT
    });
    assert!(zones > 0 && badges > 0);
    assert!(!app.highlight_omitted);
    limits_without_highlight(app);
    assert_eq!(presented(app), 0, "the highlight is left out");
    assert!(app.highlight_omitted);
    assert!(!app.preview_omitted, "the person's overlays stay");
    assert_eq!(count(app, zone_tinted), zones);
    assert_eq!(
        count(app, |quad| quad.color
            == crate::workspace_render::BADGE_LIGHT),
        badges
    );
    // The Review status is the pending step's own; the Proposal block says
    // the highlight was left out, and so does the frame evidence.
    assert_eq!(app.status_label(), status);
    assert!(
        super::zones::sidebar_texts(app)
            .iter()
            .any(|text| text.starts_with("Highlight left out")),
    );
    app.capture("left-out").unwrap();
    assert_eq!(
        evidence(&twin, "left-out.json")["proposal"]["highlight"]["left_out"],
        json!(true)
    );
    // With room again it is drawn and the notice goes.
    let app = &mut twin.app;
    app.frame_limits = tilewright_scene::SceneLimits::default();
    assert_eq!(presented(app), 2);
    assert!(!app.highlight_omitted);
    assert!(
        !super::zones::sidebar_texts(app)
            .iter()
            .any(|text| text.starts_with("Highlight left out")),
    );
}

#[test]
fn a_left_out_highlight_keeps_the_persons_brush_preview() {
    let mut twin = proposal_of(&[stroke]);
    let app = &mut twin.app;
    app.control(Control::Terrain).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    acknowledge_ui(app);
    // A stroke on (3, 3), a grass cell the step does not paint.
    let at = cell_pixel(app, 3, 3);
    route(app, &button(true, at)).unwrap();
    let white =
        |quad: &tilewright_scene::Quad| quad.color == crate::workspace_render::STROKE_PREVIEW_TINT;
    assert_eq!(count(app, white), 1);
    assert_eq!(presented(app), 3);
    limits_without_highlight(app);
    assert_eq!(presented(app), 0);
    assert!(app.highlight_omitted && !app.preview_omitted);
    assert_eq!(count(app, white), 1, "the brush preview is still drawn");
    assert!(!app.status_label().contains("Too many cells"));
}

/// The reviewer's check: a pending step's Review status is not replaced
/// when the frame has no room for the highlight.
#[test]
fn the_pending_step_status_stays_under_omission() {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    press_control(&mut twin.app);
    let app = &mut twin.app;
    assert_eq!(state(app), State::Pending { step: 0 });
    app.refresh_ui();
    let normal = app.status_label();
    assert!(normal.contains("matches its recording"));
    // Laid out as it will be once the highlight is left out (the reviewer
    // primed `preview_omitted`, the flag that carried the omission before).
    app.highlight_omitted = true;
    app.refresh_ui();
    let plain = crate::workspace_render::compose(
        &mut app.text,
        &app.ui,
        &app.prepared.scene,
        app.prepared.resources.clone(),
    )
    .unwrap();
    app.frame_limits = tilewright_scene::SceneLimits {
        maximum_quads: plain.scene.quads().len(),
        ..tilewright_scene::SceneLimits::default()
    };
    let _ = presented(app);
    assert!(app.highlight_omitted);
    let status = app.status_label();
    assert!(status.contains("matches its recording"), "status: {status}");
}

fn move_to(x: i64) -> impl Fn([u8; 32]) -> Action {
    move |expected| Action::Move {
        uid: "marker_1".into(),
        x,
        y: 2,
        expected,
    }
}

#[test]
fn no_highlight_where_the_world_moved_ahead_or_is_at_several_places() {
    // The person makes step 1's edit themselves: Moved with a resume step.
    let mut twin = twin(3, |_| {}, |_| {}, None);
    open(&mut twin);
    let action = twin.proposal.steps()[0].action().clone();
    twin.app.action(action).unwrap();
    twin.app.action(Action::Accept).unwrap();
    assert_eq!(state(&twin.app), State::Moved { resume: Some(1) });
    assert_eq!(shown(&mut twin.app), None);
    assert!(drawn(&twin.app).is_empty());
    press_control(&mut twin.app);
    assert_eq!(shown(&mut twin.app).map(|(step, _, _)| step), Some(1));
    // A chain that revisits a revision: A -> B -> A -> C, the person moves
    // the marker back by hand after step 1.
    let mut several = proposal_of(&[|e| move_to(4)(e), |e| move_to(3)(e), |e| move_to(5)(e)]);
    let app = &mut several.app;
    press_control(app);
    enter(app);
    app.action(move_to(3)(revision(&app.session))).unwrap();
    enter(app);
    assert!(matches!(state(app), State::Several { .. }));
    assert_eq!(shown(app), None);
    assert!(drawn(app).is_empty());
}

#[test]
fn a_block_that_ends_by_zooming_brings_the_highlight_back() {
    // While rendering blocks editing the panel highlights nothing; a zoom
    // that ends the block (no action, no funnel) refreshes it.
    let mut twin = twin(3, |_| {}, |_| {}, None);
    open(&mut twin);
    let step = |app: &mut Application| shown(app).map(|(step, _, _)| step);
    assert_eq!(step(&mut twin.app), Some(0));
    let catalog = twin.person.path().join("world/preview.jsonc");
    let original = std::fs::read(&catalog).unwrap();
    std::fs::write(&catalog, [original.as_slice(), b"\n"].concat()).unwrap();
    acknowledge_ui(&mut twin.app);
    assert!(route(&mut twin.app, &key(Keycode::G)).is_err());
    assert!(twin.app.blocked.is_some());
    assert_eq!(state(&twin.app), State::Paused);
    assert_eq!(step(&mut twin.app), None);
    std::fs::write(&catalog, &original).unwrap();
    route(&mut twin.app, &key(Keycode::Minus)).unwrap();
    assert!(twin.app.blocked.is_none());
    assert!(matches!(state(&twin.app), State::Refused { step: 0, .. }));
    // `shown` checks the stored highlight against a fresh one.
    assert_eq!(step(&mut twin.app), Some(0));
}

fn stamp(expected: [u8; 32]) -> Action {
    use tilewright_editor_ui::stamp::StampObject;
    Action::PlaceObjects {
        plane: "main".into(),
        layer: 0,
        x: 5,
        y: 3,
        objects: [
            ("marker", [0, 0]),
            ("stone", [1, 1]),
            ("marker", [0, 0]),
            ("bush", [-1, 1]),
        ]
        .into_iter()
        .map(|(proto, offset)| StampObject {
            proto: proto.into(),
            offset,
        })
        .collect(),
        expected,
    }
}

#[test]
fn a_stamp_highlights_each_object_tile_once_in_object_order() {
    use tilewright_editor_ui::stamp::StampObject;
    let (_person, app) = person_application(|_| {});
    let expected = revision(&app.session);
    let of = |action: Action| {
        let highlight = Highlight::of(0, &action, &app.session);
        assert_eq!(highlight.tints, expected_tints(&tiles(&highlight)));
        (highlight.concerns, tiles(&highlight))
    };
    // Two objects share (5, 3): one tile. The anchor is not highlighted
    // for itself.
    assert_eq!(
        of(stamp(expected)),
        (
            Concerns::Tiles,
            main0(&[("target", [5, 3]), ("target", [6, 4]), ("target", [4, 4])])
        )
    );
    // Tiles outside the view are named but not drawn; an object whose
    // tile overflows has none.
    let object = |dx, dy| StampObject {
        proto: "marker".into(),
        offset: [dx, dy],
    };
    assert_eq!(
        of(Action::PlaceObjects {
            plane: "main".into(),
            layer: 0,
            x: 7,
            y: 5,
            objects: vec![object(0, 0), object(1, 0), object(-9, 1)],
            expected,
        }),
        (
            Concerns::Tiles,
            main0(&[("target", [7, 5]), ("target", [8, 5]), ("target", [-2, 6])])
        )
    );
    assert_eq!(
        of(Action::PlaceObjects {
            plane: "main".into(),
            layer: 0,
            x: i64::MAX,
            y: 0,
            objects: vec![object(1, 0)],
            expected,
        }),
        (Concerns::NotPlaced, Vec::new())
    );
}

#[test]
fn a_stamp_step_reads_as_words_and_applies_through_the_panel() {
    use super::proposal::prepare_and_accept;
    use crate::prototype::proposal_view::action_words;
    use tilewright_editor_ui::stamp::StampObject;
    let mut twin = proposal_of(&[stamp]);
    let words = |app: &Application, action: &Action| {
        action_words(action, app.session.world(), app.session.catalog())
    };
    let step = twin.proposal.steps()[0].action().clone();
    assert_eq!(
        words(&twin.app, &step),
        "Place 4 objects (marker 2, stone 1, bush 1) anchored at (5, 3) on main/0"
    );
    // Shown, the highlight is the stamp's tiles.
    let (_, concerns, seen) = shown(&mut twin.app).unwrap();
    assert_eq!(concerns, Concerns::Tiles);
    assert_eq!(
        seen,
        main0(&[("target", [5, 3]), ("target", [6, 4]), ("target", [4, 4])])
    );
    // Applied through the panel with the recorded uids.
    prepare_and_accept(&mut twin, 0);
    let uids = twin.proposal.steps()[0].placed_objects().to_vec();
    assert_eq!(uids.len(), 4);
    for uid in &uids {
        assert!(twin.app.session.world().object(uid).is_some(), "{uid}");
    }
    // Many prototypes are cut after four; an object outside the view is
    // said.
    let expected = revision(&twin.app.session);
    let many = Action::PlaceObjects {
        plane: "main".into(),
        layer: 0,
        x: 0,
        y: 0,
        objects: ["a", "b", "c", "d", "e", "f", "a"]
            .into_iter()
            .enumerate()
            .map(|(index, proto)| StampObject {
                proto: proto.into(),
                offset: [i64::try_from(index).unwrap() * 2, 0],
            })
            .collect(),
        expected,
    };
    assert_eq!(
        words(&twin.app, &many),
        "Place 7 objects (a 2, b 1, c 1, d 1, 2 more prototypes) anchored at (0, 0), some outside the view on main/0"
    );
    let one = Action::PlaceObjects {
        plane: "main".into(),
        layer: 0,
        x: 2,
        y: 9,
        objects: vec![StampObject {
            proto: "lamp".into(),
            offset: [0, -9],
        }],
        expected,
    };
    assert_eq!(
        words(&twin.app, &one),
        "Place 1 object (lamp 1) anchored at (2, 9) on main/0"
    );
}
