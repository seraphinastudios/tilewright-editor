# Prototype durable inverse retention

`DurableRecovery` implements the existing commands `RecoveryStore` contract.
`History::commit` retains an exact inverse here before World publishes authored
source. This crate neither edits World files nor implements undo/redo or recovery
replay.

The application calls `from_provisioned_session_root(SourceRoot, RecoveryLimits)`
only with a retained root from its trusted prototype provisioner. The provisioner
must first create and sync a fresh private session and distinct World/recovery
children, establish their identity and separation, and pass inherited directory
descriptors to the app. Public SourceRoot APIs alone cannot prove an arbitrary
recovery path is outside a selected project. The constructor name documents this
caller proof boundary; it is not a general path-safety certification.

The store requires Linux, a checked local filesystem, current-user ownership,
mode exactly 0700 without ACLs, and an exclusive advisory lock held for its entire
lifetime. Records receive 0600 and matching access-control labels. A second
cooperating process cannot open the same store. Same-UID malicious processes and
privileged host administration are outside this prototype's isolation guarantee.
Retained handles survive path replacement; the service never reopens its root
by a display path.

At startup, any existing entry causes refusal. Enumeration is bounded, including
non-UTF-8 names. Existing records and stages must be preserved and inspected;
start a fresh provisioned session instead of deleting or automatically applying
old inverses. Some old inverses describe successful historical changes.

Each inverse gets fresh deterministic stage and final names. The service checks
absence, checks final-name label compatibility, writes and syncs the stage,
verifies exact bytes and access, rechecks final absence, renames, requires a true
directory sync result, and verifies the final bytes and access. World write and
rename primitives can replace existing names: private storage, cooperating-writer
locking and absence checks are essential parts of this composition. They are
not a hostile-writer no-clobber guarantee.

No error triggers cleanup or retry. A failed operation may have taken effect;
its stage/final identity and inverse digest remain available in `last_error()`.
The instance stays blocked. `records()` contains successfully retained record
identities; records remain after both successful and failed World publication.
Errors never include inverse text or absolute root paths. Quota errors include
limits and requested size; oversized input is rejected before hashing. Quotas are
at most 1024 records and 64 MiB exact inverse text; there is no automatic deletion.

## Validation

The crate's unit suite includes a private storage seam exercising failure before
and after every retention operation, permanent blocking and preservation,
quotas, collisions and nondurable directory outcomes. Checked Linux tests cover
private permissions, default ACL refusal, access changes, special entries,
bounded enumeration, exact bytes and digest, same-process and cross-process
locking, root retargeting, corruption, and default checked World edit/undo.
Every injected retention fault is also routed through History to verify source
publication does not happen. A failed World publication retains its inverse.

A subprocess exits without destructors at each of ten retention checkpoints;
the parent verifies surviving bytes and startup refusal. These are process-crash
proofs, not power-loss or storage-hardware fault tests. The two ignored helper
tests are explicitly invoked by the ordinary parent tests.

Run on a checked Linux filesystem, with Python 3 (already used by repository
validation) for the Linux ACL fixture. `TILEWRIGHT_CHECKED_TEST_ROOT` may select a
test directory; it does not bypass filesystem checks. The default is the OS temp
directory. Tests deliberately fail when the selected storage cannot meet the
checked contract. All direct filesystem fixture operations occur in test code;
production storage exclusively uses released World public APIs.
