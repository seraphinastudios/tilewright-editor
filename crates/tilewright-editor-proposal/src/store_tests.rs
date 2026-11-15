// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use super::{Storage, StoredProposal, retain};
use crate::ProposalError;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use tilewright_editor_ui::RefusalCode;

const NAME: &str = "proposal-0000000000000000000000000000000000000000000000000000000000000000.json";
const STAGE: &str =
    "proposal-0000000000000000000000000000000000000000000000000000000000000000.json.stage";

/// A directory in memory that fails at one numbered storage call, before
/// or after the call takes effect.
#[derive(Default)]
struct Memory {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    calls: Cell<usize>,
    fail: Option<(usize, bool)>,
    nondurable: bool,
}
impl Memory {
    fn operation<T>(&self, work: impl FnOnce() -> T) -> Result<T, ProposalError> {
        let step = self.calls.get() + 1;
        self.calls.set(step);
        let fail = || {
            ProposalError::new(
                RefusalCode::ProposalStore,
                format!("fault at storage step {step}"),
            )
        };
        if self.fail == Some((step, false)) {
            return Err(fail());
        }
        let value = work();
        if self.fail == Some((step, true)) {
            return Err(fail());
        }
        Ok(value)
    }
}
impl Storage for Memory {
    fn validate(&self) -> Result<(), ProposalError> {
        self.operation(|| ())
    }
    fn existing(&self, name: &str, _: usize) -> Result<Option<Vec<u8>>, ProposalError> {
        self.operation(|| self.files.borrow().get(name).cloned())
    }
    fn check_name(&self, _: &str, _: &str) -> Result<(), ProposalError> {
        self.operation(|| ())
    }
    fn stage_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), ProposalError> {
        self.operation(|| {
            self.files.borrow_mut().insert(name.into(), bytes.into());
        })
    }
    fn verify(&self, name: &str, bytes: &[u8]) -> Result<(), ProposalError> {
        let found = self.operation(|| self.files.borrow().get(name).cloned())?;
        if found.as_deref() == Some(bytes) {
            Ok(())
        } else {
            Err(ProposalError::new(RefusalCode::ProposalStore, "differs"))
        }
    }
    fn rename(&mut self, stage: &str, name: &str) -> Result<(), ProposalError> {
        self.operation(|| {
            assert!(
                !self.files.borrow().contains_key(name),
                "rename must never replace"
            );
            let bytes = self.files.borrow_mut().remove(stage).unwrap();
            self.files.borrow_mut().insert(name.into(), bytes);
        })
    }
    fn sync(&mut self) -> Result<bool, ProposalError> {
        self.operation(|| !self.nondurable)
    }
}

#[test]
fn the_durable_sequence_runs_in_order_and_every_fault_keeps_what_it_wrote() {
    // Order: validate, final absent, stage absent, label check, stage
    // write, stage verify, final absent, rename, sync, final verify.
    let mut memory = Memory::default();
    assert_eq!(
        retain(&mut memory, NAME, b"proposal").unwrap(),
        StoredProposal {
            name: NAME.into(),
            bytes: 8,
            existed: false
        }
    );
    assert_eq!(memory.calls.get(), 10);
    assert_eq!(
        memory.files.borrow().keys().cloned().collect::<Vec<_>>(),
        [NAME]
    );
    for step in 1..=10 {
        for after in [false, true] {
            let mut memory = Memory {
                fail: Some((step, after)),
                ..Memory::default()
            };
            let error = retain(&mut memory, NAME, b"proposal").unwrap_err();
            assert_eq!(error.code(), RefusalCode::ProposalStore);
            let completed = if after { step } else { step - 1 };
            let names: Vec<String> = memory.files.borrow().keys().cloned().collect();
            let expected: &[&str] = if completed >= 8 {
                &[NAME]
            } else if completed >= 5 {
                &[STAGE]
            } else {
                &[]
            };
            assert_eq!(names, expected, "fault at {step} after={after}");
            for bytes in memory.files.borrow().values() {
                assert_eq!(bytes, b"proposal");
            }
        }
    }
}

#[test]
fn an_equal_file_is_success_and_a_different_one_or_a_stage_is_left_alone() {
    let mut memory = Memory::default();
    memory
        .files
        .borrow_mut()
        .insert(NAME.into(), b"proposal".to_vec());
    let stored = retain(&mut memory, NAME, b"proposal").unwrap();
    assert!(stored.existed);
    // validate, existing, verify: nothing is written.
    assert_eq!(memory.calls.get(), 3);
    for (name, bytes) in [(NAME, b"other".as_slice()), (STAGE, b"proposal")] {
        let mut memory = Memory::default();
        memory.files.borrow_mut().insert(name.into(), bytes.into());
        let error = retain(&mut memory, NAME, b"proposal").unwrap_err();
        assert_eq!(error.code(), RefusalCode::ProposalStore);
        assert_eq!(
            *memory.files.borrow(),
            BTreeMap::from([(name.to_owned(), bytes.to_vec())])
        );
    }
}

#[test]
fn a_directory_sync_that_is_not_durable_fails_after_the_rename() {
    let mut memory = Memory {
        nondurable: true,
        ..Memory::default()
    };
    let error = retain(&mut memory, NAME, b"proposal").unwrap_err();
    assert!(error.message().contains("durable"), "{error}");
    assert_eq!(
        memory.files.borrow().keys().cloned().collect::<Vec<_>>(),
        [NAME]
    );
}
