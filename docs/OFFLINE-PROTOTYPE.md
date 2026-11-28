# Offline editing prototype

The prototype targets World 0.2.2 and renderer 0.7.2. Release builds require
the exact published registry dependencies; local source candidates do not
establish released-consumer acceptance. The generic
editor opens ordinary World data and its strict `preview.jsonc` catalog. A
separate game-specific toolkit adapter imports a selected external map and
exports a checked subset of edits. The editor contains no game-specific
parser and never starts commands selected by project data.

This is a schematic editor demonstration, with original MIT geometric art.
It does not claim artwork, gameplay or visual parity with the external game,
which remains the active game content/runtime. Original map bytes, licensing
and import baseline belong to the external adapter workspace and are never
bundled here.

## Fresh checked session

`tools/provision-prototype.py` copies an explicitly supplied sample or imported
World into a **new absent** task directory, with distinct World, recovery and
evidence children. It refuses links and special input files, bounds the copy,
uses private 0700 directories and 0600 files, syncs creation, records directory
identities and kernel FD mount IDs/filesystem types, rejects volatile/unknown
backing (including tmpfs, ramfs and overlay), rechecks the retained identities
at launch and passes already-open directory descriptors to the child editor.
Source paths are data only. The executable is an explicit trusted operator
argument; it is never read from the project.

```sh
python3 tools/provision-prototype.py \
  --world crates/tilewright-editor-testkit/fixtures/prototype-neutral \
  --destination /absolute/private/task-session \
  --editor target/debug/tilewright-editor \
  --font-pack /absolute/prepared/font-pack
```

A Linux binary archive includes the same trusted helper and authored sample.
From the extracted archive, use Python 3 (standard library only):

```sh
python3 tools/provision-prototype.py --world samples/prototype-neutral \
  --destination /absolute/private/new-session --editor bin/tilewright-editor \
  --font-pack "$(pwd)/font-pack"
```

Two options open an agent's proposal in the new session
([applying in the desktop](PROPOSALS.md#applying-in-the-desktop)):
`--proposal FILE` checks the file before anything is created (a regular
file, not a link, at most 16 MiB, named `proposal-<64 hex>.json`), copies
its bytes into a private `proposal/` child (0700, the file 0600, synced),
records its name, size and SHA-256 in the receipt and passes that
directory as a further descriptor; `--actor human:<name>` makes the person
the actor of every change the session accepts (default
`editor:prototype`) and is recorded in the receipt. Without them the
session, the receipt and the editor's arguments are as before; only the
standard streams and the session's directories reach the editor.

`--palettes FILE` brings a person's [favourites](#favourites) into the
session and takes their changes back out; without it nothing about the
session changes.

For an agent, `tools/provision-sandbox.py` makes a sandbox with the same
copy rules under a fresh name each run and starts the headless runner in it
([agent sandboxes](SANDBOX.md)); this helper and its output are unchanged.
A sandbox has no person, so it never has favourites.

### Favourites

A person's favourites are short ordered lists of values the catalog already
grants: terrain values, catalog [scatter palettes](#scatter-palettes) and
[Place](#placing-objects) prototypes. They live in a file of the person's
own, wherever they keep it (for example under their configuration
directory); the editor never opens a path outside the session.

```json
{
  "format": "tilewright.editor-favourites/1",
  "terrain": [
    {"grid": "floor", "value": "sand"}
  ],
  "scatter": [
    "speckle"
  ],
  "objects": [
    "stone"
  ]
}
```

The file is strict JSON (no comments, no trailing commas, no duplicate
keys) of at most 64 KiB: an object with `format` exactly
`tilewright.editor-favourites/1` and the optional lists `terrain` (objects
with exactly `grid` and `value`), `scatter` and `objects` (strings), each
of at most 32 distinct entries, and no other member. A grid, value or
prototype is 1 to 128 bytes, a scatter palette name 1 to 64, none with a
control character. A file that breaks any of this is ignored as a whole:
the session starts without favourites, the editor prints
`favourites: file ignored: <reason>` and Terrain and Place mode say so in
their Favourites section, and nothing is written back. The reason names
only the rule broken, never what the file says.

`tools/provision-prototype.py --palettes FILE` checks the file before
anything is created (a regular file, not a link, at most 64 KiB, in a
directory that is not itself a link) and keeps that directory open for the
copy back. It copies the bytes as `favourites.json` into a private
`palettes/` child of the session (0700, the file 0600, synced), records in
the receipt only that a file was given and its size
(`"palettes": {"given": true, "bytes": …}`), and passes that directory to
the editor with `--palettes-fd`. The receipt carries no digest: a small
file of a catalog's values is easy to rebuild from its SHA-256 by trying
the lists the catalog allows, so a digest would not keep the lists private.
The editor checks the directory as a proposal directory is checked (this
user's, mode exactly 0700, no access control list, a checked local file
system) when it opens it and again before every write, takes its lock, and
refuses a missing or linked `favourites.json` or one that is not 0600. The scripted demonstration makes
evidence frames, which favourites never reach, so the helper and the editor
refuse `--palettes` with `--demo-script` (and so with `--reference`).

In Terrain and Place mode a **Favourites** section comes first, after the
brush rows, before the catalog's own lists. Terrain mode shows the terrain
favourites, then the scatter favourites; Place mode the prototypes, which
the Place filter does not narrow. A favourite the catalog grants is the
catalog's own row of that value, with the same words, thumbnail, marking and
control: choosing it is choosing that value in the catalog's list, so what
the brush, Fill, Replace, a scatter stroke or a placement prepares is byte
for byte the same, and nothing about favourites is recorded in the change,
the recovery records or a proposal step. A favourite this catalog does not
grant (the catalog changed, or another world) cannot be chosen: its row
reads "Remove <value> (not granted)" (just "Remove <value>" where the row
is narrow), and its one action removes it from the list, written back as
any other change. One click removes it, with no undo in the session, and
the copy back removes it from `FILE`, so from every world that uses that
file; star it again in a world that grants it. The file still loads, and the entry stays until removed.

Below them, "Star the chosen value" adds the mode's chosen value (the
chosen terrain or scatter palette, or the prototype Place puts down) at the
end of its list, and "Unstar the chosen value" removes it. "Move it up the
favourites" moves a starred value one place up. Each change is written as
a new file in the session's `palettes/` directory,
`favourites-0001.json`, `favourites-0002.json` and so on, with an exclusive
create that never replaces anything at that name, then the directory is
synced; only then is it the session's list, and the section says
"Saved: favourites-000N.json". A change that cannot be written (a name
already taken, more than 9,999 changes, a disk error) is refused with its
reason and changes nothing. `favourites.json` itself is never written.

When the editor ends, the helper copies the newest `favourites-NNNN.json`
back to `FILE`: after a normal quit, after the editor fails, and when the
helper gets SIGINT (a terminal's Ctrl-C), SIGTERM or SIGHUP while the
editor runs. It passes those signals on to the editor rather than ending
itself, waits for the editor to quit, then copies back. (SIGKILL of the
helper skips the copy back; the session's copy stays.) Every step goes
through the directory kept open at provisioning, never through the path
again. It replaces `FILE` only when:

- the saved file is a private (0600) regular file of this user, at most
  64 KiB, of the favourites format;
- `FILE` is still the file the session copied in: the same file (device and
  inode) with the same bytes, checked just before a stage is made;
- `FILE` is owned by this user and has the owner's write bit (a person can
  freeze their favourites with `chmod a-w`).

The bytes go to a new file beside `FILE` (exclusive create, with `FILE`'s
mode), synced; then `FILE` is read once more through the kept directory
and must still be the same file with the same bytes; then the new file is
renamed over it and the directory synced. This guards against `FILE` being
replaced, removed or edited at any time up to that last read. It does not
guard against a change in the instant between the last read and the
rename (two system calls): that change is overwritten.

Otherwise `FILE` is left alone and the helper's last line says where the
session's copy is, to copy by hand, with a fixed reason word and nothing
from `FILE`'s path or contents: `{"palettes": {"returned": false, "kept":
"<session>/palettes/favourites-000N.json", "reason": "target_changed"}}`.
The words are `no_change` and `same_bytes` (nothing to copy back, so no
`kept`), `saved_invalid`, `target_changed`, `not_writable`, `stage_exists`,
`permission` and `io_error`; anything unforeseen is `io_error`. When the
stage beside `FILE` could not be removed after such a refusal, the line
also has `"stage_left": true`: a file `.<name>.tilewright-<pid>` is left
beside `FILE`, safe to delete. A copy back that succeeded says
`{"palettes": {"returned": true, "from": "favourites-000N.json"}}`; if the
directory could not be synced after the rename, the line adds
`"synced": false` and a reason word: `FILE` holds the new lists, but a
crash soon after could bring back the old ones.
Without `--editor` (provision only) nothing is copied back.

The helper never adopts existing state, changes permissions
on owner directories or deletes failed sessions. Inspect and preserve them;
choose a different new session directory for a new attempt.

Editing is bounded to Linux local filesystems checked by World. Recovery must
have mode 0700, current ownership, accepted ACL/label policy and directory sync.
A failed check blocks editing; there is no unchecked-access flag. The recovery
root and exclusive lock remain retained for the session. Its exact inverse
records remain after successful edits and failed or uncertain publication.
Existing records are reported and refused for a new session; automatic crash
replay and persistent history are outside this prototype.

The released storage API cannot prove arbitrary user-supplied roots do not
alias or securely create a new independently rooted child. The provisioning
helper's fresh Linux directory identities are deployment evidence for this
bounded run. Descriptor retention prevents later pathname replacement from
redirecting an already-open root. The app does not offer arbitrary recovery
path selection as an equivalent guarantee. Same-UID hostile writers remain
outside this private-directory/advisory-lock model.

## Controls and review

The native window presents the prepared scene through the shared wgpu/SDL3
adapter and remains open until the user closes it. A toolbar selects Select,
Move, Terrain, Place or Zone mode and exposes Clear, Undo, Redo and Delete.
Under a catalog that grants a [height range](#height-range), the sidebar's
"Sculpt heights  U" row (or U) selects [Sculpt mode](#sculpt-mode); the
toolbar is full at the minimum window width. Terrain mode
paints with the drag brush described below, or fills and replaces with a click; deletion and placement are described after it. The inspector shows
the selected object's identity, position and schema fields, or the selected
cell and explicit terrain choices. Only profile-authorized scalar fields are
editable; structured values remain read-only. With a proposal open, a
Proposal block sits above them and **G** (or its button) prepares the next
step for the ordinary Review
([applying in the desktop](PROPOSALS.md#applying-in-the-desktop)).
The tiles that step concerns are darkened on the map: black at 50 %, or 25 %
for a tile something moves from. Like the zone tints, the highlight is made
of unpickable copies of the prepared exact cell-reference quads placed in the
World's draw order, so objects on the tile and nearer tiles cover it; it
draws over a zone tint of the same tile, and a cell the person's own brush
preview marks shows only the preview. Badges stay above both. Tiles on
another plane or layer or outside the view are not drawn. The highlight
needs the same draw-order slots as the zone tints: where they cannot be
computed it is not drawn. If the overlays would take the frame past the
scene limits, the highlight is left out first and the Proposal block says
so; the other overlays are left out only if the frame still does not fit.

Click an editable field or position and type. Place the caret by clicking
(Shift-click extends), with the arrows (Shift extends), Home/End or
select-all; edit with Backspace/Delete or the clipboard, then choose Review. Long focused input keeps its tail and caret visible.
Fields are single-line: typed, pasted or composed text containing a control
character, U+2028 LINE SEPARATOR or U+2029 PARAGRAPH SEPARATOR is rejected as a
whole and leaves the draft unchanged. A stored value that already contains them
opens unchanged and keeps them unless the user deletes or replaces them.
The inspector row and the field draft show each such stored character as one
marker on the same line: `⏎` for a line break (LF, CR, VT, FF, NEL, U+2028
and a CR LF pair, which editing treats as one character), `¶` for U+2029,
`→` for a tab and `⬚` for any other control character. A row below the field
counts them ("Contains 2 line breaks"). The marker is display only: the draft,
the clipboard, the comparison with the stored value, the reviewed `SetField`
value and the source review keep the real characters. The caret moves over a
marker as over one character, and Backspace or Delete beside it removes
exactly that stored character (both characters of a CR LF pair). A draft that
still contains one can be reviewed as it is. When a prepended or combining
character next to the stored break would otherwise join the marker, a
zero-width space is drawn between them; it is part of the marker and never
stored. A stored character that is itself one of the marker symbols (`⏎`, `¶`,
`→`, `⬚`) is ordinary text and looks the same as a marker. The count row
describes the stored value and counts only real stored breaks and controls; it
does not change while a draft is edited. Invisible format characters, such as
bidirectional controls and joiners, are not control characters and are not
marked.
Text focus consumes world shortcuts. The shared renderer shapes text using the
explicit Unicode font pack; missing-glyph diagnostics preserve the complete
original value and prevent a lossy visual replacement. Exact native input,
selection, bidirectional and composition behavior require the corresponding
interactive acceptance evidence.

The source review panel shows change-centered before/after sections and can
scroll. Unchanged omitted context is marked explicitly. Complete changed spans
must fit the 64 KiB escaped-review budget and 32-file bound; an oversized review
disables acceptance and retains the pending plan for cancellation. The review
text, these limits and the refusal of an incomplete Accept belong to the
editing engine, not the window; see [the engine guide](ENGINE.md). The complete
sealed source images remain authoritative. The terminal also prints a bounded
planned source diff and diagnostics; the sealed plan retains the exact images.
Selection uses exact reference-renderer semantic masks for the matching frame;
terrain mode uses a separate opaque cell-reference scene. SDL logical pointer
positions convert to physical pixels once. Opaque panel hit regions consume
input before world picking. The world remains full-window behind the panels;
the editor does not translate or rasterize world geometry. Resize/camera/session changes
invalidate earlier masks.

### Shared string field

`--shared-string-field` (given to `tools/provision-prototype.py`, which
records `"string_field": "tilewright-ui 0.1.3"` in the receipt and passes the
flag to the editor) switches the inspector's **string** fields, the two **zone
name** drafts and the Place **palette filter** to the shared text field of
`tilewright-ui` 0.1.3, drawn by `tilewright-ui-render` with the editor's own
text system and font pack. It is off by default; without it nothing changes,
and the frame records and images of a session are byte-identical. With it,
each frame record carries the same `"string_field": "tilewright-ui 0.1.3"`
member and the editor prints it at start.

Only string field drafts, zone name drafts and the palette filter change
hands: a zone's display name ("Edit display name") and a new zone's id and
name ("New zone under X", "New top-level zone"), see [Managing
zones](#managing-zones), and the filter at the top of the sidebar in Place
mode, see [Placing objects](#placing-objects). Numbers, booleans and
references, coordinates, anchors and portal locations keep the editor's own
field. A shared draft keeps the same refusals and messages, stored line breaks
as the same markers (now drawn by the shared field), the same keys, clipboard
and input method events, and Enter or Review hands the engine the same action
with the shared field's committed text: `SetField` for a string field,
`RenameZone` or `CreateZone` for a zone draft. A zone draft keeps its own byte
limit (1,024 bytes for a display name, 1,153 for a new zone's id, space and
name), refused in the editor's own words, its own hint ("Name of meadow; empty
= none", "New zone under meadow: id, name"), the review-time checks (an empty
name is no name; the id's syntax, a used id and the 256-character name, each
with its message; text the font pack cannot draw) and the same road fence and
pause: no zone draft opens while a road is drawn or editing is paused. The
shared filter keeps the filter's own rules: the 64-byte limit with the same
refusal, every key is text while it is focused (N and O type, they do not
act), `/` or a click focuses it, Enter or a click elsewhere releases it and
keeps the text, Esc clears and releases it, a composition is dropped on
release, and it is never reviewed and never makes a change. Each committed
edit filters the palette and pulls the sidebar's scroll back as before. It
cannot be focused while a road is drawn (a road is only drawn in Terrain
mode), and while editing is paused it is focused, edited, released and cleared
as before. The arrows step the caret with the shared field's own `step_caret`,
which steps over a marker's zero-width space in one press and counts two caret
stops at the same place as one, so every press moves the caret. Its text is
the editor's own size (18 px), so an arrow or a click lands on the same visual
stop, with the same side, as in the editor's own field. The editor's own field
steps its caret by the same rule, so neither field has a press that moves
nothing on screen, including two caret stops at the same place at a change of
direction, U+0600 before a stored break, some emoji sequences beside
right-to-left text or breaks, and a value of breaks only.

The field looks like the editor's own: it is laid out with the editor's own
field row values (`workspace_render`'s `FIELD_ROW_HEIGHT`, `text_size`,
`TEXT_INSET`, `FIELD_SCROLL_MARGIN` and colours), so it takes the same
30 px row, the rows below stay where they are, and its text has the same
origin, size and colour, its selection, caret and preedit underline the
same accent, and long text scrolls at the same point. One difference
remains: the editor draws the selection, caret and underline under the
text, the shared field draws the caret and the underline over it, so the
pixels where the caret or the underline crosses a glyph's edge differ (a
few pixels, and only there). It has no
button of its own: the shared field's Apply control is switched off, its
text area takes the row's whole width, and Enter or the editor's Review
control reviews, as with the editor's own field.

The shared field is composed into the editor's frame under the editor's own
scene limits, quads and raster samples both. The field is compiled first
and the editor fits its own frame to leave room for the field's quads and
its raster samples (`tilewright-ui-render`'s `quad_count` and
`raster_sample_count`), so near the budget the proposal highlight and the
preview give way, as they do without the switch. The composed frame is then
checked against the editor's whole limits as a backstop. If the frame cannot
be drawn (no room even for the editor's share, or a structural refusal from
the shared renderer), editing pauses with the draft kept. Esc discards the
draft (or clears and releases the filter); a second Esc, a zoom or a pan
then ends the pause (if the cause is the frame budget, zooming in avoids
it next time).

What only a person can check, in a real window with the switch on. Start the
session with `--shared-string-field`; the terminal prints
`string field: tilewright-ui 0.1.3 (opt-in)`. Select a marker and click its
`label` row; then repeat 1, 2, 6, 9 and 12 in a zone draft (N, choose
meadow, "Edit display name"; and "New zone under meadow"), and 1, 2, 3, 6
and 11 in the palette filter (P, then `/`), with step 14.

1. **Looks.** Open the same field with and without the switch: the row has
   the same height and blue fill, the hint and the Review and Discard
   buttons below it do not move, and the text starts at the same place in
   the same size and colour. The caret is the same thin blue line; where it
   crosses a glyph it is drawn over the glyph instead of under it. There is
   no button of the field's own: the text runs the width of the row. Every
   row below stays reachable, in a 720×480 window too (scroll the sidebar
   with the mouse wheel).
2. **Typing and paste.** Type, paste a short text and a long one. Pasting two
   lines is refused with "Input exceeds byte budget or contains control
   characters or line breaks" and leaves the field as it was.
3. **Long value.** Type past the box's width: the caret and the newest
   letters stay visible. Press Home, then hold Right: the text scrolls along.
4. **Line breaks.** Use a value that has them (for example `Trail⏎mar⏎ker¶`).
   The markers show; Left and Right cross each marker, and every press moves
   the caret; Backspace after a marker removes exactly one
   break; a click on the left or right half of a marker puts the caret on
   that side.
5. **Copy across a break.** Select across a marker, copy, and paste into a
   plain text editor: you get a real line break, not `⏎`.
6. **Input method** (Japanese or Chinese). While composing there is an
   underline and the candidate window sits next to the caret, also when the
   text is long and scrolled. Commit inserts the text once; Esc cancels the
   composition without closing the field; Enter while composing does not
   review.
7. **Dead keys.** `´` then `e` gives `é` once.
8. **Right-to-left.** Hebrew or Arabic with digits: Left, Right, Home, End,
   Shift with the arrows, clicks and Shift-clicks. Some back-and-forth in
   mixed text is known, in the editor's own field too.
9. **Review.** Enter and Review each show the same pending change. A click
   at the right end of the field places the caret; it does not review.
   Accept, then Undo: the label returns exactly.
10. **Unsupported text.** Paste a character the font pack lacks: the hint
    says "Unsupported text; cannot review", and Enter and Review refuse with
    the same reason.
11. **Scaling.** At 125 % or 150 % display scaling the caret, clicks and the
    candidate window line up with the text.
12. **Esc and the other fields.** Esc discards the draft. Numbers,
    coordinates, anchors and portal locations look and behave exactly as
    before.
13. **Zone drafts.** "Edit display name" shows the name with "Name of
    meadow; empty = none" below; clearing it and pressing Enter reviews
    "Removes the display name 'Quiet meadow' of zone meadow; it shows no
    display name.". In "New zone
    under meadow", `Bridge` and Enter keep the draft open with the id
    message; `meadow.bridge Old bridge` reviews "Creates zone meadow.bridge
    under meadow, named 'Old bridge'.".
14. **Palette filter.** In Place mode press `/` and type `a`: the palette
    narrows as you type, and in a 720×480 window scrolled down it scrolls
    back so the rows that match stay shown. N and O type letters. Enter
    keeps `Filter: a` and releases the field; a click on a palette row
    chooses it and releases the field too. `/` again, then Esc: the filter
    is empty and the whole palette is back. Start a composition and click a
    row: the composed text is dropped. Paste more than 64 bytes: refused
    with the same message, and the filter is as it was. In steps 1 and 6
    the filter has no Review or Discard: check that only its hint, "Enter
    keeps the filter / Esc clears it", sits below the row, and that Enter
    while composing does not release the field.

### Terrain palette

Terrain mode's sidebar lists, under "Terrain palette", one row per value
the catalog's `editing.terrain` grants, grid by grid and value by value,
each with its prototype's thumbnail. A row names the World format's
terrain the value is in (the terrain transitions of the World format's
FORMAT.md, section 13), by the terrain's own id: a brush that paints a terrain
writes the terrain's first prototype, and the preview draws the edges and
variants around it. So on the profile's terrain layer a row reads, in full:

- "floor / grass" when the value is the first prototype of a terrain with
  the same id, as for both rows of the neutral sample;
- "floor / lawn (grass)" when the terrain's id differs from the value;
- "floor / lawn variant: grass_tall" when the value is another prototype
  of the terrain: a variant, which counts as that terrain where it is
  painted;
- "floor / stone (no terrain)" when the value is in no terrain.

On a grid that is not the terrain layer, or in a world whose profile has no
transitions, a row names the grid and the value, as before. Choosing a row
chooses exactly its granted value: what the brush, Fill, Replace and the
review write is unchanged, and the rows are in the same order as before.
A row too wide for the sidebar keeps the value it writes, which tells two
rows apart: it drops the grid first ("lawn variant: grass_tall"), then the
words ("lawn: grass_tall"), then shortens a terrain id of more than 16
characters to its first 8 and last 6 around "...", and only then cuts the
value in its middle, keeping its head and tail, as zone rows do. The
`catalog` report names the same terrains ([CLI.md](CLI.md)).

### Terrain drag brush

In Terrain mode, choose an explicit palette value, then press on a cell and
drag. One press-move-release gesture is one brush stroke. The press selects
its cell through the ordinary exact cell mask; selection then prepares an
identical frame, and the stroke keeps that frame's exact cell mask for its
samples, so each pointer sample costs no extra render.
The brush stamps its footprint at every sample and along the straight cell
line from the previous sample, so a fast drag leaves no gaps; a single jump
of more than 4,096 cells stamps only where it lands. Footprint cells outside
the catalog's editable view bounds are dropped. Samples over panels or map
background add nothing, and the next cell hit joins the line from the last
covered cell, as the earlier brush prototype did.

The brush is a square (the default) or a disc around the pointer's cell, with
a radius of 0 (one cell) to 8 (17 x 17). A disc covers the cells whose
centres lie within half a cell past the radius. The Terrain sidebar shows the
brush and offers smaller, larger and shape controls; `[` and `]` change the
size and `B` the shape. A stroke keeps the brush it began with and covers at
most 4,096 distinct cells (the default view's cell budget); a stamp that would
pass the cap is refused whole and the stroke stops growing.

While the button is held, a live preview marks the cells the stroke would
change if released now: a neutral translucent white tint on each such cell,
and the status line counts the covered cells and how many of them would
change. The marked cells are exactly the cells the release puts into its
`terrain.set`: the editor session computes both with one function from the
same stroke, so covered cells that already hold the chosen value stay
unmarked, cells outside the view are never covered, and a stroke that would
change nothing marks nothing. The tint is drawn on copies of the prepared
exact cell-reference quads, the quads the cell mask itself is rasterised
from, so it has each cell's projected shape, including isometric diamonds
and tiles that terrain heights bend or stretch. The copies are placed in the
World's own draw order. The first time a prepared scene needs it (when the
zone overlay or a stroke preview is shown), the preview compiles the World's
terrain, then the cell reference tiles, then the World's placed objects into
one scene with the shared renderer, purely to learn where each cell falls
among the World's draws, and keeps that for the prepared scene; sessions
that never show an overlay never pay for it. A copy then draws directly
after the last World draw before its cell: after the cell's own terrain, or,
for a cell without terrain, at that cell's own position in the order (not at
an earlier cell's), in isometric and top-down views alike. Because the cells
come before the placed objects, an object on the grid's own ground render
layer, which shares its tile's order key, is still drawn after the mark. A
cell before every World draw draws first. If that combined ordering scene
cannot be compiled (a defensive case that the default limits do not reach),
no mark is drawn and
the status line adds "Preview unavailable for this view."; the release still
prepares the whole stroke. Anything the World draws later covers the mark as
it covers the terrain: an object standing on the
cell, a nearer raised tile or a higher ground layer hides the mark there.
Where a terrain art's outline differs from the reference cell shape, the
mark can reach a thin edge of a farther tile that the click mask gives to the
changed cell, and two neighbouring marks can overlap along it. For the same
reason a nearer cell's mark or tint can cover part of a farther object that
the nearer tile's art does not actually cover; ground art that fills the
tile's reference shape does not show this. The white tint
has little contrast on very light terrain; this is not addressed yet. The
copies are unpickable: they never reach a semantic, depth or coverage plane,
so what a click picks is unchanged. Each pointer sample
reuses the cached prepared World scene; only the overlay and the interface
are composed again, and no World scene is prepared. The overlay is bounded by
the stroke's 4,096-cell cap and is a subset of a scene that already met the
renderer's scene limits; should the World, the overlay and the interface
together still exceed them, the frame is drawn without the overlay and the
status line says the cells are too many to preview, while the release still
reviews every cell. Nothing is prepared, written or added to History until
release, and Escape (or anything else that abandons the stroke) removes the
preview. Release turns the whole gesture into one
semantic `terrain.set` operation in one World transaction, with each changed
cell expecting its committed value; cells that already hold the chosen value
are left out, and a stroke that changes nothing prepares nothing. That one
transaction becomes the pending diff: the scene shows it and the review panel
lists it, and only Accept publishes it. History's World undo check covers the
whole stroke before the diff is shown, Accept retains one exact inverse
recovery record for it, and Undo and Redo restore or repeat the whole stroke
in one reviewed step.

A stroke can start only when no change is pending; it never merges into an
existing pending diff. Escape abandons a running stroke, and other world
shortcuts are ignored until release. A resize, rebuild, focus loss or
minimize ends the stroke without preparing anything. A new press (after a
missed release) also ends it and does nothing else. Motion that arrives
without the button held finishes the stroke where the release was missed.
The brush never saves on release: there is no autosave.

### Fill and replace

Terrain mode's sidebar names the current rule in a header row above the
inspector's rows, below a proposal's block ("Rule: Brush  F", always laid
out, even at the smallest window; pressing it moves to the next rule), and
lists the rules after the palette: **Brush**
(drag paints, the default), **Fill** (a click fills the joined area),
**Replace** (a click replaces every match in the view) or **Road** (clicked
points, see [Roads](#roads)). A row chooses its rule; **F** cycles Brush,
Fill, Replace, Road in Terrain mode and does nothing elsewhere. The rule is kept across modes, like the brush. With Fill or
Replace the brush rows stay but are unavailable, and `[`, `]` and `B` do
nothing; a press never starts a stroke.

Fill covers the cells joined to the clicked cell through edge neighbours
(up, down, left, right; never diagonals) that hold its terrain on the
chosen terrain's grid, without leaving the active view, even where the area
would join again outside it. Replace covers every cell of the view that
holds the clicked cell's terrain, joined or not. A cell with no terrain on
the grid fills and replaces cells with no terrain. Both set the chosen
terrain; the engine works the cells out from the committed World
([docs/ENGINE.md](ENGINE.md#fill-and-replace)), so what is previewed is
what is prepared.

While hovering the map with Fill or Replace, the cells a click there would
change are marked like a stroke preview's (white at 45 %, through the same
unpickable copies of the exact cell-reference quads), and the status line
says how many: "Fill from (1, 1) with sand: 24 cells to change; click to
review." The preview is the engine's own answer for that cell, rule and
terrain (`Session::fill_changes`), worked out when one of them, the World,
the pending change or the frame changes, never per frame; hovering
prepares nothing, writes nothing and rebuilds nothing. Over a panel or off
the map there is no preview. When a click would be refused (already that
terrain, an ungranted terrain) nothing is marked and the status line gives
the reason. When the cell overlay cannot be drawn for the view, the line
adds "Preview unavailable for this view."

A click selects the cell through the exact cell mask and prepares the fill
as one pending change: one World transaction with one `terrain.set` that
sets every covered cell, each expecting the value it holds now, reviewed,
accepted, undone and redone whole like a stroke. A full 64 by 64 view, the
largest the catalog allows, is 4,096 cells, the stroke cap: its review
(four changed chunk files, about 14 KB of excerpt) is complete, so it can
be accepted.

Hovering picks the cell from one exact cell mask per presented frame, built
on the first hover of that frame and dropped when the frame is rebuilt, as
a stroke's samples do; while a change is pending, or editing is blocked,
hovering does nothing at all, and the status line stays the pending
change's, the review limit's or the proposal step's. A change of mode or
rule, a pointer leaving the window and a window losing focus forget the
hover, and release the mask when the rule is Brush or the mode is not
Terrain: no preview shows until the pointer moves over the map again, so a
preview never names a cell the pointer is not on. A pan or zoom under a
still pointer picks the cell now under it from the new frame's mask.

To try it by hand in a real window:

1. In Terrain mode press F until the rule row says Fill, then move the
   pointer around a large grass area: the pointer and the white preview
   should keep up, also on a HiDPI screen or a large view.
2. Click to fill. Without pressing anything, move the pointer over the map:
   the status line stays "Review changes, then Accept or Cancel." Accept,
   then Undo, and check that one step restores the area.
3. Replace from a cell with no terrain in a sparse view: every empty cell of
   the view, and only of the view, is marked. Cancel.

### Scatter palettes

A catalog may name palettes for the scatter brush in an optional
`editing.palettes` member, next to the grants: each a weighted set of
terrain values the catalog already grants on one ground grid. The neutral
sample's catalog has one, `speckle`, mostly grass with some sand:

```json
"editing": {
  "terrain": [{"grid": "floor", "values": ["grass", "sand"]}],
  "palettes": [
    {"name": "speckle", "grid": "floor", "entries": [
      {"value": "grass", "weight": 3},
      {"value": "sand", "weight": 1}
    ]}
  ]
}
```

Each palette has a `name` (a World identifier of at most 64 bytes, unique
in the catalog), a `grid` with an `editing.terrain` grant, and 1 to 32
`entries`, each a `value` granted on that grid (no value twice) and an
integer `weight` from 1 to 10,000. A catalog holds at most 32 palettes. A
catalog that breaks any of these rules does not load. The member is the
same in catalog formats 1 and 2; a catalog without it loads and behaves
exactly as before, and an editor from before palettes refuses a catalog
that has it, as it refuses any member it does not know
([ADR 0005](../decisions/0005-catalog-palettes.md)). The `catalog` command
and MCP tool list the palettes under `palettes` (name, grid, and each
value with its weight), only when the catalog has some.

In Terrain mode the sidebar lists the palettes after the terrain palette,
under "Scatter palettes", each row naming its grid, its name and its
weighted values ("floor / speckle: grass 3, sand 1"). Choosing one makes
the brush scatter it instead of painting one terrain; choosing a terrain
value again goes back to painting. Fill and Replace paint one terrain, so
with a palette chosen they ask for a terrain value.

A scatter stroke is a brush stroke in every other way: the same brush,
footprint, 4,096-cell cap, live preview, one pending change and one undo
step. When the press begins it, the editor draws a fresh seed from the
operating system's random source; each covered cell then gets one of the
palette's values, drawn by weight from the seed and the cell alone
([docs/ENGINE.md](ENGINE.md#scatter)), so a value is as likely as its
weight's share of the total. The status line names the palette and the
seed ("Painting 18 cells from palette speckle (seed 1234), 11 to change;
release to review."). The preview marks exactly the cells the release will
change, computed by the engine from the same seed: cells that already hold
their drawn value stay unmarked, as for a stroke. It marks which cells
change, not which value each gets; the review and the pending scene show
the values. The seed is part of the recorded action, so a proposal step
replays the same values; another stroke draws another seed, and stroking
the same cells again usually changes some of them again.

To try it by hand in a real window on the neutral sample, which has the
palette above, with no catalog editing:

1. In Terrain mode choose "floor / speckle", then drag across the grass
   columns: the white preview marks about a quarter of the cells (those
   that draw sand) and the status line names the seed.
2. Release: the pending scene shows grass and sand mixed about 3 to 1 where
   the preview was. Accept, then Undo: one step restores the area.
3. Drag the same area again: another seed gives another mix.

### Roads

Terrain mode's fourth rule, **Road** ("Rule: Road  F"), draws a road by its
points: each click on a tile of the view adds a point, and the road is the
brush stamped along the straight cell line from each point to the next,
exactly as a drag stamps between its pointer samples (the same code, so a
drag whose pointer samples land on exactly these tiles, in this order,
covers the same cells; a drag by hand also samples the tiles in between,
and may cover others. The line rule depends on direction, so the order of
the clicks decides which cells). The width and shape are the
brush's (`[`, `]` and `B`; odd widths 1 to 17, square or disc; the disc
keeps a diagonal road as wide as a straight one); the corridor is clipped
to the view and capped at 4,096 cells, as a stroke is. The road lays the
chosen terrain value, or scatters the chosen palette. A press never starts
a drag stroke with this rule.

Keys while a road is drawn, before the global keys:

- **Enter** finishes it: it becomes one ordinary pending change for Review,
  a `paint_stroke` over the corridor with the chosen terrain, or a
  `scatter_terrain` with the chosen palette. Enter again, on the review
  shown, accepts as for every edit; one Enter never does both.
- **Backspace** removes the last point; removing the only point ends the
  road.
- **Esc** drops the road and does nothing else: no pending change is
  cancelled and the mode stays.
- `[`, `]` and `B` change the corridor, and the preview follows; H, O, pans
  and zooms keep the points, which are tiles, not pixels.
- F (another rule), a key or button of another mode, and a terrain or
  palette of another grid end the road, and the status line says so ("Road
  dropped: the rule changed; nothing was prepared.", and likewise for the
  mode and the grid; Esc gives "Road dropped; nothing was prepared."). T or
  the Terrain button in Terrain mode, and a terrain or palette of the same
  grid, keep it.
- Every other control that would prepare an edit or open a draft or a pick
  step is refused with "Finish the road with Enter or drop it with Esc
  first." when it comes from a key or a direct control, and laid out
  disabled (a press on a disabled row does nothing, as for every disabled
  row): Undo, Redo, Delete, the inspector's
  position, field and portal controls of an object selected before, the
  proposal's controls and the [Grade road](#grading-a-road) rows; so are
  the keys Z, Y, R, Delete, L, E and, with a proposal open, G. So nothing
  else is prepared while a road is drawn.

A road needs two points: with one, Enter prepares nothing and the road
stays. A click on the last point adds none. A click that hits no tile of
the view adds nothing and says so; it never picks a nearby tile. The road
does not select anything. While a change is pending a click adds no point,
refused in the words a brush press gets. While editing is paused clicks,
Enter and Backspace are refused in the pause's words and the road's marks
are not drawn; Esc still drops it. A road and a text draft never exist
together: the first road point discards an open draft (its text is not
kept), as any press on the map does, and no draft opens while a road is
drawn, so a road never takes text input. An open portal pick
step takes the next tile click, as it always does, so no road begins until
it is ended (Esc ends it first). With neither a terrain nor a palette chosen a click is refused as a
brush press is. A window losing focus keeps the road.

While a road is drawn its points are marked with ringed amber centre marks
(the shape of Sculpt mode's marks, which never show in Terrain mode), and
the cells finishing it would change are marked as a stroke's are, white at
45 %: the engine's own plan for the same stroke Enter prepares, so cells
that already hold the terrain stay unmarked. The status line counts them:
"Road of sand: 3 points, 212 cells, 180 to change; Enter to review,
Backspace removes a point, Esc drops it." When finishing would change
nothing, or be refused, nothing is marked and the line gives the engine's
reason after "Enter prepares nothing:"; the road stays so it can be
extended. There is no line from the last point to the pointer: the corridor
after each click is what is recorded. Over the scene limits the marks are
left out with "Too many cells to preview.", as for a stroke.

A road that scatters draws its seed once, from the session's random
source, at its first point (or when a palette is chosen while a terrain
road is drawn) and keeps it, so the preview marks the cells the recorded
seed changes; the status line names it ("Road from palette speckle (seed
1234): ..."). Finishing draws no seed.

What is recorded is the ordinary edit: its cells, terrain or palette and
seed, never the points. A proposal, Undo and Redo treat a road as the one
paint or scatter it is, one step each; grading it is a second edit of its
own ([Grading a road](#grading-a-road)).

To try it by hand in a real window on the unedited neutral sample:

1. Press T, choose "floor / sand", press F until the rule row says Road,
   and press `]` once (a 3 x 3 brush).
2. Click (0,1), (3,1) and (3,4): amber marks sit on the three tiles, the
   white marks cover the grass the road crosses, and the status line counts
   3 points.
3. Press Backspace (the mark on (3,4) goes), click (0,4), then press Enter:
   the status line says "Review changes, then Accept or Cancel." and the
   pending scene shows the sand road.
4. Press Enter to accept, then Z and Enter: one step takes the road away.

#### Grading a road

Once a road's change is accepted, the editor remembers the cells that
change recorded (every cell of the corridor, also those that already held
the terrain) and the road's first point. Under a catalog with a
[height range](#height-range), Terrain mode's sidebar then offers, after
the rules, "Road from (0, 1), 21 cells" (the first point, then the
cells) and two rows:

- **Grade road: Flatten** (`L`, for level) prepares a `flatten_heights`
  over exactly those cells to the committed height at the road's first
  point, read once, when the row or key is pressed, as a Flatten stroke
  reads its start tile at the press ([Sculpt mode](#sculpt-mode)).
- **Grade road: Smooth** (`E`, for even out) prepares a `smooth_heights`
  over exactly those cells with kernel 1. It reads only inside the active
  view ([ADR 0008](../decisions/0008-smooth-heights.md)): a road's cells
  within two tiles of the view's edge depend on where the view ends, as
  for a smooth stroke.

Either goes to Review like every other change: Enter accepts, Esc
cancels, and Undo and Redo take it as one step of its own, so a graded road
is two Reviews and two Undo steps (the grade, then the road). Nothing new
is recorded: a proposal, the MCP editing server and the World see an
ordinary `flatten_heights` or `smooth_heights` with the road's cells, which
replays exactly. The keys work in Terrain mode only. Every refusal of
those edits applies unchanged: the granted range (a first point whose
height is outside it refuses the flatten: "Grade road refused: the road's
first point (1, 1) has height 20, outside the granted range -8 to 8; a
flatten target must lie inside it."), nothing to change, the cell cap (a
road never has more cells than an edit may cover), and a result that cannot
be drawn at this view, which is cancelled as any edit is, with a grade's
own advice: its target and cells are the road's, so "Zoom out and grade
again, or smooth the road instead." for a flatten and "Zoom out and grade
again." for a smooth. The road stays remembered.

The rows are laid out disabled, and the line above them says why, when the
catalog grants no heights ("Grading needs editing.heights"), when no road
is remembered ("Grading: no road since last edit": grading needs a road
accepted since the last other accepted change, Undo or Redo, even when an
older road is still on the map), while a change is pending,
the grade's own too ("Grading: a change is pending"), and while editing is
paused ("Grading: editing is paused"); the keys and controls are then
refused in the words of the cause. While a road is drawn they are fenced
like every other edit.

The remembered road is one road, the last accepted, and is kept only while
its cells are still what it covers:

- accepting a grade of it keeps it (its cells are still the road's), so it
  can be smoothed again, or flattened and then smoothed; cancelling a grade
  keeps it too;
- accepting any other change forgets it, and so does accepting an Undo or
  a Redo of any step, including the road's own Undo; a Redo never brings it
  back;
- any change of the committed World or of the active view (plane, layer,
  bounds) that did not come from its grade forgets it; a World or catalog
  changed on disk pauses or blocks every edit, grades included;
- leaving Terrain mode, pans and zooms, a Reopen (also during the road's
  or a grade's Review: the change accepted after it is remembered as
  usual), drawing another road (until that one is accepted) and dropping
  it keep it.

The scripted demonstration does not grade: the neutral sample grants no
height range.

To try it by hand on a copy (the sample has no heights grant):

1. Copy `crates/tilewright-editor-testkit/fixtures/prototype-neutral` to a
   directory of your own, give the copy's `preview.jsonc` the range
   `"heights": {"min": -8, "max": 8}` inside `"editing"` and its
   `planes/main/layers/0/0_0.jsonc` a height grid of 32 rows of 32 numbers
   `x - y`, written as in the [Heights](#heights) hand test, and provision
   it as above.
2. Press T, choose "floor / sand", press F until the rule row says Road,
   press `]` once (3 x 3), click (0,1) and (5,1), and press Enter twice:
   the road is accepted, and the sidebar's last rows read "Road from (0,
   1), 21 cells", "Grade road: Flatten  L" and "Grade road: Smooth  E".
3. Press L: the review shows the chunk's `height` rows; press Enter. Press
   H: the road's three rows from x 0 to 6 are one band, height -1, the
   height of (0,1); the inspector on (6,2) reads "Height: -1" and on (7,2)
   "Height: 5".
4. Press Z and Enter: the heights come back ((2,1) reads "Height: 1")
   and the sand stays, but the Undo forgot the road: the line above the
   grade rows reads "Grading: no road since last edit" and L is refused.
   Z and Enter again: the road goes too.

### Deleting objects

Select an object, then press Delete or choose Delete in the toolbar or the
inspector. Deletion is offered only when the catalog's optional
`editing.objects` list has an entry for the object's prototype, and only for a
top-level object in the active view (the catalog's plane and spatial layer,
with its tile inside the view bounds); otherwise the control is disabled and
the key reports why. A focused text draft keeps the Delete key for itself. One
deletion is one World transaction with one `object.remove` addressed by uid
and guarded by the object's prototype; the object's inventory goes with it.
It joins the pending fence like every other edit. The review panel shows the
diff with warnings above it: when the object holds inventory objects, when a
portal anywhere or an anchor in its plane targets its tile (they keep targeting
that tile), and always that its uid, and any uids in its inventory, are removed
and must not be reused. The warnings have their own small allowance outside
the review budget; they inform and never refuse the deletion or disable
Accept. Accept retains one inverse recovery record;
Undo reinserts the object with its exact source bytes, and when the deletion
emptied a chunk, its file is deleted and Undo restores it.

The same `editing.objects` entry also grants movement and edits of the fields
whose names its `fields` member lists, with the same active-view scope. The
entry carries no field values. The per-uid `movable_uids` and `fields` grants
keep their own reach and combine with it.

### Heights

World stores one whole-number height per tile and spatial layer (any 32-bit
value; 0 where a chunk has no height grid). The preview already draws them:
top-down ground shifts up by its height and isometric ground bends. To read
them, H (or the sidebar's "Show heights  H" row, in every mode under the zone
toggle) toggles a **height tint**. It is off by default and the choice is
kept across modes.

The tint colours each tile of the view whose chunk exists by its stored
height, on a fixed ramp of eight bands from dark violet (lowest) through teal
and green to pale yellow (highest). The tint is opaque, so while it shows it
hides the ground art under it; it is a toggle, and H brings the art back.
Opaque, a tile shows its band's own colour whatever the ground (light art,
dark art or a tile without ground), and each band is at least 9 CIE L*
lighter than the band below it (tested). A translucent tint let the ground
decide part of a tile's lightness, and neighbouring bands then swapped over
light and dark ground. So the order reads without telling hues apart, and
both ends are darker and lighter than any zone tint. The range is the lowest
and highest stored height among the view's tiles: the catalog view's bounds,
including tiles drawn outside the window (panned or zoomed away, or lifted
out of the frame); tiles of chunks that do not exist are neither tinted nor
counted. It is taken in the shown World (the pending one while a change is
reviewed, then the committed one after Accept, Cancel or Undo). A tile's band is the nearest of the eight evenly spaced
band positions, a half rounded up, computed in integers:
`((h - min) × 14 + (max - min)) div (2 × (max - min))`. The lowest height is
always band 0 and the highest band 7, whatever the span. A flat view (every
height the same) has no tint at all. The tint uses the stored heights, not
the stacked ones, so a profile's `layer_height` does not change it.

While the tint shows, the sidebar gives its legend under the toggle:
"Heights 0 to 12", then "Darkest: lowest, 0" and "Lightest: highest, 12" with
the two end bands as swatches. While zone tints are drawn too (the zone
overlay shows and some tile of the view has a zone), a last line says "Zone
tints cover it: O hides them", or in Zone mode, where zones always show and
O does nothing, "Zone tints cover it in Zone mode", since tiles inside zones
then do not match the swatches. A flat view reads "Heights: all 0, no tint",
and a view without chunks "Heights: no tiles in view". The tint is made like
the zone tints: unpickable copies of the prepared cell-reference quads in the
World's draw order, so objects, nearer tiles and the terrain transition art
drawn over a tile (such as the sample's grass edges on sand) still draw over
it, the semantic planes
and every pick are unchanged, and a tile the prepared cell scene does not draw
(for example ground lifted far out of the frame) has no tint either. A zone
tint of the same tile draws over it, and the proposal highlight and the brush
preview over both; turn the zones off to read the heights alone. The brush
preview (white) and the proposal highlights (black) signal by lightness too,
so on the tint a previewed or highlighted tile can read as a neighbouring
band (a previewed tile lighter, a highlighted one darker); turn the tint off while painting or reviewing a
proposal to read those marks. Sculpt mode's preview is a centre mark
instead, which no band looks like ([Sculpt mode](#sculpt-mode)). When the cells' draw order cannot be computed
the status line says "Height tint unavailable for this view." (or names the
height tint with the zone overlay and stroke preview when those are wanted
too), and when the overlays would pass the scene
limits it says which were left out.

The inspector of a selected tile shows its height after its value: "Height:
7", or with a profile `layer_height` the stacked height first, then the
stored height and the layer's base: "Height: 39 (7 + base 32)" (layer ×
`layer_height` + height, computed in 128-bit integers; a narrow sidebar cuts
the line with "..." after the stacked height). A
tile of a chunk that does not exist reads "Height: none, no chunk here". The
desktop writes heights only in [Sculpt mode](#sculpt-mode), under a
[height range](#height-range) (agents and proposals can too), and nothing
marks a slope "too steep": the profile's
movement limits are only reported as stored (`overview`'s
`profile.movement`), because the format does not define what they mean.

Hand test (the sample has no heights, so use a copy):

1. Copy `crates/tilewright-editor-testkit/fixtures/prototype-neutral` to a
   directory of your own, and give the copy's
   `planes/main/layers/0/0_0.jsonc` a height grid inside `terrain`, for
   example 32 rows of 32 numbers `3 × x - y` written as
   `"height": {"rows": [[0, 3, 6, ...], ...]}`. Provision the copy with
   `tools/provision-prototype.py --world <copy>` as above.
2. Press H: the view is tinted dark at the bottom left (-5) to light at the
   top right (21), and the legend reads "Heights -5 to 21". Press O: the zone
   tints draw over it; O again hides them.
3. Click tile (3, 2): the inspector reads "Height: 7". Press H again: the tint
   and legend go, and the frame is the one before the first H.

The World pixels of the existing demonstration captures are unchanged, since
the tint is off by default; their interface changed: the sidebar has the
height toggle row, and the inspector of a selected tile has the height line.

#### Height range

A catalog may let the editor change tile heights in an optional
`editing.heights` member, next to the grants: the range of heights it may
write in the active view's plane and spatial layer. For example, to allow
heights from -64 to 64:

```json
"editing": {
  "terrain": [{"grid": "floor", "values": ["grass", "sand"]}],
  "heights": {"min": -64, "max": 64}
}
```

`min` and `max` are integers with `min < max`, both from -1,024 to 1,024,
the editor's hard bound; anything else, or any other member, refuses the
whole catalog. The bound is only a sanity limit for the arithmetic and the
wire form, not a promise that every range inside it can be drawn. A bent
isometric tile maps its ground art onto a mesh, and art taller than the
diamond magnifies the differences between corner heights, so what a view
can draw depends on the ground art and the zoom: choose a range your
views can draw. Measured on renderer 0.7.1 at zoom 8 (the largest), with
the worst pattern of ±H around a tile: top-down ground has no limit;
isometric diamond-only art (32 × 16 or 64 × 32) draws up to about ±1,430;
art twice the diamond's height (the neutral sample's 32 × 32 on a 32 × 16
diamond) up to about ±460; art four times its height (64 × 128 on 64 × 32)
up to about ±129. [ADR 0006](../decisions/0006-catalog-heights.md) has the
table. If an edit's result cannot be drawn at the current view, the
desktop cancels it and says so, with the renderer's reason ("Change
refused: it cannot be drawn at this view, so it was cancelled.", then
"Zoom out, or raise or lower by less." for a height adjustment, "Zoom
out, or flatten to a height nearer the tiles around it." for a flatten,
"Zoom out." for a smooth, "Zoom out and prepare the step again." for a
proposal's height step, whose amount, target or cells the person cannot
change, and "Zoom out or pan and
try again." for any other edit); editing continues. The
member is the same in catalog formats 1 and 2; a catalog without it loads
and behaves exactly as before, and an editor from before it refuses a
catalog that has it. The `catalog` command and MCP tool report it as
`heights` (`min` and `max`), only when the catalog has it. The neutral
sample has none.

Under a range, an `adjust_heights` action (an agent's `edit`, a proposal
step, or a stroke of the desktop's [Sculpt mode](#sculpt-mode)) raises or
lowers the covered
cells of the active view by a flat amount, once each, and stops at the
range's ends ([docs/ENGINE.md](ENGINE.md#raise-and-lower)). Hand test with
the copy above, its catalog given `"heights": {"min": -8, "max": 32}`: in a
sandbox of it ([docs/SANDBOX.md](SANDBOX.md)), `edit` the action
`{"kind": "adjust_heights", "plane": "main", "layer": 0, "cells": [[3, 2]], "amount": 8}`
with its `expected`, then `accept`: `tile` at (3, 2) reports `height` 15,
and the submitted proposal applied in the desktop shows the raised tile
under H.

A `flatten_heights` action sets the covered cells to one recorded target
height the range holds, wherever they were, also cells outside the range,
which is how a tile too far outside for a raise or lower is brought back
([docs/ENGINE.md](ENGINE.md#flatten)). In the same sandbox, `edit`
`{"kind": "flatten_heights", "plane": "main", "layer": 0, "cells": [[2, 2], [3, 2]], "target": -4}`
with its `expected`, then `accept`: `tile` reports `height` -4 at both.

A `smooth_heights` action moves each covered cell toward the weighted mean
of the committed heights of the 5 × 5 tiles centred on it, inside the
view, with the fixed rule `kernel` 1 ([docs/ENGINE.md](ENGINE.md#smooth)).
In the same sandbox, `edit`
`{"kind": "smooth_heights", "plane": "main", "layer": 0, "cells": [[3, 2]], "kernel": 1}`
with its `expected`, then `accept`: (3, 2), at -4 among heights from -4 to
15, reads 2228 / 256 = 8.7 above itself on average, so `tile` reports
`height` 5.

#### Sculpt mode

Under a catalog with a [height range](#height-range), the sidebar has a
"Sculpt heights  U" row after the height rows, in every mode; it and U enter
Sculpt mode. Without the range the row is not shown, and U is refused:
"Sculpt is unavailable: the catalog does not grant height editing
(editing.heights)." Entering the mode shows the [height tint](#heights);
H still hides it there, for the ground art, and leaving the mode puts the
tint back as it was before entering, whatever H did in between.

A drag raises or lowers the heights of the cells it covers, by a flat step,
once per stroke: the brush collects the cells exactly as the terrain brush
does (the same square or circle, `[`, `]` and B, the same clipping to the
view and 4,096-cell cap, and the same interruptions), and a cell the brush
passes twice changes once. There is no falloff and nothing accumulates
while dragging. The release prepares one `adjust_heights` action
([docs/ENGINE.md](ENGINE.md#raise-and-lower)) with the stroke's cells and
the signed step, for the ordinary Review, Accept or Cancel; during Review
the tint shows the pending heights, and Undo and Redo take the whole stroke.

The sidebar's rows: above the scrolled rows, "Sculpt: Raise by 8  F" names
the tool and step (pressing it, or F, switches between Raise and Lower);
then "Granted heights -64 to 64", the brush rows, "Raise: drag lifts each
cell once" and "Lower: drag sinks each cell once", and "Step 1  1" and
"Step 8  8" (keys 1 and 8). The tool and step are kept across modes. The
status line reads "Ready. Drag to raise heights by 8."

The third tool, Flatten (F after Lower; F again goes on to Smooth), sets
every cell a drag covers to one height: the height of the tile where the
drag starts. That target is read once, at the press, from the committed
World (a stroke cannot start while a change is pending), and stays fixed
for the whole stroke, whatever heights the drag passes over; the release
prepares one `flatten_heights` action ([docs/ENGINE.md](ENGINE.md#flatten))
that records it. Its rows read "Flatten: drag levels to the start tile",
and the pinned row "Sculpt: Flatten  F", then "Sculpt: Flatten to 8  F"
while a stroke runs. The step rows are dimmed and keys 1 and 8 do nothing
for it (the step stays kept for Raise and Lower). The status line reads
"Ready. Drag to flatten heights to the start tile's height." A press on a
tile without a map chunk is refused ("Flatten needs a starting tile with
ground: tile (33, 1) has no map chunk."), and so is a press on a tile whose
height is outside the granted range ("Flatten refused: this tile's height
30 is outside the granted range -8 to 8; start on a tile inside it."):
the target would not be granted, and clamping it into the range would
flatten to a height nobody pointed at. A flatten brings any tile back
into the range, however far outside it is, but such a tile may not be
drawn where it stands, so it cannot be pointed at: press on a neighbour
inside the range and let the brush cover its cell (a 3 × 3 brush reaches
it), then read the count in the status line.

The fourth tool, Smooth (F after Flatten; F again goes back to Raise),
evens out the heights of the cells a drag covers: each moves toward the
weighted mean of the 5 × 5 tiles centred on it, reaching two tiles out
(weights 1, 4, 6, 4, 1 across times 1, 4, 6, 4, 1 down, so the cell itself
36 of 256), rounded to a whole height, an exact half toward where the cell
was. The heights read are the committed ones, never another cell's new
height, so the result does not depend on the order or extent of the drag;
tiles outside the stroke are read too, but not those outside the view or
in chunks that do not exist, so cells within two tiles of the view's edge
depend on where the view ends. Every height read counts as if clamped
into the granted range, so a smooth never writes outside it, and a tile
far outside it is brought in. The release prepares one `smooth_heights`
action ([docs/ENGINE.md](ENGINE.md#smooth)) with kernel 1, the only
version of that rule. Its row reads "Smooth: drag evens out heights", the
pinned row "Sculpt: Smooth  F", and the status line "Ready. Drag to smooth
heights toward their neighbours." It has one fixed strength: the step
rows are dimmed and keys 1 and 8 do nothing for it; a second stroke
smooths further until the ground rests (it did in every case measured).
A cell is "already even" when it
is within half a unit of the weighted mean it reads: what decides that is
how sharply the slope bends, not how large a step is, and an even slope of
any steepness is kept where all 25 tiles are read. Near the view's edge it
is not: one stroke changes a slope's end within two tiles of the edge,
where the mean is one-sided, and repeated strokes flatten it further in.
A slope of 4 across the sample's 8-wide view, smoothed whole until it
rests, ends as 10, 10, 11, 13, 15, 17, 18, 18 (its relief from 28 down to
8); across a 16-wide view as 10, 10, 11, 13, 16, 20, …, 44, 47, 49, 50, 50.
To keep a slope that runs to the view's edge, smooth with a stroke that
stays more than two tiles away from that edge: every cell it covers then
reads all 25 tiles (where their chunks exist), and an even slope stays
exactly as it is. A step of 1 is already even and a step of 2
becomes 0, 1, 1, 2. A raise of 8 smoothed once becomes 0, 2, 6, 8, a
second time 0, 1, 3, 5, 7, 8, and then rests. To see it on a copy with the
range -8 to 8: in Raise by 8 with a 7x7 brush (`]` three times from the
1x1), click tile (7, 2): columns 4 to 7 rise to 8. Press F three times
for Smooth and drag from (3, 2) to (4, 2): "Smoothing 48 cells, 12 to
change, 36 already even"; after Enter every row reads 0, 0, 0, 2, 6, 8,
8, 8 (the inspector shows each tile's height). The same drag again: "24 to change, 24 already
even", then 0, 0, 1, 3, 5, 7, 8, 8. A third: "Smoothing 48 cells refused:
smooth changes no cell: 48 covered cells already even, ...".

While dragging, before the release, each cell the release would change
carries a mark: a magenta copy of the cell's flat shape at half its size
about its centre, ringed by a light line inside a dark navy one, so the
cell's own height band still shows around it. The ring is at least 40 CIE L*
lighter or darker than every height band (tested), so the mark separates
from the band by lightness, not by hue alone. Each ring line is 1.5 pixels,
or a quarter of the mark's own size when that is less, so the ring stays
inside its tile at every zoom; at the smallest zooms a mark too small for a
ring shows as the magenta mark alone. No height band (which fills the whole
tile, violet to pale yellow), zone tint, proposal highlight (darker, the
whole tile) or badge (in a corner) looks like it. Marks draw where badges
do, above all World content, with the badges over them: no object, nearer
tile or tall art hides a mark, and while the stroke runs a mark can cover
part of an object on its tile, or of an object or nearer tile in front of
it. A mark sits on its own tile's ground, so read it by the ground around
it, not by what it covers. Cells outside the window are counted in the
status line but not marked. Each marked tile costs three quads however bent
its ground. When a very large stroke, with the height tint and zones shown,
would take the frame past what one frame can draw, all overlays (tint,
zones, badges and marks) are left out for that frame and the status line
ends "Too many cells to preview."; the release still prepares every cell.
Turning the zones off (O, between strokes) or zooming in brings the marks
back.
The marked cells are the engine's own answer
(`tilewright_editor_ui::sculpt::plan`, `flatten_plan` for Flatten, or
`smooth_plan` for Smooth, on the committed World): the cells Accept writes that this view draws. A changed
cell whose tile is not drawn at this view (a height far outside the range,
which only a flatten or a smooth changes) has nothing to mark; the status line counts
it ("…, 35 to change, 2 not drawn at this view; release to review.", only
when there are any) and Accept writes it too. For Flatten the status line reads
"Flattening 37 cells to 8, 35 to change, 2 already there; release to
review.", cells already at the target being left out and counted; for
Smooth "Smoothing 37 cells, 30 to change, 7 already even; release to
review.", cells whose height would not change being left out and
counted. The status line says what the release would do: "Raising 37
cells by 8, 35 to change, 2 at or above the top; release to review." Cells
at or beyond the end the step moves toward ("at or above the top", "at or
below the bottom") and cells of chunks that do not exist ("without a
chunk") are left out and counted. When the engine would refuse the stroke,
nothing is marked, the status line gives its words ("Lowering 2 cells by 1
refused: cell (6, 0) has height 6, above the granted range -4 to 4: lowering
it into the range needs an amount of at least 2"), the details repeat them
whole ("Release prepares nothing: ..."), and the release is refused the same
way and prepares nothing; avoid that cell, or use the larger step. A stroke
that would change no cell is refused with the engine's count of the cells at
the end and without a chunk. A release whose result cannot be drawn at the
current view is cancelled, as for any edit ("Change refused: ..., Zoom out,
or raise or lower by less.", for Flatten "Zoom out, or flatten to a
height nearer the tiles around it.", and for Smooth, which has no amount
to lessen though a stroke's edge can come out steeper, "Zoom out."), and
the mode carries on. While rendering
pauses editing, a press is refused with the pause's reason. With a proposal
open, the person's own strokes between steps are prepared and accepted like
any other edit of theirs. A frame captured while a sculpt stroke runs has a
`sculpt_preview` member: the signed `amount`, the covered `cells`, and either
`changes`, `at_end`, `missing` and the `first` eight changed cells, or
`refused` with the engine's message. A Flatten stroke's has `target` in
place of `amount` and `at_target` in place of `at_end`; a Smooth stroke's
has `kernel` (1) in place of `amount` and `even` in place of `at_end`.
When some changed cells are not drawn, each kind's record also has
`undrawn`: their `count` and the `first` eight.

The scripted demonstration has no sculpt step: the neutral sample grants no
height range, and it stays that way. Hand test on a copy (every height of the
sample is 0, so the tint shows nothing, "Heights: all 0, no tint", until the
first Accept; for the look of the marks over many bands, give the copy
heights as in the [Heights](#heights) hand test):

1. Copy `crates/tilewright-editor-testkit/fixtures/prototype-neutral` to a
   directory of your own. In the copy's `preview.jsonc`, add this line inside
   `"editing": { ... }`, after its last member (put a comma after that
   member):

   ```json
   "heights": {"min": -8, "max": 8}
   ```

   Provision the copy with `tools/provision-prototype.py --world <copy>` as
   above, and press U: the pinned row reads "Sculpt: Raise by 1  F" and the
   status line "Ready. Drag to raise heights by 1." Press 8.
2. Drag from tile (1, 1) to (3, 1) with the 1x1 brush and keep the button
   down: three magenta marks with dark and light rims appear, and the status
   line reads "Raising 3 cells by 8, 3 to change; release to review."
   Release: the review shows the chunk's new `height` rows. Press Enter: the
   three tiles turn the lightest band ("Heights 0 to 8"), and the inspector,
   on (1, 1) where the drag began, reads "Height: 8".
3. Drag from (2, 1) to (4, 1) and hold: only (4, 1) is marked, and the status
   line reads "Raising 3 cells by 8, 1 to change, 2 at or above the top;
   release to review." Press Esc while holding, then release: nothing is
   pending.
4. Press F (the pinned row reads "Sculpt: Lower by 8  F"), then click tile
   (1, 1): a click is a one-cell stroke, and the review shows its height
   going from 8 to 0. Press Esc to cancel it.
5. Press F (the pinned row reads "Sculpt: Flatten  F" and the step rows
   dim). Press on tile (1, 1), height 8, and drag to (1, 3) with the 1x1
   brush, holding the button: the pinned row reads "Sculpt: Flatten to 8
   F", (1, 2) and (1, 3) are marked, and the status line reads "Flattening
   3 cells to 8, 2 to change, 1 already there; release to review."
   Release and press Enter: the inspector, on (1, 1), still reads "Height:
   8", and (1, 3) now has the lightest band too.
6. Press F (the pinned row reads "Sculpt: Smooth  F"). Drag from (2, 2) to
   (4, 2) with the 1x1 brush and hold: all three are marked, and the
   status line reads "Smoothing 3 cells, 3 to change; release to review."
   (4, 2) moves too: the raised tiles two tiles away count. Release and
   press Enter: the inspector, on (2, 2), reads "Height: 3" (its 5 × 5
   mean is 768 / 256 = 3 exactly), (3, 2) reads 2 and (4, 2) 1. Press F to
   go back to Raise, then T: the tint goes off again, as it was before U.

### Zones

Zone mode (the toolbar's Zone, or N) shows which zone every tile of the view
is in. World keeps a zone per tile and layer in each chunk's `zone` grid; an
empty cell, or a chunk without a zone grid, is in the plane's default zone,
and a tile with neither has no zone. A tile's display name is the first zone
walking up its parent chain that has a `name` (an unnamed child shows its
parent's name). The editor computes this walk; it stops at an unknown parent,
a repeated zone or 64 zones, although World already refuses cycles.

The overlay tints each tile of the view that has a zone; tiles without a zone,
and tiles whose chunk does not exist, get no tint. Tints are copies of the
prepared exact cell-reference quads (the quads the cell mask is rasterised
from), unpickable and placed in the World scene's draw order exactly like the
terrain stroke preview's marks, so each draws where its tile's ground is
drawn, before the objects on it, any nearer raised tile and higher ground
layers, which cover it as they cover the ground: the part of a tile an
object hides shows no tint. A tile without ground keeps its own place in
that order, so a zoned but unpainted area stays visible and is covered only
by what is on it or nearer, exactly as ground there would be. Objects on the
grid's own ground render layer (a placed object may use any granted render
layer, on sub-layer 0 or above) are drawn after their tile's tint too. A nearer tile's tint does draw
over the part of a farther object that overhangs that nearer tile, as the
nearer tile's ground would, including where the tile has no ground. When the
cells' places in the draw order cannot be computed for a view (see the
terrain brush), no tint is drawn and the status line says "Zone overlay and
stroke preview unavailable for this view." The semantic, depth and coverage planes, and so every pick,
are unchanged.

Colours come from eight fixed tints (the Okabe-Ito colour-blind-safe set with
black replaced by indigo) at 40 % opacity. They are assigned over all zone
definitions of the plane in the shown World (the pending one while a change
is reviewed), sorted by id: each zone prefers the tint a stable hash of its
id selects, and a zone whose preferred tint an earlier zone took gets the
next free one. A zone's colour therefore changes only when the plane's zone
definitions change, never because of a stroke or what is in view. Creating
or deleting a zone can move other zones' tints (a zone sorted after it whose
preferred tint collides); the overlay, the legend and its swatches follow the
pending definitions while the change is reviewed, and the committed ones
again after Accept, Cancel or Undo. Two zones may also get similar tints
(the set has two oranges); the legend's ids and the inspector stay the
reliable cue. Zones share a tint only when the
plane defines more than eight, and the legend then says "shared tint" on
those rows. The legend's swatches are the tints drawn opaque; a translucent
tint over varied terrain does not match its swatch exactly, so the tile's
shape in the overlay and the inspector's zone line are the reliable cue.

If the overlay would take the frame past the scene limits it is left out and
the status line says so ("Zone overlay left out: too many cells for one
frame."). During a stroke the zone tints and the stroke preview are composed
together, so both are dropped together and the status line says "Too many
cells to preview."

The overlay is always on in Zone mode. In the other modes it is off by
default and O (or the sidebar's "Show zones and badges  O" row; "Show
zones  O" in Anchor mode) toggles it; the choice is kept across modes. The
anchor and portal badges (see Anchors) show wherever the zone overlay shows,
and always in Anchor mode. The World pixels of the existing demonstration captures of
other modes are therefore unchanged; their interface did change: the toolbar
buttons are narrower to make room for Zone, the sidebar has the toggle row,
and the inspector of a selected tile has zone lines.

In Zone mode the sidebar lists every zone of the plane, sorted by id, with its
tint as a swatch, its display name and how many tiles of the view it holds,
or "not in view". A last row stands for the plane's default zone. Only the
first 256 zones are listed, and then the header still counts every zone and
a row says "First 256 zones shown"; there is no filter yet. Zone ids come first in
these rows; when a row is too narrow the display name and count are dropped
first, and then the id is elided in its middle, keeping its head and its
tail, because hierarchical ids share their beginnings. The inspector shows a
selected tile's zone (marked when it is the plane default), its parent chain
outermost first (outer zones give way to `...` first; the tile's own zone is
kept whole) and its display name, in every mode that selects tiles.

Painting zones needs the catalog's optional `editing.zones` member,
`{"paint": true, "manage": false}`: capability flags, never a list of zone
ids, so the catalog keeps loading when zones change. `manage` grants zone
management (below). Without `paint` or `manage` the zone rows are disabled, a press
only selects the tile and the status line says painting is not granted.
With it, choose a zone row (or the default-zone row) and drag: the gesture,
brush size and shape keys, caps, view clipping and pending fence are those of
the terrain brush. Samples come from the ground cell mask; the release writes
the `zone` grid. Tiles whose chunk does not exist are left out of a zone
stroke like tiles outside the view, because painting them would create a
chunk document holding only a zone grid; tiles of an existing chunk that
merely have no ground stay paintable, and a stroke that reaches no existing
chunk is refused, saying its tiles are in map chunks that do not exist.

One stroke is one World transaction with one `terrain.set` on the grid
`zone`, a cell entry with `expect` for every covered cell whose own value
changes, and value `null` for the default-zone row (the cell empties and
shows the plane's default zone again). Cells are compared by their own
value, not the zone they display, so painting the default zone onto an empty
cell writes it explicitly and is a change. World adds or removes the chunk's
zone palette entries and the zone grid itself as needed; Undo and Redo
restore the exact bytes. The review shows a neutral note per kind of change,
such as "Zones: 5 tiles move to path (Stone path).", "Zones: 1 tile returns
to the default zone, meadow (Quiet meadow)." or, for empty cells given the
default zone explicitly, "Zones: 2 tiles now hold meadow (Quiet meadow)
explicitly (they showed it as the plane default)."

While a zone stroke runs, its live preview shows the result: the cells the
release would change are drawn in the target zone's tint at 80 % opacity,
twice the resting 40 %, in place of their resting tint (the default-zone row
shows the plane default's tint, or no tint when the plane has no default).
The terrain brush keeps its white mark, which draws over a zone tint of the
same cell when the overlay is toggled on.

#### Managing zones

With `editing.zones.manage` a zone row of the Zone mode list also chooses
the zone to manage (without `paint` it only does that, and the status line
says so), and the sidebar shows it below the list: its id, marked as fixed;
its display name, or "none" and the name it inherits; its parent; how many
tiles of the view show it, and how many of those only as the plane default
(an empty cell shows the default zone without holding it); how many tiles
hold it explicitly in the whole plane ("Held by 3 tiles in the plane"), every
chunk of every layer, not only the view; chunks whose zone palette lists it with no tile; its child zones;
whether it is the plane's default zone; and its `properties`, read-only, as
compact JSON. A zone draft and the parent list belong to the zone they were
opened for: choosing another zone, leaving Zone mode, or the zone ceasing to
exist in the shown World (deleted, or its creation undone) ends them, and the
choice itself is dropped when the zone no longer exists. The draft's hint
names the zone it edits ("Name of meadow; empty = none") or the parent a new
zone goes under. Each is an
edit of the plane's `zones.jsonc` reviewed as one World transaction, with
one recovery record when accepted and byte-exact Undo and Redo:

- **Zone ids are fixed.** Nothing changes an id; "Edit display name" opens
  the text draft with the current name (the single-line rules of field
  drafts apply, and stored line breaks show as markers). Review writes one
  `set` of `["zones", {"id": X}, "name"]` expecting the old name, or an
  absent `test` and a `set` when the zone had none. An empty draft means no
  name: the member is removed (expecting the old name) and the zone shows
  the name its parents give it, if any. A name has at most 256 characters,
  as the format says.
- **Parent.** "Parent: ... (change)" lists the plane's zones except the zone
  itself and its descendants, so a cycle cannot be chosen, plus "No parent
  (top level)". Like the zone list it shows the first 256 in id order and
  then says how many there are; it is computed when it opens and again when
  zone definitions change. A choice writes one `set` or `remove` of the `parent` member,
  guarded the same way. The Session refuses a parent that is unknown, the
  zone itself or one of its descendants on its own, whatever list was
  shown; World's validation stands behind it.
- **New zone.** "New zone under X" or "New top-level zone" opens a draft:
  type the id, then optionally a space and the display name (ids cannot hold
  spaces). The name part is trimmed, and only whitespace means no name; a
  display-name draft keeps its text exactly, as field drafts do. The id must be a World identifier (`^[a-z0-9][a-z0-9_.-]{0,127}$`)
  the plane does not use; the draft stays open until it is. Review writes an
  absent `test` of `["zones", {"id": new}]` and an `insert` at the end of the
  `zones` array, or `document.create` of `zones.jsonc` when the plane has
  none (Undo deletes that document again). A new zone holds no tile; it is
  in the list at once and the brush paints it.
- **Delete.** Only an unused zone can be deleted: no tile of any chunk on
  any layer holds it and no chunk's zone palette lists it, no zone has it as
  parent, and it is not the plane's `default_zone`. Otherwise the row is
  disabled with those facts shown above it and one "In use: ..." line per
  reason under it, and the Session refuses with each reason and its count
  ("zone is in use and cannot be deleted: 6 tiles hold it, in 3 chunks on 2
  layers; 1 child zone (meadow.pond); it is the plane's default zone"). A
  chunk palette that lists the zone with no tile (only a hand edit leaves
  one) is its own reason, naming the first three such chunks by layer and
  chunk ("layer 0 chunk 0_0", the chunk file `layers/0/0_0.jsonc`); to drop
  the entry, paint one tile of that chunk with the zone and then back: a
  stroke drops a zone's entry when its last tile in the chunk is emptied or
  repainted, while strokes of other zones keep it. Deleting writes one `remove` of the entry,
  expecting its exact canonical JSON. Deleting the last zone leaves an empty
  `zones` array; the document stays.

The review lists one neutral note per definition change, worked out by
comparing the zone definitions before and after, so Undo and Redo are
described too: "Renames zone meadow from 'Quiet meadow' to 'Open meadow'.",
"Names zone X 'N'.", "Removes the display name 'N' of zone X; it shows 'M'
from zone Y." (the zone up its chain the shown name now comes from),
"Moves zone X under Y.", "Makes zone X a top-level zone.", "Creates zone X
under Y, named 'N'." and "Deletes unused zone X.". Editing `properties` or
`custom`, setting the plane's default zone, and merging or splitting zones
are not offered; a profile whose `zone_properties` schema requires a
property makes World refuse a new zone, and the refusal is shown with
nothing pending.

### Anchors

An anchor is a named tile, `{id, layer, at}` in the plane's
`planes/<p>/anchors.jsonc`; a portal on an object can lead to one. Anchor
mode (the toolbar's Anchor, or K) shows them:

- **Badges.** Every tile of the view that holds an anchor gets a small light
  badge in its first corner (top left in the top-down sample), and every tile
  that holds an object with a portal, its own or one carried in its
  inventory, a small dark badge in its last corner (top right). Each badge has
  a 1 px outline of the other tone, so the light one reads on light ground and
  the dark one on dark ground; the outline is at least 1 px measured across
  each edge, the slanted edges of an isometric diamond included. A badge is
  the tile's own exact cell shape
  scaled down toward that corner. Badges are markers, not tints: they are
  drawn over objects, above all World content (after the last World quad,
  below the interface), so an object on the tile, even one that covers the
  whole tile such as a door, never hides one. They are unpickable: they never
  reach the semantic, depth or coverage planes, and
  a click on a badged tile still picks that tile. The badges show always in
  Anchor mode and, in the other modes, whenever the zone overlay shows (Zone
  mode, or after O). In Anchor mode O adds the zone tints. Anchors on other
  layers or outside the view are listed, not drawn. Badges need none of the
  World draw-order slots the zone tints use, so they still show when the zone
  overlay cannot be drawn.
- **List.** The sidebar names what the badges mean (each legend row carries
  its badge's colour) and lists every anchor of the plane, sorted by id: its
  layer and position, "in view" or "not in view", and how many portals in the
  whole world name it. Only the first 256 are listed, with the real total in
  the header. Ids come first and are elided in the middle when a row is too
  narrow, as zone ids are. A row chooses the anchor; the chosen one shows its
  fixed id, its place and the objects whose portals name it (the first three:
  uid or "(no uid)", plane, layer and tile, "carried" for an inventory entry).
- **Inspector.** A click in Anchor mode selects the tile under the pointer
  through the exact cell mask, objects standing on it included. In every mode
  a selected tile lists its anchors and how many portals name each, and a
  selected object with a portal shows its target: a plane, layer and tile, or
  an anchor id and where that anchor is (see Portals for editing it). A
  portal without a `plane` member targets its own object's plane.

Editing needs the catalog's optional `editing.anchors` member, `{"edit":
true}`: a capability flag, never a list of anchor ids, so the catalog keeps
loading when anchors change. Without it the editing rows are absent and the
status line says anchor editing is not granted. Every edit is limited to the
active view's plane (create and move to tiles of the view; delete works on any
anchor of the plane, in view or not, as zone deletion does), needs no pending change, and is one World transaction on
`anchors.jsonc` with one recovery record when accepted and byte-exact Undo
and Redo; the Session checks the grant, the plane, the view and the revision
again whatever the window offered, and World's validation stands behind it.

- **New anchor.** Select a tile of the view, press "New anchor at (x, y)",
  type the id and press Enter. The id must be a World identifier the plane does
  not use; the draft stays open until it is. The anchor is created on the
  selected tile and the view's spatial layer: an absent `test` of
  `["anchors", {"id": new}]` and an `insert` at the end of the `anchors`
  array, or `document.create` of `anchors.jsonc` when the plane has none
  (Undo deletes that document again). Two anchors may share a tile.
- **Move.** Choose an anchor that is on a tile of the view, select its new
  tile and press "Move here: (x, y)": one `set` of its `at`, expecting the old
  position (its `layer` stays). Every portal that names it now leads to the
  new tile; the review lists them.
- **Delete.** Only an anchor no portal names can be deleted. Portals are
  looked for everywhere: every plane, layer and chunk, placed objects and
  inventory entries at any depth, with or without a uid. Otherwise the row
  reads "Delete: 1 portal names it", disabled, under the list of those
  objects, and the Session refuses with the count and the first three
  ("anchor is named by portals and cannot be deleted: 1 portal: object
  gate_east on main layer 0 (7, 0)"). Deleting writes one `remove` of the
  entry, expecting its exact canonical JSON; deleting the last anchor leaves
  an empty `anchors` array.
- **Ids are fixed in this version.** Nothing renames an anchor. To get the
  same effect, create a new anchor, point the portals at it (see Portals),
  then delete the old one.

The review notes compare the anchors before and after, so Undo and Redo are
described too: "Creates anchor camp on layer 0 at (2, 3).", "Moves anchor
arrival from layer 0 (7, 3) to layer 0 (6, 2)." with "Portals naming it now
lead to the new tile (1 portal: object gate_east on main layer 0 (7, 0))." or
"No portal in the world names it.", and "Deletes anchor camp (layer 0, (3,
4)); no portal in the world names it.". A move or a delete also warns,
without blocking Accept, that scripts or saves outside the world may refer to
the anchor id. The counts, the lists of naming objects, the Delete row and
the badges follow the World shown: a pending portal change moves them
before it is accepted.

### Portals

A portal is the `portal` member of one object: `{"plane"?, "layer", "at"}`
leads to a tile, `{"plane"?, "anchor"}` to an anchor, and without `plane` it
leads into the object's own plane. Portals are one-ended: a way back is
another object with its own portal, so a target tile with no way back is
normal and nothing warns about it. Portals are edited one object at a time.

Select the object (in Select or Move mode a click picks it). The inspector
says where its portal leads: "Portal: none", "Portal: to main, layer 0, (-2,
2)", or "Portal: to anchor arrival" with the anchor's plane, layer and tile on
the next row ("Not defined in its plane" when the plane lacks it). With the
grant it then offers four rows; each edit is one World transaction on the
object, addressed by its uid, with one recovery record when accepted and
byte-exact Undo and Redo:

- **Target an anchor** opens a list under it: a hint naming the object, the
  number of anchors in the whole world, then the anchors grouped by plane, the
  view's plane first and the others in id order, each with its layer and tile.
  At most 256 are listed; then "First 256 anchors shown" follows the real
  total. Ids are elided in the middle when a row is too narrow. The anchor the
  portal names already is marked and disabled. A row prepares the change.
- **Target a tile in view** starts a pick step: the row reads "Click a tile
  in the view (Esc ends)" and the status line names the object. The next
  click on the view picks the tile under it through the exact cell mask,
  objects on it included, and prepares a portal to that tile on the view's
  spatial layer; the object stays selected. A click that hits no tile keeps
  the step open; Esc or pressing the row again ends it.
- **Type a location** opens a one-line draft: `layer x y` for the object's
  own plane, or `plane layer x y`; spaces or commas separate the values. The
  plane must exist, the layer is a 32-bit integer and x and y are integers
  within plus or minus 2^53 - 1, as World accepts. An error (and a target
  where the portal already leads) keeps the draft open with its text.
- **Clear portal** prepares its removal; disabled when there is none.

The written portal leaves `plane` out when the target is in the object's own
plane, as the format defaults it, and names it otherwise. Setting a portal on
an object without one is an absent `test` of `["portal"]` and a `set` that
adds the member at the end of the object; changing it is one `set` expecting
the committed portal's canonical JSON; clearing it is one `remove` expecting
it. World removes a last member with the separator before it, and a comment
between them goes with it; the rest of the file stays byte for byte.

An open list, pick step or typed draft is for one object and one World. It
ends with the next action of any kind (a choice, Accept, Cancel, Undo, Redo,
Reopen, another selection, Clear, deleting the object), a click on the view
outside a pick step, Esc, another step or draft, and a mode switch, so none
can act on an object that changed or is gone. The rows are disabled while a
change is pending.

The review notes compare every object's portal before and after, so Undo and
Redo are described too: "gate_east now leads to anchor lookout (main, layer
0, (-2, 0)); it led to anchor arrival (main, layer 0, (7, 3)).", "gate_west
now has a portal to main, layer 0, (4, 3).", "gate_west no longer has a
portal; it led to main, layer 0, (4, 3).". World checks that a target plane
and a target anchor exist, but not that a location target has a map chunk or
ground, so the review warns, never refuses, using the warnings' own review
allowance:

- "Warning: gate_west's portal target is in a map chunk that does not exist;
  World does not check portal targets."
- "Warning: gate_west's portal target has no ground on the floor grid; World
  does not check portal targets." The grid is the one tiles are picked on when
  the view opens: the first grid the catalog grants terrain editing on, else
  the profile's first ground grid.
- "Warning: gate_west's portal leads to the tile the object stands on." (for
  anchor targets too).

A location on another plane or layer is checked the same way in the loaded
World, which holds every plane, and the review adds that the view cannot
show it.

Editing needs `"portal": true` on the `editing.objects` entry of the object's
prototype. It reaches the same objects as that entry's deletion and moves:
top-level objects of the active view (the catalog's plane and spatial layer,
inside the view bounds). The target may be anywhere. An entry without
`portal`, or with `false`, grants no portal editing and the object shows its
target only; a catalog without `editing.objects` loads and behaves as before.
The Session checks again whatever the window offered: the grant, that the
object is placed (not carried in an inventory) and in the view, that no
change is pending, the source revision, that the target plane exists, that a
target anchor exists in that plane, and that the portal does not already
lead there; World's validation stands behind it, and a World refusal leaves
nothing pending. Objects without a uid cannot be selected, so their portals
are not editable here; neither are carried objects' portals or those of
objects on other planes or layers. Creating a pair of portals in one step is
not offered: set each end on its own object.

### Placing objects

Choose Place in the toolbar or press P (a focused text draft keeps P for
itself). The sidebar lists the prototypes the catalog's `editing.objects`
entries grant, one row each, sorted by prototype id, with the chosen one
marked; entering Place mode chooses the first row when none is chosen, and
clicking a row (its thumbnail included) chooses it. Without any entry, Place
is disabled and P reports why. Click a tile, empty or not: the exact cell-reference mask
picks it, as in Terrain mode. One click is one World transaction with one
`object.insert` on that tile of the active view (the catalog's plane and
spatial layer, inside the view bounds), appended on top of the tile's
stack. The new object carries a new uid, the entry's render `layer` and the
field values of the entry's optional `defaults`; World validates it against
the profile schema before the diff is shown, and refuses it, writing
nothing, when the defaults do not satisfy the schema. Nothing can be placed
while a change is pending.

A filter field sits at the top of the sidebar in Place mode, above the
scrolled rows. Click it, or press `/` (or keypad divide) in Place mode, to
focus it, then type; clicking always works, while `/` was only tested on
layouts where it needs no modifier. The palette keeps the prototypes whose
id contains the text, ignoring case, and its header counts the matches. Case is folded one character at a time with
Unicode's full lowercase mapping, without locale tailoring or context
(capital sigma always becomes σ, never final ς) and without normalization,
so a precomposed accent does not match a decomposed one; text still being
composed in an input method does not filter until it is committed. The field
takes the single-line rules of other inputs (control characters and U+2028
and U+2029 are refused) and holds at most 64 bytes of UTF-8. While it is
focused every key is text: P, Q, Delete and the pan and tool keys type
or edit, they do not act. Enter, or a click anywhere else, releases focus
and keeps the filter; the click also does what it normally does (a tile
click in Place mode places, a row click chooses, a toolbar click acts). Esc
clears the filter and releases focus. With the field
unfocused, Esc first clears a non-empty filter, and only the next Esc
cancels a pending change as before. The filter is view state only: it is
bound to no selection or World revision, never creates a transaction, and
never changes what may be placed. Under the filter field a pinned row, which
never scrolls and is not clickable, always names the chosen prototype in
full with its thumbnail, `Placing: crate`, so the choice stays visible when
the filter hides its palette row; it is dimmed while a pending change blocks
placing. A hidden choice stays chosen and a tile click still places it; its
palette row is marked only while shown. When nothing matches, one neutral
"No prototype matches the filter." row stands in for the list. The filter
stays editable while a change is pending, when the rows are disabled as
before. Re-entering Place keeps the filter; switching to another tool clears
it, so Place opens with the whole palette. With `--shared-string-field` the
shared text field edits the filter, by the same rules (see [Shared string
field](#shared-string-field)).

Each palette row is 40 logical pixels tall, and starts with a thumbnail of
its prototype: the prototype's ready preview texture from the catalog, the
same resource the World view draws, fitted into a 36-pixel square with its
aspect ratio kept, centred and snapped to whole physical pixels. Thumbnails
are extra textured quads in the composed workspace scene and need no
renderer change. Only rows laid out in the sidebar get one, and rows are
laid out only when they fit whole, so a thumbnail never reaches the toolbar,
the review panel or the World view; scrolling never cuts one in half. They
are not pickable and are not in the World scene, so the exact semantic masks
used for selection, painting and placement are unchanged by them; a press
on a thumbnail is a press on its row. Disabled rows show a grey, faded
thumbnail.

The uid is `o.` followed by 128 random bits from the operating system in
lowercase base32 (26 characters), as the World format recommends, so uids
placed on different branches do not collide. One that already names an
object in the World is drawn again, a bounded number of times. The scripted
demonstration uses a fixed, seeded sequence instead, so its evidence is
reproducible.

The review panel names the prototype, the tile and the new uid above the
diff, and warns, without refusing, when an object of the same prototype
already stands on the tile. Accept retains one inverse recovery record and
selects the new object. It is then an ordinary object of its prototype:
the same entry lets the author select, move, relabel and delete it. Undo
removes it with byte-exact source, and a placement that created a chunk
file is undone by deleting that file.

#### Stamps

A catalog may name stamps, groups of objects put down together, in an
optional `editing.stamps` member next to the grants. Each is a `name` and
its `objects`, each a prototype and its `offset` `[dx, dy]` in plane tiles
from the tile the stamp is put down on (its anchor):

```json
"editing": {
  "objects": [{"proto": "crate", "layer": "item"}, {"proto": "lamp", "layer": "item"}],
  "stamps": [
    {"name": "camp", "objects": [
      {"proto": "crate", "offset": [0, 0]},
      {"proto": "lamp", "offset": [1, 0]},
      {"proto": "crate", "offset": [0, 1]}
    ]}
  ]
}
```

A `name` is a World identifier of at most 64 bytes, unique in the catalog.
A stamp holds 1 to 64 objects, in the order they are placed and stacked;
each `proto` must be granted by `editing.objects`, and each offset is two
integers from −4,095 to 4,095. Objects may repeat and two may share a
tile, as the engine allows. A catalog holds at most 32 stamps. A catalog
that breaks any of these rules does not load, and its message names the
stamp and the object ("stamp camp object 2: stamp prototype is not
granted by editing.objects"); `lint` reports it as `catalog.invalid` with
that message, and warns (`catalog.stamp_unplaceable`) about a stamp that
no tile of the view can anchor with every object inside the view, in any
turn, which can never be put down whole ([lint findings](CLI.md#lint-findings)).
That is a stamp too wide or too tall for the view, or one whose objects lie
too far from the anchor: one crate at `[8, 0]` never fits an 8-wide view. The
member is the same in catalog formats 1 and 2: optional members may be
added within a version ([ADR 0004](../decisions/0004-project-art-in-the-preview.md)).
A catalog without it loads and behaves exactly as before, and an editor
from before stamps refuses a catalog that has it, as it refuses any
`editing` member it does not know ("unknown or missing catalog key"). The
`catalog` command and the MCP catalog tool do not list stamps; an agent
places a stamp with `place_objects` and its own objects
([MCP editing](MCP-EDITING.md)). The neutral sample has no stamps. The
choices behind the form and the keys are in
[ADR 0010](../decisions/0010-catalog-stamps.md).

In Place mode the sidebar lists the stamps after the prototypes, under
"Stamps: R turns, M mirrors", one row per stamp in name order with its
count first ("3 objects: camp"), so a long name is cut, never the count. The palette filter keeps the stamps whose name
matches, by the prototypes' rule, and the header then counts them
("Stamps: 1 of 3 match"). Choosing a stamp makes a tile click put the
whole stamp down instead of one prototype; choosing a prototype row goes
back to single objects. The pinned row under the filter then reads
"Stamping 3 objects: camp", and under it two buttons show and change the
layout: "Turn: 90°  R" and "Mirror: on  M". Choosing another stamp starts
it in its catalog layout; the chosen stamp and its layout stay chosen
across modes, as the prototype does. Stamps cannot be favourites yet: while a stamp is
chosen the favourites' star row reads "Star: a stamp is chosen" and is
disabled, and no favourite prototype row is marked. A favourite prototype
row then chooses its prototype and ends the stamp, as the catalog row
does.

**R and M** move the offsets, never an object's art, by one convention in
plane tiles about the anchor:

- **M mirrors** the layout across the anchor's column, the plane's `y`
  axis through the anchor: `[dx, dy]` becomes `[-dx, dy]`. The rule is on
  the grid, the same in both projections. Top-down the column is vertical
  on screen, so M is a left-right mirror; in an isometric view the column
  runs down-left on screen, so M flips across that diagonal, not across
  the screen's vertical.
- **R turns** it a quarter turn clockwise: `[dx, dy]` becomes `[-dy, dx]`,
  so `+x` goes to `+y`. Both projections draw `+y` a quarter turn clockwise
  from `+x` (top-down: right, then down; isometric: down-right, then
  down-left), so the turn is clockwise on screen.
- Each key acts on the layout as it is shown. Four R or two M give the
  catalog layout back, and the keys reach the eight turns and mirrors of the
  square. The buttons show the state as a turn applied after an optional
  mirror, so R then M reads "Turn: 270°" and "Mirror: on".

R and M turn and mirror only with a stamp chosen in Place mode; otherwise
R is Reopen and M does nothing, as before. To reach Reopen while
stamping, choose a prototype row or switch mode. In a blocked session (an
uncertain publication) nothing can be placed, so R keeps its old meaning,
Reopen, even with a stamp chosen; Reopen re-inspects the source but does
not unblock the session. M does nothing there, and the Turn and Mirror
buttons are disabled. A focused text draft or palette
filter takes R and M as text. A road is drawn only in Terrain mode, so a
stamp control is never laid out while one is drawn; its fence refuses them
all the same. R and M touch no World, so they work while a change is
pending or editing is paused. While a portal tile pick is open the next
click picks the portal's target, so no stamp is previewed or promised
then.

Over the map the stamp's preview follows the pointer: the tile of every
object a click would put down is marked in the brush preview's white, the
same copies of the exact cell quads, and the status line says where and how
many ("Stamp camp at (3, 2): 3 objects; click to review."). Objects that
would land outside the view are not drawn and are counted instead ("Stamp
camp at (7, 5): 2 of 3 objects outside the view."). The preview is the
click's own action, so the marked tiles are exactly the recorded ones.

A click prepares one `place_objects` change anchored on the clicked tile,
with the turned and mirrored offsets ([ENGINE.md](ENGINE.md#stamps)), and
goes to Review like any edit: one transaction, one review, one undo step,
the same uids and bytes as the same objects placed by an agent. Every
refusal and warning is the engine's, in its own words: a stamp that
reaches outside the view is refused whole and prepares nothing; one whose
objects land on occupied tiles shows the engine's warning first in the
review and is still accepted. While a change is pending the rows are
disabled, the pinned row is dimmed and a click prepares nothing, and while
editing is paused nothing is previewed and a click is refused with the
pause's reason, as for a prototype. An accepted stamp leaves the selection
as it was.

To try it by hand on a temporary copy of the neutral sample (the sample
itself has no stamps): copy `crates/tilewright-editor-testkit/fixtures/prototype-neutral`
somewhere outside the checkout, add the `stamps` member above to the copy's
`preview.jsonc` `editing` section (its `objects` already grant crate and
lamp), and provision the copy. In Place mode choose "3 objects: camp",
hover the map and press R and M to watch the marked tiles turn and flip,
click a tile, read the review, Accept, then Undo.

Planned edits are visible before acceptance. Accept retains the exact inverse
before World publication; Cancel publishes nothing. Undo and redo prepare World
transactions and require acceptance through the same path. A pending plan must
be accepted or cancelled before another edit, undo or redo is prepared. Valid
semantic selections survive acceptance. Native input cannot accept a pending
change before its current frame has been presented. An uncertain
applied/durable/cleaned result blocks further editing and preserves its record.
Reopen is explicit inspection/reload, never blind recovery or retry.

Some views cannot be drawn: steep heights under isometric ground art taller
than its diamond, for example, cannot be bent at a high zoom, and the
renderer refuses the whole view. A zoom or pan into such a view is refused:
the view stays at its last good zoom and position, the status line says
"Cannot zoom in: this view cannot be drawn." (or zoom out, or pan there),
the details say the view stayed where it was and give the renderer's
reason, and editing continues. A later refused input's notice replaces that
line; the next zoom, pan or control clears it.

Rendering can still pause editing, and the status line and the details say
which of three causes it is and how it ends. An edit or a proposal step
whose result cannot be drawn at the current view is cancelled when it is
prepared ("Change refused: it cannot be drawn at this view, so it was
cancelled.", see [Height range](#height-range)), so a pause from a
pending change only arises when the view changes after the change was
prepared. When the session's World itself cannot be drawn at the current
view ("Editing paused: this view cannot be drawn."), zooming out, panning or Cancel (key or button) rebuild
the frame; while paused, zoom and pan steps are kept even when their view
does not draw either, so stepping out one key at a time works. When the
session's catalog or its art changed on disk ("Editing paused: catalog or
art files changed."), editing resumes only when the files match what the
session loaded again: put them back, then zoom or pan once to check again
(nothing checks them on its own); otherwise quit and start a new session.
Cancel still drops a pending change there and says "Pending change
cancelled. Editing stays paused." When the frame could not be composed or
shown ("Editing paused: the frame could not be shown."), the next zoom, pan
or Cancel tries again. Every other action, Accept, Undo, Redo, Reopen, map
clicks and the proposal's control included, is refused with the pause's
reason. The first rebuild that succeeds ends the pause and says "The view can be drawn again. Editing is possible again."; an open
proposal's panel and highlight come back with it. The window title reads
"editing paused" meanwhile. This is separate from a blocked session (an
uncertain publication), which only Reopen ends.

The first external-map edit moves an eligible UID-bearing top-level object only
when its original block has one existing `x` and `y`, within original bounds.
Same-chunk coordinate changes use guarded generic World path transactions to
preserve source object order. Cross-chunk relocation still uses World's
`object.move` and may exceed the adapter's export allowlist; the exporter refuses
unsupported structural/order changes. Other fields, ordering and bytes are
preserved by the adapter's checked export.
The neutral sample also exposes a typed field and terrain paint operation.
External-map floors remain objects; no generic terrain capability is inferred.

Export is a separate explicit toolkit CLI operation on committed World data.
Pending scenes are never export input. Keep the original content tree read-only
and export into a new task-owned output. See the adapter's own usage and tests
for its manifest/stale-source/unsupported-edit rejection contract.

## Project art

The preview draws the resources its `preview.jsonc` catalog lists: raw
RGBA8 images in `preview-assets/*.atx1` files inside the world. The neutral
sample's are original MIT art. A project may also show **its own art**, under
its own license, by the owner's decision of 2026-11-17
([ADR 0004](../decisions/0004-project-art-in-the-preview.md)). This is not
legal advice: the user is responsible for their art's license, for every
copy the editor makes of it (below), and for where those copies go.

**The catalog.** Format `tilewright.editor-preview/2` is format 1 with one
change: a resource's `license` may be `MIT` or `project`, exactly so and
nothing else. `project` declares art the world's project supplies under its
own license; `author` and `notice` are still required and kept as written.
The value is self-declared: the editor cannot check it. A catalog may mix
both values. A format 1 catalog keeps accepting only `MIT`, and refuses
`project` with its usual error. An editor that knows only format 1 refuses
every format 2 catalog ("unsupported preview format"), so an older editor
never shows project art unmarked. A format 2 catalog with only `MIT`
resources loads, draws and reports exactly as the same catalog in format 1.
The version rule: a format tag is a promise about what a catalog may hold,
so a change that weakens it needs a new version. Any new `license` value
does: format 2 means "every resource is `MIT` or `project`", nothing else.
Optional members may be added within a version, as the `editing` members
have been, because every format member is closed (only user-keyed maps such
as `defaults` hold names the format does not list): an older editor refuses
a catalog with a member it does not know.

**What a project does.** Its own tools, outside this repository, write the
art as ATX1 files and the catalog entries. The editor has no image decoder
and converts nothing. An ATX1 file is the four bytes `ATX1`, the width and
the height as little-endian unsigned 32-bit integers, then `width × height`
pixels of four bytes each (red, green, blue, alpha), tightly packed, row by
row from the top, as the released `tilewright-render-resources` 0.7.2 crate
defines the envelope. Width and height are at least 1. Alpha is straight,
not premultiplied: both renderers blend the stored colour by its alpha
(source over), and the values are used as stored, with no gamma conversion
(the GPU textures are `Rgba8Unorm`), so write the colours as they should
appear; the repository's
`crates/tilewright-editor-testkit/fixtures/prototype-neutral/generate_assets.py`
writes the sample's.
The catalog entry gives the file's lowercase SHA-256, which the loader
checks, and its width and height, which must match the header. Limits: a
side of at most 8,192 pixels; 16 MiB (16,777,216 bytes) per file, the
12-byte header included, so at most 4,194,301 pixels per image (a 2,048 ×
2,048 image, 12 bytes over, is refused; 2,048 × 2,047 fits); 64 MiB of
resource files and 1,024 resources per catalog; a catalog of at
most 1 MiB. The provisioning helpers copy the whole world, `preview-assets/`
included, within their own limits (4,096 entries, 64 MiB, depth 32), so
uncompressed art reaches them sooner than compressed files would. Each
prototype is drawn with one whole resource, anchored `ground` or `standing`,
on one tile: there are no atlas frames, anchor offsets or multi-tile
footprints yet.

**What "project art is loaded" means.** The loaded catalog has at least one
resource with the license `project`, whether or not any is drawn in the
current view or region. The editor records nothing about the art beyond
that count; the catalog revision already identifies the catalog's text.

**What is marked.** While project art is loaded:

- the desktop shows `Project art loaded` on the review row, beside Accept
  and Cancel, in every mode;
- every evidence frame (`<name>.png`) carries the project-art mark, and its
  `<name>.json` record gains `"project_art": {"resources": N,
  "mark_rows": R}`;
- every `render --out` file and every MCP `render` image carries the same
  mark, and its report `catalog.project_art` and `image.mark_rows`
  ([docs/CLI.md](CLI.md#render), [docs/MCP.md](MCP.md#project-art));
- `overview`, `tile`, `catalog` and `lint` report `catalog.project_art`, and
  lint a `catalog.project_art` info finding, so a reader knows before
  rendering.

The mark is one rule for every PNG the editor makes: rows added below the
image, never drawn over it, so the image's own pixels and coordinates are
unchanged. They are a dark separator and an amber band reading `PROJECT
ART: CHECK ITS LICENSE BEFORE SHARING` in black capitals from a fixed
bitmap font (`PROJECT ART` on an image narrower than 276 pixels), scaled
with the width: 12 rows on a narrow image, 24 on a 720-pixel evidence frame,
48 from 1,104 pixels. An image narrower than the 72 pixels `PROJECT ART`
needs (a one-tile `render`, say) is widened to 72 pixels with band-coloured
columns right of it, its full height, so the mark always reads whole; its
own pixels stay at the top left. Without project art nothing is marked and every frame,
record and report is as before.

**Where copies go.** The editor never bundles, uploads or copies project
art anywhere but the session and its outputs: the provisioned session copy
and sandbox copies (which persist until the user deletes them), evidence
frames, `render` files, and the images the MCP servers return to their
client. MCP `render` is always allowed: with an AI agent as the client, its
images usually go to a model provider, and whether the art's license allows
that is the user's to check. The read-only commands and the read-only MCP
server read the world in place, without a session copy.

**Never in these repositories.** Project art never enters this repository,
its fixtures, goldens, docs, issues or release packages; please do not
attach marked frames to issues. Tests use generated pixels, or the MIT
sample labelled `project` in a temporary copy of its catalog. Foundation
checks refuse any committed catalog, in any directory, that is not plain JSON
with only `MIT` licenses, and any committed `.atx1` file the asset manifest
does not list; packaging refuses a sample catalog that is not MIT only.

## Evidence and limits

`--demo-script` drives the same session actions, command history and scene
preparation as interactive input, including a placement with seeded uids and
a palette search (it types `A`, captures `palette`, then places the first
match) and, after the terrain step, a 3x3 sand drag stroke over grass and
sand captured mid-gesture as `stroke-preview` and then cancelled with Escape,
so it changes nothing. When the catalog grants zone painting it ends in Zone
mode: `zones` shows the overlay and legend, then a one-cell `path` stroke down
column 5 is captured mid-gesture with its live preview
(`zone-stroke-preview`), released (`zone-stroke`, pending review) and
accepted (`zone-painted`). When it also grants zone management, it then
renames `meadow` to "Open meadow" (`zone-rename`, pending), creates
`meadow.grove` named "Grove" under it (`zone-create`, pending), paints the
tile (2,4) with it (`zone-created`), shows that it cannot be deleted while
that tile holds it (`zone-delete-refused`: the Delete row disabled with its
reason, as the window shows it; a press on a disabled row does nothing),
undoes the paint and deletes it
(`zone-delete`, pending; `zone-deleted`). When the catalog grants anchor
editing it then enters Anchor mode (`anchors`: the badges and the list),
selects the tile (2,3) and creates the anchor `camp` there (`anchor-create`,
pending; `anchor-created`), selects (3,4) and moves `camp` there
(`anchor-move`, pending), shows that `arrival`, which the portal of
`gate_east` names, cannot be deleted (`anchor-delete-refused`: the Delete row
disabled under the object that names it), and deletes `camp`
(`anchor-delete`, pending; `anchor-deleted`). When an `editing.objects`
entry grants `portal`, it then selects `gate_east` (`portal`), opens its
anchor list (`portal-anchors`) and retargets it to `lookout`
(`portal-retarget`, pending; `portal-retargeted`); selects `gate_west`,
starts the pick step (`portal-pick`) and clicks the gate's own tile
(`portal-tile-warning`: pending with the own-tile warning, then cancelled),
starts it again and clicks (4,3) (`portal-tile`, pending;
`portal-tile-set`), and clears that portal (`portal-clear`, pending;
`portal-cleared`). The scripted move and deletion earlier take objects whose
portal is not editable, so the gates are left for this step. When the
catalog grants terrain it ends in Terrain mode: with Fill and the last
palette value it hovers (1,1) (`fill-preview`); with Replace and the first
value it hovers (5,1) (`replace-preview`), clicks there (`replace-pending`)
and cancels, so it changes nothing; the brush rule and terrain choice are
restored. Those frames' JSON has a `fill_preview` member: the rule, start
cell, terrain and how many cells are marked. When the catalog names a
scatter palette it then chooses the first (`speckle` in the sample) and
drags a 3x3 brush from (1,1) to (2,4), over 20 cells of the grass columns,
capturing the live preview mid-gesture (`scatter-preview`; the status line
names the seed), the pending review (`scatter`) and the accepted result
(`scattered`); the brush and the terrain choice are restored. Its seed comes
from the same seeded source as the placements' uids, so the drawn values
are the same on every run. It then draws a road: with sand, the Road rule
and a one-cell brush it clicks (0,1), (3,1) and (3,5) (`road-preview`,
whose JSON has a `road` member with the points, the brush, the covered and
changed cells, or, while editing is paused, `refused` with the pause's
words in place of the changed cells), presses Enter (`road`, pending) and accepts (`road-built`);
the rule and the brush are restored. The sample grants no height range, so the
demonstration does not sculpt ([Sculpt mode](#sculpt-mode)) or grade the road
([Grading a road](#grading-a-road)). It is a
deterministic scripted demonstration,
not a claim of human input. `--reference` produces composed workspace CPU
reference evidence and
must be reported separately from an actual visible native window run. Native
presentation/backend/adapter evidence cannot be inferred from passing unit tests
or CPU frames. Screenshot/video artifacts must identify which path produced
them and the exact source revision and dependencies.

Packages include the exact allowlisted OFL Unicode font pack and authored MIT sample data/resources, immutable
registry references, notices, checksums, SBOM and provenance, and at their top
level this guide and the guides it links to (`SANDBOX.md`, `MCP-EDITING.md`,
`PROPOSALS.md`, `ENGINE.md`, `MCP.md`, `CLI.md`); the design records they
link to are not included. Imported worlds,
real game maps/art, adapter baseline, recovery records and session evidence
are excluded, as is any project art ([Project art](#project-art)); the
packaged sample's catalog declares only `MIT` resources, which packaging
checks. Windows build/test/package success is not Windows durable-editing
or native D3D12 qualification.

Later work includes a full widget/text-editing system, arbitrary-root
provisioning APIs, automatic crash replay, persistent undo, additional
external-map edit forms, original art/effects qualification, broader platform
durability and game-runtime cutover. Each needs its own evidence and
authority; none is implied by this prototype.

## Explicit Unicode font preparation

The native editor requires `--font-pack ROOT`. Binary packages carry the fixed
`font-pack/` directory beside `bin/`; the provisioner forwards that explicit
absolute root. It checks the exact immutable manifest and all font/notice
bytes before creating a session. It does not search operating-system fonts or
fetch anything. Source candidate provisioning records local byte verification
and `registry_proof: false`, independently of released-consumer evidence.

Prepare fonts as a separate explicit build step after `cargo fetch --locked`:

```sh
python3 tools/prepare-font-pack.py prepare --root . --acquire \
  --output /absolute/new-prepared-font-pack
tools/package-linux.sh /absolute/new-dist 0.1.0-test.1 \
  /absolute/new-prepared-font-pack
TILEWRIGHT_PREPARED_FONT_PACK=/absolute/new-prepared-font-pack tools/validate.sh
```

Preparation obtains the manifest only from the exact checksum-verified registry
`tilewright-render-ui` crate. Public upstream downloads use the manifest's
fixed revision, paths, sizes and hashes, with redirects, proxy inheritance,
cookies and authentication disabled. Packaging verifies the prepared pack
again, copies every original OFL notice, records the binding to the UI archive,
and checks font data identities/hashes/licenses in the binary CycloneDX SBOM.
A missing pack or notice, extra file, altered payload, link, changed manifest
or local dependency override refuses production packaging. Preparation refuses
existing destinations; inspect and remove failed partial preparations explicitly.

For a local source candidate, stage a separate review artifact explicitly:

```sh
python3 tools/prepare-font-pack.py candidate --root . \
  --candidate-ui /absolute/renderer/crates/tilewright-render-ui \
  --font-pack /absolute/renderer/assets/unicode --stage /absolute/candidate-stage
```

This mode labels its provenance as a local candidate and cannot pass the
production registry verifier. The public UI crate carries only the exact
423,554-byte metadata manifest; the approximately 39.6 MB unmodified font
payloads are distributed separately and loaded from the explicit pack root.
The pack covers a finite repertoire, including common Latin/Greek/Cyrillic,
Arabic/Hebrew, the selected Indic scripts, Thai, SC CJK, Korean Hangul and
monochrome emoji. Missing scalars and shaping/variation sequences still require
diagnostics; this is not a promise of universal font coverage.
