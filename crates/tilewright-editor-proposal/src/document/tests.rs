// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use super::{Proposal, ProposalStep, read_back};
use serde_json::json;
use tilewright_editor_ui::{Action, RefusalCode};

fn step(value: serde_json::Value, placed: Option<&str>) -> ProposalStep {
    ProposalStep {
        action: Action::SetField {
            uid: "u".into(),
            field: "f".into(),
            value,
            expected: [1; 32],
        },
        placed: placed.map(str::to_owned),
        transaction: String::new(),
        transaction_sha256: [0; 32],
        before: [1; 32],
        after: [2; 32],
    }
}

#[test]
fn a_step_that_reads_back_differently_is_refused() {
    let recorded = [step(json!(1.0), None), step(json!("a"), Some("o.x"))];
    assert_eq!(read_back(&recorded, &recorded), Ok(()));
    // A writer or reader that turned the number 1.0 into the integer 1,
    // or lost a placed uid.
    for decoded in [
        [step(json!(1), None), step(json!("a"), Some("o.x"))],
        [step(json!(1.0), None), step(json!("a"), None)],
    ] {
        let error = read_back(&decoded, &recorded).unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalInvalid);
    }
    assert_eq!(
        read_back(&recorded[..1], &recorded).unwrap_err().code(),
        RefusalCode::ProposalInvalid
    );
}

/// A reader whose numbers drift (it reads every field value back as
/// another value) makes recording fail, not applying.
#[cfg(target_os = "linux")]
#[test]
fn recording_refuses_a_document_that_reads_back_differently() {
    use tilewright_editor_commands::{History, RecoveryStore};
    use tilewright_editor_document::WorldDocument;
    use tilewright_editor_preview::{PreviewLimits, loader};
    use tilewright_editor_ui::{Actor, Session};
    struct Records;
    impl RecoveryStore for Records {
        fn retain(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let source = tilewright_editor_testkit::fixture_world("prototype-neutral");
    let mut pending = vec![(source.clone(), dir.path().to_path_buf())];
    while let Some((from, to)) = pending.pop() {
        std::fs::create_dir_all(&to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push((entry.path(), to.join(entry.file_name())));
            } else {
                std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    let document = WorldDocument::open(dir.path()).unwrap();
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
        Actor::new("agent:read-back").unwrap(),
    );
    let expected = session.history().document().revision();
    let action = Action::SetField {
        uid: "marker_1".into(),
        field: "label".into(),
        value: json!("Label"),
        expected,
    };
    session.dispatch(action, &mut Records).unwrap();
    session.dispatch(Action::Accept, &mut Records).unwrap();
    let honest = Proposal::record(&session, "1", [0; 32], None, Proposal::decode);
    assert!(honest.is_ok());
    let drifting = |bytes: &[u8]| {
        let mut proposal = Proposal::decode(bytes)?;
        if let Action::SetField { value, .. } = &mut proposal.steps[0].action {
            *value = json!("Label ");
        }
        Ok(proposal)
    };
    let error = Proposal::record(&session, "1", [0; 32], None, drifting).unwrap_err();
    assert_eq!(
        (error.code(), error.step()),
        (RefusalCode::ProposalInvalid, Some(0))
    );
}
