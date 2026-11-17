# The MCP server

`tilewright-editor mcp ROOT` serves read-only observation of one world to a
[Model Context Protocol](https://modelcontextprotocol.io) client, such as a
coding agent. The client starts it as a subprocess and talks to it over its
standard input and output. Its tools answer the questions the read-only
commands of [docs/CLI.md](CLI.md) answer, with the same reports, and page
their long lists (issue #57). It is the crate `tilewright-editor-mcp`,
which the binary runs. Editing is a separate server and crate, the MCP
editing server of [docs/MCP-EDITING.md](MCP-EDITING.md), which reuses this
server's protocol loop and observe tools; nothing in this server can reach
it.

```sh
cargo run --locked --package tilewright-editor -- mcp crates/tilewright-editor-testkit/fixtures/prototype-neutral
```

A client that starts stdio servers from a command and its arguments would
name, for example, `{"command": "tilewright-editor", "args": ["mcp",
"/path/to/world"]}`. **No MCP client product has been tested against this
server.** It is tested with its own stdio tests (goldens checked against
the shapes of the revision's specification, schema checks, a fuzz test)
and through the packaged binary in the Linux and Windows packaging smoke
tests.
`ROOT` is positional, as for every other command, and the server takes no
options. It reads the world, never writes it: see
[Read-only by construction](#read-only-by-construction).

## Protocol

The server implements MCP revision
[2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28)
and only that revision, as the World toolkit's own server does. That
revision is stateless: there is no `initialize` handshake, and every
request names its protocol version in `params._meta`. **Clients that speak
only revision 2025-11-25 or an earlier one cannot connect.** Their
`initialize` is refused with an error that names the revision this server
supports. The revision's stdio transport asks a client that speaks both
eras to send `server/discover` first.

- Transport: stdio. Each message is one line of UTF-8 JSON-RPC 2.0, ended
  by a newline; a line may end in CR LF instead, and blank lines are
  skipped. Standard output carries replies and nothing else. Notes for
  people go to standard error: one line when the server starts (the
  revision it serves, or how many problems keep the world from loading) and
  one for each message refused as too long; they hold no world text.
  Messages are handled one at a time, in order. The server stops when its
  input ends, and quietly, with status 0, when its output is closed.
- Every request needs `params._meta` with
  `io.modelcontextprotocol/protocolVersion` (a string) and
  `io.modelcontextprotocol/clientCapabilities` (an object); the server uses
  no client capability. `io.modelcontextprotocol/clientInfo` is optional
  and ignored. A request for any version but `2026-07-28` gets the
  revision's `UnsupportedProtocolVersionError`.
- Methods: `server/discover`, `tools/list` and `tools/call`.
  `server/discover` gives the supported versions, the capabilities (`tools`,
  without list change notifications) and the instructions. The server has
  no resources, prompts, sampling or elicitation, and sends no requests or
  notifications.
- Every result has `resultType: "complete"` and names the server in
  `_meta["io.modelcontextprotocol/serverInfo"]`, whose `version` is the
  version string the editor build reports for itself (`tilewright-editor
  version`): the release version in a release package, `0.1.0-dev` in any
  other build. The `server/discover` and
  `tools/list` results may be cached for an hour (`ttlMs` 3600000,
  `cacheScope` `public`): they do not change while the server runs.
- Notifications get no reply, not even a wrong one. As in the World
  server, any object without an `id` is treated as a notification, even
  one that is not a valid request (`{"jsonrpc": "2.0", "method": 1}`), and
  gets no reply; JSON-RPC 2.0's own example answers that with -32600 and a
  null `id`. This is a known divergence.
  `notifications/cancelled` is accepted and needs nothing, because each
  request is answered before the next is read. A response sent to the
  server, whatever its ID, is ignored.
- Messages are read with World's strict JSONC parser, with comments
  refused: duplicate keys, trailing commas, a byte-order mark, integers
  past 2^53 − 1, nesting deeper than 32 and trailing content are errors. A
  request ID is a string or an integer. Batches (JSON arrays) are refused.
- Replies are written by hand with the CLI's escaping (see
  [World text is data](#world-text-is-data)), so no value can break a line
  of the transport or reach a terminal as a control sequence.

### Errors

Protocol errors are JSON-RPC errors with a fixed message and a `data`
object. An error about a message whose ID could not be read has no `id`.

| Code | `data` | When |
| --- | --- | --- |
| -32700 | `code`: a `jsonc.*` code, or `request.not_utf8` | The line is not UTF-8 JSON within the limits |
| -32600 | `code`: `request.too_large` | The line is longer than 64 KiB |
| -32600 | `code`: `request.not_object`, `request.id`, `request.jsonrpc`, `request.method` | Not a JSON-RPC request |
| -32601 | `code`: `method.legacy`, `supported`: `["2026-07-28"]` | `initialize`, from a client of an earlier revision |
| -32601 | `code`: `method.unknown` | Any other method |
| -32602 | `code`: `params.meta` | `params._meta` without its protocol version or client capabilities |
| -32602 | `code`: `params.not_object`, `params.name`, `params.arguments` | Malformed params |
| -32602 | `code`: `tool.unknown` | A tool this server does not have |
| -32602 | `code`: `cursor.invalid` | A cursor for `tools/list`, which lists every tool on one page |
| -32022 | `supported`: `["2026-07-28"]`, `requested` | Another protocol version |

A tool that is called correctly but cannot answer returns a result with
`isError: true` and, as its first text content, one line the server writes,
`<tool> failed: <code>: <message>`:

| Code | When |
| --- | --- |
| `arguments.invalid` | An argument the tool does not take, of the wrong type or range, a required one missing, or a `cursor` without `section` |
| `world.invalid` | The world does not load. A second text content holds the CLI's load-failure document, `tilewright.editor-load-failure/1`, in compact JSON: its `diagnostics` are those that fit the result budget, `diagnostics_more` counts the rest |
| `revision.changed` | The `revision` argument is not the snapshot's World revision; the line ends with the current revision, or says the world does not load |
| `cursor.invalid` | A cursor not issued for this tool, these arguments and this section, or past the end of the list |
| `cursor.stale` | A cursor issued for another snapshot of the world |
| `target.unknown` | `tile`: the world has no plane with that id. `render`: the region is not inside the catalog's view |
| `output.budget` | The result would exceed its budget (see [Limits](#limits)); for `render`, a region past 4,096 tiles, an image past its pixel bounds or a PNG past its byte bound, each with its own message |
| `catalog.absent`, `catalog.invalid`, `preview.unprepared`, `render.failed` | `render` only, as for the CLI: no view to draw, or the reference renderer failed; `render.failed` also when the drawn image could not be encoded as a PNG |

Arguments are checked first, then `revision`, then whether the world
loads, then the cursor.

## Tools

Every tool is marked read-only, non-destructive, idempotent and closed
world in its annotations, and has an input and an output schema;
`tools/list` gives them in a fixed order. A successful result has
`structuredContent`, which conforms to the output schema, and, as its first
text content, the same JSON serialized exactly as sent, as the revision
says a tool with structured content should, so a client that passes only
the text to a model passes the data. No tool takes a path.

| Tool | Arguments | Result |
| --- | --- | --- |
| `overview` | `section`, `limit`, `cursor`, `revision` | `tilewright.editor-overview/1`, as `overview --json` prints it |
| `tile` | `plane`, `layer`, `x`, `y` (all required), `section`, `limit`, `cursor`, `revision` | `tilewright.editor-tile/1`, as `tile --json ROOT PLANE LAYER X Y` prints it |
| `catalog` | `match`, `section`, `limit`, `cursor`, `revision` | `tilewright.editor-catalog/1`, as `catalog --json [--match TEXT]` prints it |
| `lint` | `section`, `limit`, `cursor`, `revision` | `tilewright.editor-lint/1`, as `lint --json` prints it |
| `render` | `region`, `scale`, `revision` | `tilewright.editor-render/1`, as `render --json` prints it, and the PNG as image content |
| `refresh` | none | Reloads the world: its `revision`, `valid`, the diagnostic count, the `previous_revision`, and whether the snapshot `changed` |

The reports are built by the same functions as the CLI's, from the same
facts; a test compares each with the CLI's `--json` document. The output
shapes are those of [docs/CLI.md](CLI.md#output-shapes), with two
differences: JSON is compact (no whitespace), and each paged list is
followed by `<list>_cursor` as well as `<list>_more`.

- Arguments are what the input schemas say, and the server accepts exactly
  what they accept at their bounds (tested from the schemas sent): an
  `integer` may be written with a zero fraction (`3.0`), as JSON Schema
  allows, but not with another; string lengths count characters, as
  `maxLength` does; `cursor` needs `section` (`dependentRequired`). One
  rule is the server's alone: `render`'s width and height are at least 1
  (below).
- `tile`: `plane` is a string of 1 to 256 characters, `layer` a 32-bit
  integer, `x` and `y` integers of magnitude at most 2^53 − 1 (the parser
  refuses larger integers, -32700).
- `catalog`: `match` keeps entries whose id contains it, ignoring case, as
  `--match` does; at most 64 characters (`--match` takes 64 bytes). Unlike
  `--match`, it may start with `--`.
- `lint`: its findings are computed once per snapshot, when the world is
  loaded, since they prepare the catalog's view; a call only lists them.
- `render`: `region` is `[x, y, width, height]` in tiles (in the schema,
  four integers; that width and height are at least 1 the description
  says and the server checks, since a per-item rule would need
  `prefixItems`, which draft-07 hosts read differently), inside the
  catalog's view (the whole view by default); `scale` is 1 to 4 (default
  1). The region is framed and drawn exactly as `render` frames and draws
  it, and the image is encoded by the same encoder, so it is the PNG
  `render --out` writes for the same arguments, byte for byte (the
  packaging smoke tests check this on Linux and Windows). Its report's
  `png` gives the PNG's `bytes` and `sha256`. The image is returned as
  image content (`type` `image`, base64 `data`, `mimeType` `image/png`)
  after the text content; nothing is written.

Every report names `revision`, the World revision (the lowercase hex
SHA-256 of the files World's load read) it was computed at. Every tool
but `refresh` takes an optional `revision`, 64 lowercase hex digits: when
the snapshot is not at that World revision, the call fails with
`revision.changed`. A world that does not load has no revision, so any
`revision` given fails then.

## Paging

Each list the CLI cuts at a bound, with `<list>_more`, is paged here. One
pattern serves every tool: the report lists the first items of each paged
list and gives a cursor to the rest, and `section` asks for one page of
one list instead.

| Tool | Sections |
| --- | --- |
| `overview` | `profile.render_layers`, `profile.terrains`, `planes`, `catalog.view_zones`, `catalog.grants.movable_uids`, `catalog.grants.fields`, `catalog.grants.terrain`, `catalog.grants.objects` |
| `tile` | `objects`, `anchors`, `arrivals` |
| `catalog` | `prototypes`, `terrain_palette`, `zone_palette` |
| `lint` | `findings` |

A section is the list's path in the report: `catalog.grants.fields` is the
member `fields` of `grants` of `catalog`. A `section` that is not one of the
tool's is `arguments.invalid`. A real section that this world's report does
not hold (the `catalog.*` sections of a world whose catalog is absent or
does not load) is an empty page: `total` 0, no items, no cursor.

- **Without `section`**, the result is the report. Each paged list holds at
  most `limit` items (default 50, at most 500) and at most the CLI's bound
  for it, fewer when the result's byte budget runs out first (lists are
  filled in the order the report lists them), followed by
  `<list>_more` (how many items follow) and `<list>_cursor`: a cursor to
  the next item, or null when the list is complete.
- **With `section`**, the result is one page of that list,
  `tilewright.editor-page/1`: `revision`, `section`, `offset` (the index of
  its first item), `total` (how many items the list has), `items` (at most
  `limit`, each shaped as in the report, fewer when the byte budget runs
  out: `byte_budget_reached` then says so), and `next_cursor`, or null on
  the last page. The CLI's bound does not apply to a page: every item is
  reachable.
- A cursor goes back with the same tool, the same arguments and `section`
  set to the list's path; `limit` may change between pages. Lists nested in
  an item (a plane's spatial layers, an object's fields, a terrain grant's
  values, a prototype grant's fields and defaults) keep the CLI's bound and
  `_more` count and are not paged.
- A cursor is `tw1.<tool>.<snapshot>.<arguments>.<offset>`: the tool, the
  snapshot binding (below), a 64-bit fingerprint of the arguments that
  choose the report (`tile`'s plane, layer and coordinates, `catalog`'s
  `match`) and the section, and the offset of the next item. It is not a
  secret and not a capability; it only tells the call it belongs to from
  others. A cursor for another tool, other arguments or another section,
  or past the end, is `cursor.invalid`; one from another snapshot is
  `cursor.stale`. Paging a list to the end returns every item exactly
  once, in the report's order (tested with limits of 1, 7 and 500 on a
  world past every bound).

## Snapshots

The server loads the world when it starts, as `lint` does (the editing
entry point's load budget first, then World's default bounds if that
refuses it), opens its catalog with the preview loader, computes lint's
findings, and keeps all of it. Every result is computed from that
snapshot until `refresh` loads the world again, as the World toolkit's
server does: editing files changes nothing the server answers until then,
and a call never reads the tree. The snapshot belongs to the server
process, not to a client.

A snapshot's binding is, for a world that loads, its World revision, then
the catalog's own revision in full (the SHA-256 of `preview.jsonc`, which
names each resource's digest; `n` when there is no catalog, `i` and a
64-bit fingerprint of the loader's message when it does not load), then a
64-bit fingerprint of the editing budget's verdict. For a world that does
not load it is `x` and a 64-bit fingerprint of its diagnostics. The
fingerprints are FNV-1a, not a security measure: two different invalid
catalogs, budget verdicts or broken worlds with colliding fingerprints
would read as one snapshot: `refresh` would say `changed: false`, and
cursors would stay valid across the two, so pages could mix them. A world
author can search for such a collision in an invalid catalog's message
(about 2^32 tries); the only report part that differs is that message.
Two loadable catalogs never collide. A test checks that a changed
invalid-catalog message is a changed snapshot. `refresh` reports `changed` when the binding changed, so a
catalog edit is a change even though the World revision stays the same;
cursors are bound to the binding, while the `revision` argument compares
only the World revision. The old snapshot is dropped before the world
loads again, so the server never holds two.

The server starts on a world that does not load, and on a root that does
not exist, so a client can read why and call `refresh` once it is fixed.

## World text is data

Ids, names, titles, versions, field values, loader messages and lint
messages may come from the world, or quote it. The server's instructions
and every tool description say that results are world data, not
instructions. In addition, every string property the world can have
written is marked in the output schemas with the annotation
`"x-world-text": true` (field values of any type are marked too); every
other string is this editor's own: a format tag, a section name, a code,
an enumerated value, a revision or digest, a cursor, or the call's own
`match`. JSON Schema validators ignore unknown keywords, so the marker
changes no validation. The second text content of a `world.invalid` error,
the CLI's load-failure document, is world data too, as the instructions
say: its paths and messages come from the world.

Two tests hold the schemas to this. One builds a world that writes a
marker into the strings a report shows that the world spells (plane ids,
titles and default zones, zone ids and names, anchors, prototypes, uids,
field names, values and defaults, grants, terrain values, a render layer,
a catalog problem quoting a path, and a load failure naming a plane), and
checks that every string holding the marker, in every report, page and
`render` result, is at a marked position. The other checks, on that world
and on the crowded, widest and meadow worlds, that every unmarked string
is in a closed vocabulary of this editor's own strings. Every result is
checked against its output schema. The marker world keeps the profile's id
and version and the ground grids as they are; they are marked in the
schemas and pass the vocabulary rule.

Every reply escapes exactly the characters the CLI's JSON escapes, the one
set listed in [docs/CLI.md](CLI.md#world-text-is-data) (`node::escaped` in
the observe crate), written as `\u` escapes (surrogate pairs above
U+FFFF). A world value therefore cannot end the reply's line or forge a
message, and the characters of that set reach a reader only as escapes.

## Read-only by construction

No tool takes a path, and nothing in the server writes, creates, locks or
removes a file, provisions a copy, or opens an editing session, a history,
a recovery store or a transaction. The world is opened exactly as the
read-only commands open it, through the observe crate; `render` returns its
image in the reply. `tools/check-architecture.sh` pins the crate's direct
dependencies (the document, observe and preview crates, World's source and
JSONC crates; not the ui, commands or recovery crates). Over the observe
and MCP crates' sources, comment lines aside, it fails on any of these
names or calls. They are text searches, not a proof:

- `Session`, `History`, `DurableRecovery`, `RecoveryStore`,
  `tilewright_edit::`, the commands or recovery crates, or `provision`;
  for the MCP crate also the ui crate;
- `SourceRoot`, `SourceDirectory`, `SourceLock`, `canonicalize`,
  `make_directory`, a method call `.directory(` or `.lock(`, or a call
  (`.f(` or `::f(`) of `rename`, `remove_file`, `remove_directory`,
  `set_own_access`, `make_directory`, `write` or `create`;
- the standard library's `fs`, `net` or `process` modules however they are
  imported: each file is searched with its lines joined (a file that
  cannot be read fails the check), and inside a `use ...;` statement a bare
  `fs`, `net` or `process` followed by `::`, `,`, `}`, `;` or `as` is
  refused, so `use std::{io, fs};` over several lines is caught; elsewhere
  `fs::`, `net::` and `process::` are, so `std::process::exit` and
  `process::Command` are too, while a variable `process` or an enum variant
  `Command` is allowed; and the whole words `File`, `OpenOptions`,
  `DirBuilder`, `create_dir` and `create_dir_all`. String literals and
  trailing comments are searched as code: one that spells such a name fails
  the check;
- and, as for every editor crate, `std::fs`, `std::net`,
  `std::process::Command`, `tokio::` or a function named `parse`,
  `serialize`, `write` or `save`.

A test runs
every tool, every paged section, `refresh`, hostile requests and a world
that stops loading on a copy of a world, and checks that every file, its
bytes, permissions and modification time are unchanged and that no entry
appears in or beside it.

## Limits

| Bound | Value |
| --- | --- |
| Bytes of one message | 65,536 |
| Bytes of one `tools/call` result as sent, both copies counted, without `render`'s image | 262,144 (256 KiB) |
| Bytes the items of a result's paged lists may cost, both copies counted | 196,608 |
| `limit` | 1 to 500, default 50 |
| Bytes of a cursor | 256 |
| `render`: tiles in the region | 4,096 |
| `render`: image side; image pixels | 2,048; 1,048,576 |
| `render`: `scale` | 1 to 4 |
| `render`: PNG bytes (before base64) | 196,608 |
| `render`: one reply line at most: the result, the largest PNG's 262,144 base64 bytes, and the fields around them | about 524,700 |

A tool's result is sent twice, as `structuredContent` and as its text
copy, where every `"` and `\` costs two bytes; the budget counts both. Each
list item is costed as it is listed, and a list ends before the first item
that would take the result past its budget. Lists are filled in the order
the report lists them, and once one item has not fitted, no later list of
the result admits another (tested on the widest world: the lists that hold
items are a prefix of the tool's sections), so the cursors pick up where
the budget ran out. Free text keeps the CLI's bound (256 characters, then
`<name>_clipped: true`). A result whose members outside the paged lists
alone exceed the budget is refused with `output.budget` rather than cut;
no report reaches that in the tests' widest world. `render`'s image is
outside the result budget and has its own bounds, far below the CLI's:
one call draws at most a mebipixel.

Beyond the load, a call does at most one pass over the loaded world, as
the CLI's command does; `lint`'s view preparation runs once per load.

## Not here

Editing tools, change sets and proposals (the MCP editing server's, in
[docs/MCP-EDITING.md](MCP-EDITING.md)), deeper
analysis such as reachability, an HTTP transport, resources and prompts,
and attaching to a running desktop editor.
