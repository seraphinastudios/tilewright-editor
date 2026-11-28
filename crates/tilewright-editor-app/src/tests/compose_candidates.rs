// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! The frame's compose candidates: the whole overlay, then the person's own
//! overlays without the proposal highlight, then no overlay. Boundary tests
//! through the native loop's presentation, and a differential against the
//! nested fallback the candidate list replaced, kept here as a test-scoped
//! oracle.
use super::proposal::{Twin, open, press_control, twin};
use super::zones::{button, key, route};
use super::{acknowledge_ui, brush_application, cell_pixel};
use crate::prototype::{Application, ComposeCandidate};
use crate::workspace_render::{CompositionError, Presentation};
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use tilewright_scene::{Quad, SceneLimits};

/// The nested fallback the candidate list replaced, as it stood before,
/// with each call of `compose_with_overlay` made through `compose` so its
/// composes can be counted. Only an oracle for the differential below.
mod oracle {
    use super::{CompositionError, Presentation, Quad};

    pub(super) fn compose_with_preview(
        compose: &mut impl FnMut(&[Quad]) -> Result<Presentation, CompositionError>,
        overlay: &[Quad],
    ) -> Result<(Presentation, bool), CompositionError> {
        match compose(overlay) {
            Err(CompositionError::Scene(tilewright_scene::Error::LimitExceeded))
                if !overlay.is_empty() =>
            {
                compose(&[]).map(|presentation| (presentation, true))
            }
            result => result.map(|presentation| (presentation, false)),
        }
    }

    pub(super) fn compose_highlight_first(
        compose: &mut impl FnMut(&[Quad]) -> Result<Presentation, CompositionError>,
        overlay: &[Quad],
        person: &[Quad],
    ) -> Result<(Presentation, bool, bool), CompositionError> {
        if overlay.len() == person.len() {
            return compose_with_preview(compose, overlay)
                .map(|(presentation, omitted)| (presentation, omitted, false));
        }
        match compose(overlay) {
            Err(CompositionError::Scene(tilewright_scene::Error::LimitExceeded)) => {
                compose_with_preview(compose, person)
                    .map(|(presentation, omitted)| (presentation, omitted, true))
            }
            result => result.map(|presentation| (presentation, false, false)),
        }
    }
}

/// The whole overlay, the person's own and whether a highlight is among
/// the whole, built as `workspace_presentation` builds them.
fn overlays(app: &Application) -> (Vec<Quad>, Vec<Quad>, bool) {
    use crate::prototype::{
        badge_overlay, height_overlay, proposal_overlay, road_point_marks, stroke_overlay,
        zone_overlay,
    };
    use tilewright_editor_ui::brush::{Stroke, StrokeTarget};
    let mut person = height_overlay(app);
    person.extend(zone_overlay(app));
    let mut stroke = stroke_overlay(app);
    if matches!(
        app.tools.stroke().map(Stroke::target),
        Some(
            StrokeTarget::Heights { .. }
                | StrokeTarget::Flatten { .. }
                | StrokeTarget::Smooth { .. }
        )
    ) {
        person.append(&mut stroke);
    }
    person.extend(road_point_marks(app));
    person.extend(badge_overlay(app));
    let mut overlay = person.clone();
    let highlight = proposal_overlay(app);
    let highlighted = !highlight.is_empty();
    overlay.extend(highlight);
    overlay.extend(stroke.iter().copied());
    person.extend(stroke);
    (overlay, person, highlighted)
}

/// A composition's result as the tests compare it, and how many composes
/// it took.
#[derive(Debug, PartialEq)]
struct Outcome {
    result: Result<(Vec<Quad>, usize, bool, bool), String>,
    composes: usize,
}

fn outcome(
    result: Result<(Presentation, bool, bool), CompositionError>,
    composes: usize,
) -> Outcome {
    Outcome {
        result: result
            .map(|(presentation, preview, highlight)| {
                (
                    presentation.scene.quads().to_vec(),
                    presentation.unsupported_runs,
                    preview,
                    highlight,
                )
            })
            .map_err(|error| error.to_string()),
        composes,
    }
}

/// Compose with the candidate list under `limits`, counting composes.
fn candidates_outcome(
    app: &mut Application,
    (overlay, person, highlighted): &(Vec<Quad>, Vec<Quad>, bool),
    limits: SceneLimits,
) -> Outcome {
    let candidates = crate::prototype::compose_candidates(overlay, person, *highlighted);
    let mut composes = 0;
    let result = crate::prototype::compose_first_fitting(&candidates, |overlay| {
        composes += 1;
        crate::workspace_render::compose_with_overlay(
            &mut app.text,
            &app.ui,
            &app.prepared.scene,
            app.prepared.resources.clone(),
            overlay,
            limits,
        )
    });
    outcome(result, composes)
}

/// Compose with the oracle under `limits`, counting composes.
fn oracle_outcome(
    app: &mut Application,
    (overlay, person, _): &(Vec<Quad>, Vec<Quad>, bool),
    limits: SceneLimits,
) -> Outcome {
    let mut composes = 0;
    let mut compose = |overlay: &[Quad]| {
        composes += 1;
        crate::workspace_render::compose_with_overlay(
            &mut app.text,
            &app.ui,
            &app.prepared.scene,
            app.prepared.resources.clone(),
            overlay,
            limits,
        )
    };
    let result = oracle::compose_highlight_first(&mut compose, overlay, person);
    outcome(result, composes)
}

/// The raster samples drawing `presentation` takes, by the scene library's
/// own check.
fn raster_samples(presentation: &Presentation) -> u64 {
    let scene = &presentation.scene;
    let fits = |samples| {
        tilewright_scene::SceneBundle::from_quads(
            scene.revision(),
            scene.clock_millis(),
            scene.viewport(),
            scene.clear_color(),
            scene.quads().iter().copied(),
            SceneLimits {
                maximum_raster_samples: samples,
                ..SceneLimits::default()
            },
        )
        .is_ok()
    };
    let (mut low, mut high) = (0, SceneLimits::default().maximum_raster_samples);
    while low < high {
        let middle = low + (high - low) / 2;
        if fits(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

/// The frame each candidate composes under the default limits, with the
/// interface as laid out now.
fn candidate_frames(
    app: &mut Application,
    (overlay, person, highlighted): &(Vec<Quad>, Vec<Quad>, bool),
) -> Vec<(Presentation, bool, bool)> {
    crate::prototype::compose_candidates(overlay, person, *highlighted)
        .iter()
        .map(|candidate: &ComposeCandidate<'_>| {
            let presentation = crate::workspace_render::compose_with_overlay(
                &mut app.text,
                &app.ui,
                &app.prepared.scene,
                app.prepared.resources.clone(),
                candidate.overlay,
                SceneLimits::default(),
            )
            .unwrap();
            (
                presentation,
                candidate.preview_omitted,
                candidate.highlight_omitted,
            )
        })
        .collect()
}

/// Compare the candidate list with the oracle in `app`'s state, under each
/// of the four layouts the flags give, at limits just around every
/// candidate's quads and raster samples. Returns the cases compared and
/// how many of them each candidate (by flags) or an error answered.
fn differential(app: &mut Application) -> (usize, std::collections::BTreeMap<String, usize>) {
    let mut cases = 0;
    let mut answered = std::collections::BTreeMap::new();
    let saved = (app.preview_omitted, app.highlight_omitted);
    for flags in [(false, false), (false, true), (true, false), (true, true)] {
        (app.preview_omitted, app.highlight_omitted) = flags;
        app.refresh_ui();
        let lists = overlays(app);
        let frames = candidate_frames(app, &lists);
        let mut limits = vec![
            SceneLimits::default(),
            SceneLimits {
                maximum_quads: 0,
                ..SceneLimits::default()
            },
        ];
        for (presentation, _, _) in &frames {
            let quads = presentation.scene.quads().len();
            let samples = raster_samples(presentation);
            for delta in -2..=2_i64 {
                limits.push(SceneLimits {
                    maximum_quads: quads.saturating_add_signed(delta.try_into().unwrap()),
                    ..SceneLimits::default()
                });
                limits.push(SceneLimits {
                    maximum_raster_samples: samples.saturating_add_signed(delta),
                    ..SceneLimits::default()
                });
            }
        }
        for limits in limits {
            let new = candidates_outcome(app, &lists, limits);
            let old = oracle_outcome(app, &lists, limits);
            assert_eq!(new, old, "flags {flags:?}, limits {limits:?}");
            assert!(new.composes <= 3);
            let key = match &new.result {
                Ok((_, _, preview, highlight)) => {
                    format!("preview {preview} highlight {highlight}")
                }
                Err(error) => format!("error {error}"),
            };
            *answered.entry(key).or_insert(0) += 1;
            cases += 1;
        }
    }
    (app.preview_omitted, app.highlight_omitted) = saved;
    app.refresh_ui();
    (cases, answered)
}

/// A proposal open with its highlight on screen and the zone overlay and
/// badges shown.
fn highlighted_with_zones() -> Twin {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    press_control(&mut twin.app);
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::O)).unwrap();
    twin
}

/// A proposal open with its highlight on screen and nothing of the
/// person's own drawn.
fn highlighted_alone() -> Twin {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    press_control(&mut twin.app);
    acknowledge_ui(&mut twin.app);
    twin
}

/// A proposal open with its highlight on screen and the person's brush
/// preview on a cell.
fn highlighted_with_stroke() -> Twin {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    let app = &mut twin.app;
    app.control(Control::Terrain).unwrap();
    app.control(Control::Palette {
        grid: "floor".into(),
        prototype: "sand".into(),
    })
    .unwrap();
    acknowledge_ui(app);
    let at = cell_pixel(app, 3, 3);
    route(app, &button(true, at)).unwrap();
    twin
}

/// A proposal open with its highlight on screen and the shared string
/// field editing a marker's label.
fn highlighted_with_shared_field() -> Twin {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    let app = &mut twin.app;
    app.select_visible_object(crate::prototype::Capability::Field)
        .unwrap();
    app.control(Control::Field("label".into())).unwrap();
    acknowledge_ui(app);
    twin
}

/// No proposal, a brush stroke's preview on the World.
fn stroke_only() -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = brush_application();
    app.control(Control::BrushSize(8)).unwrap();
    acknowledge_ui(&mut app);
    let start = cell_pixel(&mut app, 3, 3);
    route(&mut app, &button(true, start)).unwrap();
    (temporary, app)
}

/// A quad of the neutral World, for the lists' own tests.
fn any_quad() -> Quad {
    let (_temporary, app) = brush_application();
    app.prepared.scene.quads()[0]
}

#[test]
fn the_candidates_are_the_whole_overlay_then_the_persons_then_none() {
    let quad = any_quad();
    let (whole, person) = (vec![quad; 3], vec![quad; 2]);
    let flags = |list: &[ComposeCandidate<'_>]| {
        list.iter()
            .map(|c| (c.overlay.len(), c.preview_omitted, c.highlight_omitted))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        flags(&crate::prototype::compose_candidates(&whole, &person, true)),
        [(3, false, false), (2, false, true), (0, true, true)]
    );
    // A highlight and nothing of the person's: leaving the highlight out is
    // the bare frame, listed once.
    assert_eq!(
        flags(&crate::prototype::compose_candidates(
            &whole[..1],
            &[],
            true
        )),
        [(1, false, false), (0, false, true)]
    );
    assert_eq!(
        flags(&crate::prototype::compose_candidates(
            &person, &person, false
        )),
        [(2, false, false), (0, true, false)]
    );
    assert_eq!(
        flags(&crate::prototype::compose_candidates(&[], &[], false)),
        [(0, false, false)]
    );
}

#[test]
fn only_an_exceeded_limit_before_the_last_candidate_moves_to_the_next() {
    let quad = any_quad();
    let (whole, person) = (vec![quad; 3], vec![quad; 2]);
    let candidates = crate::prototype::compose_candidates(&whole, &person, true);
    let exceeded = || CompositionError::Scene(tilewright_scene::Error::LimitExceeded);
    // Any error but the limit is returned from the first candidate.
    let mut tried = Vec::new();
    let result = crate::prototype::compose_first_fitting(&candidates, |overlay| {
        tried.push(overlay.len());
        Err(CompositionError::TargetMismatch)
    });
    assert!(matches!(result, Err(CompositionError::TargetMismatch)));
    assert_eq!(tried, [3]);
    // The limit moves on; a bare frame past it is returned, once each.
    let mut tried = Vec::new();
    let result = crate::prototype::compose_first_fitting(&candidates, |overlay| {
        tried.push(overlay.len());
        Err(exceeded())
    });
    assert!(matches!(
        result,
        Err(CompositionError::Scene(
            tilewright_scene::Error::LimitExceeded
        ))
    ));
    assert_eq!(tried, [3, 2, 0]);
}

/// The quads the shared field (when it is prepared now) sets aside.
fn reserved_quads(app: &mut Application) -> usize {
    crate::prototype::shared_field_reservation(
        crate::prototype::prepare_shared_field(app)
            .unwrap()
            .as_ref(),
    )
    .0
}

/// Lay `app` out with `flags`; the candidate frames, the shared field's
/// reserved quads and the status then.
fn laid_out(
    app: &mut Application,
    flags: (bool, bool),
) -> (Vec<(Presentation, bool, bool)>, usize, String) {
    (app.preview_omitted, app.highlight_omitted) = flags;
    app.frame_limits = SceneLimits::default();
    app.refresh_ui();
    let reserved = reserved_quads(app);
    let frames = candidate_frames(app, &overlays(app));
    (frames, reserved, app.status_label())
}

/// The rule the candidates follow, by quad counts alone: the first whose
/// frame fits within `limit` quads, if any.
fn first_fitting(frames: &[(Presentation, bool, bool)], limit: usize) -> Option<usize> {
    frames
        .iter()
        .position(|(presentation, _, _)| presentation.scene.quads().len() <= limit)
}

/// The editor's own quads of a presented frame, the shared field's left out.
fn editor_quads(presentation: &Presentation) -> Vec<Quad> {
    presentation
        .scene
        .quads()
        .iter()
        .copied()
        .filter(|quad| quad.semantic_id.0 < crate::shared_field::SHARED_FIRST_SEMANTIC)
        .collect()
}

/// Through the native loop's presentation: limits exactly at each
/// candidate's frame present that candidate with its flags and status in
/// one pass; one quad less presents the next candidate, and the second
/// pass, laid out with its flags, presents the first candidate that fits
/// that layout, or fails as the bare frame fails. With the shared field
/// on, its reserved quads are added to the limit. Returns the limits
/// checked.
fn assert_boundaries(app: &mut Application) -> usize {
    let mut checked = 0;
    let lists = overlays(app);
    let candidates = crate::prototype::compose_candidates(&lists.0, &lists.1, lists.2);
    let flags: Vec<(bool, bool)> = candidates
        .iter()
        .map(|candidate| (candidate.preview_omitted, candidate.highlight_omitted))
        .collect();
    for at in 0..flags.len() {
        let (frames, reserved, status) = laid_out(app, flags[at]);
        let quads = frames[at].0.scene.quads().len();
        assert_eq!(first_fitting(&frames, quads), Some(at));
        for limit in [quads, quads - 1] {
            laid_out(app, flags[at]);
            app.frame_limits.maximum_quads = limit + reserved;
            let presented = crate::prototype::workspace_presentation(app);
            let (presented_flags, presented_status) = (
                (app.preview_omitted, app.highlight_omitted),
                app.status_label(),
            );
            // The first pass, laid out with this candidate's flags.
            let Some(first) = first_fitting(&frames, limit) else {
                let Err(error) = presented else {
                    panic!("candidate {at} at {limit}: the bare frame fitted");
                };
                assert!(error.contains("LimitExceeded"), "{error}");
                assert!(!error.starts_with("shared field"), "{error}");
                assert_eq!(presented_flags, flags[at]);
                checked += 1;
                continue;
            };
            let (expected, final_frames) = if first == at {
                (
                    Some(first),
                    frames.iter().map(|(p, _, _)| editor_quads(p)).collect(),
                )
            } else {
                // Laid out again with the first pass's flags.
                assert_eq!(first, at + 1);
                let (frames, again, _) = laid_out(app, flags[first]);
                let second = first_fitting(&frames, limit + reserved - again);
                (
                    second,
                    frames
                        .iter()
                        .map(|(p, _, _)| editor_quads(p))
                        .collect::<Vec<_>>(),
                )
            };
            match (expected, presented) {
                (Some(index), Ok(presentation)) => {
                    assert_eq!(editor_quads(&presentation), final_frames[index]);
                    assert_eq!(presented_flags, flags[index], "candidate {at} at {limit}");
                    if index == at {
                        assert_eq!(presented_status, status);
                    }
                }
                (None, Err(error)) => {
                    assert!(error.contains("LimitExceeded"), "{error}");
                    assert!(!error.starts_with("shared field"), "{error}");
                }
                (expected, presented) => panic!(
                    "candidate {at} at {limit}: expected {expected:?}, got {:?}",
                    presented.map(|p| p.scene.quads().len())
                ),
            }
            checked += 1;
        }
    }
    laid_out(app, (false, false));
    checked
}

/// An app state to test, made when its turn comes, and its name.
type State = (fn() -> Twin, &'static str);

/// The app states with a highlight on screen.
const HIGHLIGHTED: [State; 4] = [
    (highlighted_with_zones, "highlight, zones and badges"),
    (highlighted_alone, "highlight alone"),
    (highlighted_with_stroke, "highlight and brush preview"),
    (highlighted_with_shared_field, "highlight and shared field"),
];

#[test]
fn every_candidate_presents_at_its_own_limit_and_gives_way_one_below() {
    let mut checked = 0;
    for state in HIGHLIGHTED {
        checked += assert_boundaries(&mut state.0().app);
    }
    let (_temporary, mut app) = stroke_only();
    checked += assert_boundaries(&mut app);
    let (_temporary, mut app) = brush_application();
    acknowledge_ui(&mut app);
    checked += assert_boundaries(&mut app);
    assert_eq!(checked, 2 * (3 + 2 + 3 + 2 + 2 + 1));
}

#[test]
fn the_candidates_compose_what_the_nested_fallback_composed() {
    let mut total = 0;
    let mut answered = std::collections::BTreeMap::new();
    let mut run = |app: &mut Application, name: &str| {
        let (cases, by) = differential(app);
        eprintln!("{name}: {cases} cases {by:?}");
        total += cases;
        for (key, count) in by {
            *answered.entry(key).or_insert(0) += count;
        }
    };
    for (state, name) in HIGHLIGHTED {
        run(&mut state().app, name);
    }
    let (_temporary, mut app) = stroke_only();
    run(&mut app, "brush preview, no proposal");
    let (_temporary, mut app) = brush_application();
    acknowledge_ui(&mut app);
    run(&mut app, "nothing drawn over the World");
    eprintln!("total: {total} cases {answered:?}");
    // Every candidate, and the bare frame's failure, answered some case.
    for key in [
        "preview false highlight false",
        "preview false highlight true",
        "preview true highlight true",
        "preview true highlight false",
    ] {
        assert!(
            answered.get(key).is_some_and(|&n| n > 0),
            "{key}: {answered:?}"
        );
    }
    assert!(answered.keys().any(|key| key.starts_with("error")));
}
