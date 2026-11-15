# Change-set proposals

An agent edits only in its own provisioned sandbox, where it may prepare
and accept steps; what it hands over is a proposal. Only a person applies
a proposal to their world, one step at a time, through the normal Review
and Accept (issue #57, [ADR 0003](../decisions/0003-change-set-proposals.md)).
This page describes the format, its bounds and its checks, as the
`tilewright-editor-proposal` crate implements them. Applying a proposal,
the proposal store, making sandboxes, submitting from an agent and opening
a proposal in the desktop are later work; nothing described here lets an
agent publish to a person's world, or publishes anything at all.

## The document

A proposal is one UTF-8 file of strict JSON (`tilewright.editor-proposal/1`):

```json
{"body":{"actor":"agent:sandbox","base":"a07d…2bf4","catalog":"6156…360d","client":null,
 "editor":"0.1.0","receipt":"0707…0707","result":"7d06…281b","steps":[{"action":{
 "expected":"a07d…2bf4","kind":"move","uid":"marker_1","x":4,"y":2},"after":"7d06…281b",
 "before":"a07d…2bf4","placed":null,"transaction":"{\"actor\":\"agent:sandbox\",…}",
 "transaction_sha256":"6b16…d12d"}]},"body_sha256":"49e8…c67f","format":"tilewright.editor-proposal/1"}
```

(Shown wrapped and with digests shortened; the writer emits one line.)

Top level, exactly these members:

| Member | Value |
|---|---|
| `format` | `"tilewright.editor-proposal/1"`. |
| `body` | The body below. |
| `body_sha256` | SHA-256 of the body's exact bytes, 64 lowercase hexadecimal digits. |

The body, exactly these members:

| Member | Value |
|---|---|
| `editor` | The release version of the application that wrote it, as the caller passes it to `Proposal::from_session` (the version that application reports): 1 to 64 printable ASCII characters, no space. Informational; nothing checks it against the reader. |
| `actor` | The actor of every step, a valid engine actor (1 to 256 printable ASCII characters). In a sandbox it comes from the operator's flag, never from the client. |
| `client` | `null`, or an untrusted label of at most 256 bytes, such as the name an MCP client gave itself. It is never an actor and is shown escaped (`Proposal::client_display`). |
| `base` | The World revision the first step starts from. |
| `result` | The World revision the last step ends at. |
| `catalog` | The revision of the editor catalog (`preview.jsonc`) the steps were made under. |
| `receipt` | The digest of the sandbox's provisioning receipt, recorded as given. |
| `steps` | The applied steps, oldest first: 1 to 256. |

Each step, exactly these members:

| Member | Value |
|---|---|
| `action` | The edit Action that prepared the step, in the typed form below. |
| `placed` | For `place`, the uid it assigned (a generated uid, `o.` and 26 base32 characters); `null` for every other action. |
| `transaction` | The sealed World transaction the step was accepted as, exactly, with the sandbox's actor. |
| `transaction_sha256` | SHA-256 of `transaction`'s bytes. |
| `before` | The World revision the step starts from. |
| `after` | The World revision it ends at. |

Revisions and digests are 64 lowercase hexadecimal digits.

### Actions

An action is an object with a `kind` and exactly the members of that kind,
each always present; an absent option is `null`. `expected` is the revision
its author saw. Coordinates are integers within ±(2^53 − 1), the World
format's range; layers fit 32 bits; strings are at most 4,096 bytes and
hold no NUL.

| `kind` | Members besides `kind` and `expected` |
|---|---|
| `move` | `uid`, `x`, `y` |
| `set_field` | `uid`, `field`, `value` (any JSON value, nested at most 8 deep and 64 KiB encoded, as the engine allows) |
| `paint` | `plane`, `layer`, `grid`, `x`, `y`, `proto` |
| `paint_stroke` | `plane`, `layer`, `grid`, `cells` (1 to 4,096 `[x, y]` pairs, distinct and in increasing order, the engine's own rule for a stroke), `proto` |
| `paint_zones` | `plane`, `layer`, `cells` (as for `paint_stroke`), `zone` (string or `null`) |
| `remove` | `uid` |
| `place` | `plane`, `layer`, `x`, `y`, `proto` |
| `rename_zone` | `plane`, `zone`, `name` (string or `null`) |
| `set_zone_parent` | `plane`, `zone`, `parent` (string or `null`) |
| `create_zone` | `plane`, `zone`, `name`, `parent` |
| `delete_zone` | `plane`, `zone` |
| `create_anchor`, `move_anchor` | `plane`, `anchor`, `x`, `y` |
| `delete_anchor` | `plane`, `anchor` |
| `set_portal` | `uid`, `target`: `null`, `{"kind": "anchor", "plane", "anchor"}` or `{"kind": "location", "plane", "layer", "at": [x, y]}` |

Selection, review, history and reopen actions (`Select`, `ClearSelection`,
`Accept`, `Cancel`, `Undo`, `Redo`, `Reopen`) have no form and are never in
a proposal.

### What a reader requires and what the writer emits

A reader takes any document the strict reader accepts: whitespace and
member order are free, and strings and numbers may be spelled any way JSON
allows. It refuses comments, a byte-order mark, duplicate keys, unknown or
missing members, wrong types, non-finite numbers and integers beyond
±(2^53 − 1). The body digest covers the body's bytes exactly as they are in
the file, so two writers that format a number differently still read each
other's proposals.

The writer emits one fixed form: minified, members in sorted order, no
trailing newline, strings escaping only `"`, `\` and characters below
U+0020, and numbers as `serde_json` writes them (for example `1e+16`,
`-0.0`). Tests pin these exact bytes, so a dependency change that alters
them is noticed.

## Bounds

All in `tilewright_editor_proposal::limits`:

| Bound | Value |
|---|---|
| Steps | 256 (History keeps up to 1,024) |
| File | 16 MiB, refused before it is read (History may retain 64 MiB) |
| One step's transaction | 1 MiB |
| Files one step changes | 32, the engine's review limit, so each step can be reviewed completely |
| Action strings | 4,096 bytes |
| Stroke cells | 4,096, the brush's limit |
| Field values | 8 levels of nesting, 4,096 values (itself included) and 64 KiB encoded, the engine's budget; objects at most 256 members, World's reader limit |
| JSON values in one proposal | 256 × (3 × 4,096 + 64) + 64 = 3,162,176, keys included; it follows from the bounds above, so no proposal within them reaches it |
| Client label | 256 bytes |
| Editor version | 64 bytes |

## Building and reading

`Proposal::from_session(session, editor, receipt, client)` records a session's
applied steps (`Session::applied_steps`: the undo stack, so undone steps are
not in it). It is refused with pending.exists while a change is pending,
with session.blocked in a blocked session, with `proposal.empty` when no
step is applied, with `proposal.invalid` for a step the session did not
prepare itself, with `proposal.chain` when the steps do not end at the
session's revision, and with `proposal.budget` over a bound. What it builds
is read back with `Proposal::decode` before it is returned.

`Proposal::decode` checks everything that needs no world, in this order,
and every `Proposal` value has passed all of it:

1. size, UTF-8 and strict JSON within the bounds (duplicate keys, comments,
   a byte-order mark, non-finite numbers and out-of-range integers refused);
2. the format tag and exactly the documented members, types and bounds,
   stroke cells included;
3. the body digest, then each step's transaction digest;
4. the chain: the first step starts at `base`, each step at the revision the
   one before ended at, the last ends at `result`, and no step leaves the
   revision unchanged; each action's `expected` is its step's `before`;
5. each transaction: a sealed World transaction (`base` and `result`
   recorded) by the proposal's actor that changes 1 to 32 files; a `place`
   step's transaction is one `object.insert` whose object has its `placed`
   uid and no inventory (the engine's Place never writes one), no other
   step records a uid, and no two steps place the same uid.

None of this shows that a step's transaction is what its Action
prepares: verification reads both, but only compares the Action's
`expected` revision and a placement's uid with the transaction. Only
dispatching the Action again when the proposal is applied, and comparing
what it prepares with the recorded transaction, establishes that.

## Applying (next)

Applying is the next part of this work and is not in the crate yet. As the
owner decided, it will dispatch each step's Action again through the
engine on the person's session, under the person's catalog grants and
with the person's actor (a placement with the uid it recorded, through
`Session::dispatch_placing`); require the transaction it prepares to equal
the recorded one in everything but the actor; leave each equal step
pending for the person's normal Review and Accept, one at a time; and
apply a proposal only to the exact revision it was made from (the strict
base), never rebasing it.

## What the digests protect

The body digest and each transaction digest detect corruption and
accidental edits. They do not prevent forgery: whoever writes a proposal,
including an agent that runs as the same user with a shell, can write
matching digests, and the recorded `receipt`, `actor` and `client` are
only what it says. Safety comes from three things instead: every step is
dispatched again through the engine under the person's own catalog grants;
the strict base, so a proposal applies only to the exact world it was made
from and cannot be replayed; and the person's Review and Accept of every
step. Nothing in the crate publishes.

## Files

A proposal file is named `proposal-<body_sha256>.json`
(`Proposal::file_name`). The store that writes and reads these files in a
sandbox's `proposals/` directory comes with applying.
