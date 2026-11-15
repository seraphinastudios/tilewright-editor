// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Proposal files in a directory the caller opened (in a sandbox, the
//! `proposals/` directory the provisioner passes), through the World
//! toolkit's confined access, as the recovery store writes its records.
//!
//! A proposal is kept as `proposal-<body digest>.json`; no caller chooses
//! a name. Writing stages the bytes beside it, verifies them and their
//! private access, renames the stage into place and syncs the directory,
//! then verifies the file again. An existing file of that name with the
//! same bytes is success; one with other bytes, a link or anything but a
//! regular file is refused and left alone. The directory's lock keeps
//! cooperating writers apart; like the recovery store's, it does not stop
//! a process of the same user that ignores it.
use tilewright_editor_ui::RefusalCode;
use tilewright_source::{Access, AccessError, Mounts, SourceDirectory, SourceLock, SourceRoot};

use crate::{Proposal, ProposalError, budget, limits, unhex};

/// A proposal file a store holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredProposal {
    pub name: String,
    pub bytes: u64,
    /// Whether the file was already there with these bytes.
    pub existed: bool,
}

/// A private directory of proposal files, locked for as long as it is
/// held. Linux only: elsewhere the durable sequence cannot be checked.
pub struct ProposalStore {
    storage: RootStorage,
}

impl ProposalStore {
    /// Takes `root` (an already opened directory) as a store.
    ///
    /// # Errors
    /// Refuses with `proposal.store` on platforms other than Linux, for a
    /// directory that is not the current user's alone (mode 0700, no
    /// access control list), not on a checked local file system, or locked
    /// by another holder.
    pub fn open(root: SourceRoot) -> Result<Self, ProposalError> {
        if !cfg!(target_os = "linux") {
            return Err(store(
                "proposal files are written on Linux checked storage only",
            ));
        }
        let directory = root
            .directory("", false)
            .map_err(|error| store(access_detail(&error)))?
            .ok_or_else(|| store("the proposal directory is missing"))?;
        let access = private_access(&directory)?;
        let lock = root.lock().map_err(|error| store(access_detail(&error)))?;
        Ok(Self {
            storage: RootStorage {
                root,
                directory,
                access,
                _lock: lock,
            },
        })
    }

    /// Writes `proposal` durably, or finds it already written.
    ///
    /// # Errors
    /// Refuses with `proposal.store` when another file has its name, the
    /// stage name is taken, the directory's access changed, or any step of
    /// the durable sequence fails; whatever was there is left in place.
    pub fn retain(&mut self, proposal: &Proposal) -> Result<StoredProposal, ProposalError> {
        retain(&mut self.storage, &proposal.file_name(), proposal.bytes())
    }
}

/// Reads and verifies the proposal file `name` in `root`, at most
/// [`limits::BYTES`] read; the name must be the one a store gives it.
///
/// # Errors
/// Refuses with `proposal.invalid` for a name a store does not give,
/// `proposal.budget` for a file over the size bound (before reading it),
/// `proposal.store` for a missing file, a link or anything but a regular
/// file, `proposal.digest` when the name's digest is not the body's, and
/// whatever [`Proposal::decode`] refuses.
pub fn read_proposal(root: &SourceRoot, name: &str) -> Result<Proposal, ProposalError> {
    let digest = name
        .strip_prefix("proposal-")
        .and_then(|rest| rest.strip_suffix(".json"))
        .and_then(unhex)
        .ok_or_else(|| crate::invalid("a proposal file is named proposal-<64 hex digits>.json"))?;
    let bytes = match root.read(name, limits::BYTES as u64) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(store("the proposal file is missing")),
        Err(AccessError::TooLarge(_)) => {
            return Err(budget("the proposal is larger than 16 MiB"));
        }
        Err(error) => return Err(store(access_detail(&error))),
    };
    let proposal = Proposal::decode(&bytes)?;
    if proposal.body_sha256() != digest {
        return Err(ProposalError::new(
            RefusalCode::ProposalDigest,
            "the file name does not name the proposal's body digest",
        ));
    }
    Ok(proposal)
}

fn store(message: impl Into<String>) -> ProposalError {
    ProposalError::new(RefusalCode::ProposalStore, message)
}

/// The category of an access error, never a path.
fn access_detail(error: &AccessError) -> String {
    match error {
        AccessError::Path(_) => "invalid relative name".into(),
        AccessError::Link(_) => "symbolic link refused".into(),
        AccessError::Unexpected(_) => "not a regular file or directory".into(),
        AccessError::TooLarge(_) => "bounded access limit exceeded".into(),
        AccessError::Locked => "another holder has the proposal directory's lock".into(),
        AccessError::Io(_, error) => format!(
            "filesystem I/O {:?} (OS {:?})",
            error.kind(),
            error.raw_os_error()
        ),
    }
}

fn private_access(directory: &SourceDirectory) -> Result<Access, ProposalError> {
    let access = directory
        .own_access()
        .map_err(|error| store(access_detail(&error)))?;
    if access.mode != 0o700 || access.user != access.new_file().user {
        return Err(store(
            "the proposal directory must be the current user's with mode exactly 0700, without access control lists",
        ));
    }
    if directory
        .unchecked_file_system(&Mounts::read())
        .map_err(|error| store(access_detail(&error)))?
        .is_some()
    {
        return Err(store(
            "the proposal directory is not on a checked local file system",
        ));
    }
    Ok(access)
}

// A private seam, as in the recovery store: unit tests inject faults into
// the same sequence; callers cannot substitute a backend.
pub(crate) trait Storage {
    fn validate(&self) -> Result<(), ProposalError>;
    /// The bytes at `name` if it is a regular file of at most
    /// `limit` bytes, `None` when nothing is there.
    fn existing(&self, name: &str, limit: usize) -> Result<Option<Vec<u8>>, ProposalError>;
    fn check_name(&self, stage: &str, name: &str) -> Result<(), ProposalError>;
    fn stage_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), ProposalError>;
    fn verify(&self, name: &str, bytes: &[u8]) -> Result<(), ProposalError>;
    fn rename(&mut self, stage: &str, name: &str) -> Result<(), ProposalError>;
    fn sync(&mut self) -> Result<bool, ProposalError>;
}

struct RootStorage {
    root: SourceRoot,
    directory: SourceDirectory,
    access: Access,
    _lock: SourceLock,
}

impl Storage for RootStorage {
    fn validate(&self) -> Result<(), ProposalError> {
        if private_access(&self.directory)? != self.access {
            return Err(store(
                "the proposal directory's access changed since opening",
            ));
        }
        Ok(())
    }
    fn existing(&self, name: &str, limit: usize) -> Result<Option<Vec<u8>>, ProposalError> {
        if !self
            .root
            .exists(name)
            .map_err(|error| store(access_detail(&error)))?
        {
            return Ok(None);
        }
        // A link or special file at the name is refused by the read.
        match self.root.read(name, limit as u64) {
            Ok(Some(bytes)) => Ok(Some(bytes)),
            Ok(None) => Err(store("an entry at the name vanished while reading")),
            Err(error) => Err(store(access_detail(&error))),
        }
    }
    fn check_name(&self, stage: &str, name: &str) -> Result<(), ProposalError> {
        self.directory
            .check_final_name(stage, name)
            .map_err(|error| store(access_detail(&error)))
    }
    fn stage_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), ProposalError> {
        self.directory
            .write(name, Some(&self.access.new_file()), bytes)
            .map_err(|error| store(access_detail(&error)))
    }
    fn verify(&self, name: &str, bytes: &[u8]) -> Result<(), ProposalError> {
        let found = self
            .root
            .read(name, bytes.len() as u64)
            .map_err(|error| store(access_detail(&error)))?;
        if found.as_deref() != Some(bytes) {
            return Err(store("the written bytes differ from the proposal"));
        }
        if self
            .directory
            .access(name)
            .map_err(|error| store(access_detail(&error)))?
            != Some(self.access.new_file())
        {
            return Err(store("the proposal file's access is not the private 0600"));
        }
        Ok(())
    }
    fn rename(&mut self, stage: &str, name: &str) -> Result<(), ProposalError> {
        self.directory
            .rename(stage, name)
            .map_err(|error| store(access_detail(&error)))
    }
    fn sync(&mut self) -> Result<bool, ProposalError> {
        self.directory
            .sync()
            .map_err(|error| store(access_detail(&error)))
    }
}

/// The durable sequence: an existing equal file is success; otherwise
/// stage, verify, rename into a still-free name, sync, verify.
pub(crate) fn retain(
    storage: &mut impl Storage,
    name: &str,
    bytes: &[u8],
) -> Result<StoredProposal, ProposalError> {
    let stored = |existed| StoredProposal {
        name: name.to_owned(),
        bytes: bytes.len() as u64,
        existed,
    };
    storage.validate()?;
    if let Some(found) = storage.existing(name, limits::BYTES)? {
        if found != bytes {
            return Err(store(
                "another file has this proposal's name; nothing was replaced",
            ));
        }
        storage.verify(name, bytes)?;
        return Ok(stored(true));
    }
    let stage = format!("{name}.stage");
    if storage.existing(&stage, limits::BYTES)?.is_some() {
        return Err(store(
            "a stage of this proposal is already there; preserve it and inspect",
        ));
    }
    storage.check_name(&stage, name)?;
    // World's write replaces what is at the stage name; the lock and the
    // absence check above make it a fresh name for cooperating writers.
    storage.stage_bytes(&stage, bytes)?;
    storage.verify(&stage, bytes)?;
    if storage.existing(name, limits::BYTES)?.is_some() {
        return Err(store(
            "a file appeared at this proposal's name; nothing was replaced",
        ));
    }
    storage.rename(&stage, name)?;
    if !storage.sync()? {
        return Err(store(
            "the directory sync did not make the proposal durable",
        ));
    }
    storage.verify(name, bytes)?;
    Ok(stored(false))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
