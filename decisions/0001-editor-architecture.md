# ADR 0001: editor authority and dependency direction

Status: accepted for M1.

The editor is an offline authoring consumer. The world toolkit owns source
syntax, lossless documents, schemas, catalogs, diagnostics, compilers, semantic
diffs, and atomic file transactions. The Tilewright renderer owns scene
types, resources, device/GPU state, output targets, semantic masks, and
offscreen rendering. This repository owns project/tab state, explicit user
commands and history, document-to-scene adaptation, authoring panels/tools,
diagnostic presentation, preview composition, recovery UI, and packages.

The dependency direction is:

```text
world toolkit release   -> document adapter -> commands -> application
renderer release        -> preview adapter  -> application
project state -> commands/UI/test fakes
```

Production manifests pin immutable released source revisions. Coordinated local
overrides stay outside committed manifests. There is no game client, protocol,
server, game-specific, parser, writer, network service, or alternate renderer
dependency.

M1 uses the shared renderer's versioned scene/API/resource crates and its
deterministic reference test renderer to prove an empty viewport. GPU and SDL
handles stay below released renderer/application boundaries. The application
owns only SDL lifecycle; headless model tests do not initialize SDL.

## World transaction migration

The command model keeps a `WorldDocument` plus bounded undo/redo records.
`History::prepare` requires the author's observed world revision, plans the
semantic transaction without writes, verifies the plan's before-image against
that revision, and calls World's `Plan::check_undo`. Callers inspect the pending
plan's file before/after images, sealed transaction and inverse before committing.
A failed preparation discards any older pending preview; cancel changes no files.
A blocked recovery session retains its pending inverse even if another command
or cancel is attempted. The document retains the toolkit source root opened at
load time; its display path never becomes fresh write authority. On Unix this
is a directory handle; Windows retains the toolkit's documented path-based
limitations and explicit unchecked-access requirement.

`History::commit` first asks the application's `RecoveryStore` to durably retain
that exact inverse outside authored source. The integration must use toolkit
storage, refuse an existing record, and keep records after failure. This is a
required integration interface, not an implemented desktop recovery store.
Only then does World publish the reviewed plan with the caller's explicit access
policy. The editor never automatically opts into unchecked access. World offers
atomic replacement per file and rollback on ordinary failure, not a crash-atomic
multi-file filesystem transaction. The durable inverse is necessary for recovery.

Any publication error, including an applied but non-durable or incompletely
cleaned result, keeps history unadvanced and blocks further operations in that
session. Callers must present World's `applied`, `durable`, and `cleaned` outcome,
inspect the recovery record and source, and reopen after recovery. A recovery
storage failure occurs before publication and permits retry. No-op commits do
not write a record or consume history. Undo/redo are prepared inverse transactions
with identical validation, recovery and publication requirements. A successful
new edit discards the redo branch; failed edits do not. Each entry retains both
the original sealed forward command and its inverse; undo/redo move that fixed
entry between stacks. Both directions count toward the memory limit at edit
admission, so asymmetric create/delete commands cannot exhaust the budget while
undoing already accepted work. History is bounded by entry count and retained
UTF-8 record bytes.

Whole-world revision checks are optimistic. World rechecks consulted files and
validates publication but permits concurrent valid changes to unrelated files.
After publication the editor reloads its retained root and compares the actual
world against the reviewed result. A mismatch or reopen error explicitly reports
that the edit was applied, retains recovery and unadvanced history, and blocks
the session. This does not promise a global atomic revision guard against
noncooperating external writers; a change after the reload is caught at the next
operation.

The pre-migration application exposed version, synthetic headless state, and
window lifecycle smoke commands. Editing and undo/redo existed as headless
library APIs, not a GUI or edit/save CLI. The World migration preserves those
library capabilities and provides `headless ROOT` for the World-backed smoke
path. A transactional desktop UI, persistence backend and automation interface
remain separate work; the migration does not claim those features exist.


## Bounded offline application composition

The offline prototype adds a native persistent populated SDL window, a generic
World session/action model, strict catalog/ready-resource loading, shared-renderer
scene/mask adaptation and a durable recovery implementation. It continues using
this ADR's existing History planning/publication/undo path. The document can now
retain explicit prototype source budgets without changing ordinary open defaults.

Trusted provisioning creates fresh separate private World/recovery/evidence
roots on supported persistent local Linux storage, verifies retained-FD mount
identity, syncs creation and passes descriptors to the app. This is a bounded
deployment proof, not a general arbitrary-root constructor or alias-proof API.
The native shell delegates source/recovery/evidence I/O to World and projection,
geometry, rendering and exact masks to the released renderer. External game
adapters remain separately owned; export is an explicit external toolkit command.

Rich panels/text, arbitrary-root provisioning, persistent history, automatic
recovery replay, broader platform durability and gameplay/content runtime cutover
remain future work. See [offline prototype](../docs/OFFLINE-PROTOTYPE.md).
