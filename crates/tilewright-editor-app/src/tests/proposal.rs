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
use crate::prototype::proposal_view::{ROW_CHARACTERS, State, shown};
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

const AGENT: &str = "agent:sandbox \"desk\" \\ 7";
const PERSON: &str = "human:tester";
const RECEIPT: [u8; 32] = [7; 32];
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

fn edit_json(root: &Path, path: &str, change: impl FnOnce(&mut Value)) {
    let path = root.join(path);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn open_session(root: &Path, actor: &str) -> Session {
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

fn revision(session: &Session) -> [u8; 32] {
    session.history().document().revision()
}

/// Dispatch `action` on `session` and accept it.
fn accept(session: &mut Session, action: Action) {
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
const KINDS: usize = 15;
#[allow(clippy::too_many_lines)]
fn edit(kind: usize, expected: [u8; 32]) -> Action {
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
struct Twin {
    person: tempfile::TempDir,
    agent: tempfile::TempDir,
    app: Application,
    proposal: Proposal,
}

fn twin(
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
    let proposal = Proposal::from_session(&session, "0.1.0", RECEIPT, client).unwrap();
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

fn scratch() -> std::path::PathBuf {
    std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
}

/// The 1280x800 test application, its session rebuilt with the person's
/// actor as `--actor human:tester` makes it.
fn person_application(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, Application) {
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

fn open(twin: &mut Twin) {
    let name = twin.proposal.file_name();
    twin.app
        .open_proposal(Some((twin.proposal.clone(), name)))
        .unwrap();
    acknowledge_ui(&mut twin.app);
}

fn state(app: &Application) -> State {
    app.proposal
        .as_ref()
        .unwrap()
        .state(&app.session, app.blocked.is_some())
}

fn evidence(twin: &Twin, name: &str) -> Value {
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

/// The panel's rows, each checked to be laid out in the sidebar (wrapped
/// to its width: the start of a long one).
fn panel(app: &mut Application) -> Vec<String> {
    let rows = app
        .proposal
        .as_ref()
        .unwrap()
        .rows(&app.session, app.blocked.is_some());
    let laid: String = sidebar_texts(app)
        .concat()
        .chars()
        .filter(|c| *c != ' ')
        .collect();
    for row in &rows {
        let start: String = row.chars().filter(|c| *c != ' ').take(40).collect();
        assert!(laid.contains(&start), "{row:?} is not laid out in {laid:?}");
    }
    rows
}

/// Press the panel's control with the pointer, as the person would.
fn press_control(app: &mut Application) {
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
fn prepare_and_accept(twin: &mut Twin, step: usize) {
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
            "Proposal step {} of {steps} by {}: matches its recording. Accept applies it; Cancel stops the proposal.",
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
            "editor": "0.1.0",
            "this_editor": env!("CARGO_PKG_VERSION"),
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
            .any(|text| text == "Step 1 of 15: Move marker_1 on main/0 from (3, 2) to (4, 2)"),
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
            "state": "finished", "pending_step": false})
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
            .any(|text| text == "Catalog differs from the recording: your grants apply"),
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
}

/// The proposal with its body changed by `change` and its transaction
/// digests fixed up, as a writer who can recompute digests would.
fn forged(proposal: &Proposal, change: impl FnOnce(&mut Value)) -> Proposal {
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
        texts
            .iter()
            .any(|text| text == "Recorded by editor 9.9.9-other (this editor 0.1.0): may diverge"),
        "{texts:?}"
    );
    // Before preparing, the step's words come from its Action: "Other".
    prepare_and_accept(&mut twin, 0);
    let texts = panel(&mut twin.app);
    assert!(
        texts
            .iter()
            .any(|text| text == "Step 2 of 2: Set label of marker_1 to \"Other\""),
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
        rows.iter()
            .any(|text| text
                .starts_with("Step 1 of 1: Set f\\u{2028}\\u{200b} of u\\u{202e}\\u{1b}xxx")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|text| text.starts_with(
            "Client label (unverified): lab\\u{1b}]0;t\\u{7}\\u{202e}el\\u{200b}\\u{2028}"
        )),
        "{rows:?}"
    );
    for scroll in 0..2 {
        twin.app.sidebar_scroll = scroll;
        let texts = sidebar_texts(&mut twin.app);
        for text in texts.iter().chain(&rows) {
            assert!(
                !text.chars().any(tilewright_editor_observe::node::escaped),
                "a raw character in {text:?}"
            );
            assert!(text.chars().count() <= ROW_CHARACTERS, "{}", text.len());
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
            crate::prototype::proposal_view::action_words(
                twin.proposal.steps()[step].action(),
                twin.app.session.world(),
                twin.app.session.catalog(),
            )
        })
        .collect();
    assert_eq!(
        words,
        [
            "Move marker_1 on main/0 from (3, 2) to (4, 2)",
            "Set label of marker_1 to \"Renamed\"",
            "Paint (0, 0) on main/0 floor with sand",
            "Paint 2 cells on main/0 floor with sand",
            "Paint 1 cell on main/0 into zone path",
            "Delete marker_2 (marker) on main/0 at (1, 4)",
            "Place marker on main/0 at (2, 2)",
            "Rename zone path on main to Trail",
            "Put zone path on main under meadow",
            "Create zone camp on main named Camp at the top level",
            "Delete zone spare on main",
            "Create anchor camp on main at (2, 2)",
            "Move anchor arrival on main from (7, 3) to (6, 3)",
            "Delete anchor lookout on main at (-2, 0), outside the view",
            "Point the portal of gate_west to anchor arrival on main",
        ]
    );
}

#[test]
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
    let words = |action: &Action| {
        crate::prototype::proposal_view::action_words(
            action,
            app.session.world(),
            app.session.catalog(),
        )
    };
    assert_eq!(
        words(&Action::DeleteAnchor {
            plane: "main".into(),
            anchor: "far".into(),
            expected
        }),
        "Delete anchor far on main at (400, 3), outside the view"
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
        "Paint 2 cells on main/0 back to the default zone, some outside the view"
    );
    assert_eq!(
        words(&Action::Move {
            uid: "missing".into(),
            x: 1,
            y: 1,
            expected
        }),
        "Move missing (not placed in this world) to (1, 1)"
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
    let proposal = Proposal::from_session(&session, "0.1.0", RECEIPT, None).unwrap();
    let (_person, mut app) = person_application(|_| {});
    let name = proposal.file_name();
    app.open_proposal(Some((proposal, name))).unwrap();
    let rows = app.proposal.as_ref().unwrap().rows(&app.session, false);
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

#[test]
fn the_panel_fits_the_smallest_window() {
    let mut twin = twin(2, |_| {}, |_| {}, Some(LABEL));
    twin.app.ui = crate::workspace_ui::WorkspaceUi::new((720, 480), (720, 480));
    twin.app.view.camera.width = 720;
    twin.app.view.camera.height = 480;
    twin.app.rebuild().unwrap();
    open(&mut twin);
    // Every row of the block and its control is laid out above the bottom.
    assert_eq!(
        row_enabled(&mut twin.app, &Control::ProposalPrepare),
        Some(true)
    );
    let texts = panel(&mut twin.app);
    assert!(
        texts.iter().any(|text| text.starts_with("Step 1 of 2")),
        "{texts:?}"
    );
    press_control(&mut twin.app);
    assert_eq!(state(&twin.app), State::Pending { step: 0 });
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
