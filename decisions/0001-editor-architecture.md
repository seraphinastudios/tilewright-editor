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
