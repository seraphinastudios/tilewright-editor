// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use super::on_top;
use crate::Proposal;
use tilewright_editor_ui::AppliedStep;

fn applied(transaction: &str, before: [u8; 32], after: [u8; 32]) -> AppliedStep<'_> {
    AppliedStep {
        origin: None,
        transaction,
        before,
        after,
    }
}

#[test]
fn only_the_prepared_transaction_itself_is_on_top() {
    let proposal =
        Proposal::decode(include_bytes!("../../tests/golden/proposal-move.json")).unwrap();
    let step = &proposal.steps()[0];
    let sealed = step
        .transaction()
        .replacen("agent:sandbox", "human:tester", 1);
    assert_ne!(sealed, step.transaction());
    let (before, after) = (step.before(), step.after());
    assert!(on_top(Some(applied(&sealed, before, after)), step, &sealed));
    // Another transaction between the same revisions is not the step.
    let other = sealed.replacen("Editor", "editor", 1);
    assert_ne!(other, sealed);
    assert!(!on_top(Some(applied(&other, before, after)), step, &sealed));
    assert!(!on_top(Some(applied(&sealed, after, after)), step, &sealed));
    assert!(!on_top(
        Some(applied(&sealed, before, before)),
        step,
        &sealed
    ));
    assert!(!on_top(None, step, &sealed));
}
