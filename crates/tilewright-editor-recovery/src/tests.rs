// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};

#[derive(Default)]
struct Memory {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    calls: Cell<usize>,
    fail: Option<(usize, bool)>,
    nondurable: bool,
}
impl Memory {
    fn operation<T>(
        &self,
        kind: FailureKind,
        work: impl FnOnce() -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let step = self.calls.get() + 1;
        self.calls.set(step);
        let fail = || (kind, format!("fault at storage step {step}"));
        if self.fail == Some((step, false)) {
            return Err(fail());
        }
        let value = work()?;
        if self.fail == Some((step, true)) {
            return Err(fail());
        }
        Ok(value)
    }
}
impl Storage for Memory {
    fn validate(&self) -> Result<(), StorageError> {
        self.operation(FailureKind::Privacy, || Ok(()))
    }
    fn exists(&self, name: &str) -> Result<bool, StorageError> {
        self.operation(FailureKind::Read, || {
            Ok(self.files.borrow().contains_key(name))
        })
    }
    fn check_name(&self, _: &str, _: &str) -> Result<(), StorageError> {
        self.operation(FailureKind::CheckName, || Ok(()))
    }
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        self.operation(FailureKind::Write, || {
            assert!(
                !self.files.borrow().contains_key(name),
                "write must never replace"
            );
            self.files.borrow_mut().insert(name.into(), bytes.into());
            Ok(())
        })
    }
    fn verify(&self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        self.operation(FailureKind::Read, || {
            assert_eq!(
                self.files.borrow().get(name).map(Vec::as_slice),
                Some(bytes)
            );
            Ok(())
        })
    }
    fn rename(&mut self, stage: &str, name: &str) -> Result<(), StorageError> {
        self.operation(FailureKind::Rename, || {
            assert!(
                !self.files.borrow().contains_key(name),
                "rename must never replace"
            );
            let bytes = self.files.borrow_mut().remove(stage).unwrap();
            self.files.borrow_mut().insert(name.into(), bytes);
            Ok(())
        })
    }
    fn sync(&mut self) -> Result<bool, StorageError> {
        self.operation(FailureKind::DirectorySync, || Ok(!self.nondurable))
    }
}

#[test]
fn all_operation_failure_and_crash_boundaries_preserve_artifacts_and_permanently_block() {
    // Successful order: privacy, stage absence, final absence, label check,
    // write+file sync, stage verify, final absence, rename, dir sync, final verify.
    for step in 1..=10 {
        for after in [false, true] {
            let memory = Memory {
                fail: Some((step, after)),
                ..Memory::default()
            };
            let mut store = Store::new(memory, RecoveryLimits::default());
            let error = store.retain("exact\r\n inverse\n").unwrap_err();
            assert!(store.records.is_empty());
            let saved = store.storage.files.borrow().clone();
            let completed = if after { step } else { step - 1 };
            if completed >= 8 {
                assert_eq!(
                    saved.keys().cloned().collect::<Vec<_>>(),
                    ["inverse-0001.json"]
                );
            } else if completed >= 5 {
                assert_eq!(
                    saved.keys().cloned().collect::<Vec<_>>(),
                    ["inverse-0001.json.stage"]
                );
            } else {
                assert!(saved.is_empty());
            }
            for bytes in saved.values() {
                assert_eq!(bytes, b"exact\r\n inverse\n");
            }
            let calls = store.storage.calls.get();
            assert_eq!(store.retain("different retry").unwrap_err(), error);
            assert_eq!(store.storage.calls.get(), calls);
            assert_eq!(*store.storage.files.borrow(), saved);
        }
    }
}

#[test]
fn quotas_and_existing_names_do_not_write_or_replace() {
    for name in ["inverse-0001.json", "inverse-0001.json.stage"] {
        let memory = Memory::default();
        memory
            .files
            .borrow_mut()
            .insert(name.into(), b"preserve".to_vec());
        let mut store = Store::new(memory, RecoveryLimits::default());
        assert_eq!(
            store.retain("new").unwrap_err().kind,
            FailureKind::OccupiedName
        );
        assert_eq!(store.storage.files.borrow().len(), 1);
        assert_eq!(store.storage.files.borrow()[name], b"preserve");
    }
    for limits in [
        RecoveryLimits {
            max_record_bytes: 2,
            ..RecoveryLimits::default()
        },
        RecoveryLimits {
            max_total_bytes: 2,
            max_record_bytes: 2,
            ..RecoveryLimits::default()
        },
    ] {
        let mut store = Store::new(Memory::default(), limits);
        assert_eq!(store.retain("abc").unwrap_err().kind, FailureKind::Quota);
        assert_eq!(store.storage.calls.get(), 0);
    }
    let mut store = Store::new(
        Memory::default(),
        RecoveryLimits {
            max_records: 1,
            ..RecoveryLimits::default()
        },
    );
    store.retain("abc").unwrap();
    assert_eq!(store.retain("def").unwrap_err().kind, FailureKind::Quota);
    assert_eq!(store.records.len(), 1);
    let mut store = Store::new(
        Memory::default(),
        RecoveryLimits {
            max_total_bytes: 5,
            max_record_bytes: 5,
            ..RecoveryLimits::default()
        },
    );
    store.retain("abc").unwrap();
    assert_eq!(store.retain("def").unwrap_err().kind, FailureKind::Quota);
    assert_eq!(store.total_bytes, 3);
}

#[test]
fn nondurable_directory_keeps_final_and_fails() {
    let mut store = Store::new(
        Memory {
            nondurable: true,
            ..Memory::default()
        },
        RecoveryLimits::default(),
    );
    assert_eq!(
        store.retain("inverse").unwrap_err().kind,
        FailureKind::NotDurable
    );
    assert_eq!(
        store.storage.files.borrow()["inverse-0001.json"],
        b"inverse"
    );
    assert!(store.records.is_empty());
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs,
        os::unix::{
            fs::{PermissionsExt, symlink},
            net::UnixListener,
        },
        path::Path,
    };
    use tilewright_editor_commands::{Error, History};
    use tilewright_editor_document::WorldDocument;

    fn session_root() -> tempfile::TempDir {
        let root = std::env::var_os("TILEWRIGHT_CHECKED_TEST_ROOT")
            .map_or_else(std::env::temp_dir, std::path::PathBuf::from);
        let dir = tempfile::Builder::new()
            .prefix("tilewright-recovery-")
            .tempdir_in(root)
            .unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        // Tests, like the provisioner, establish the creation durability first.
        fs::File::open(dir.path()).unwrap().sync_all().unwrap();
        fs::File::open(dir.path().parent().unwrap())
            .unwrap()
            .sync_all()
            .unwrap();
        dir
    }
    fn open(path: &Path) -> Result<DurableRecovery, RecoveryError> {
        DurableRecovery::from_provisioned_session_root(
            SourceRoot::new(path),
            RecoveryLimits::default(),
        )
    }
    fn error(path: &Path) -> RecoveryError {
        match open(path) {
            Ok(_) => panic!("must refuse"),
            Err(e) => e,
        }
    }

    #[test]
    fn checked_linux_exact_bytes_permissions_lock_and_existing_store_refusal() {
        let dir = session_root();
        let mut store =
            open(dir.path()).expect("requires checked local test filesystem; no unchecked bypass");
        assert_eq!(error(dir.path()).kind, FailureKind::Locked);
        let inverse = "\r\n{\"exact\":\"☃\"}\n";
        let record = store.retain_exact(inverse).unwrap().clone();
        assert_eq!(
            fs::read(dir.path().join(&record.name)).unwrap(),
            inverse.as_bytes()
        );
        assert_eq!(
            record.sha256,
            <[u8; 32]>::from(Sha256::digest(inverse.as_bytes()))
        );
        assert_eq!(
            fs::metadata(dir.path().join(&record.name))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
        assert!(!dir.path().join(format!("{}.stage", record.name)).exists());
        drop(store);
        assert_eq!(error(dir.path()).kind, FailureKind::ExistingEntries);
    }

    #[test]
    fn checked_linux_retarget_does_not_redirect_retained_authority() {
        let parent = session_root();
        let original = parent.path().join("root");
        let moved = parent.path().join("moved");
        let other = parent.path().join("other");
        for path in [&original, &other] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::File::open(parent.path()).unwrap().sync_all().unwrap();
        let mut store = open(&original).unwrap();
        fs::rename(&original, &moved).unwrap();
        symlink(&other, &original).unwrap();
        let record = store.retain_exact("original retained root").unwrap();
        assert!(moved.join(&record.name).exists());
        assert!(fs::read_dir(&other).unwrap().next().is_none());
    }

    #[test]
    fn checked_linux_privacy_drift_and_special_entries_refuse_without_cleanup() {
        for mode in [0o755, 0o770, 0o2700, 0o1700, 0o4700] {
            let dir = session_root();
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(error(dir.path()).kind, FailureKind::Privacy);
        }
        let dir = session_root();
        let mut store = open(dir.path()).unwrap();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o750)).unwrap();
        assert_eq!(
            store.retain_exact("private").unwrap_err().kind,
            FailureKind::Privacy
        );
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
        for entry in ["stage", "symlink", "directory", "socket", "final"] {
            let dir = session_root();
            let target = dir.path().join("inverse-0001.json");
            let _socket = match entry {
                "stage" => {
                    fs::write(dir.path().join("inverse-0001.json.stage"), "old stage").unwrap();
                    None
                }
                "symlink" => {
                    symlink("missing", &target).unwrap();
                    None
                }
                "directory" => {
                    fs::create_dir(&target).unwrap();
                    None
                }
                "socket" => Some(UnixListener::bind(&target).unwrap()),
                _ => {
                    fs::write(&target, "old final").unwrap();
                    None
                }
            };
            assert_eq!(error(dir.path()).kind, FailureKind::ExistingEntries);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn checked_linux_reserved_targets_added_after_open_are_preserved() {
        for name in ["inverse-0001.json", "inverse-0001.json.stage"] {
            let dir = session_root();
            let mut store = open(dir.path()).unwrap();
            fs::write(dir.path().join(name), "existing").unwrap();
            assert_eq!(
                store.retain_exact("new").unwrap_err().kind,
                FailureKind::OccupiedName
            );
            assert_eq!(fs::read(dir.path().join(name)).unwrap(), b"existing");
        }
    }

    fn copy_tree(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap();
        fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let to = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &to);
            } else {
                fs::copy(entry.path(), &to).unwrap();
                fs::set_permissions(&to, fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
    }
    const CHUNK: &str = "planes/meadow/layers/0/0_0.jsonc";
    const PAINT: &str = r#"{"format":"tilewright.transaction/1","operations":[{"op":"terrain.set","plane":"meadow","layer":0,"grid":"ground","cells":[{"at":[10,10],"value":"sand","expect":"water"}]}]}"#;

    #[test]
    fn checked_history_retention_failure_never_publishes_and_success_keeps_inverse() {
        let world = session_root();
        copy_tree(
            &tilewright_editor_testkit::fixture_world("meadow"),
            world.path(),
        );
        let mut history = History::new(WorldDocument::open(world.path()).unwrap());
        let original = fs::read(world.path().join(CHUNK)).unwrap();
        history
            .prepare(history.document().revision(), "paint", PAINT)
            .unwrap();
        let recovery = session_root();
        let mut store = open(recovery.path()).unwrap();
        fs::write(recovery.path().join("inverse-0001.json.stage"), "preserve").unwrap();
        assert!(matches!(
            history.commit(&mut store, &tilewright_edit::PublishOptions::default()),
            Err(Error::Recovery(_))
        ));
        assert_eq!(fs::read(world.path().join(CHUNK)).unwrap(), original);
        assert!(history.pending().is_some());
        let next = session_root();
        let mut next_store = open(next.path()).unwrap();
        history
            .commit(&mut next_store, &tilewright_edit::PublishOptions::default())
            .unwrap();
        assert_ne!(fs::read(world.path().join(CHUNK)).unwrap(), original);
        assert_eq!(next_store.records().len(), 1);
        let saved = fs::read(next.path().join(&next_store.records()[0].name)).unwrap();
        history.prepare_undo().unwrap();
        history
            .commit(&mut next_store, &tilewright_edit::PublishOptions::default())
            .unwrap();
        assert_eq!(fs::read(world.path().join(CHUNK)).unwrap(), original);
        assert_eq!(
            fs::read(next.path().join(&next_store.records()[0].name)).unwrap(),
            saved
        );
        history
            .prepare(history.document().revision(), "paint again", PAINT)
            .unwrap();
        let blocker_root = SourceRoot::new(world.path());
        let _project_lock = blocker_root.lock().unwrap();
        assert!(matches!(
            history.commit(&mut next_store, &tilewright_edit::PublishOptions::default()),
            Err(Error::Publish(_))
        ));
        assert_eq!(next_store.records().len(), 3);
        assert!(next.path().join(&next_store.records()[2].name).exists());
        assert_eq!(fs::read(world.path().join(CHUNK)).unwrap(), original);
    }

    #[test]
    fn checked_linux_default_acl_is_refused_without_permission_repair() {
        let dir = session_root();
        // Python is already a repository validation prerequisite. Construct a
        // minimal Linux default ACL on this test-owned directory only; it need
        // not alter mode bits to prove ACL checks are distinct from mode checks.
        let status = std::process::Command::new("python3")
            .args(["-c", "import os,struct,sys; a=struct.pack('<I',2)+b''.join(struct.pack('<HHI',t,p,0xffffffff) for t,p in [(1,7),(4,0),(32,0)]); os.setxattr(sys.argv[1], 'system.posix_acl_default', a)"])
            .arg(dir.path()).status().unwrap();
        assert!(
            status.success(),
            "checked Linux ACL fixture setup must succeed"
        );
        assert_eq!(
            fs::metadata(dir.path()).unwrap().permissions().mode() & 0o7777,
            0o700
        );
        assert_eq!(error(dir.path()).kind, FailureKind::Privacy);
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn checked_linux_enumeration_bound_and_corrupt_record_refuse() {
        let dir = session_root();
        for n in 0..3 {
            fs::write(dir.path().join(format!("entry{n}")), "old").unwrap();
        }
        let result = DurableRecovery::from_provisioned_session_root(
            SourceRoot::new(dir.path()),
            RecoveryLimits {
                max_records: 1,
                ..RecoveryLimits::default()
            },
        );
        assert!(matches!(
            result,
            Err(RecoveryError {
                kind: FailureKind::ExistingEntries,
                ..
            })
        ));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
        let dir = session_root();
        let mut store = open(dir.path()).unwrap();
        store
            .store
            .storage
            .write("inverse-0001.json.stage", b"correct")
            .unwrap();
        fs::write(dir.path().join("inverse-0001.json.stage"), b"corrupt").unwrap();
        assert_eq!(
            store
                .store
                .storage
                .verify("inverse-0001.json.stage", b"correct")
                .unwrap_err()
                .0,
            FailureKind::Verify
        );
    }

    struct CrashCheckpoint {
        storage: RootStorage,
        step: Cell<usize>,
        stop: usize,
    }
    impl CrashCheckpoint {
        fn finish<T>(&self, value: Result<T, StorageError>) -> Result<T, StorageError> {
            assert!(value.is_ok());
            let step = self.step.get() + 1;
            self.step.set(step);
            if step == self.stop {
                // No destructors, lock release or application cleanup. This
                // proves process-crash windows, not hardware power-loss behavior.
                std::process::exit(77);
            }
            value
        }
    }
    impl Storage for CrashCheckpoint {
        fn validate(&self) -> Result<(), StorageError> {
            self.finish(self.storage.validate())
        }
        fn exists(&self, name: &str) -> Result<bool, StorageError> {
            self.finish(self.storage.exists(name))
        }
        fn check_name(&self, stage: &str, name: &str) -> Result<(), StorageError> {
            self.finish(self.storage.check_name(stage, name))
        }
        fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
            let result = self.storage.write(name, bytes);
            self.finish(result)
        }
        fn verify(&self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
            self.finish(self.storage.verify(name, bytes))
        }
        fn rename(&mut self, stage: &str, name: &str) -> Result<(), StorageError> {
            let result = self.storage.rename(stage, name);
            self.finish(result)
        }
        fn sync(&mut self) -> Result<bool, StorageError> {
            let result = self.storage.sync();
            self.finish(result)
        }
    }

    #[test]
    #[ignore = "subprocess helper invoked with test-owned root and crash checkpoint"]
    fn crash_child() {
        let root = std::env::var_os("TILEWRIGHT_CRASH_TEST_ROOT").unwrap();
        let stop = std::env::var("TILEWRIGHT_CRASH_TEST_STEP")
            .unwrap()
            .parse()
            .unwrap();
        let store = open(Path::new(&root)).unwrap();
        let storage = CrashCheckpoint {
            storage: store.store.storage,
            step: Cell::new(0),
            stop,
        };
        Store::new(storage, RecoveryLimits::default())
            .retain("exact crash inverse\r\n")
            .unwrap();
        panic!("checkpoint did not exit");
    }

    #[test]
    fn checked_linux_process_exit_at_every_retention_checkpoint_preserves_state() {
        for stop in 1..=10 {
            let dir = session_root();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::linux::crash_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("TILEWRIGHT_CRASH_TEST_ROOT", dir.path())
                .env("TILEWRIGHT_CRASH_TEST_STEP", stop.to_string())
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(77));
            if stop < 5 {
                assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
                assert!(open(dir.path()).is_ok());
            } else {
                let name = if stop < 8 {
                    "inverse-0001.json.stage"
                } else {
                    "inverse-0001.json"
                };
                assert_eq!(
                    fs::read(dir.path().join(name)).unwrap(),
                    b"exact crash inverse\r\n"
                );
                assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
                assert_eq!(error(dir.path()).kind, FailureKind::ExistingEntries);
            }
        }
    }

    struct FaultStore(Store<Memory>);
    impl RecoveryStore for FaultStore {
        fn retain(&mut self, inverse: &str) -> Result<(), String> {
            self.0.retain(inverse).map_err(|error| error.to_string())
        }
    }
    #[test]
    fn every_injected_retention_fault_prevents_history_publication() {
        let world = session_root();
        copy_tree(
            &tilewright_editor_testkit::fixture_world("meadow"),
            world.path(),
        );
        let original = fs::read(world.path().join(CHUNK)).unwrap();
        for step in 1..=10 {
            for after in [false, true] {
                let mut history = History::new(WorldDocument::open(world.path()).unwrap());
                history
                    .prepare(history.document().revision(), "paint", PAINT)
                    .unwrap();
                let memory = Memory {
                    fail: Some((step, after)),
                    ..Memory::default()
                };
                let mut recovery = FaultStore(Store::new(memory, RecoveryLimits::default()));
                assert!(matches!(
                    history.commit(&mut recovery, &tilewright_edit::PublishOptions::default()),
                    Err(Error::Recovery(_))
                ));
                assert_eq!(fs::read(world.path().join(CHUNK)).unwrap(), original);
                assert!(history.pending().is_some());
                assert_eq!(history.depths(), (0, 0));
            }
        }
    }

    #[test]
    #[ignore = "subprocess helper invoked while parent owns the recovery lock"]
    fn lock_child() {
        let root = std::env::var_os("TILEWRIGHT_CRASH_TEST_ROOT").unwrap();
        assert_eq!(error(Path::new(&root)).kind, FailureKind::Locked);
    }

    #[test]
    fn checked_linux_simultaneous_process_session_is_refused() {
        let dir = session_root();
        let _owner = open(dir.path()).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::linux::lock_child",
                "--ignored",
                "--nocapture",
            ])
            .env("TILEWRIGHT_CRASH_TEST_ROOT", dir.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}
