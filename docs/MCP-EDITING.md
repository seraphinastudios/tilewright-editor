# The MCP editing server

`tilewright-editor sandbox-mcp` serves editing in one agent sandbox to a
[Model Context Protocol](https://modelcontextprotocol.io) client over
stdio: the sandbox runner's requests ([docs/SANDBOX.md](SANDBOX.md)) as
MCP tools, and the read-only server's observe tools ([docs/MCP.md](MCP.md))
on the sandbox world. An agent prepares, reviews and accepts steps in its
own provisioned copy and submits them as a proposal file
([docs/PROPOSALS.md](PROPOSALS.md)). **Only a person applies a proposal**,
to their own world, one step at a time through the normal Review and
Accept, in the desktop
([applying in the desktop](PROPOSALS.md#applying-in-the-desktop)). No tool applies a
proposal, makes, lists or deletes a sandbox, or reaches a person's world.

It is the crate `tilewright-editor-mcp-sandbox`. The read-only server
(`tilewright-editor mcp ROOT`, crate `tilewright-editor-mcp`) is unchanged
and cannot reach any of this: it names no session, sandbox or proposal
(`tools/check-architecture.sh`), and this crate depends on it, not the
other way round.

**No MCP client product has been tested against this server**, and no
native window was involved in testing it. It is tested with its own stdio
tests (goldens, the line runner's replies, schema agreement, world-text
marking, a fuzz test), an end-to-end test through the binary and the
provisioner, and the Linux packaging smoke test.

The owner decided (2026-11-14 and 2026-11-15): MCP revision 2026-07-28
only, hand-written JSON-RPC over stdio with no SDK and no async runtime;
an agent edits and accepts only in its own provisioned sandbox and submits
a proposal, and only a person applies it; no agent tool publishes to the
project or creates a sandbox; the agent's actor comes from the operator's
flag, and the name a client gives itself is only an untrusted label.

## Starting it

The provisioning helper starts it, with `--mcp`:

```sh
python3 tools/provision-sandbox.py --mcp \
  --world /path/to/world \
  --destination-parent /absolute/private/sandboxes \
  --actor agent:example --client-label "example client" \
  --editor /path/to/tilewright-editor
```

The helper is the one of [docs/SANDBOX.md](SANDBOX.md#starting-a-sandbox),
with the same checks, copy, receipt and descriptors; `--mcp` only makes it
replace itself with `tilewright-editor sandbox-mcp` instead of
`tilewright-editor sandbox`, with the same options. It prints nothing on
standard output, so the server's protocol owns standard output from its
first byte. `--mcp` needs `--editor`.

The command is fixed, so it is what an operator names in a client's
configuration of stdio servers, for example `{"command": "python3",
"args": ["/path/to/tools/provision-sandbox.py", "--mcp", "--world",
"/path/to/world", "--destination-parent", "/absolute/private/sandboxes",
"--actor", "agent:example", "--editor", "/path/to/tilewright-editor"]}`.
Every start makes a new sandbox under the parent; one server process
serves one sandbox. The actor is the operator's choice, never the
client's. Remove old sandboxes by hand; nothing deletes them.

`tilewright-editor sandbox-mcp` takes exactly the runner's options
(`--world-fd`, `--recovery-fd`, `--proposals-fd`, `--actor`,
`--receipt-sha256`, `--client-label`) and refuses to start as the runner
does. Standard error gets the helper's line, then
`tilewright-editor sandbox-mcp: ready actor=<actor> revision=<hex>`, and
one line for each message refused as too long. The JSON-lines runner is
unchanged and still started without `--mcp`.

## A session and its end

The server's session is the sandbox's only run. There is no `quit` tool:
the client ends the session by closing the server's standard input (or its
output), and the server exits with status 0. **Steps not submitted by then
are lost**: once a step was accepted, the sandbox's recovery store holds
records and the sandbox cannot be started again, by this server or the
runner, and a pending change is never kept. So an agent calls `submit`
after each batch of steps worth keeping; a later `submit` records every
step from the start again, so submitting often loses nothing. The sandbox
itself, with its world copy and its proposals, stays on disk for the
person.

## Protocol

The protocol is the read-only server's, unchanged, from the same code
([docs/MCP.md](MCP.md#protocol)): revision 2026-07-28 only, no
`initialize` (refused with `method.legacy`), `params._meta` with the
protocol version and client capabilities on every request,
`server/discover`, `tools/list` and `tools/call`, the same JSON-RPC errors
with the same `data.code`s, `resultType`, `ttlMs` and `cacheScope` on
discovery and the tool list, the same 64 KiB message bound. Notifications
get no reply, and, as in the read-only server and the World toolkit's, an
invalid JSON-RPC object without an `id` is treated as one and gets no
reply either (a documented divergence from JSON-RPC 2.0's own example).

Three things differ. `serverInfo`'s title is `Tilewright editor (sandbox
editing)`. Its log lines start `tilewright-editor sandbox-mcp:`. And a
message is read under wider JSON limits, so every action the proposal
format records fits as `params.arguments.action`: 4,096 array items, 256
object members, 22,288 values, strings up to the message bound, nesting to
32 (the read-only server's: 1,000, 128, 10,000, 16 KiB, 32). The 64 KiB
line bound is the same, so the largest strokes fit only with small
coordinates.

## Tools

`tools/list` gives the eight editing tools, then the five observe tools,
on one page, in this order. Every tool has an input and an output schema
(JSON Schema 2020-12). No tool takes a path, a descriptor, an actor, a
label or a receipt.

| Tool | Arguments | Runner request | Result besides `ok`, `request`, `state` |
| --- | --- | --- | --- |
| `status` | none | `status` | `actor`, `client`, `editor`, `receipt`, `catalog`, `catalog_verified`, `view`, `proposals`, `recovery_failed` |
| `edit` | `action` (required) | `edit` | `pending` |
| `review` | none | `review` | `pending`, or `null` |
| `accept` | none | `accept` | `applied`, or `null` for an accepted undo |
| `cancel` | none | `cancel` | `pending`: `null` |
| `undo` | none | `undo` | `pending` |
| `redo` | none | `redo` | `pending` |
| `submit` | none | `submit` | `proposal` |
| `overview`, `tile`, `catalog`, `lint`, `render` | as in [docs/MCP.md](MCP.md#tools) | none | the read-only server's reports, on the sandbox world |

The members are the runner's, described in
[docs/SANDBOX.md](SANDBOX.md#the-line-protocol). An editing tool's
`structuredContent` is exactly the reply the line runner writes for the
same request, and its first text content is that JSON serialized as sent;
a test runs one script through both on twin sandboxes and compares them
byte for byte. A refusal is the same: a result with `isError: true` whose
structured content is the runner's refusal, with `ok: false`, its stable
`code`, the engine's code in `engine` when the engine decided it, the
proposal `step` it concerns, the runner's `message` (which may quote world
text, as data) and the session's `state`. Each editing tool's output schema
is an `anyOf` of its success and its refusal, which exclude each other
through their `ok` consts.

An argument a tool does not take (any argument but `action`, and `action`
missing from `edit`) is refused the runner's way, with `request.invalid`,
`engine` `null`. An `action` not in the proposal format's form is refused
with `proposal.invalid` (or `proposal.budget` for a string, stroke or value
over its bound), as the runner refuses it, within both readers' limits.
Past them, both front ends refuse, but with different codes, because the
runner reads a request line under narrower JSON limits (12 levels deep,
12,352 values) than this server reads a message (32 levels, 22,288
values): a field `value` nested 9 or 10 levels deep is refused by both
with `proposal.budget`, one nested 11 to 28 levels deep here with
`proposal.budget` and by the runner with `request.parse`, and one nested
29 or more levels deep here by the parser (-32700). A test pins
these depths. The byte-for-byte comparison with the runner holds for
requests within both readers' limits.

The observe tools are the read-only server's: the same reports, paging,
cursors, `revision` argument, `render` image and caps, and errors, which
are the read-only server's one line `<tool> failed: <code>: <message>`
without structured content ([docs/MCP.md](MCP.md#errors)). `refresh` is
not offered: the snapshot follows the sandbox on its own (below). A sandbox
world whose catalog contains project art is served the same way, with the
same `catalog.project_art` member and marked `render` images
([docs/MCP.md](MCP.md#project-art)); the sandbox copy, like the world it was
made from, holds that art, and `render` sends its pixels to the client.

### The `action` argument

`action` is one Action in the proposal format's form
([docs/PROPOSALS.md](PROPOSALS.md#actions)), the one wire form for edits:
`kind` and exactly that kind's members, every one present, `null` for an
absent option, and `expected`, the sandbox revision it was made for
(`state.revision`). The input schema says this as an `anyOf` with one
choice per kind (21), each listing exactly its members as required, with
`additionalProperties: false`, and at the action's root the list of kinds
and the two members every kind has. It uses only keywords that draft-07
and 2020-12 read alike: `anyOf` rather than `oneOf` (the kinds' consts
already exclude each other, and more hosts take `anyOf`), and a cell or a
location's `at` as `items` with `minItems` and `maxItems` 2 rather than
`prefixItems`, which a draft-07 reading would turn into "no item allowed". Strings are at most 4,096 characters
and hold no NUL; `x`, `y` and a cell's coordinates are integers of
magnitude at most 2^53 − 1; `layer` is a 32-bit integer; `cells` holds 1 to
4,096 distinct `[x, y]` pairs; `contiguous` (of `fill_terrain`) is a
boolean; `seed` (of `scatter_terrain`) is an integer from 0 to
4,294,967,295 and `palette` names one of the catalog's palettes (the
neutral sample's is `speckle`), which the
`catalog` tool lists under `palettes`, a paged list like the others (page it
with `section: "palettes"`); `amount` (of `adjust_heights`) is an integer
from -1,024 to -1 or from 1 to 1,024 (two `anyOf` choices, so the schema
itself excludes 0) and needs the catalog's height range, which the
`catalog` tool reports as `heights`; `target` (of `flatten_heights`) is an
integer from -1,024 to 1,024 that the range must hold; `kernel` (of
`smooth_heights`) is the integer 1, written as `minimum` and `maximum` 1
so that `1.0` is admitted alike everywhere, and its description gives the
rule; `objects` (of `place_objects`, a stamp) is 1 to 64 objects, each
exactly a `proto` and an `offset` `[dx, dy]` of integers from -4,095 to
4,095 from the anchor tile `x`, `y`, inserted in that order (64 objects
with 128-byte prototypes at the widest offsets make a request of under 16
KiB, well inside the 64 KiB a message may be); a portal `target` is `null`, an anchor or a location; a field
`value` is any JSON value. The kind's `const` decides the
choice, so a member of the wrong type fails the one choice that applies;
there is no ambiguity of the kind the revision warns about for
compositions at a schema's root.

The server stays the authority: it decodes `action` with the proposal
crate's decoder, as the runner does. A test evaluates the schema sent in
`tools/list` and the server on every kind, at and past each bound, with
each member missing, an extra member and an unknown kind, and requires them
to agree (more than 300 cases). To make them agree, the server rewrites a
number with no fraction (`3.0`, `1e3`), which JSON Schema's `integer`
admits, to the integer it is, in `x`, `y`, `layer`, `seed`, `amount`, `kernel`, a flatten's `target`, the cells, a
stamp object's `offset` and a location's `layer` and `at`, before decoding; a field `value` is never
rewritten. What JSON Schema cannot say, the server still refuses, and a
test holds each:

- the order of a stroke's cells (increasing, as the engine requires);
- a string's size in UTF-8 bytes: the bound is 4,096 bytes, and
  `maxLength` counts characters, so a string of 4,096 characters beyond
  ASCII is refused with `proposal.budget`;
- a field value's budgets: 8 levels deep, 4,096 values, 256 members in an
  object, 64 KiB encoded, integers within the World range.

A NUL, an integer past 2^53 − 1 or more than 4,096 items anywhere in the
message is refused by the parser before any tool runs (-32700), as in the
read-only server.

## Annotations

The revision defines the hints: `readOnlyHint` (the tool does not modify
its environment; default false), `destructiveHint` (meaningful only when
not read-only: it may perform destructive updates; false means only
additive updates; default true), `idempotentHint` (calling it again with
the same arguments has no additional effect; default false) and
`openWorldHint` (it may interact with an open world of external entities;
default true). Every tool sends all four. The tool's environment here is
its sandbox: the session, the world copy, `recovery/` and `proposals/`.
Clients must treat annotations as untrusted hints, as the revision says.

| Tool | `readOnlyHint` | `destructiveHint` | `idempotentHint` | `openWorldHint` |
| --- | --- | --- | --- | --- |
| `status` | true | false | true | false |
| `edit` | false | false | true | false |
| `review` | true | false | true | false |
| `accept` | false | false | true | false |
| `cancel` | false | false | true | false |
| `undo` | false | false | true | false |
| `redo` | false | false | true | false |
| `submit` | false | false | true | false |
| `overview` | true | false | true | false |
| `tile` | true | false | true | false |
| `catalog` | true | false | true | false |
| `lint` | true | false | true | false |
| `render` | true | false | true | false |

Everything an editing tool changes is inside one disposable sandbox copy
that the person's world never sees, so no tool is marked destructive.
This is the project's reading (the change-set design's "sandbox only,
undoable"); **the specification's stricter reading**, "false means only
additive updates", **would mark `accept` and `cancel` destructive**. Per
tool:

- `status`: changes nothing, so it is read-only.
- `edit`: adds a pending change and writes nothing; an identical retry is
  refused (`pending.exists`, or `revision.stale` once the first was
  accepted).
- `review`: changes nothing, so it is read-only.
- `accept`: publishes into the sandbox copy only, and `undo` reverses it
  exactly; a retry finds nothing pending (`pending.none`).
- `cancel`: drops only a pending change, which the agent can prepare again
  with the same `edit`; a retry does nothing.
- `undo`, `redo`: prepare a pending change, as `edit` does; a retry while
  it is pending is refused (`pending.exists`).
- `submit`: creates a proposal file and never replaces one; a retry gives
  the same file (`existed: true`, nothing written).
- The observe tools: read the copy; loading the snapshot again changes
  nothing.
- Nothing reaches anything outside the sandbox, so every tool is
  closed-world.

**After a lost reply.** Because a retry has no second effect, it can be
refused although the first call worked. A retried `accept` refused with
`pending.none`, or a retried `edit`, `undo` or `redo` refused with
`pending.exists`, means the first call took effect: `state.undo` and
`state.revision` in that refusal, `status` and `review` tell which change
is now published or pending.

## Observing the sandbox

The observe tools answer from a snapshot of the sandbox world, loaded as
the read-only server loads a world, from the sandbox's copy. Before an
observe call, when the sandbox's revision moved since the snapshot was
loaded (an accepted step, an accepted undo or redo), the snapshot is loaded
again; otherwise it is reused. So:

- the observe tools answer from the last snapshot loaded: normally the
  sandbox's current revision (`state.revision` of the editing tools),
  which every result names and the `revision` argument compares against;
- a pending change is not observed: it is not published until `accept`;
- cursors are bound to the snapshot, so a cursor issued before an accept is
  `cursor.stale` after it, as after `refresh` in the read-only server;
- loading costs what the read-only server's load costs, bounded by the
  editing entry point's load budget (the sandbox world fits it, or the
  sandbox would not open), and happens at most once per revision, only
  when an observe tool is called; the server holds the session's world and
  one snapshot.

The snapshot follows the session only: a change made to the copy outside
the protocol is not seen until the next accepted step, and may then make
Accept fail (see below). In a blocked session the snapshot is the last
one loaded, which need not be what is on disk after a publication that
could not be verified. Should the copy ever stop loading, the observe
tools report `world.invalid` as the read-only server does; its message is
the read-only server's and names `refresh`, which this server does not
offer (calling it is `tool.unknown`): the snapshot is loaded again after
the next accepted step.

## Errors

Protocol errors are the read-only server's
([docs/MCP.md](MCP.md#errors)): `tool.unknown` for any tool but these
thirteen, `quit` and `refresh` among them. Tool errors:

| Code | When |
| --- | --- |
| The engine's codes ([docs/ENGINE.md](ENGINE.md#refusal-codes)) | As the runner returns them: `grant.missing`, `revision.stale` (an `expected` that is not the current revision), `pending.exists` (an edit, undo, redo or submit while a change is pending), `pending.none` (accept with nothing pending), `review.incomplete` (accept while the engine's review is incomplete), `history.nothing_to_undo`, and others. |
| `proposal.invalid`, `proposal.budget` | `edit`: an action not in the proposal format's form, or over a bound. `submit`: `proposal.empty` with no step applied, `proposal.budget` past 256 steps or 16 MiB (undo steps and submit again), `proposal.store` when the file cannot be retained. |
| `request.invalid` | An argument the tool does not take, or `edit` without `action`. |
| `session.blocked` | A publication failed or could not be verified (`publish.failed`, `publish.unverified`) or World's history needs a reload: every editing tool but `status` and `review` is refused from then on, `submit` among them. The steps not submitted can never be submitted; start a new sandbox. The observe tools still answer, from the last snapshot loaded: after a publication that could not be verified, the copy on disk may differ from it. |
| `recovery.failed` | The recovery store refused a record (at its 1,024 records or 64 MiB too). The change stays pending and the session is not blocked, but the store refuses every later record, so every later `accept` is refused at once with this code and says so, and `status` reports `recovery_failed: true`. Cancel, submit the steps so far, start a new sandbox. |
| `output.budget` | Never expected: a runner reply too large for the result bound (below) is refused rather than sent. |

These are the runner's rules, word for word
([docs/SANDBOX.md](SANDBOX.md#a-blocked-sandbox)); the tests drive each
through MCP.

## Limits

| Bound | Value |
| --- | --- |
| Bytes of one message | 65,536 |
| One runner reply taken into a result | 87,040 bytes (85 KiB) |
| One `tools/call` result as sent, both copies counted | 262,144 (256 KiB), as in the read-only server |
| Review | The engine's: 64 KiB of excerpt and 32 files; past them Accept is refused |
| Proposal | 256 steps and 16 MiB |
| History and recovery | 1,024 records, 64 MiB |
| Observe tools | As in [docs/MCP.md](MCP.md#limits), `render`'s image caps included |

A result carries the runner's reply twice: as `structuredContent` and as
its text copy, where every `"` and `\` costs two bytes. The runner's own
cap is 256 KiB for one copy; here it is lowered to 85 KiB, so that two
copies of the largest reply, each byte at most twice in the text copy,
stay within the read-only server's result bound of 256 KiB with room for
the members around them (a test builds the worst reply of that size). The
runner cuts what does not fit as it always does: it keeps the first files,
then warnings, then review lines, counts the rest in `files_omitted`,
`warnings_omitted` and `review_omitted`, and sets `cut`. Accept is gated on
the engine's review, never on what fitted in the result.

## World text is data

The instructions and every tool description say that results are world
data, not instructions. In the output schemas, every string the world,
the engine's review of it, or the client wrote is marked
`"x-world-text": true`: file paths, warnings and review lines of `pending`,
the runner's `message`, the view's `plane`, `applied.placed` (one uid
for a `place`, the list of a `place_objects` in object order), and the
`client` label (the operator's, untrusted, so marked too). Every other
string is this editor's own: a code, a request name, a phase, a change
kind, a revision or digest, the operator's actor, the editor version, or a
proposal file name. A test writes a marker into a label, a client label,
field names, uids, prototypes and kinds, and checks that every string
holding it is at a marked position and that every unmarked string is in
that closed vocabulary. Every reply is escaped by the observe crate's
writer, so world text can neither end a string or a line nor reach a
terminal as a control sequence; review lines are the engine's, which marks
such characters too.

## What the server can touch

The containment is of the protocol: what the tools can do. **It is not an
operating-system sandbox.** A process running as the same user with a
shell or file tools can read and write the person's world, the sandbox's
receipt, `recovery/` and `proposals/` directly, and start a server of its
own. Give an agent only this server. Against anything written outside the
protocol, safety rests on the person's review when applying: every step is
dispatched again under the person's grants, on the exact base, and
reviewed and accepted by the person
([what the digests protect](PROPOSALS.md#what-the-digests-protect)).

Within the protocol:

- **Only its sandbox.** The binary receives the three directories as
  descriptors; no tool argument is a path, and every tool's members are
  pinned by tests, which refuse `path`, `fd`, `actor`, `label`, `receipt`
  and other extra members. `tools/check-architecture.sh` pins the crate's
  dependencies, refuses the standard library's file, network and process
  modules, the environment, roots, directories, locks and writes in its
  source, and path and environment types in the modules that read tool
  arguments. It also refuses print macros and the process's standard
  streams in the observe, MCP, sandbox and editing crates, so nothing but
  a reply reaches standard output, and a test runs every tool in a child
  process with capture off and finds nothing else there.
- **Edits go through the runner.** Every editing tool is one runner
  request through its typed entry point (`Sandbox::respond`), so the
  runner's checks, the engine's grants and review gate, and its stores'
  refusals all apply.
- **Accept publishes into the copy; submit writes one proposal file**, as
  in the runner. The observe tools only read the copy.
- **Nothing else.** No tool applies a proposal, makes or deletes a
  sandbox, or opens the person's world.

## Tests

`crates/tilewright-editor-mcp-sandbox/tests`: `protocol.rs` (discovery
and the tool list against goldens, a session through every editing tool
and its refusals against a golden, each editing tool's results against the
line runner's on a twin sandbox, the observe tools after accepts and
undos, cursors going stale, arguments refused, `review.incomplete`,
`proposal.budget`, the recovery latch and a blocked session against
goldens, the readers' depth limits against the runner's, nothing on the
process's standard output, the log), `schema.rs` (the `action` schema
against the server, and what the schema cannot say; its evaluator knows
no `oneOf` or `prefixItems`), `hostile.rs` (world-text marking, and random
and mutated messages that each get exactly the replies the rules give).
`crates/tilewright-editor-app/tests/mcp_sandbox.rs` runs the provisioner
with `--mcp` and the binary, edits with undo and redo, observes and
renders, submits, closes the input, reads the proposal back, applies it
step by step to a second provisioned copy under a person's actor, and ends
with byte-identical worlds and an untouched source; it then starts the same
command again and gets a new sandbox. `tools/test_sandbox_provision.py`
checks the `--mcp` launch, and `tools/package-linux.sh` starts the packaged
binary through the provisioner. Tests that rely on permissions skip when
run as root.

## A short session

Trimmed (`…` for digests, review lines and repeated members; `_meta`
omitted from requests and results):

```text
> {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"status","arguments":{}}}
< {"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","content":[{"type":"text","text":"{\"ok\":true,…}"}],"structuredContent":{"ok":true,"request":"status","actor":"agent:docs","client":"example client","editor":"0.1.0-dev","receipt":"…","catalog":"…","catalog_verified":true,"view":{"plane":"main","layer":0,"bounds":[0,0,8,6]},"proposals":0,"recovery_failed":false,"state":{"revision":"3345…","phase":"ready","pending":false,"undo":0,"redo":0}},"isError":false}}
> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"edit","arguments":{"action":{"kind":"move","uid":"marker_1","x":4,"y":2,"expected":"3345…"}}}}
< {"jsonrpc":"2.0","id":2,"result":{…,"structuredContent":{"ok":true,"request":"edit","pending":{"base":"3345…","result":"7f41…","complete":true,"files":[{"path":"planes/main/layers/0/0_0.jsonc","change":"modified","before_bytes":4057,"after_bytes":4057}],…,"cut":false},"state":{"revision":"3345…","phase":"pending","pending":true,"undo":0,"redo":0}},"isError":false}}
> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"accept","arguments":{}}}
< {"jsonrpc":"2.0","id":3,"result":{…,"structuredContent":{"ok":true,"request":"accept","applied":{"before":"3345…","after":"7f41…","placed":null},"state":{"revision":"7f41…","phase":"ready","pending":false,"undo":1,"redo":0}},"isError":false}}
> {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"edit","arguments":{"action":{"kind":"paint","plane":"main","layer":0,"grid":"floor","x":0,"y":0,"proto":"stone","expected":"7f41…"}}}}
< {"jsonrpc":"2.0","id":4,"result":{…,"structuredContent":{"ok":false,"request":"edit","code":"grant.missing","engine":"grant.missing","step":null,"message":"terrain capability or bounds refused","state":{…}},"isError":true}}
> {"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"tile","arguments":{"plane":"main","layer":0,"x":4,"y":2}}}
< {"jsonrpc":"2.0","id":5,"result":{…,"structuredContent":{"format":"tilewright.editor-tile/1","revision":"7f41…",…,"objects":[{"uid":"marker_1",…}],…},"isError":false}}
> {"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"submit","arguments":{}}}
< {"jsonrpc":"2.0","id":6,"result":{…,"structuredContent":{"ok":true,"request":"submit","proposal":{"file":"proposal-67df….json","body_sha256":"67df…","bytes":1781,"steps":1,"base":"3345…","result":"7f41…","existed":false},"state":{…}},"isError":false}}
```

The client then closes the server's input, and the server exits.

## Not here

Applying a proposal (a person does, in the desktop), any tool that makes or deletes sandboxes or touches a person's
world, an HTTP transport, resources and prompts, reachability, Windows and
macOS editing, and attaching to a running desktop editor.
