// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! A proposal opened in the desktop: made in-test with
//! `Proposal::from_session` on a twin copy of the person's world, then
//! applied through the application's own panel control, keys and Review,
//! on the CPU scene path, without a window.
use super::zone_manage::{enter, row_enabled};
use super::zones::{button, key, route, sidebar_texts, zone_application};
use super::{acknowledge_ui, copy_private};
use crate::prototype::Application;
use crate::prototype::proposal_view::{
    PANEL_HEIGHT, PANEL_LINES, Row, State, WRAPPED_ROWS, action_words, panel_lines, row_characters,
    shown,
};
use crate::workspace_ui::Control;
use sdl3::keyboard::Keycode;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tilewright_editor_commands::{History, RecoveryStore};
use tilewright_editor_document::{WorldDocument, revision_hex};
use tilewright_editor_preview::{PreviewLimits, loader};
use tilewright_editor_proposal::{Proposal, hex};
use tilewright_editor_testkit::fixture_world;
use tilewright_editor_ui::{Action, Actor, PortalTarget, RefusalCode, Session};

pub(super) const AGENT: &str = "agent:sandbox \"desk\" \\ 7";
const PERSON: &str = "human:tester";
pub(super) const RECEIPT: [u8; 32] = [7; 32];
const LABEL: &str = "client\u{1b}[31m\u{202e}\u{200b}\u{2028}";

#[derive(Default)]
struct Records;
impl RecoveryStore for Records {
    fn retain(&mut self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// The neutral fixture with one unused zone, `spare`, to delete.
fn spare_zone(root: &Path) {
    edit_json(root, "planes/main/zones.jsonc", |zones| {
        zones["zones"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "spare"}));
    });
}

pub(super) fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

pub(super) fn open_session(root: &Path, actor: &str) -> Session {
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

pub(super) fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}

/// Dispatch `action` on `session` and accept it.
pub(super) fn accept(session: &mut Session, action: Action) {
    let kind = format!("{action:?}");
    session
        .dispatch(action, &mut Records)
        .unwrap_or_else(|error| panic!("{kind}: {} {error}", error.code()));
    session
        .dispatch(Action::Accept, &mut Records)
        .unwrap_or_else(|error| panic!("accept {kind}: {} {error}", error.code()));
}

/// The 15 edit kinds, in an order in which each applies after the ones
/// before it on the neutral fixture with the `spare` zone.
pub(super) const KINDS: usize = 15;
#[allow(clippy::too_many_lines)]
pub(super) fn edit(kind: usize, expected: [u8; 32]) -> Action {
    let main = || String::from("main");
    match kind {
        0 => Action::Move {
            uid: "marker_1".into(),
            x: 4,
            y: 2,
            expected,
        },
        1 => Action::SetField {
            uid: "marker_1".into(),
            field: "label".into(),
            value: json!("Renamed"),
            expected,
        },
        2 => Action::Paint {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            x: 0,
            y: 0,
            proto: "sand".into(),
            expected,
        },
        3 => Action::PaintStroke {
            plane: main(),
            layer: 0,
            grid: "floor".into(),
            cells: vec![[0, 1], [1, 1]],
            proto: "sand".into(),
            expected,
        },
        4 => Action::PaintZones {
            plane: main(),
            layer: 0,
            cells: vec![[6, 0]],
            zone: Some("path".into()),
            expected,
        },
        5 => Action::Remove {
            uid: "marker_2".into(),
            expected,
        },
        6 => Action::Place {
            plane: main(),
            layer: 0,
            x: 2,
            y: 2,
            proto: "marker".into(),
            expected,
        },
        7 => Action::RenameZone {
            plane: main(),
            zone: "path".into(),
            name: Some("Trail".into()),
            expected,
        },
        8 => Action::SetZoneParent {
            plane: main(),
            zone: "path".into(),
            parent: Some("meadow".into()),
            expected,
        },
        9 => Action::CreateZone {
            plane: main(),
            zone: "camp".into(),
            name: Some("Camp".into()),
            parent: None,
            expected,
        },
        10 => Action::DeleteZone {
            plane: main(),
            zone: "spare".into(),
            expected,
        },
        11 => Action::CreateAnchor {
            plane: main(),
            anchor: "camp".into(),
            x: 2,
            y: 2,
            expected,
        },
        12 => Action::MoveAnchor {
            plane: main(),
            anchor: "arrival".into(),
            x: 6,
            y: 3,
            expected,
        },
        13 => Action::DeleteAnchor {
            plane: main(),
            anchor: "lookout".into(),
            expected,
        },
        _ => Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Anchor {
                plane: main(),
                anchor: "arrival".into(),
            }),
            expected,
        },
    }
}

/// The person's application on a copy of the neutral fixture (changed by
/// `person`), with the person's actor; and the agent's twin copy (changed
/// by `agent`), where `steps` edit kinds are accepted and recorded.
pub(super) struct Twin {
    pub(super) person: tempfile::TempDir,
    pub(super) agent: tempfile::TempDir,
    pub(super) app: Application,
    pub(super) proposal: Proposal,
}

pub(super) fn twin(
    steps: usize,
    agent: impl FnOnce(&Path),
    person: impl FnOnce(&Path),
    client: Option<&str>,
) -> Twin {
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    spare_zone(&world);
    agent(&world);
    let mut session = open_session(&world, AGENT);
    session.set_uid_source(Box::new(tilewright_editor_ui::uid::SeededUidSource::new(
        11,
    )));
    for kind in 0..steps {
        let expected = revision(&session);
        accept(&mut session, edit(kind, expected));
    }
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        client,
    )
    .unwrap();
    let (temporary, app) = person_application(|root| {
        spare_zone(root);
        person(root);
    });
    Twin {
        person: temporary,
        agent: agent_root,
        app,
        proposal,
    }
}

pub(super) fn scratch() -> std::path::PathBuf {
    std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
}

/// The 1280x800 test application, its session rebuilt with the person's
/// actor as `--actor human:tester` makes it.
pub(super) fn person_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
    let (temporary, mut app) = zone_application(prepare);
    let document = WorldDocument::open_prototype(&temporary.path().join("world")).unwrap();
    let (catalog, resources) = loader::load(
        document.source(),
        document.world(),
        PreviewLimits::default(),
    )
    .unwrap();
    app.session = Session::new(
        History::new(document),
        catalog,
        resources,
        Actor::new(PERSON).unwrap(),
    );
    app.rebuild().unwrap();
    acknowledge_ui(&mut app);
    (temporary, app)
}

pub(super) fn open(twin: &mut Twin) {
    let name = twin.proposal.file_name();
    twin.app
        .open_proposal(Some((twin.proposal.clone(), name)))
        .unwrap();
    acknowledge_ui(&mut twin.app);
}

pub(super) fn state(app: &Application) -> State {
    app.proposal
        .as_ref()
        .unwrap()
        .state(&app.session, app.blocked.is_some())
}

pub(super) fn evidence(twin: &Twin, name: &str) -> Value {
    serde_json::from_slice(&fs::read(twin.person.path().join("evidence").join(name)).unwrap())
        .unwrap()
}

fn evidence_names(twin: &Twin) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(twin.person.path().join("evidence"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Every file of a tree, by relative path.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                found.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    found
}

/// The panel's rows, as built for the sidebar's width; the header, the
/// step's words and the state line, which the height cap never drops, are
/// checked to be laid out (the start of each).
fn panel(app: &mut Application) -> Vec<String> {
    let rows = panel_rows(app);
    let laid: String = sidebar_texts(app)
        .concat()
        .chars()
        .filter(|c| *c != ' ')
        .collect();
    for (kind, row) in &rows {
        if matches!(kind, Row::Header | Row::Step | Row::State) {
            let start: String = row.chars().filter(|c| *c != ' ').take(24).collect();
            assert!(laid.contains(&start), "{row:?} is not laid out in {laid:?}");
        }
    }
    rows.into_iter().map(|(_, text)| text).collect()
}

fn panel_rows(app: &mut Application) -> Vec<(Row, String)> {
    let state = app.proposal_state().unwrap();
    let characters = row_characters(&mut app.text, app.ui.sidebar_width());
    app.proposal
        .as_ref()
        .unwrap()
        .rows(&app.session, state, characters)
}

/// Press the panel's control with the pointer, as the person would.
pub(super) fn press_control(app: &mut Application) {
    try_press_control(app).unwrap();
}

/// Press the panel's control with the pointer; the action's result.
fn try_press_control(app: &mut Application) -> Result<(), String> {
    let (_, control, enabled) = control(app).expect("the panel offers a control");
    assert!(enabled);
    acknowledge_ui(app);
    let widget = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control.as_ref() == Some(&control))
        .cloned()
        .unwrap();
    #[allow(clippy::cast_precision_loss)]
    let at = [
        (widget.bounds.x + widget.bounds.width / 2) as f32,
        (widget.bounds.y + widget.bounds.height / 2) as f32,
    ];
    route(app, &button(true, at))?;
    route(app, &button(false, at))
}

/// The panel's control row: label, control and enabled.
fn control(app: &mut Application) -> Option<(String, Control, bool)> {
    app.refresh_ui();
    app.ui
        .widgets
        .iter()
        .find(|widget| {
            matches!(
                widget.control,
                Some(Control::ProposalPrepare | Control::ProposalContinue)
            )
        })
        .map(|widget| {
            (
                widget.text.clone(),
                widget.control.clone().unwrap(),
                widget.enabled,
            )
        })
}

/// Prepare the next step with the panel's button, check it is pending
/// for review and nothing else changed, and accept it with Enter.
pub(super) fn prepare_and_accept(twin: &mut Twin, step: usize) {
    let app = &mut twin.app;
    assert_eq!(state(app), State::Ready { step });
    let before = revision(&app.session);
    let undo = app.session.history().depths().0;
    let words = |app: &mut Application| {
        panel(app)
            .into_iter()
            .find(|row| row.starts_with(&format!("Step {} of", step + 1)))
    };
    let offered = words(app);
    assert!(offered.is_some());
    press_control(app);
    assert_eq!(state(app), State::Pending { step });
    // The words still describe the committed world the step changes.
    assert_eq!(words(app), offered);
    // Preparing changed nothing: the session is where it was until Accept.
    assert_eq!(revision(&app.session), before);
    assert_eq!(app.session.history().depths().0, undo);
    let steps = twin.proposal.steps().len();
    assert_eq!(
        app.status_label(),
        format!(
            "Step {} of {steps} matches its recording (recorded as {}). Accept applies it; Cancel stops the proposal.",
            step + 1,
            shown(AGENT)
        )
    );
    // The Review shows the session's own review of the freshly prepared
    // change, and the prepared change carries the person's actor.
    assert!(app.session.review().complete);
    assert_eq!(app.diff_lines(), app.session.review().lines);
    let pending = app.session.history().pending().unwrap().sealed().to_owned();
    assert!(
        pending.contains("\"actor\": \"human:tester\"")
            || pending.contains("\"actor\":\"human:tester\""),
        "{pending}"
    );
    assert_ne!(pending, twin.proposal.steps()[step].transaction());
    assert_eq!(
        control(app),
        None,
        "nothing to prepare while the step is pending"
    );
    enter(app);
    assert_eq!(revision(&app.session), twin.proposal.steps()[step].after());
    assert_eq!(app.session.history().depths().0, undo + 1);
}

#[test]
#[allow(clippy::too_many_lines)]
fn every_step_applies_through_prepare_and_the_persons_accept() {
    let mut twin = twin(KINDS, |_| {}, |_| {}, Some(LABEL));
    assert!(
        !evidence_names(&twin)
            .iter()
            .any(|name| name.starts_with("proposal"))
    );
    open(&mut twin);
    let opened = evidence(&twin, "proposal-open.json");
    let proposal = twin.proposal.clone();
    assert_eq!(
        opened,
        json!({
            "format": "tilewright.prototype-proposal/1",
            "proposal": hex(&proposal.body_sha256()),
            "file": proposal.file_name(),
            "editor": crate::build_version::EDITOR_VERSION,
            "this_editor": crate::build_version::EDITOR_VERSION,
            "agent": "agent:sandbox \"desk\" \\\\ 7",
            "client": "client\\u{1b}[31m\\u{202e}\\u{200b}\\u{2028}",
            "person": PERSON,
            "base": hex(&proposal.base()),
            "result": hex(&proposal.result()),
            "catalog": hex(&proposal.catalog()),
            "steps": KINDS,
            "notices": [],
            "opened": {"begun": true},
        })
    );
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with("World: at its base")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text == "Step 1 of 15: Move marker_1 to (4, 2), from (3, 2), on main/0"),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text == "Applied 0 of 15, remaining 15"),
        "{texts:?}"
    );
    for step in 0..KINDS {
        prepare_and_accept(&mut twin, step);
        let record = evidence(&twin, &format!("proposal-step-{:06}.json", step + 1));
        let applied = twin.app.session.applied_steps().last().unwrap();
        let recorded = &proposal.steps()[step];
        let last = twin.app.recovery.records().last().unwrap();
        assert_eq!(
            record,
            json!({
                "format": "tilewright.prototype-proposal-step/1",
                "sequence": step + 1,
                "proposal": hex(&proposal.body_sha256()),
                "file": proposal.file_name(),
                "step": step + 1,
                "steps": KINDS,
                "actor": PERSON,
                "recorded_transaction_sha256": hex(&recorded.transaction_sha256()),
                "applied_transaction_sha256": hex(&sha256(applied.transaction)),
                "before": hex(&recorded.before()),
                "after": hex(&recorded.after()),
                "recovery": {"name": last.name, "sha256": revision_hex(&last.sha256)},
            })
        );
        assert_ne!(
            record["recorded_transaction_sha256"],
            record["applied_transaction_sha256"]
        );
    }
    assert_eq!(state(&twin.app), State::Finished);
    assert_eq!(control(&mut twin.app), None);
    let texts = panel(&mut twin.app);
    assert!(
        texts.iter().any(|text| text == "All 15 steps applied."),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text == "World: after step 15 of 15"),
        "{texts:?}"
    );
    // The person's world now equals the agent's, byte for byte; every
    // applied transaction carries the person's actor.
    assert_eq!(
        files(&twin.person.path().join("world")),
        files(&twin.agent.path().join("world"))
    );
    assert_eq!(twin.app.session.applied_steps().len(), KINDS);
    for applied in twin.app.session.applied_steps() {
        assert_eq!(applied.origin.unwrap().actor().as_str(), PERSON);
    }
    // The G key does nothing more: there is no step left.
    acknowledge_ui(&mut twin.app);
    assert!(route(&mut twin.app, &key(Keycode::G)).is_err());
    // The frame evidence carries the proposal while one is open.
    twin.app.capture("finished").unwrap();
    let frame = evidence(&twin, "finished.json");
    assert_eq!(
        frame["proposal"],
        json!({"proposal": hex(&proposal.body_sha256()), "steps": KINDS, "applied": KINDS,
            "state": "finished", "pending_step": false, "highlight": null})
    );
}

fn sha256(text: &str) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(text.as_bytes()).into()
}

#[test]
fn without_a_proposal_nothing_is_shown_or_recorded() {
    let (temporary, mut app) = person_application(|_| {});
    assert!(app.proposal.is_none());
    app.open_proposal(None).unwrap();
    assert!(!sidebar_texts(&mut app).concat().contains("Proposal"));
    assert_eq!(control(&mut app), None);
    acknowledge_ui(&mut app);
    route(&mut app, &key(Keycode::G)).unwrap();
    assert_eq!(app.session.history().pending().map(|_| ()), None);
    app.capture("plain").unwrap();
    let frame: Value =
        serde_json::from_slice(&fs::read(temporary.path().join("evidence/plain.json")).unwrap())
            .unwrap();
    assert!(frame.get("proposal").is_none());
    let names: Vec<_> = fs::read_dir(temporary.path().join("evidence"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn cancel_stops_the_proposal_and_resume_continues_it() {
    let mut twin = twin(3, |_| {}, |_| {}, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    press_control(&mut twin.app);
    assert_eq!(state(&twin.app), State::Pending { step: 1 });
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::Escape)).unwrap();
    assert_eq!(state(&twin.app), State::Stopped { resume: Some(1) });
    assert!(
        twin.app
            .proposal
            .as_ref()
            .unwrap()
            .progress
            .as_ref()
            .unwrap()
            .stopped()
    );
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Stopped by Cancel. Resume continues it."),
        "{texts:?}"
    );
    assert_eq!(
        control(&mut twin.app),
        Some((
            "Resume at step 2 (G)".into(),
            Control::ProposalContinue,
            true
        ))
    );
    // While a change of the person's own is pending, nothing is offered.
    let expected = revision(&twin.app.session);
    twin.app
        .action(Action::Move {
            uid: "marker_2".into(),
            x: 2,
            y: 5,
            expected,
        })
        .unwrap();
    assert_eq!(state(&twin.app), State::Stopped { resume: None });
    assert_eq!(control(&mut twin.app), None);
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::Escape)).unwrap();
    // Resume through the key: a new progress, nothing prepared yet.
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::G)).unwrap();
    assert!(twin.app.session.history().pending().is_none());
    prepare_and_accept(&mut twin, 1);
    prepare_and_accept(&mut twin, 2);
    assert_eq!(state(&twin.app), State::Finished);
    // The cancelled step left no record; the three applied ones did.
    let steps: Vec<_> = evidence_names(&twin)
        .into_iter()
        .filter(|name| name.starts_with("proposal-step-"))
        .collect();
    assert_eq!(steps.len(), 3);
}

#[test]
fn an_undone_step_is_prepared_again_and_recorded_again() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    // Undo is reviewed and accepted like any change.
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::Z)).unwrap();
    assert_eq!(state(&twin.app), State::OtherPending);
    assert!(twin.app.status_label().starts_with("Review changes"));
    enter(&mut twin.app);
    assert_eq!(revision(&twin.app.session), twin.proposal.base());
    assert_eq!(state(&twin.app), State::Undone { step: 0 });
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Step 1 undone: prepare it again (or Redo)."),
        "{texts:?}"
    );
    assert_eq!(
        control(&mut twin.app),
        Some(("Prepare step 1 (G)".into(), Control::ProposalPrepare, true))
    );
    press_control(&mut twin.app);
    assert_eq!(state(&twin.app), State::Pending { step: 0 });
    enter(&mut twin.app);
    prepare_and_accept(&mut twin, 1);
    // Applied three times in all: step 1 twice, each with its own record.
    let first = evidence(&twin, "proposal-step-000001.json");
    let again = evidence(&twin, "proposal-step-000002.json");
    let last = evidence(&twin, "proposal-step-000003.json");
    assert_eq!(
        (
            first["step"].clone(),
            again["step"].clone(),
            last["step"].clone()
        ),
        (json!(1), json!(1), json!(2))
    );
    assert_eq!(again["sequence"], 2);
    assert_ne!(first["recovery"], again["recovery"]);
    assert_eq!(
        files(&twin.person.path().join("world")),
        files(&twin.agent.path().join("world"))
    );
}

#[test]
fn a_redo_of_an_undone_step_continues_without_preparing() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    for keycode in [Keycode::Z, Keycode::Return, Keycode::Y, Keycode::Return] {
        acknowledge_ui(&mut twin.app);
        route(&mut twin.app, &key(keycode)).unwrap();
    }
    assert_eq!(state(&twin.app), State::Ready { step: 1 });
    prepare_and_accept(&mut twin, 1);
    assert_eq!(state(&twin.app), State::Finished);
}

#[test]
fn the_persons_own_edit_between_steps_holds_the_proposal_until_undone() {
    let mut twin = twin(3, |_| {}, |_| {}, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    // The person's own edit: a move the proposal does not make.
    let expected = revision(&twin.app.session);
    twin.app
        .action(Action::Move {
            uid: "marker_2".into(),
            x: 2,
            y: 5,
            expected,
        })
        .unwrap();
    assert_eq!(state(&twin.app), State::OtherPending);
    // Today's status line: it is the person's change, not the step.
    assert_eq!(
        twin.app.status_label(),
        "Review changes, then Accept or Cancel."
    );
    assert_eq!(control(&mut twin.app), None);
    acknowledge_ui(&mut twin.app);
    assert!(route(&mut twin.app, &key(Keycode::G)).is_err());
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Your own change is pending: the proposal waits."),
        "{texts:?}"
    );
    enter(&mut twin.app);
    assert_eq!(state(&twin.app), State::Moved { resume: None });
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "The world moved: undo your own edits to continue."),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "World: not on its path"),
        "{texts:?}"
    );
    assert_eq!(
        control(&mut twin.app),
        None,
        "no control while the person's edit is on top"
    );
    // Continuing from where the world is is refused: it is not on the path.
    let error = twin.app.continue_proposal().unwrap_err();
    assert!(error.contains("proposal.base_stale"), "{error}");
    assert!(
        twin.app
            .prepare_proposal_step()
            .unwrap_err()
            .contains("proposal.base_stale")
    );
    assert!(twin.app.session.history().pending().is_none());
    // Undo the person's edit: the proposal continues where it was.
    for keycode in [Keycode::Z, Keycode::Return] {
        acknowledge_ui(&mut twin.app);
        route(&mut twin.app, &key(keycode)).unwrap();
    }
    prepare_and_accept(&mut twin, 1);
    prepare_and_accept(&mut twin, 2);
    assert_eq!(
        files(&twin.person.path().join("world")),
        files(&twin.agent.path().join("world"))
    );
}

#[test]
fn a_world_ahead_of_the_progress_offers_continue_from_its_step() {
    let mut twin = twin(3, |_| {}, |_| {}, None);
    open(&mut twin);
    // The person makes the proposal's first edit themselves: the world is
    // after step 1, while the progress still expects step 1.
    let action = twin.proposal.steps()[0].action().clone();
    twin.app.action(action).unwrap();
    twin.app.action(Action::Accept).unwrap();
    assert_eq!(state(&twin.app), State::Moved { resume: Some(1) });
    let texts = panel(&mut twin.app);
    assert!(
        texts.iter().any(|text| text == "World: after step 1 of 3"),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text == "The world moved: undo your edits or continue from step 2."),
        "{texts:?}"
    );
    assert_eq!(
        control(&mut twin.app),
        Some((
            "Continue from step 2 (G)".into(),
            Control::ProposalContinue,
            true
        ))
    );
    // Preparing the expected step directly is refused, with nothing pending.
    assert!(
        twin.app
            .prepare_proposal_step()
            .unwrap_err()
            .contains("proposal.base_stale")
    );
    assert!(twin.app.session.history().pending().is_none());
    press_control(&mut twin.app);
    assert_eq!(state(&twin.app), State::Ready { step: 1 });
    prepare_and_accept(&mut twin, 1);
    prepare_and_accept(&mut twin, 2);
}

#[test]
fn a_grant_the_persons_catalog_lacks_is_refused_with_the_engines_code() {
    let mut twin = twin(
        3,
        |_| {},
        |root| {
            edit_json(root, "preview.jsonc", |catalog| {
                catalog["editing"]["terrain"][0]["values"] = json!(["grass"]);
            });
        },
        None,
    );
    open(&mut twin);
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Catalog differs; your grants apply"),
        "{texts:?}"
    );
    assert_eq!(
        evidence(&twin, "proposal-open.json")["notices"],
        json!([{"catalog_differs": {"proposal": hex(&twin.proposal.catalog()),
            "session": hex(&twin.app.session.catalog().revision())}}])
    );
    prepare_and_accept(&mut twin, 0);
    prepare_and_accept(&mut twin, 1);
    let before = revision(&twin.app.session);
    let world = files(&twin.person.path().join("world"));
    for _ in 0..2 {
        acknowledge_ui(&mut twin.app);
        let error = route(&mut twin.app, &key(Keycode::G)).unwrap_err();
        assert!(error.contains("proposal.step_refused"), "{error}");
        assert_eq!(
            state(&twin.app),
            State::Refused {
                step: 2,
                code: RefusalCode::ProposalStepRefused,
                engine: Some(RefusalCode::GrantMissing)
            }
        );
        assert!(twin.app.session.history().pending().is_none());
        assert_eq!(revision(&twin.app.session), before);
        let texts = panel(&mut twin.app);
        assert!(
            texts
                .iter()
                .any(|text| text
                    == "Refused: proposal.step_refused (grant.missing). Nothing changed."),
            "{texts:?}"
        );
        // Refused, not stopped: trying again is offered.
        assert_eq!(control(&mut twin.app).map(|control| control.2), Some(true));
    }
    assert_eq!(files(&twin.person.path().join("world")), world);
    // Any action of the person's own clears the refusal, and it does not
    // come back after an edit and its undo.
    let expected = revision(&twin.app.session);
    twin.app
        .action(Action::Move {
            uid: "marker_2".into(),
            x: 2,
            y: 5,
            expected,
        })
        .unwrap();
    for keycode in [Keycode::Return, Keycode::Z, Keycode::Return] {
        acknowledge_ui(&mut twin.app);
        route(&mut twin.app, &key(keycode)).unwrap();
    }
    assert_eq!(revision(&twin.app.session), before);
    assert_eq!(state(&twin.app), State::Ready { step: 2 });
    // At the smallest window, while the step is refused, the catalog
    // notice (a likely cause) is laid out and the counts give way.
    smallest_window(&mut twin.app);
    acknowledge_ui(&mut twin.app);
    assert!(route(&mut twin.app, &key(Keycode::G)).is_err());
    acknowledge_ui(&mut twin.app);
    let lines = block_lines(&mut twin.app);
    assert!(
        lines
            .iter()
            .any(|line| line == "Catalog differs; your grants apply"),
        "{lines:#?}"
    );
    assert!(
        !lines.iter().any(|line| line.starts_with("Applied ")),
        "{lines:#?}"
    );
}

/// The proposal with its body changed by `change` and its transaction
/// digests fixed up, as a writer who can recompute digests would.
pub(super) fn forged(proposal: &Proposal, change: impl FnOnce(&mut Value)) -> Proposal {
    let document: Value = serde_json::from_slice(proposal.bytes()).unwrap();
    let mut body = document["body"].clone();
    change(&mut body);
    for step in body["steps"].as_array_mut().unwrap() {
        let text = step["transaction"].as_str().unwrap().to_owned();
        step["transaction_sha256"] = Value::from(hex(&sha256(&text)));
    }
    let body = body.to_string();
    let bytes = format!(
        "{{\"body\":{body},\"body_sha256\":{},\"format\":\"tilewright.editor-proposal/1\"}}",
        Value::from(hex(&sha256(&body)))
    );
    Proposal::decode(bytes.as_bytes()).unwrap_or_else(|error| panic!("{} {error}", error.code()))
}

#[test]
fn a_diverged_step_is_refused_and_changes_nothing() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    twin.proposal = forged(&twin.proposal, |body| {
        body["steps"][1]["action"]["value"] = json!("Other");
        body["editor"] = json!("9.9.9-other");
    });
    open(&mut twin);
    let texts = panel(&mut twin.app);
    assert!(
        texts.iter().any(|text| *text
            == format!(
                "Recorded by another editor build (9.9.9-other; this is {}): steps may diverge",
                crate::build_version::EDITOR_VERSION
            )),
        "{texts:?}"
    );
    // Before preparing, the step's words come from its Action: "Other".
    prepare_and_accept(&mut twin, 0);
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Step 2 of 2: Set label on marker_1 to \"Other\""),
        "{texts:?}"
    );
    let before = revision(&twin.app.session);
    let depths = twin.app.session.history().depths();
    let world = files(&twin.person.path().join("world"));
    let error = try_press_control(&mut twin.app).unwrap_err();
    assert!(error.contains("proposal.step_diverged"), "{error}");
    assert_eq!(
        state(&twin.app),
        State::Refused {
            step: 1,
            code: RefusalCode::ProposalStepDiverged,
            engine: None
        }
    );
    assert!(twin.app.session.history().pending().is_none());
    assert_eq!(revision(&twin.app.session), before);
    assert_eq!(twin.app.session.history().depths(), depths);
    assert_eq!(files(&twin.person.path().join("world")), world);
    let texts = panel(&mut twin.app).concat();
    assert!(texts.contains("Diverged"), "{texts}");
    assert!(
        texts.contains(
            "Diverged from its recording. Nothing changed. Recorded by another editor build."
        ),
        "{texts}"
    );
    assert!(texts.contains("9.9.9-other"), "{texts}");
}

#[test]
fn a_proposal_for_another_world_does_not_apply_and_editing_goes_on() {
    let mut twin = twin(
        2,
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
    open(&mut twin);
    assert_eq!(state(&twin.app), State::DoesNotApply);
    let texts = panel(&mut twin.app);
    for expected in [
        "Does not apply here (proposal.base_stale)".to_owned(),
        format!("Its base: {}", &hex(&twin.proposal.base())[..16]),
        format!("This world: {}", &hex(&revision(&twin.app.session))[..16]),
        "Does not apply to this world; nothing can be prepared.".to_owned(),
    ] {
        assert!(texts.contains(&expected), "{expected} {texts:?}");
    }
    assert_eq!(control(&mut twin.app), None);
    assert!(twin.app.prepare_proposal_step().is_err());
    acknowledge_ui(&mut twin.app);
    assert!(route(&mut twin.app, &key(Keycode::G)).is_err());
    assert!(twin.app.session.history().pending().is_none());
    assert_eq!(
        evidence(&twin, "proposal-open.json")["opened"],
        json!({"applies": false, "code": "proposal.base_stale",
            "world": hex(&revision(&twin.app.session))})
    );
    // Normal editing stays available.
    let expected = revision(&twin.app.session);
    twin.app
        .action(Action::Move {
            uid: "marker_1".into(),
            x: 5,
            y: 2,
            expected,
        })
        .unwrap();
    enter(&mut twin.app);
    assert_ne!(revision(&twin.app.session), expected);
    assert_eq!(state(&twin.app), State::DoesNotApply);
}

#[test]
fn a_world_already_after_a_step_resumes_at_start_up() {
    let mut twin = twin(3, |_| {}, |_| {}, None);
    // The person applied two steps in an earlier session.
    for step in 0..2 {
        let action = twin.proposal.steps()[step].action().clone();
        twin.app.action(action).unwrap();
        twin.app.action(Action::Accept).unwrap();
    }
    open(&mut twin);
    assert_eq!(
        evidence(&twin, "proposal-open.json")["opened"],
        json!({"resumed_after": 2})
    );
    let texts = panel(&mut twin.app);
    assert!(
        texts.iter().any(|text| text == "World: after step 2 of 3"),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text == "Applied 2 of 3, remaining 1"),
        "{texts:?}"
    );
    prepare_and_accept(&mut twin, 2);
    assert_eq!(state(&twin.app), State::Finished);
    assert_eq!(
        evidence(&twin, "proposal-step-000001.json")["step"],
        json!(3)
    );
}

#[test]
fn a_reopen_keeps_the_history_and_the_progress() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    open(&mut twin);
    prepare_and_accept(&mut twin, 0);
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::R)).unwrap();
    assert_eq!(state(&twin.app), State::Ready { step: 1 });
    prepare_and_accept(&mut twin, 1);
    assert_eq!(state(&twin.app), State::Finished);
}

#[test]
fn the_key_waits_for_the_displayed_panel() {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    // A change the person has not seen yet: the frame is not presented.
    twin.app.presented_generation = None;
    let error = route(&mut twin.app, &key(Keycode::G)).unwrap_err();
    assert!(error.contains("displayed panel"), "{error}");
    assert!(twin.app.session.history().pending().is_none());
    acknowledge_ui(&mut twin.app);
    route(&mut twin.app, &key(Keycode::G)).unwrap();
    assert_eq!(state(&twin.app), State::Pending { step: 0 });
}

#[test]
fn a_blocked_session_offers_nothing() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    open(&mut twin);
    twin.app.blocked = Some("test".into());
    assert_eq!(state(&twin.app), State::Blocked);
    assert_eq!(control(&mut twin.app), None);
    assert!(twin.app.prepare_proposal_step().is_err());
    assert!(twin.app.continue_proposal().is_err());
    assert!(twin.app.session.history().pending().is_none());
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "This session cannot continue (blocked)."),
        "{texts:?}"
    );
}

#[test]
fn hostile_labels_and_action_text_reach_no_row_unescaped() {
    let long = "x".repeat(5000);
    let label = format!(
        "lab\u{1b}]0;t\u{7}\u{202e}el\u{200b}\u{2028}\u{2029}\u{feff}\u{e0041}{}",
        &long[..150]
    );
    let hostile = format!("v\u{1b}[2J\u{202e}\u{200b}\u{2028}\n\t{}", &long[..3000]);
    let mut twin = twin(1, |_| {}, |_| {}, Some(&label));
    twin.proposal = forged(&twin.proposal, |body| {
        let mut step = body["steps"][0].clone();
        step["action"] = json!({"kind": "set_field", "uid": format!("u\u{202e}\u{1b}{}", &long[..4000]),
            "field": "f\u{2028}\u{200b}", "value": {"k": hostile}, "expected": step["before"]});
        body["steps"][0] = step;
    });
    open(&mut twin);
    let rows = panel(&mut twin.app);
    assert!(
        rows.iter().any(|text| text
            .starts_with("Step 1 of 1: Set f\\u{2028}\\u{200b} on u\\u{202e}\\u{1b}xxx")
            && text.contains(" to {")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|text| text.starts_with(
            "Client label (unverified): lab\\u{1b}]0;t\\u{7}\\u{202e}el\\u{200b}\\u{2028}"
        )),
        "{rows:?}"
    );
    let bound = row_characters(&mut twin.app.text, twin.app.ui.sidebar_width());
    for scroll in 0..2 {
        twin.app.sidebar_scroll = scroll;
        let texts = sidebar_texts(&mut twin.app);
        for text in texts.iter().chain(&rows) {
            assert!(
                !text.chars().any(tilewright_editor_observe::node::escaped),
                "a raw character in {text:?}"
            );
            assert!(text.chars().count() <= bound, "{}", text.len());
        }
    }
    let opened =
        fs::read_to_string(twin.person.path().join("evidence/proposal-open.json")).unwrap();
    let client = serde_json::from_str::<Value>(&opened).unwrap()["client"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        !client.chars().any(tilewright_editor_observe::node::escaped),
        "{client:?}"
    );
    assert!(client.starts_with("lab\\u{1b}"), "{client}");
    // The engine refuses the step (no such object), and nothing else
    // happens.
    let error = try_press_control(&mut twin.app).unwrap_err();
    assert!(error.contains("(target.unknown)"), "{error}");
    assert!(twin.app.session.history().pending().is_none());
}

#[test]
fn every_kind_reads_as_words_from_its_action_and_the_persons_world() {
    let twin = twin(KINDS, |_| {}, |_| {}, None);
    let words: Vec<String> = (0..KINDS)
        .map(|step| {
            action_words(
                twin.proposal.steps()[step].action(),
                twin.app.session.world(),
                twin.app.session.catalog(),
            )
        })
        .collect();
    assert_eq!(
        words,
        [
            "Move marker_1 to (4, 2), from (3, 2), on main/0",
            "Set label on marker_1 to \"Renamed\"",
            "Paint (0, 0) with sand (floor on main/0)",
            "Paint 2 cells with sand (floor on main/0)",
            "Paint 1 cell into zone path (on main/0)",
            "Delete marker_2 at (1, 4) (marker on main/0)",
            "Place marker at (2, 2) on main/0",
            "Rename zone path to Trail on main",
            "Put zone path under meadow on main",
            "Create zone camp at the top level, named Camp, on main",
            "Delete zone spare on main",
            "Create anchor camp at (2, 2) on main",
            "Move anchor arrival to (6, 3), from (7, 3), on main",
            "Delete anchor lookout at (-2, 0), outside the view on main",
            "Point the portal of gate_west to anchor arrival at (7, 3) on main",
        ]
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn outside_the_view_is_said_where_the_world_has_it() {
    let (_person, app) = person_application(|root| {
        edit_json(root, "planes/main/anchors.jsonc", |anchors| {
            anchors["anchors"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": "far", "layer": 0, "at": [400, 3]}));
        });
    });
    let expected = revision(&app.session);
    let words = |action: &Action| action_words(action, app.session.world(), app.session.catalog());
    assert_eq!(
        words(&Action::DeleteAnchor {
            plane: "main".into(),
            anchor: "far".into(),
            expected
        }),
        "Delete anchor far at (400, 3), outside the view on main"
    );
    assert_eq!(
        words(&Action::DeleteAnchor {
            plane: "main".into(),
            anchor: "gone".into(),
            expected
        }),
        "Delete anchor gone on main (not in this world)"
    );
    assert_eq!(
        words(&Action::PaintZones {
            plane: "main".into(),
            layer: 0,
            cells: vec![[1, 1], [900, 1]],
            zone: None,
            expected
        }),
        "Paint 2 cells, some outside the view back to the default zone (on main/0)"
    );
    assert_eq!(
        words(&Action::Move {
            uid: "missing".into(),
            x: 1,
            y: 1,
            expected
        }),
        "Move missing to (1, 1) (not placed in this world)"
    );
    assert_eq!(
        words(&Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Location {
                plane: "main".into(),
                layer: 0,
                at: [2, 3]
            }),
            expected
        }),
        "Point the portal of gate_west to (2, 3) on main/0"
    );
    // Where the words name a tile outside the view, they say so: a portal's
    // location or anchor, and the object a field change concerns.
    assert_eq!(
        words(&Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Location {
                plane: "main".into(),
                layer: 1,
                at: [2, 3]
            }),
            expected
        }),
        "Point the portal of gate_west to (2, 3), outside the view on main/1"
    );
    assert_eq!(
        words(&Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Anchor {
                plane: "main".into(),
                anchor: "far".into()
            }),
            expected
        }),
        "Point the portal of gate_west to anchor far at (400, 3), outside the view on main"
    );
    assert_eq!(
        words(&Action::SetPortal {
            uid: "gate_west".into(),
            target: Some(PortalTarget::Anchor {
                plane: "main".into(),
                anchor: "gone".into()
            }),
            expected
        }),
        "Point the portal of gate_west to anchor gone on main"
    );
    assert_eq!(
        words(&Action::SetField {
            uid: "marker_negative".into(),
            field: "label".into(),
            value: json!("Far"),
            expected
        }),
        "Set label on marker_negative to \"Far\" (outside the view)"
    );
    assert_eq!(
        words(&Action::SetPortal {
            uid: "gate_west".into(),
            target: None,
            expected
        }),
        "Clear the portal of gate_west"
    );
}

#[test]
fn a_long_proposal_shows_counts_only() {
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    let mut session = open_session(&world, AGENT);
    for step in 0..256 {
        let expected = revision(&session);
        accept(
            &mut session,
            Action::Move {
                uid: "marker_1".into(),
                x: 4 + step % 2,
                y: 2,
                expected,
            },
        );
    }
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
    let (_person, mut app) = person_application(|_| {});
    let name = proposal.file_name();
    app.open_proposal(Some((proposal, name))).unwrap();
    let rows = panel_rows(&mut app);
    assert!(rows.len() <= 8, "{rows:?}");
    let texts = panel(&mut app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Applied 0 of 256, remaining 256"),
        "{texts:?}"
    );
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.starts_with("Step "))
            .count(),
        1
    );
    press_control(&mut app);
    enter(&mut app);
    let texts = panel(&mut app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Applied 1 of 256, remaining 255"),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.starts_with("Step 2 of 256: Move marker_1")),
        "{texts:?}"
    );
}

/// The 720x480 window, the camera and the scene resized to it.
fn smallest_window(app: &mut Application) {
    app.ui = crate::workspace_ui::WorkspaceUi::new((720, 480), (720, 480));
    app.view.camera.width = 720;
    app.view.camera.height = 480;
    app.rebuild().unwrap();
}

/// The widest legal panel: a long agent actor, a 256-byte client label, a
/// 64-character editor version, the catalog notice and a refused step.
fn widest_proposal(diverged: bool) -> Proposal {
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    spare_zone(&world);
    // A recorded actor that is not an agent's: the caveat row too.
    let mut session = open_session(&world, &format!("human:{}", "h".repeat(60)));
    for kind in 0..2 {
        let expected = revision(&session);
        accept(&mut session, edit(kind, expected));
    }
    let label = "c".repeat(256);
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        Some(&label),
    )
    .unwrap();
    forged(&proposal, |body| {
        if diverged {
            body["steps"][1]["action"]["value"] = json!("W".repeat(30));
        } else {
            body["steps"][0]["action"]["uid"] = json!("u".repeat(4000));
        }
        body["editor"] = json!(format!("9.9.9-{}", "e".repeat(58)));
        body["catalog"] = json!("ab".repeat(32));
    })
}

#[test]
#[allow(clippy::too_many_lines)]
fn the_panel_fits_the_smallest_window() {
    let proposal = widest_proposal(false);
    assert_eq!(proposal.editor().len(), 64);
    let (_person, mut app) = person_application(spare_zone);
    smallest_window(&mut app);
    let name = proposal.file_name();
    app.open_proposal(Some((proposal, name))).unwrap();
    acknowledge_ui(&mut app);
    // The step is refused (no object has that uid): a refusal and the
    // catalog notice are on the panel too.
    let error = try_press_control(&mut app).unwrap_err();
    assert!(error.contains("proposal.step_refused"), "{error}");
    acknowledge_ui(&mut app);
    let rows = panel_rows(&mut app);
    assert!(
        rows.iter().any(|(kind, _)| *kind == Row::Notice),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|(_, text)| text.starts_with("Refused:")),
        "{rows:?}"
    );
    app.refresh_ui();
    let x = app.ui.sidebar_x();
    let sidebar: Vec<_> = app
        .ui
        .widgets
        .iter()
        .filter(|widget| widget.layout.x >= x && widget.layout.y >= 56)
        .cloned()
        .collect();
    // The block stays within its cap, its control included; the long rows
    // are one fitted line each, cut with a visible "...".
    let button = sidebar
        .iter()
        .find(|widget| widget.control == Some(Control::ProposalPrepare))
        .expect("the control is laid out");
    assert!(button.layout.y + button.layout.height <= 56 + PANEL_HEIGHT);
    let block: Vec<_> = sidebar
        .iter()
        .filter(|widget| widget.layout.y < button.layout.y)
        .collect();
    assert!(
        block.len() <= PANEL_LINES,
        "{:?}",
        block
            .iter()
            .map(|widget| (widget.layout.y, &widget.text))
            .collect::<Vec<_>>()
    );
    for start in [
        "Proposal ",
        "Client label (unverified): ccc",
        "Recorded by another editor build (9.9.9-e",
    ] {
        let line = block
            .iter()
            .find(|widget| widget.text.starts_with(start))
            .unwrap_or_else(|| panic!("{start}"));
        assert!(line.text.ends_with("..."), "{}", line.text);
    }
    // The step's words: cut by the cap (the caveat row takes a line), the
    // cut marked, the destination begun.
    let first = block
        .iter()
        .position(|widget| widget.text.starts_with("Step 1 of 2: Move uuu"))
        .expect("the step row is laid out");
    let step: Vec<&str> = block[first..]
        .iter()
        .map(|widget| widget.text.as_str())
        .take_while(|text| !text.starts_with("Refused"))
        .collect();
    assert!(step.len() < WRAPPED_ROWS, "{step:?}");
    assert!(step.concat().contains(" to (4"), "{step:?}");
    assert!(step.concat().ends_with("..."), "{step:?}");
    // The rest of the sidebar is laid out below the block.
    assert!(sidebar.iter().any(|widget| widget.text == "Inspector"));
    // An open text draft: its field, hint and Review/Discard buttons are
    // laid out under the block and hit-testable; nothing else has its text.
    app.control(Control::Zone).unwrap();
    app.control(Control::ZonePalette(
        tilewright_editor_ui::tools::ZoneChoice::Zone("meadow".into()),
    ))
    .unwrap();
    app.control(Control::ZoneName).unwrap();
    acknowledge_ui(&mut app);
    app.refresh_ui();
    let editable: Vec<_> = app
        .ui
        .widgets
        .iter()
        .filter(|widget| widget.editable.is_some())
        .cloned()
        .collect();
    assert_eq!(editable.len(), 1);
    assert_eq!(editable[0].control, Some(Control::EditDraft));
    let below = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::ProposalPrepare))
        .map(|widget| widget.layout.y + widget.layout.height)
        .unwrap();
    assert!(below <= 56 + PANEL_HEIGHT);
    for control in [
        Control::EditDraft,
        Control::ApplyDraft,
        Control::CancelDraft,
    ] {
        let widget = app
            .ui
            .widgets
            .iter()
            .find(|widget| widget.control.as_ref() == Some(&control))
            .unwrap_or_else(|| panic!("{control:?} is not laid out"));
        assert!(
            widget.layout.y >= below,
            "{control:?} {} {below}",
            widget.layout.y
        );
        assert!(widget.layout.y + widget.layout.height <= 480);
        let hit = app.ui.hit(
            widget.bounds.x + widget.bounds.width / 2,
            widget.bounds.y + widget.bounds.height / 2,
        );
        assert_eq!(hit, crate::workspace_ui::Hit::Control(control));
    }
    let prepare = app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::ProposalPrepare))
        .unwrap();
    assert!(prepare.editable.is_none());
}

/// The block's lines as laid out, above its control (or all of it).
fn block_lines(app: &mut Application) -> Vec<String> {
    app.refresh_ui();
    let x = app.ui.sidebar_x();
    let end = app
        .ui
        .widgets
        .iter()
        .find(|widget| {
            matches!(
                widget.control,
                Some(Control::ProposalPrepare | Control::ProposalContinue)
            )
        })
        .map_or(56 + PANEL_HEIGHT, |widget| widget.layout.y);
    app.ui
        .widgets
        .iter()
        .filter(|widget| widget.layout.x >= x && widget.layout.y >= 56 && widget.layout.y < end)
        .map(|widget| widget.text.clone())
        .collect()
}

#[test]
fn the_widest_panel_keeps_its_state_caveat_editor_and_words_on_screen() {
    for diverged in [false, true] {
        let proposal = widest_proposal(diverged);
        let (_person, mut app) = person_application(spare_zone);
        smallest_window(&mut app);
        let name = proposal.file_name();
        app.open_proposal(Some((proposal, name))).unwrap();
        acknowledge_ui(&mut app);
        if diverged {
            press_control(&mut app);
            enter(&mut app);
        }
        let error = try_press_control(&mut app).unwrap_err();
        assert!(error.contains("proposal.step_"), "{error}");
        acknowledge_ui(&mut app);
        let state = state(&app);
        let expected_code = if diverged {
            RefusalCode::ProposalStepDiverged
        } else {
            RefusalCode::ProposalStepRefused
        };
        assert!(
            matches!(state, State::Refused { code, .. } if code == expected_code),
            "{state:?}"
        );
        let lines = block_lines(&mut app);
        let laid: String = lines.concat().chars().filter(|c| *c != ' ').collect();
        let words = app.proposal.as_ref().unwrap().state_words(state);
        let state_text: String = words.chars().filter(|c| *c != ' ').collect();
        assert!(
            laid.contains(&state_text),
            "{words:?} not whole in {lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "Recorded actor is not an agent actor"),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("Recorded by another editor build")),
            "{lines:#?}"
        );
        let step = if diverged {
            "Step 2 of 2: Set label on"
        } else {
            "Step 1 of 2: Move uuu"
        };
        assert!(
            lines.iter().any(|line| line.starts_with(step)),
            "{lines:#?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("Proposal ") && line.contains(", recorded actor: ")),
            "{lines:#?}"
        );
    }
}

#[test]
fn the_catalog_notice_outranks_the_counts_while_a_step_is_refused() {
    let rows = [
        (Row::Header, "h".to_owned()),
        (Row::State, "s".to_owned()),
        (Row::Counts, "counts".to_owned()),
        (Row::Notice, "notice".to_owned()),
    ];
    let (_person, mut app) = person_application(|_| {});
    let width = app.ui.sidebar_width();
    assert_eq!(
        panel_lines(&mut app.text, &rows, width, 3, true),
        ["h", "s", "notice"]
    );
    assert_eq!(
        panel_lines(&mut app.text, &rows, width, 3, false),
        ["h", "s", "counts"]
    );
    // A taller window gives the block up to a third of the sidebar.
    assert_eq!(
        crate::prototype::proposal_view::panel_height(480),
        PANEL_HEIGHT
    );
    assert_eq!(crate::prototype::proposal_view::panel_height(1200), 381);
}

#[test]
fn an_ordinary_proposal_keeps_every_row_at_the_smallest_window() {
    let mut twin = twin(2, |_| {}, |_| {}, Some(LABEL));
    smallest_window(&mut twin.app);
    open(&mut twin);
    assert_eq!(
        row_enabled(&mut twin.app, &Control::ProposalPrepare),
        Some(true)
    );
    let texts = sidebar_texts(&mut twin.app).concat();
    for row in panel(&mut twin.app) {
        let start: String = row.chars().take(12).collect();
        assert!(texts.contains(&start), "{row} in {texts}");
    }
    press_control(&mut twin.app);
    assert_eq!(state(&twin.app), State::Pending { step: 0 });
}

#[test]
fn a_cut_row_keeps_its_deciding_facts_and_every_bound_holds() {
    let (_person, mut app) = person_application(|_| {});
    let width = app.ui.sidebar_width();
    let expected = revision(&app.session);
    let long = "x".repeat(100);
    let words = |action: &Action, app: &Application| {
        action_words(action, app.session.world(), app.session.catalog())
    };
    // The 48-character cut of every string from the proposal.
    let set = words(
        &Action::SetField {
            uid: "marker_1".into(),
            field: "label".into(),
            value: json!(long),
            expected,
        },
        &app,
    );
    assert_eq!(
        set,
        format!("Set label on marker_1 to \"{}…", "x".repeat(46))
    );
    // A long move keeps its destination, and a new zone its parent, in the
    // three lines the step row has.
    let laid = |text: String, app: &mut Application| {
        panel_lines(
            &mut app.text,
            &[(Row::Step, text)],
            width,
            PANEL_LINES,
            false,
        )
    };
    let moved = format!(
        "Step 1 of 1: {}",
        words(
            &Action::Move {
                uid: long.clone(),
                x: 4,
                y: 900,
                expected
            },
            &app
        )
    );
    let lines = laid(moved, &mut app);
    assert_eq!(lines.len(), WRAPPED_ROWS);
    assert!(lines.concat().contains("to (4, 900)"), "{lines:?}");
    let created = format!(
        "Step 1 of 1: {}",
        words(
            &Action::CreateZone {
                plane: long.clone(),
                zone: long.clone(),
                name: Some(long.clone()),
                parent: Some(format!("parent{long}")),
                expected,
            },
            &app
        )
    );
    let lines = laid(created, &mut app);
    assert_eq!(lines.len(), WRAPPED_ROWS, "{lines:?}");
    assert!(lines.concat().contains("under parentxxx"), "{lines:?}");
    assert!(lines[WRAPPED_ROWS - 1].ends_with("..."), "{lines:?}");
    // The row bound is what three lines hold: that many full stops wrap to
    // three lines, a dozen more to four.
    let characters = row_characters(&mut app.text, width);
    let dots = |count: usize, app: &mut Application| {
        crate::workspace_render::wrap_text(&mut app.text, &".".repeat(count), width, 20).len()
    };
    assert!(dots(characters, &mut app) <= WRAPPED_ROWS);
    assert!(dots(characters + 12, &mut app) > WRAPPED_ROWS);
    // And rows are cut to it, with a visible ellipsis.
    let twin = twin(1, |_| {}, |_| {}, Some(&"l".repeat(200)));
    let mut twin = twin;
    open(&mut twin);
    let state = state(&twin.app);
    let rows = twin
        .app
        .proposal
        .as_ref()
        .unwrap()
        .rows(&twin.app.session, state, 40);
    let label = &rows.iter().find(|(kind, _)| *kind == Row::Label).unwrap().1;
    assert_eq!(label.chars().count(), 40);
    assert!(label.ends_with('…'));
    // A one-line row stays one line; the state line wraps to two.
    let lines = panel_lines(
        &mut twin.app.text,
        &[
            (Row::Header, "h ".repeat(200)),
            (Row::State, "s ".repeat(200)),
        ],
        width,
        PANEL_LINES,
        false,
    );
    assert_eq!(lines.len(), 3, "{lines:?}");
}

#[test]
fn a_chain_that_revisits_a_revision_says_redo_continues_after_an_undo() {
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    let mut session = open_session(&world, AGENT);
    for x in [4, 3] {
        let expected = revision(&session);
        accept(
            &mut session,
            Action::Move {
                uid: "marker_1".into(),
                x,
                y: 2,
                expected,
            },
        );
    }
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
    assert_eq!(proposal.base(), proposal.result());
    let (_person, mut app) = person_application(|_| {});
    let name = proposal.file_name();
    app.open_proposal(Some((proposal, name))).unwrap();
    acknowledge_ui(&mut app);
    press_control(&mut app);
    enter(&mut app);
    for keycode in [Keycode::Z, Keycode::Return] {
        acknowledge_ui(&mut app);
        route(&mut app, &key(keycode)).unwrap();
    }
    assert_eq!(
        state(&app),
        State::Several {
            redo: true,
            undo: false
        }
    );
    assert_eq!(control(&mut app), None);
    let texts = panel(&mut app);
    assert!(
        texts.iter().any(|text| text
            == "The world is at several places on its path: Redo continues the proposal."),
        "{texts:?}"
    );
    for keycode in [Keycode::Y, Keycode::Return] {
        acknowledge_ui(&mut app);
        route(&mut app, &key(keycode)).unwrap();
    }
    assert_eq!(state(&app), State::Ready { step: 1 });
    press_control(&mut app);
    enter(&mut app);
    assert_eq!(state(&app), State::Finished);
}

#[test]
fn several_places_without_a_redo_say_what_continues() {
    // A -> B -> A -> C: after step 1 the person moves the marker back by
    // hand. Nothing can be redone; undoing that edit continues.
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    let mut session = open_session(&world, AGENT);
    for x in [4, 3, 5] {
        let expected = revision(&session);
        accept(
            &mut session,
            Action::Move {
                uid: "marker_1".into(),
                x,
                y: 2,
                expected,
            },
        );
    }
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
    let (_person, mut app) = person_application(|_| {});
    let name = proposal.file_name();
    app.open_proposal(Some((proposal, name))).unwrap();
    acknowledge_ui(&mut app);
    press_control(&mut app);
    enter(&mut app);
    let expected = revision(&app.session);
    app.action(Action::Move {
        uid: "marker_1".into(),
        x: 3,
        y: 2,
        expected,
    })
    .unwrap();
    enter(&mut app);
    assert_eq!(
        state(&app),
        State::Several {
            redo: false,
            undo: true
        }
    );
    assert_eq!(control(&mut app), None);
    let texts = panel(&mut app);
    assert!(
        texts.iter().any(|text| text
            == "The world is at several places on its path: undo your last edits to continue."),
        "{texts:?}"
    );
    for keycode in [Keycode::Z, Keycode::Return] {
        acknowledge_ui(&mut app);
        route(&mut app, &key(keycode)).unwrap();
    }
    assert_eq!(state(&app), State::Ready { step: 1 });
    // With nothing to undo or redo, the panel says it cannot continue.
    assert_eq!(
        app.proposal.as_ref().unwrap().state_words(State::Several {
            redo: false,
            undo: false
        }),
        "The world is at several places on its path: the proposal cannot continue from here."
    );
}

#[test]
fn a_failed_accept_shows_its_own_error() {
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    press_control(&mut twin.app);
    // The recovery store cannot take the record the Accept must retain.
    let recovery = twin.person.path().join("recovery");
    fs::set_permissions(
        &recovery,
        std::os::unix::fs::PermissionsExt::from_mode(0o500),
    )
    .unwrap();
    acknowledge_ui(&mut twin.app);
    let error = route(&mut twin.app, &key(Keycode::Return)).unwrap_err();
    fs::set_permissions(
        &recovery,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    assert!(error.contains("Recovery("), "{error}");
    assert!(!error.contains("applied"), "{error}");
    assert_eq!(revision(&twin.app.session), twin.proposal.base());
    assert!(
        evidence_names(&twin)
            .iter()
            .all(|name| !name.starts_with("proposal-step-0"))
    );
}

#[test]
fn a_recorded_actor_that_is_not_an_agent_is_shown_with_a_caveat() {
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    let mut session = open_session(&world, "human:x");
    let expected = revision(&session);
    accept(&mut session, edit(0, expected));
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
    let (_person, mut app) = person_application(|_| {});
    let name = proposal.file_name();
    app.open_proposal(Some((proposal, name))).unwrap();
    acknowledge_ui(&mut app);
    let texts = panel(&mut app);
    assert!(texts[0].ends_with(", recorded actor: human:x"), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|text| text == "Recorded actor is not an agent actor"),
        "{texts:?}"
    );
    press_control(&mut app);
    assert!(
        app.status_label()
            .starts_with("Step 1 of 1 matches its recording (recorded as human:x)."),
        "{}",
        app.status_label()
    );
    // Not refused: the person decides.
    enter(&mut app);
    assert_eq!(state(&app), State::Finished);
    // An agent actor has no caveat.
    let mut twin = twin(1, |_| {}, |_| {}, None);
    open(&mut twin);
    assert!(
        !panel(&mut twin.app)
            .iter()
            .any(|text| text.contains("not an agent actor"))
    );
}

#[test]
fn a_delete_says_what_the_object_carries() {
    let (_person, app) = person_application(|root| {
        edit_json(root, "planes/main/layers/0/0_0.jsonc", |chunk| {
            chunk["objects"][1]["inventory"] = json!([
                {"proto": "crate", "inventory": [{"proto": "marker"}]},
                {"proto": "marker"}
            ]);
        });
    });
    let expected = revision(&app.session);
    let words = |uid: &str| {
        action_words(
            &Action::Remove {
                uid: uid.into(),
                expected,
            },
            app.session.world(),
            app.session.catalog(),
        )
    };
    assert_eq!(
        words("marker_2"),
        "Delete marker_2 at (1, 4) and the 3 objects it carries (marker on main/0)"
    );
    assert_eq!(
        words("marker_1"),
        "Delete marker_1 at (3, 2) (marker on main/0)"
    );
}

#[test]
fn a_record_that_cannot_be_written_still_leaves_the_step_applied() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    open(&mut twin);
    // Something already holds the first record's name.
    fs::write(
        twin.person
            .path()
            .join("evidence/proposal-step-000001.json"),
        b"{}",
    )
    .unwrap();
    press_control(&mut twin.app);
    acknowledge_ui(&mut twin.app);
    let error = route(&mut twin.app, &key(Keycode::Return)).unwrap_err();
    assert!(
        error.starts_with(
            "Step 1 applied; its evidence record proposal-step-000001.json could not be written: "
        ),
        "{error}"
    );
    assert_eq!(
        revision(&twin.app.session),
        twin.proposal.steps()[0].after()
    );
    assert_eq!(state(&twin.app), State::Ready { step: 1 });
    prepare_and_accept(&mut twin, 1);
    // The next record takes the next number; the first is never reused.
    assert_eq!(evidence(&twin, "proposal-step-000002.json")["step"], 2);
    assert_eq!(evidence(&twin, "proposal-step-000001.json"), json!({}));
}

#[test]
fn g_then_enter_before_the_frame_is_presented_is_refused() {
    let mut twin = twin(2, |_| {}, |_| {}, None);
    open(&mut twin);
    acknowledge_ui(&mut twin.app);
    let before = revision(&twin.app.session);
    route(&mut twin.app, &key(Keycode::G)).unwrap();
    assert_eq!(state(&twin.app), State::Pending { step: 0 });
    let error = route(&mut twin.app, &key(Keycode::Return)).unwrap_err();
    assert!(error.contains("waits"), "{error}");
    assert_eq!(revision(&twin.app.session), before);
    assert!(twin.app.session.history().pending().is_some());
    // The Accept button likewise waits for the presented frame.
    let accept = twin
        .app
        .ui
        .widgets
        .iter()
        .find(|widget| widget.control == Some(Control::Accept))
        .cloned()
        .unwrap();
    twin.app.presented_generation = None;
    #[allow(clippy::cast_precision_loss)]
    let at = [
        (accept.bounds.x + accept.bounds.width / 2) as f32,
        (accept.bounds.y + accept.bounds.height / 2) as f32,
    ];
    assert!(route(&mut twin.app, &button(true, at)).is_err());
    assert_eq!(revision(&twin.app.session), before);
    // The key handler's own guard, behind the event router's: Enter
    // reaching it before the prepared frame is presented is refused too.
    twin.app.presented_generation = None;
    let error = crate::prototype::native_key(&mut twin.app, Keycode::Return).unwrap_err();
    assert!(
        error.contains("waits for the current displayed input"),
        "{error}"
    );
    assert_eq!(revision(&twin.app.session), before);
    enter(&mut twin.app);
    assert_eq!(
        revision(&twin.app.session),
        twin.proposal.steps()[0].after()
    );
}

/// A small deterministic generator (no new dependency).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
}

/// Random interleavings of G, Enter, Esc, Z, Y, R, the person's own edits
/// and presented or unpresented frames against a 3-step proposal: the
/// committed world changes only on an Enter on a presented frame while a
/// change was pending; a step is applied only from its pending state, at
/// its recorded result; the panel says "pending" exactly when the pending
/// change is the one the control prepared; no control while anything is
/// pending; one record per applied step; nothing panics. Seeds:
/// `TILEWRIGHT_PROPOSAL_SEEDS` (default 12; the review ran 80).
#[test]
#[allow(clippy::too_many_lines)]
fn interruptions_never_apply_a_step_without_the_persons_accept() {
    let seeds: u64 = std::env::var("TILEWRIGHT_PROPOSAL_SEEDS")
        .ok()
        .and_then(|seeds| seeds.parse().ok())
        .unwrap_or(12);
    for seed in 0..seeds {
        let mut twin = twin(3, |_| {}, |_| {}, None);
        open(&mut twin);
        let mut rng = Lcg(seed * 7919 + 13);
        let mut prepared = false;
        let mut applied = 0usize;
        let mut log = Vec::new();
        for _ in 0..40 {
            if rng.next(10) < 7 {
                acknowledge_ui(&mut twin.app);
            }
            let presented = twin.app.presented_generation == Some(twin.app.view.frame_revision);
            let before = revision(&twin.app.session);
            let pending_before = twin.app.session.history().pending().is_some();
            let state_before = state(&twin.app);
            let operation = rng.next(8);
            let result = match operation {
                0 | 1 => route(&mut twin.app, &key(Keycode::G)),
                2 | 3 => route(&mut twin.app, &key(Keycode::Return)),
                4 => route(&mut twin.app, &key(Keycode::Escape)),
                5 => route(
                    &mut twin.app,
                    &key(if rng.next(2) == 0 {
                        Keycode::Z
                    } else {
                        Keycode::Y
                    }),
                ),
                6 => {
                    let expected = revision(&twin.app.session);
                    #[allow(clippy::cast_possible_wrap)]
                    let x = 1 + rng.next(4) as i64;
                    twin.app.action(Action::Move {
                        uid: "marker_2".into(),
                        x,
                        y: 4,
                        expected,
                    })
                }
                _ => route(&mut twin.app, &key(Keycode::R)),
            };
            log.push(format!(
                "{operation} presented={presented} {state_before:?} -> {:?}",
                result.as_ref().err()
            ));
            let after = revision(&twin.app.session);
            let pending_after = twin.app.session.history().pending().is_some();
            let enter = matches!(operation, 2 | 3);
            if after != before {
                assert!(
                    enter && pending_before && presented,
                    "seed {seed}: the world changed without an accepted Enter: {log:#?}"
                );
            }
            if let (true, true, State::Pending { step }) = (enter, after != before, state_before) {
                // The person accepted the prepared step: it lands exactly on
                // its recorded result and writes one record.
                assert!(prepared, "seed {seed}: {log:#?}");
                assert_eq!(after, twin.proposal.steps()[step].after(), "seed {seed}");
                applied += 1;
            }
            if matches!(operation, 0 | 1) && !pending_before && pending_after {
                prepared = true;
            }
            if !pending_after {
                prepared = false;
            }
            let shown = state(&twin.app);
            assert_eq!(
                matches!(shown, State::Pending { .. }),
                prepared,
                "seed {seed}: {shown:?} {log:#?}"
            );
            assert_eq!(
                twin.app.status_label().contains("matches its recording"),
                prepared,
                "seed {seed}"
            );
            if pending_after {
                assert_eq!(control(&mut twin.app), None, "seed {seed}: {log:#?}");
            }
            let _ = panel(&mut twin.app);
        }
        let records = evidence_names(&twin)
            .into_iter()
            .filter(|name| name.starts_with("proposal-step-0"))
            .count();
        assert_eq!(records, applied, "seed {seed}: {log:#?}");
    }
}

#[test]
fn start_up_options_go_together_and_name_a_person() {
    let arguments =
        |extra: &[&str]| -> Vec<String> { extra.iter().map(|text| (*text).to_owned()).collect() };
    let others = [
        &"/proc/self/fd/3".into(),
        &"/proc/self/fd/4".into(),
        &"/proc/self/fd/5".into(),
    ];
    let run = |extra: &[&str]| crate::prototype::proposal_arguments(&arguments(extra), others);
    assert_eq!(run(&[]).unwrap(), (None, "editor:prototype".into()));
    assert_eq!(
        run(&[
            "--proposal-fd",
            "6",
            "--proposal-name",
            "p",
            "--actor",
            "human:a b"
        ])
        .unwrap(),
        (
            Some(("/proc/self/fd/6".into(), "p".into())),
            "human:a b".into()
        )
    );
    for (extra, message) in [
        (&["--proposal-fd", "6"][..], "go together"),
        (&["--proposal-name", "p"][..], "go together"),
        (
            &["--proposal-fd", "4", "--proposal-name", "p"][..],
            "distinct",
        ),
        (
            &["--proposal-fd", "2", "--proposal-name", "p"][..],
            "standard streams",
        ),
        (
            &["--proposal-fd", "x", "--proposal-name", "p"][..],
            "invalid",
        ),
        (&["--actor", "agent:x"][..], "human:<name>"),
        (&["--actor", "human:"][..], "human:<name>"),
        (&["--actor"][..], "missing --actor value"),
    ] {
        let error = run(extra).unwrap_err();
        assert!(error.contains(message), "{extra:?} {error}");
    }
}

#[test]
fn a_bad_proposal_file_is_a_start_up_error_with_its_code() {
    let twin = twin(1, |_| {}, |_| {}, None);
    let directory = tempfile::tempdir_in(scratch()).unwrap();
    let root = directory.path().to_path_buf();
    let name = twin.proposal.file_name();
    fs::write(root.join(&name), twin.proposal.bytes()).unwrap();
    let read = |name: &str| {
        crate::prototype::proposal_view::read(Some(&(root.clone(), name.to_owned())), false)
    };
    let (proposal, read_name) = read(&name).unwrap().unwrap();
    assert_eq!(
        (proposal.body_sha256(), read_name),
        (twin.proposal.body_sha256(), name.clone())
    );
    assert!(
        crate::prototype::proposal_view::read(None, true)
            .unwrap()
            .is_none()
    );
    let other = format!("proposal-{}.json", "0".repeat(64));
    fs::write(root.join(&other), twin.proposal.bytes()).unwrap();
    let oversize = format!("proposal-{}.json", "1".repeat(64));
    fs::File::create(root.join(&oversize))
        .unwrap()
        .set_len((16 << 20) + 1)
        .unwrap();
    let garbled = format!("proposal-{}.json", "2".repeat(64));
    fs::write(root.join(&garbled), b"{\"body\":").unwrap();
    for (name, code) in [
        (other.as_str(), "proposal.digest"),
        (oversize.as_str(), "proposal.budget"),
        (garbled.as_str(), "proposal.invalid"),
        ("proposal.json", "proposal.invalid"),
        (
            &format!("proposal-{}.json", "3".repeat(64)),
            "proposal.store",
        ),
    ] {
        let error = read(name).unwrap_err();
        assert!(
            error.starts_with(&format!("proposal refused: {code}: ")),
            "{name} {error}"
        );
    }
    let error =
        crate::prototype::proposal_view::read(Some(&(root.clone(), name)), true).unwrap_err();
    assert!(error.contains("--reference"), "{error}");
}

/// The state recomputed from scratch, as 56edbcd did: the stored progress is
/// cloned and settled against the session now, and every field that the
/// settle may move (`next`, `stopped`) is read from the settled clone, so a
/// missed funnel settle shows as a difference. The stored refusal (`last`)
/// is the only stored input.
fn fresh_state(app: &Application) -> State {
    use tilewright_editor_proposal::Settled;
    use tilewright_editor_ui::SessionPhase;
    let open = app.proposal.as_ref().unwrap();
    let session = &app.session;
    if app.blocked.is_some() || session.status().phase == SessionPhase::Blocked {
        return State::Blocked;
    }
    let Some(progress) = &open.progress else {
        return State::DoesNotApply;
    };
    let proposal = &open.proposal;
    let positions = open.positions(session);
    let one = if positions.len() == 1 {
        Some(positions[0])
    } else {
        None
    };
    let mut fresh = progress.clone();
    match fresh.settle(proposal, session) {
        Ok(Settled::Pending { step }) => return State::Pending { step },
        Ok(Settled::OtherPending { .. }) => return State::OtherPending,
        Err(_) => return State::Moved { resume: one },
        Ok(_) => {}
    }
    let pending = session.history().pending().is_some();
    if fresh.stopped() {
        return State::Stopped {
            resume: one.filter(|_| !pending),
        };
    }
    if pending {
        return State::OtherPending;
    }
    let steps = proposal.steps().len();
    let next = fresh.next();
    let expected = proposal.steps().get(next).map_or(
        proposal.result(),
        tilewright_editor_proposal::ProposalStep::before,
    );
    if session.history().document().revision() != expected {
        return match positions.as_slice() {
            [p] if *p < next => State::Undone { step: *p },
            [p] => State::Moved { resume: Some(*p) },
            [] if next == steps => State::Finished,
            [] => State::Moved { resume: None },
            _ => {
                let (undo, redo) = session.history().depths();
                State::Several {
                    redo: redo > 0,
                    undo: undo > 0,
                }
            }
        };
    }
    if next == steps {
        return State::Finished;
    }
    match open.last {
        crate::prototype::proposal_view::Last::Refused { step, code, engine } if step == next => {
            State::Refused { step, code, engine }
        }
        crate::prototype::proposal_view::Last::Undone { step } if step == next => {
            State::Undone { step }
        }
        _ => State::Ready { step: next },
    }
}

/// A proposal of moves of `marker_1` to each x in turn (y = 2) on the plain
/// neutral fixture: [4, 3] returns to its base, [4, 3, 5] passes it again.
fn moves_twin(xs: &[i64]) -> Twin {
    let agent_root = tempfile::tempdir_in(scratch()).unwrap();
    let world = agent_root.path().join("world");
    copy_private(&fixture_world("prototype-neutral"), &world);
    spare_zone(&world);
    let mut session = open_session(&world, AGENT);
    for x in xs {
        let expected = revision(&session);
        accept(
            &mut session,
            Action::Move {
                uid: "marker_1".into(),
                x: *x,
                y: 2,
                expected,
            },
        );
    }
    let proposal = Proposal::from_session(
        &session,
        crate::build_version::EDITOR_VERSION,
        RECEIPT,
        None,
    )
    .unwrap();
    let (person, app) = person_application(spare_zone);
    Twin {
        person,
        agent: agent_root,
        app,
        proposal,
    }
}

/// Variants: 0 plain 3 steps; 1 step 2 diverges; 2 step 3 refused by the
/// person's catalog (and the catalog notice); 3 a chain that revisits its
/// base ([4, 3, 5]).
fn oracle_variant(variant: u64) -> Twin {
    match variant {
        0 => twin(3, |_| {}, |_| {}, None),
        1 => {
            let mut twin = twin(3, |_| {}, |_| {}, None);
            twin.proposal = forged(&twin.proposal, |body| {
                body["steps"][1]["action"]["value"] = json!("Other");
                body["editor"] = json!("9.9.9-other");
            });
            twin
        }
        2 => twin(
            3,
            |_| {},
            |root| {
                edit_json(root, "preview.jsonc", |catalog| {
                    catalog["editing"]["terrain"][0]["values"] = json!(["grass"]);
                });
            },
            None,
        ),
        _ => moves_twin(&[4, 3, 5]),
    }
}

/// The application-level property test of the first review, extended: more
/// operations (the person doing the next step's edit themselves, a clear
/// selection, pressing the control with the pointer), four proposal
/// variants (diverged, refused, revisiting) and an oracle that recomputes the
/// state from scratch after every operation and compares it with the stored
/// reading's state and with what the laid-out panel shows (state line and
/// control). Seeds: `TILEWRIGHT_PROPOSAL_ORACLE_SEEDS` (default 8; the
/// delta review ran 400).
#[test]
#[allow(clippy::too_many_lines)]
fn the_stored_reading_never_differs_from_a_fresh_settle() {
    let seeds: u64 = std::env::var("TILEWRIGHT_PROPOSAL_ORACLE_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let operations = 40;
    let mut seen: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut accepted_total = 0usize;
    for seed in 0..seeds {
        let variant = seed % 4;
        let mut twin = oracle_variant(variant);
        open(&mut twin);
        let mut rng = Lcg(seed * 104_729 + 7);
        let mut prepared = false;
        let mut applied = 0usize;
        let mut log = Vec::new();
        for _ in 0..operations {
            if rng.next(10) < 7 {
                acknowledge_ui(&mut twin.app);
            }
            let presented = twin.app.presented_generation == Some(twin.app.view.frame_revision);
            let before = revision(&twin.app.session);
            let pending_before = twin.app.session.history().pending().is_some();
            let state_before = state(&twin.app);
            let operation = rng.next(12);
            let result = match operation {
                0 | 1 => route(&mut twin.app, &key(Keycode::G)),
                2 | 3 => route(&mut twin.app, &key(Keycode::Return)),
                4 => route(&mut twin.app, &key(Keycode::Escape)),
                5 => route(&mut twin.app, &key(Keycode::Z)),
                6 => route(&mut twin.app, &key(Keycode::Y)),
                7 => {
                    let expected = revision(&twin.app.session);
                    #[allow(clippy::cast_possible_wrap)]
                    let x = 1 + rng.next(4) as i64;
                    twin.app.action(Action::Move {
                        uid: "marker_2".into(),
                        x,
                        y: 4,
                        expected,
                    })
                }
                8 => route(&mut twin.app, &key(Keycode::R)),
                9 => {
                    // The person makes the next step's edit themselves.
                    let next = twin
                        .app
                        .proposal
                        .as_ref()
                        .unwrap()
                        .progress
                        .as_ref()
                        .map_or(0, tilewright_editor_proposal::Progress::next);
                    let index =
                        (next + usize::from(rng.next(2) == 1)).min(twin.proposal.steps().len() - 1);
                    let action = twin.proposal.steps()[index].action().clone();
                    twin.app.action(action)
                }
                10 => twin.app.action(Action::ClearSelection),
                _ => {
                    // The control with the pointer, when one is laid out.
                    match control(&mut twin.app) {
                        Some((_, _, true)) => {
                            let widget = twin
                                .app
                                .ui
                                .widgets
                                .iter()
                                .find(|w| {
                                    matches!(
                                        w.control,
                                        Some(Control::ProposalPrepare | Control::ProposalContinue)
                                    )
                                })
                                .cloned()
                                .unwrap();
                            #[allow(clippy::cast_precision_loss)]
                            let at = [
                                (widget.bounds.x + widget.bounds.width / 2) as f32,
                                (widget.bounds.y + widget.bounds.height / 2) as f32,
                            ];
                            route(&mut twin.app, &button(true, at))
                                .and_then(|()| route(&mut twin.app, &button(false, at)))
                        }
                        _ => Ok(()),
                    }
                }
            };
            log.push(format!(
                "{operation} presented={presented} {state_before:?} -> {:?}",
                result.as_ref().err()
            ));
            let after = revision(&twin.app.session);
            let pending_after = twin.app.session.history().pending().is_some();
            let enter = matches!(operation, 2 | 3);
            if after != before {
                assert!(
                    enter && pending_before && presented,
                    "seed {seed}: the world changed without an accepted Enter: {log:#?}"
                );
            }
            if let (true, true, State::Pending { step }) = (enter, after != before, state_before) {
                assert!(prepared, "seed {seed}: {log:#?}");
                assert_eq!(after, twin.proposal.steps()[step].after(), "seed {seed}");
                applied += 1;
            }
            let pressed_control = matches!(operation, 0 | 1 | 11);
            if pressed_control && !pending_before && pending_after {
                prepared = true;
            }
            if !pending_after {
                prepared = false;
            }
            // The oracle: the stored reading's state is the fresh one.
            let shown = state(&twin.app);
            let fresh = fresh_state(&twin.app);
            assert_eq!(
                shown, fresh,
                "seed {seed} variant {variant}: stored vs fresh: {log:#?}"
            );
            assert_eq!(twin.app.proposal_state(), Some(fresh), "seed {seed}");
            *seen.entry(fresh.key()).or_default() += 1;
            assert_eq!(
                matches!(shown, State::Pending { .. }),
                prepared,
                "seed {seed}: {shown:?} {log:#?}"
            );
            assert_eq!(
                twin.app.status_label().contains("matches its recording"),
                prepared,
                "seed {seed}"
            );
            // What the laid-out panel shows: the oracle's state line (its
            // first 20 visible characters) and the oracle's control.
            let words = twin.app.proposal.as_ref().unwrap().state_words(fresh);
            let start: String = words.chars().filter(|c| *c != ' ').take(20).collect();
            let laid: String = sidebar_texts(&mut twin.app)
                .concat()
                .chars()
                .filter(|c| *c != ' ')
                .collect();
            assert!(
                laid.contains(&start),
                "seed {seed}: {words:?} not in {laid:?}"
            );
            let ready = twin.app.blocked.is_none() && twin.app.session.can_prepare();
            let expected_control = twin.app.proposal.as_ref().unwrap().control(fresh, ready);
            assert_eq!(
                control(&mut twin.app),
                expected_control,
                "seed {seed}: {fresh:?} {log:#?}"
            );
            if pending_after {
                assert_eq!(control(&mut twin.app), None, "seed {seed}: {log:#?}");
            }
            // The frame member agrees too.
            let frame = twin.app.proposal_frame().unwrap();
            assert_eq!(frame["state"], json!(fresh.key()), "seed {seed}");
            assert_eq!(
                frame["pending_step"],
                json!(matches!(fresh, State::Pending { .. })),
                "seed {seed}"
            );
        }
        let records = evidence_names(&twin)
            .into_iter()
            .filter(|name| name.starts_with("proposal-step-0"))
            .count();
        assert_eq!(records, applied, "seed {seed}: {log:#?}");
        accepted_total += applied;
    }
    // Every variant reaches more than the ready and pending states.
    assert!(seen.len() >= 5, "{seen:?} {accepted_total}");
}
