// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Bounded exact-inverse retention, before authored-source publication.
//!
//! This service owns no project writer or persistent history. Its constructor
//! accepts an already retained root from the trusted prototype provisioner;
//! see [`DurableRecovery::from_provisioned_session_root`]. Old records are never
//! deleted, overwritten or automatically replayed. Every uncertain retention
//! blocks this store permanently and preserves all possible artifacts.

use sha2::{Digest, Sha256};
use std::fmt;
use tilewright_editor_commands::RecoveryStore;
use tilewright_source::{Access, AccessError, Mounts, SourceDirectory, SourceLock, SourceRoot};

/// Hard bounded application-state quotas, reducible by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryLimits {
    pub max_records: usize,
    pub max_total_bytes: u64,
    pub max_record_bytes: u64,
}

impl Default for RecoveryLimits {
    fn default() -> Self {
        Self {
            max_records: 1024,
            max_total_bytes: 64 << 20,
            max_record_bytes: 64 << 20,
        }
    }
}

impl RecoveryLimits {
    fn valid(self) -> bool {
        let hard = Self::default();
        self.max_records > 0
            && self.max_records <= hard.max_records
            && self.max_total_bytes > 0
            && self.max_total_bytes <= hard.max_total_bytes
            && self.max_record_bytes > 0
            && self.max_record_bytes <= self.max_total_bytes
    }
}

/// A successfully retained exact inverse; relative identity, never root authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryRecord {
    pub name: String,
    pub sha256: [u8; 32],
    pub bytes: u64,
}

/// Stable phase/category for UI diagnostics without leaking filesystem paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    UnsupportedPlatform,
    InvalidLimits,
    Privacy,
    UnsupportedFileSystem,
    Locked,
    ExistingEntries,
    Quota,
    OccupiedName,
    CheckName,
    Write,
    Read,
    Verify,
    Rename,
    DirectorySync,
    NotDurable,
    Open,
}

/// Retention failed. Artifacts named here must be preserved for inspection.
///
/// A failed write or rename may have taken effect. These fields identify the
/// attempted record, not a claim that either name exists or is durable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryError {
    pub kind: FailureKind,
    pub stage_name: Option<String>,
    pub record: Option<Box<RecoveryRecord>>,
    pub limits: RecoveryLimits,
    pub detail: String,
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "recovery {:?}: {}", self.kind, self.detail)?;
        if let (Some(stage), Some(record)) = (&self.stage_name, &self.record) {
            write!(f, "; preserve stage={stage} final={} sha256=", record.name)?;
            for byte in record.sha256 {
                write!(f, "{byte:02x}")?;
            }
            write!(f, " bytes={}", record.bytes)?;
        }
        write!(
            f,
            "; limits records={} total_bytes={} record_bytes={}",
            self.limits.max_records, self.limits.max_total_bytes, self.limits.max_record_bytes
        )
    }
}
impl std::error::Error for RecoveryError {}

/// Retains a rooted handle and an exclusive advisory lock for its entire lifetime.
pub struct DurableRecovery {
    store: Store<RootStorage>,
}

impl DurableRecovery {
    /// Accept a root opened once from the trusted prototype session provisioner.
    ///
    /// **Caller proof boundary:** before calling, the provisioner must create a
    /// fresh private session, prove recovery and authored World directories are
    /// distinct and outside one another by retained Linux directory identities,
    /// and durably sync their creation and parent entries. The app obtains this
    /// root from the provisioner's inherited directory descriptor. This method
    /// cannot establish separation from an arbitrary project using public World
    /// APIs. It neither creates directories nor repairs arbitrary permissions.
    ///
    /// Linux checked local filesystems only; 0700 ownership/ACL/labels and a
    /// lifetime lock are checked here. A nonempty store is always refused; use
    /// a new provisioned session and preserve the old one. The lock coordinates
    /// cooperating writers, not malicious processes running as the same UID.
    pub fn from_provisioned_session_root(
        root: SourceRoot,
        limits: RecoveryLimits,
    ) -> Result<Self, RecoveryError> {
        let initial = |kind, detail: String| RecoveryError {
            kind,
            stage_name: None,
            record: None,
            limits,
            detail,
        };
        if !cfg!(target_os = "linux") {
            return Err(initial(
                FailureKind::UnsupportedPlatform,
                "editing requires Linux checked storage".into(),
            ));
        }
        if !limits.valid() {
            return Err(initial(
                FailureKind::InvalidLimits,
                "quotas must be positive and within hard limits".into(),
            ));
        }
        let directory = root
            .directory("", false)
            .map_err(|e| initial(FailureKind::Open, access_detail(&e)))?
            .ok_or_else(|| initial(FailureKind::Open, "provisioned root missing".into()))?;
        let access = private_access(&directory).map_err(|(kind, detail)| initial(kind, detail))?;
        let lock = root.lock().map_err(|e| {
            initial(
                if matches!(e, AccessError::Locked) {
                    FailureKind::Locked
                } else {
                    FailureKind::Open
                },
                access_detail(&e),
            )
        })?;
        let storage = RootStorage {
            root,
            directory,
            access,
            _lock: lock,
        };
        storage
            .validate()
            .map_err(|(kind, detail)| initial(kind, detail))?;
        let (names, count) = storage
            .root
            .names("", limits.max_records.saturating_mul(2))
            .map_err(|e| initial(FailureKind::ExistingEntries, access_detail(&e)))?
            .ok_or_else(|| initial(FailureKind::Open, "provisioned root missing".into()))?;
        if count != 0 {
            // Bounded and escaped; no file contents or absolute paths in errors.
            return Err(initial(
                FailureKind::ExistingEntries,
                format!(
                    "nonempty store ({count} entries; first names {:?}); preserve it and provision a fresh session",
                    names.iter().take(8).collect::<Vec<_>>()
                ),
            ));
        }
        Ok(Self {
            store: Store::new(storage, limits),
        })
    }

    pub fn records(&self) -> &[RecoveryRecord] {
        &self.store.records
    }
    pub fn last_error(&self) -> Option<&RecoveryError> {
        self.store.blocked.as_ref()
    }

    /// Retains exact UTF-8 bytes, returning only after verified durable rename.
    pub fn retain_exact(&mut self, inverse: &str) -> Result<&RecoveryRecord, RecoveryError> {
        self.store.retain(inverse)?;
        Ok(self
            .store
            .records
            .last()
            .expect("successful retention records identity"))
    }
}

impl RecoveryStore for DurableRecovery {
    fn retain(&mut self, inverse: &str) -> Result<(), String> {
        self.retain_exact(inverse)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

// The seam is private: a caller cannot substitute a memory backend for native
// editing. Unit tests inject faults into the same retention state machine.
type StorageError = (FailureKind, String);
trait Storage {
    fn validate(&self) -> Result<(), StorageError>;
    fn exists(&self, name: &str) -> Result<bool, StorageError>;
    fn check_name(&self, stage: &str, name: &str) -> Result<(), StorageError>;
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), StorageError>;
    fn verify(&self, name: &str, bytes: &[u8]) -> Result<(), StorageError>;
    fn rename(&mut self, stage: &str, name: &str) -> Result<(), StorageError>;
    fn sync(&mut self) -> Result<bool, StorageError>;
}

struct RootStorage {
    root: SourceRoot,
    directory: SourceDirectory,
    access: Access,
    _lock: SourceLock,
}

fn access_detail(error: &AccessError) -> String {
    // AccessError may include a user-supplied root path. Deliberately retain
    // only the category/OS kind; record names and operation phase are separate.
    match error {
        AccessError::Path(_) => "invalid relative name".into(),
        AccessError::Link(_) => "symbolic link refused".into(),
        AccessError::Unexpected(_) => "unexpected or missing entry type".into(),
        AccessError::TooLarge(_) => "bounded access limit exceeded".into(),
        AccessError::Locked => "another session holds the recovery lock".into(),
        AccessError::Io(_, e) => {
            format!("filesystem I/O {:?} (OS {:?})", e.kind(), e.raw_os_error())
        }
    }
}

fn private_access(directory: &SourceDirectory) -> Result<Access, StorageError> {
    let access = directory
        .own_access()
        .map_err(|e| (FailureKind::Privacy, access_detail(&e)))?;
    if access.mode != 0o700 || access.user != access.new_file().user {
        return Err((
            FailureKind::Privacy,
            "root must be owned by the current user with mode exactly 0700, without ACLs".into(),
        ));
    }
    if directory
        .unchecked_file_system(&Mounts::read())
        .map_err(|e| (FailureKind::UnsupportedFileSystem, access_detail(&e)))?
        .is_some()
    {
        return Err((
            FailureKind::UnsupportedFileSystem,
            "root is not on a checked local filesystem".into(),
        ));
    }
    Ok(access)
}

impl Storage for RootStorage {
    fn validate(&self) -> Result<(), StorageError> {
        if private_access(&self.directory)? != self.access {
            return Err((
                FailureKind::Privacy,
                "root access changed since opening".into(),
            ));
        }
        Ok(())
    }
    fn exists(&self, name: &str) -> Result<bool, StorageError> {
        self.root
            .exists(name)
            .map_err(|e| (FailureKind::Read, access_detail(&e)))
    }
    fn check_name(&self, stage: &str, name: &str) -> Result<(), StorageError> {
        self.directory
            .check_final_name(stage, name)
            .map_err(|e| (FailureKind::CheckName, access_detail(&e)))
    }
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        self.directory
            .write(name, Some(&self.access.new_file()), bytes)
            .map_err(|e| (FailureKind::Write, access_detail(&e)))
    }
    fn verify(&self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        let found = self
            .root
            .read(name, bytes.len() as u64)
            .map_err(|e| (FailureKind::Read, access_detail(&e)))?;
        if found.as_deref() != Some(bytes) {
            return Err((
                FailureKind::Verify,
                "retained bytes differ from exact inverse".into(),
            ));
        }
        let access = self
            .directory
            .access(name)
            .map_err(|e| (FailureKind::Privacy, access_detail(&e)))?;
        if access != Some(self.access.new_file()) {
            return Err((
                FailureKind::Privacy,
                "record access differs from private 0600 policy".into(),
            ));
        }
        Ok(())
    }
    fn rename(&mut self, stage: &str, name: &str) -> Result<(), StorageError> {
        self.directory
            .rename(stage, name)
            .map_err(|e| (FailureKind::Rename, access_detail(&e)))
    }
    fn sync(&mut self) -> Result<bool, StorageError> {
        self.directory
            .sync()
            .map_err(|e| (FailureKind::DirectorySync, access_detail(&e)))
    }
}

struct Store<S> {
    storage: S,
    limits: RecoveryLimits,
    records: Vec<RecoveryRecord>,
    total_bytes: u64,
    blocked: Option<RecoveryError>,
}
impl<S: Storage> Store<S> {
    fn new(storage: S, limits: RecoveryLimits) -> Self {
        Self {
            storage,
            limits,
            records: Vec::new(),
            total_bytes: 0,
            blocked: None,
        }
    }

    fn retain(&mut self, inverse: &str) -> Result<(), RecoveryError> {
        if let Some(error) = &self.blocked {
            return Err(error.clone());
        }
        let bytes = inverse.len() as u64;
        // Enforce bounds before hashing or allocating a descriptor for input.
        if bytes > self.limits.max_record_bytes
            || self.records.len() >= self.limits.max_records
            || self
                .total_bytes
                .checked_add(bytes)
                .is_none_or(|total| total > self.limits.max_total_bytes)
        {
            let error = RecoveryError {
                kind: FailureKind::Quota,
                stage_name: None,
                record: None,
                limits: self.limits,
                detail: format!(
                    "quota exhausted (requested {bytes} bytes); preserve this store and provision a fresh session"
                ),
            };
            self.blocked = Some(error.clone());
            return Err(error);
        }
        let record = RecoveryRecord {
            name: format!("inverse-{:04}.json", self.records.len() + 1),
            sha256: Sha256::digest(inverse.as_bytes()).into(),
            bytes: inverse.len() as u64,
        };
        let stage = format!("{}.stage", record.name);
        let result = self.attempt(&stage, &record, inverse.as_bytes());
        match result {
            Ok(()) => {
                self.total_bytes += record.bytes; // bounded above, no intervening mutation
                self.records.push(record);
                Ok(())
            }
            Err((kind, detail)) => {
                let error = RecoveryError {
                    kind,
                    detail,
                    stage_name: Some(stage),
                    record: Some(Box::new(record)),
                    limits: self.limits,
                };
                self.blocked = Some(error.clone());
                Err(error)
            }
        }
    }

    fn attempt(
        &mut self,
        stage: &str,
        record: &RecoveryRecord,
        bytes: &[u8],
    ) -> Result<(), StorageError> {
        self.storage.validate()?;
        self.absent(stage)?;
        self.absent(&record.name)?;
        self.storage.check_name(stage, &record.name)?;
        // World write removes an existing target. Only a private, exclusively
        // locked store plus these absence checks makes this fresh-name write safe.
        self.storage.write(stage, bytes)?;
        self.storage.verify(stage, bytes)?;
        self.absent(&record.name)?;
        self.storage.rename(stage, &record.name)?;
        if !self.storage.sync()? {
            return Err((
                FailureKind::NotDurable,
                "directory sync did not establish durability".into(),
            ));
        }
        self.storage.verify(&record.name, bytes)?;
        Ok(())
    }
    fn absent(&self, name: &str) -> Result<(), StorageError> {
        if self.storage.exists(name)? {
            return Err((
                FailureKind::OccupiedName,
                format!("reserved name {name} already exists; no overwrite attempted"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
