# Prototype durable inverse retention

`DurableRecovery` implements the existing commands `RecoveryStore` contract.
`History::commit` retains an exact inverse here before World publishes authored
source. This crate neither edits World files nor implements undo/redo or recovery
replay.

The application calls `from_provisioned_session_root(SourceRoot, RecoveryLimits)`
only with a retained root from its trusted prototype provisioner. The provisioner
must first create and sync a fresh private session and distinct World/recovery
children, establish their identity and separation, and pass inherited directory
descriptors to the app. It must also verify through those retained descriptors
that recovery uses supported nonvolatile local storage, refusing tmpfs, ramfs,
memfd, unknown and unsupported filesystems before enabling editing. Public
SourceRoot APIs alone cannot prove an arbitrary recovery path is outside a
selected project or establish physical persistence. World's checked-filesystem
classification includes tmpfs, where successful sync does not preserve data
across power loss. The constructor name documents this caller proof boundary;
it is not a general path-safety or storage-hardware certification.

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

The wrapper performs no additional cleanup or retry after an error. The released
World write primitive may remove its own partial stage on write, access or file
sync failure. A failed operation may have taken effect; any surviving stage and
final record remain preserved, with the attempted identity and inverse digest
available in `last_error()`. The instance stays blocked and source publication
cannot proceed through this failed retention. `records()` contains successfully
retained record identities; records remain after both successful and failed World publication.
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
directory. Tests deliberately fail when the selected storage cannot meet World's
checked contract. A tmpfs test root can exercise the retention protocol, faults
and process-crash windows, but provides no persistent-storage acceptance proof.
Production acceptance additionally requires the provisioner's nonvolatile-storage
checks and evidence from the selected persistent filesystem. All direct filesystem
fixture operations occur in test code; production storage exclusively uses
released World public APIs.
