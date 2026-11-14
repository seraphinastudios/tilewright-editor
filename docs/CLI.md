# Read-only commands

The `tilewright-editor` binary answers five read-only questions about a world
without opening a window or an editing session: `overview`, `tile`,
`catalog`, `lint` and `render` (issue #57). The MCP server's observe tools
will return the same reports later. `render` is the only one that writes,
and it writes at most one new file, never inside the world (see
[render](#render)). They are tested on Linux and Windows in CI; macOS is
expected to work, since nothing in them is platform-specific, but no CI job
builds or tests it. Editing stays Linux-only.

```sh
tilewright-editor overview [--json] ROOT
tilewright-editor tile [--json] ROOT PLANE LAYER X Y
tilewright-editor catalog [--json] [--match TEXT] ROOT
tilewright-editor lint [--json] ROOT
tilewright-editor render [--json] --out FILE [--region X Y W H] [--scale N] ROOT
```

`ROOT` is a world source root, the directory holding `world.jsonc`, given as
a path the way `open ROOT` takes it (it need not be UTF-8). This and every
later command spell it the same way: a positional `ROOT` right after the
command. Options (`--json`, `--match TEXT`, `--out FILE`, `--region X Y W H`,
`--scale N`) may come anywhere after the command, each at most once; an
argument starting with `--` is always an option, so a root cannot start with
`--`, and `--match` and `--out` refuse a value that starts with `--` (a usage
error, so `catalog --match --json ROOT` is not a search for `--json`). `LAYER` is
a spatial layer (32-bit) and `X Y` are plane tile coordinates (64-bit).

## Read-only by construction

`overview`, `tile` and `catalog` open the world with `WorldDocument::open`
(World's default load bounds), so they can describe a world the editing
entry point refuses as over its load budget. `lint` first opens it with the
editing entry point's budget (`WorldDocument::open_prototype`). If that
succeeds, its World is the one the default load gives: both loads read the
same files with the same parser and validation, and limits can only refuse
a world, never change one (a test compares the two Worlds). If it fails and
the default load succeeds, the budget alone refused the world: lint reports
`world.over_editor_budget` and its other findings come from the default
load. If both fail, the default load's diagnostics are the load failure, as
for every other command. The two loads are not atomic: a tree changed
between them can give a budget finding for a different tree than the
revision the report names. Each command then opens the editor catalog,
`preview.jsonc`, with the preview loader.
Nothing else is opened: no session, recovery store, provisioned copy,
transaction or publication. The crate that builds the reports,
`tilewright-editor-observe`, never constructs a `Session` and has no direct
dependency on the transaction, history or recovery crates (it reaches them
only through the ui crate, for the functions the desktop editor draws from).
`tools/check-architecture.sh` pins its direct dependencies and fails if its
non-test sources name `Session`, `History`, a recovery store, the
transaction or recovery crates, or provisioning, or if they open a directory
through World's file access, resolve a path or call a `write`. The crate
opens no file for writing: `render` gets pixels from it, and the binary
encodes them and writes the one file `--out` names, outside the world. A
test runs every printing command in both forms on a copy of the neutral
sample and checks that every file, its bytes, its permissions and its
modification time are unchanged and that no entry appears beside the world;
another does the same for `render`, on success (where the one new file
outside the world is the only change) and for every refusal.

## World text is data

Ids, names, titles, versions, field values and loader messages come from
the world. In both forms they are values, never member names or structure:

- `--json` prints one JSON document on stdout. Every escaped character (the
  set below) is written as a `\u` escape, as a UTF-16 surrogate pair above
  U+FFFF, so the document is safe to print as it is and parses with any
  strict JSON parser. Integers are JSON integers.
- Without `--json`, the same document prints as indented `name: value` lines.
  Each value is one escaped field, every escaped character written as its
  `\u{...}` escape: a world value cannot add or forge a line, move the
  cursor, retitle the terminal, reorder text or hide characters.

The escaped set (`node::escaped` in the observe crate) is every control
character (C0, DEL and C1); U+2028 and U+2029; the bidirectional formatting
characters U+061C, U+200E, U+200F, U+202A to U+202E and U+2066 to U+2069;
and the invisible or format characters U+00AD, U+180E, U+200B to U+200F,
U+2060 to U+2064, U+FEFF, U+FFF9 to U+FFFB and the tag characters U+E0000 to
U+E007F. Variation selectors are not escaped.

The text form is for people. It is not unambiguous, so scripts and agents
parse `--json`:

- a null value and the string `none` both print `none` (and `(none)`, an
  empty list or object, is also what a string `(none)` prints);
- a backslash is printed as itself, so world text holding the characters
  `\u{1b}` prints exactly like an escaped ESC;
- an empty string prints `""`, as does the two-character string `""`.

Readers that pass world text on, to a person or to a model, should label it
as data: a field value that reads like an instruction is still only a value.

## Output shapes

Every document starts with `format` and, except for refusals and load
failures, `revision`: the World revision (SHA-256 of the files the load read)
it describes. Shapes are stable within a format version; a later version adds
members or changes the tag.

The `catalog` member reports the editor catalog: `status` is `absent` (no
`preview.jsonc`), `invalid` (with the loader's `problem`) or `loaded`.

- `tilewright.editor-overview/1`: `profile` (id, version, projection, render
  and ground layers, terrain layer, terrains, layer height), `definitions`,
  `plane_count`, `planes` (each with title, kind, chunk size, default zone,
  counts of zones, anchors, stairwells, chunks, placed objects, carried
  inventory objects and portals, and per spatial layer its chunk count and
  the tile bounds `[x, y, width, height]` its chunks cover, or null when those
  do not fit in 64 bits), and `catalog`:
  the view (plane, layer, bounds), presentation, how many view tiles have a
  zone and how many each zone holds, prototype and resource counts, and
  `grants`: per-uid moves and fields, terrain grids and values,
  per-prototype object entries (layer, fields, default field names,
  portal), and the zone and anchor flags.
- `tilewright.editor-tile/1`: `tile`, `chunk` (path and chunk coordinates, or
  null), `ground` per grid, `height`, `zone` (id, whether it is the plane
  default, path innermost first, display name), `catalog.in_view`, `objects`
  on the tile in source order (uid, prototype, render layer, sub-layer, z,
  field overrides with their kind, portal target, direct inventory entries,
  carried objects at any depth, whether it has `custom` data, and `allowed`:
  what the catalog lets the author do in the active view: move, delete,
  portal and the editable field names), `anchors` on the tile with how many
  portals name each, and `arrivals`: objects anywhere whose portal leads to
  the tile, by location or through an anchor on it.
- `tilewright.editor-catalog/1`: `match`, `prototypes` (resource, anchor,
  whether placeable, and for placeable ones their layer, fields, default
  field names and portal flag, plus the terrain grids that offer them),
  `terrain_palette` (grid and value) and `zone_palette` (id and display
  name) of the view's plane. `--match TEXT` keeps entries whose id contains
  `TEXT`, ignoring case, exactly as the desktop Place palette filter does; it
  takes at most 64 bytes.
- `tilewright.editor-lint/1`: `counts` by severity and `findings`, each with a
  stable `code`, a `severity`, a `location` (some of `path`, `plane`,
  `layer`, `tile`, `uid`, `carried`, `anchor`, `zone`, `grid`, `codes`), a `message`
  this editor wrote and a `detail` quoting the loader when there is one.
- `tilewright.editor-load-failure/1`: `problems` and `diagnostics` (path,
  line, column, byte span, code and message), when the world does not load.
- `tilewright.editor-render/1`: `catalog` (status, view, presentation),
  `image` (plane, layer, `region` `[x, y, width, height]`, `tiles`, `scale`,
  `width` and `height` in pixels), `diagnostics` (the notes the preview adds
  when it prepares the region, each a `code` and a `message`) and `png`
  (`bytes`, and `sha256` of the file written). The report never names the
  output path.
- `tilewright.editor-refusal/1` (with `--json` only): `code` and `message`.
  A refusal's message is this editor's text, never world text. `render`'s
  write failure (exit 5) prints one too, with code `output.write`.

## Lint findings

Lint reports what the editor's existing checks find, nothing more. World's
own diagnostics come with a world that does not load. Reachability is not
checked: the profile cannot yet say what blocks movement.

Severities are defined by the desktop editor's entry points:

- `open ROOT` and these read-only commands need only World to load the
  world; they do not need the catalog.
- `window` opens no world.
- The editing entry point, `prototype`, which the provisioning helper
  starts on a private copy, opens the world with the prototype load budget
  (`tilewright_editor_document::prototype_load_options`: at most 256 files,
  1,024 directory entries and 8 MiB read, 1 MiB per document), then requires
  `preview.jsonc` to load and the catalog's view to be prepared.
- Before that, the provisioning helper (`tools/provision-prototype.py`)
  copies the whole tree with limits of its own: at most 4,096 directory
  entries and 4,096 regular files, 64 MiB in total and a directory depth of
  32, and it refuses links and special files. Lint does not check these:
  these commands read only through the World loader, which sees World's
  files and none of the helper's tree walk.

An **error** means the editing entry point cannot open or show the world as
it is; `lint` then exits 1. A **warning** is probably a mistake, but the
editing entry point opens the world. **Info** is worth knowing; nothing is
wrong.

| Code | Severity | Meaning |
|---|---|---|
| `world.over_editor_budget` | error | The prototype load budget refuses the world, which loads with World's default bounds. `location.codes` lists World's diagnostic codes for the budgeted load as World reported them, such as `source.budget` and the `source.missing_file` of the files it then left unread. |
| `catalog.absent` | error | The world has no `preview.jsonc`. The editing entry point cannot open it; `open ROOT` and these commands can. |
| `catalog.invalid` | error | `preview.jsonc` is there but does not load. |
| `preview.unprepared` | error | The catalog's view cannot be prepared for display, for example an object in it whose prototype the catalog cannot draw. |
| `preview.*` (others) | info | A note the preview adds when it prepares the view, such as `preview.effects-neutral`. |
| `portal.target_own_tile` | warning | A portal leads to the tile its object stands on. |
| `portal.target_no_chunk` | warning | A location target is in a map chunk that does not exist. |
| `portal.target_no_ground` | warning | A location target has no ground on the pick grid. |
| `anchor.unreferenced` | info | No portal names the anchor. |
| `zone.unused` | info | No tile, chunk palette or child zone uses the zone and it is not the plane default. |

The portal checks are the ones the desktop review shows when a portal is
edited; the pick grid is the first grid the catalog grants terrain editing
on, else the profile's first ground grid. A portal carried in an inventory
is checked from the tile of the placed object that carries it.

## render

`render` draws a region of the editor catalog's view with the CPU reference
renderer, the renderer every pick mask and the scripted demo's evidence come
from, and writes it as an RGBA PNG.

- **What it draws.** The World scene only: ground and placed objects of the
  view's plane and layer, as the desktop editor draws them, on its dark clear
  colour. No cell overlay, zone tint, marker, selection or interface. The
  world's art is drawn as it is; the renderer draws no text.
- **Which region.** The catalog's whole view by default, or `--region X Y W
  H`, a tile rectangle (`W` and `H` at least 1) that must lie inside the view
  bounds (`target.unknown` otherwise). A region of more than 4,096 tiles is
  refused (`output.budget`) before it is compared with the view.
- **How it is framed.** The camera is the desktop's default view camera
  narrowed to the region: centred on it, each art pixel `--scale` screen
  pixels (an integer from 1 to 8, default 1). The image is that camera's
  viewport, centred on the region and sized to hold everything the region
  draws (its ground, and objects that reach past their tile), so nothing the
  region draws is cut. Both sides are even, at least 2. The whole view's
  image is therefore exactly the middle of the CPU reference rendering of
  the scene the desktop prepares for its default 1280 × 800 view (tested,
  top-down and isometric); the desktop itself draws that scene on the GPU,
  which this does not compare with. Because the frame is centred, art that
  reaches far to one side leaves an empty margin on the other.
- **Regions and the whole view.** A region draws only the objects and ground
  of its own tiles. Its image equals that part of the whole view's image
  only when no art of the region's tiles reaches past the region and no art
  of other tiles reaches into it. In the neutral sample, a marker's art
  reaches 8 pixels above its tile: a region holding the marker's tile and
  the tile above is that part of the view, while one with the marker's tile
  on its top row is framed 8 pixels taller above and, centred, below (both
  tested). Art of a tile outside the region (a tall object, raised ground,
  an isometric neighbour) is never drawn into it.
- **Bounds.** The image is at most 8,192 pixels a side and 16,777,216 pixels
  in all, the reference renderer's own target limits; past them `render` is
  refused (`output.budget`). The region is first compiled in an 8,194 ×
  8,194 viewport, the largest image and a pixel more on every side, to find
  what it draws; if any object or tile of the region has no art in that
  viewport, it is refused the same way. A region the desktop's default view
  can prepare but that, at this scale, exceeds the renderer's scene budget
  (its quads or the raster work they take) is refused `output.budget` too;
  `preview.unprepared` means the desktop's default view cannot be prepared
  either, and lint says why. The PNG may be at most 68,165,632 bytes, a
  backstop checked after encoding: storing an image of the most pixels
  uncompressed would take about 67.1 MB.
- **Determinism.** The same world and arguments give the same pixels on
  every platform CI tests, Linux and Windows (the goldens, digests of the
  decoded pixels, are checked on both); other platforms are expected to
  match but are not tested. The same build gives the same PNG bytes: the
  encoder is pure Rust with fixed settings (8-bit RGBA, balanced
  compression, adaptive filtering) and writes only the IHDR, IDAT and IEND
  chunks (tested: the header and the chunk list, not the compression). A
  different version of the encoder may compress differently.

### What it writes

`render` writes at most one file, never inside the world: the new file
`--out FILE` names. It creates no directory and, apart from the race below,
replaces nothing.

- `FILE`'s directory must exist and be a directory that can be searched
  (`output.directory`).
- `FILE`'s last component must be a name every platform makes as written
  (`output.name`), one rule everywhere: UTF-8, at most 255 bytes, without
  `\`, `:` or NUL, not `.` or `..`, not ending in a dot or a space, not a
  Windows device name (CON, PRN, AUX, NUL, COM0 to COM9, LPT0 to LPT9, also
  with superscript digits, in any case, whatever follows a dot), and with no
  separator after it. The directories before it need not be UTF-8.
- `FILE` must be outside the world root (`output.inside-world`). Its
  directory, with every link resolved, may be neither the root, with every
  link resolved, nor beneath it, so a link to the world, or to a directory
  in it, is inside. The two are also compared with each path component
  lowercased: Linux keeps the case a path was typed in when it resolves it,
  even on a file system that ignores case (a network share, FAT or exFAT
  media, a casefolded directory), where `/share/WORLD` is the world
  `/share/World`. Lowercasing approximates those file systems' folding and
  fails closed: on a file system that tells case apart, a different
  directory whose path differs from the world's only in case, such as a
  sibling `WORLD` of `world`, is refused too. Aliases that neither can see,
  such as a bind mount of the world elsewhere, are not detected.
- Nothing may be at `FILE`'s name: no file, directory or link, not even a
  dangling link (`output.exists`). A link at the name is never followed. On a
  file system that ignores case, an entry whose name differs only in case
  counts.
- The image is drawn and encoded in memory before the file is made. The file
  is made through World's confined file access (`tilewright_source`), as the
  scripted demo writes its evidence; the editor's crates may not open files
  with the standard library. World's writer creates the file exclusively,
  not through a link, syncs it, and removes what is at the name again if
  writing fails, so a failed write leaves no file of this command's.
  A failure to write (exit 5, `output.write`) prints the path as given,
  escaped, and the system's reason on stderr.
- The race: World's writer first removes whatever is at the name, then
  creates the file. The name is checked again just before, through the
  directory that was checked (on Unix one directory handle, opened after the
  path was resolved and compared; on Windows the resolved path, reopened).
  A file another process creates at the name between that last check and
  the write is deleted and replaced, and one swapped in after a failed write
  can be removed by its cleanup. A directory swapped in for the resolved path
  before it is opened (on Windows, at any time) is not detected. A writer
  that only creates needs a create-only write in World.

The order is: the arguments (exit 2), the world loads (exit 3: its root must
resolve before the output is compared with it), the output path (the
`output.*` refusals), then the view and the drawing (`catalog.*`,
`target.unknown`, `preview.unprepared`, `output.budget`, `render.failed`).
A bad output path is therefore refused before anything is drawn, even for a
world whose view cannot be drawn. The world tree, the files beside it and
the output directory are left exactly as they were on every refusal and
apart from the one new file on success, which the tests check for each
refusal code except `render.failed`, which no test world can trigger.

## Limits

Every list is bounded; a bounded list `name` is followed by `name_more`, the
number of items it left out. Free text longer than its bound is cut and
followed by `name_clipped: true`. The numbers live in
`crates/tilewright-editor-observe/src/limits.rs`:

| Bound | Value |
|---|---|
| Characters of one free-text value | 256 |
| Bytes of one printed report | 1,048,576 (1 MiB) |
| `overview`: planes | 64 |
| `overview`: spatial layers per plane | 16 |
| `overview`: render layers, terrains, view zones, entries of each grant list | 64 each |
| Field names, default names and granted fields per object or prototype; fields per object | 8 |
| `tile`: objects | 16 |
| `tile`: anchors on the tile; arrivals | 8 each |
| `catalog`: prototypes, terrain choices | 128 each |
| `catalog`: zones | 64 |
| `catalog`: `--match` | 64 bytes |
| `lint`: findings listed (all are counted) | 256 |
| Load failure: diagnostics listed (`problems` counts all, at most 256) | 128 |
| `render`: tiles in the region | 4,096 |
| `render`: image side; image pixels | 8,192; 16,777,216 |
| `render`: `--scale` | 1 to 8 |
| `render`: PNG bytes | 68,165,632 |
| `render`: diagnostics listed | 16 |

Beyond loading the world, each command does work proportional to the world's
size at most. The bounds are computed so that every report fits the output
budget at its worst: every list full, every id at World's longest and every
free-text value made of characters that print as 12-byte escapes. Tests
build a near-worst case and check it fits; the largest JSON reports there
are a load failure (about 827 KB), `overview` (about 448 KB) and `tile`
(about 437 KB). A report past the budget would be refused
(`output.budget`) rather than cut. There is no paging yet: `--match` narrows
`catalog`, and `tile` takes one tile at a time.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. For `lint`: no finding is an error. |
| 1 | `lint` only: at least one finding is an error. |
| 2 | Usage error: nothing is printed on stdout and the usage goes to stderr. |
| 3 | The world does not load: the load failure is the output. |
| 4 | Refused, with a stable code: `target.unknown` (the tile's plane does not exist, or the region is not inside the view), `output.budget` (a report, region, image or scene past its bound), and for `render` `catalog.absent`, `catalog.invalid`, `preview.unprepared` (no view to draw; lint says why), `render.failed` (the reference renderer failed), `output.directory`, `output.name`, `output.inside-world` or `output.exists`. |
| 5 | `render` only: the image could not be written (`output.write`); this command leaves no file of its own (see the race under [render](#render)). |

`lint` follows the world CLI's `diff` convention (0 clean, 1 findings, 2
usage) and adds 3 for a world that does not load, which every command shares.
