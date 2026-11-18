# ADR 0004: a project's own art in the preview

Status: accepted, 2026-11-17 (issue #56).

The preview catalog accepted only MIT resources, so a world imported from a
real project showed coloured placeholders, and the only way to see its art
was to label that art `MIT`, which would be false. Issue #56 asked whether
the editor may draw art a project supplies under its own license, provided
it never enters these repositories.

## Owner decisions, 2026-11-17

1. **Allowed, staged, project pack first.** The project's own tools write
   the raw-pixel resource files (ATX1) the editor already loads; the editor
   gains an honest `project` license value in a new catalog format. No image
   decoder in the editor for now; an offline converter may follow.
2. **Evidence frames and CLI `render` files are allowed** while project art
   is loaded, visibly marked, with a member in their records.
3. **MCP `render` images are always allowed** while project art is loaded;
   the user is responsible for their art's license. (The note before the
   decision recommended an operator start flag; the owner chose otherwise.)
4. **Never in these repositories.** Screenshots or videos showing a
   project's art never appear in these repositories, their docs, issues or
   releases; each project decides for its own channels.

## Implementation decisions (this PR, not the owner's)

These were chosen by the implementing agent and the coordinating session
on 2026-11-17 to carry out the owner's decisions; the owner can revisit
any of them.

- Catalog format `tilewright.editor-preview/2` is format 1 with one change:
  a resource's `license` is `MIT` or `project`, a closed set. Format 1
  keeps accepting only `MIT`. An editor that knows only format 1 refuses
  format 2, so no older editor shows project art without the marks. Mixed
  catalogs are allowed; `author` and `notice` stay required.
- Versioning rule: a format tag is a promise about what a catalog may
  hold, and a change that weakens it needs a new version. **Any new
  `license` value needs a new format version**: format 2 means "every
  resource is `MIT` or `project`", and tools other than the editor may rely
  on that. Optional members may be added within a version, since every
  format member is closed and an older editor refuses a member it does not
  know. (Not every object is closed: user-keyed maps such as an object
  grant's `defaults` hold profile field names the format does not list.)
- "Project art is loaded" means the loaded catalog has at least one
  `project` resource, drawn or not. The catalog and the prepared preview
  expose that count; nothing else about the art is recorded.
- One mark for every PNG the editor makes (evidence frames, `render` files,
  MCP `render` images): rows added below the image, never over it, so the
  image's own pixels and coordinates stay as they were. An image narrower
  than the 72 pixels the short text needs is widened to 72 pixels with
  band-coloured columns right of it, so the mark always reads whole
  (coordinator's choice after review, 2026-11-17); the columns are counted
  like the rows. The text, `PROJECT
  ART: CHECK ITS LICENSE BEFORE SHARING`, names no game and makes no claim
  about the license; it is drawn with a fixed bitmap font in the preview
  crate, the same on every platform and without a font file. The added rows
  count against the image bounds. The records and reports gain
  `project_art` / `catalog.project_art`, `image.mark_rows` and, on a
  widened image, `image.mark_columns`, only when
  marked, so every output of other worlds is unchanged.
- Reports that describe the catalog state the count, and lint has a
  `catalog.project_art` info finding, so a reader knows before rendering.
  The desktop shows a persistent label while project art is loaded.
- Tests use generated pixels or the MIT sample labelled `project` in a
  temporary copy. The foundation checks refuse any committed catalog, in
  any directory, that is not plain JSON with only `MIT` licenses, and any
  committed `.atx1` file the asset manifest does not list; packaging
  refuses a sample catalog that is not MIT only.

## Not decided here

An offline converter from common image formats, atlas frames, anchor
offsets and multi-tile footprints, and copying art from outside the world
tree into a session. Each needs its own issue and decision.

[The prototype guide](../docs/OFFLINE-PROTOTYPE.md#project-art) documents
the format, the marks and the user's responsibilities. This is not legal
advice.
