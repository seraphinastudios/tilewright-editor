# Provenance

All implementation, tests, documentation, and synthetic fixtures in this
repository are MIT work by Seraphina Studios. No outside implementation, tests,
configuration, or fixtures are copied or adapted. Dependency font data retains
its own license as recorded below.

Independent implementation remains the default. Before any outside source is
inspected as implementation reference or reused, confirm that its license is
compatible with MIT and record the exact source, revision, destination,
transformation, and notice here. GPL/AGPL material is never copied, adapted, or
used as a build or runtime dependency.

`provenance/reuse.json` records reused material (none), and
`provenance/assets.json` is the exact authored neutral-sample asset allowlist.
Every listed standalone ATX1 resource records its generator source, author,
MIT license, digest, dimensions, transformation and notice. Foundation checks
verify the payload and reject unlisted resources or paths outside the sample.
The package data allowlist contains only this original neutral World fixture;
external imported game maps/art, recovery and task state are excluded.

Authored project content retains its individual license. Opening, previewing,
editing, validating, or packaging it does not apply this repository's MIT
license. Attribution and unknown licensing fields remain lossless data.

Dependency notice text in `THIRD_PARTY_NOTICES.md` is retained verbatim from
`libloading` 0.8.9 `LICENSE` (ISC; crates.io archive SHA-256
`d7c4b02199fee7c5d21a5ae7d8cfa79a6ef5bb2fc834d6e9058e89c825efdc55`)
and `unicode-ident` 1.0.27 `LICENSE-UNICODE` (Unicode-3.0; archive SHA-256
`a2c754d6c33795a1c324727428e5a7dedb5b06195f9890bdbcba760d3e246563`).
Transformation: verbatim license notice inclusion for binary/source package
attribution; no dependency implementation is copied into editor source.

## Shared renderer UI font candidate

The package policy prepares attribution for `tilewright-render-ui` **0.6.0**
only. This is a reviewable common-Latin font candidate; this record does not
claim owner acceptance, publication, or a released consumer migration. Production
Cargo pins remain the immutable released graph until that separate transition.

The shared renderer implementation is MIT. Its bundled Roboto Mono source,
bitmap atlas, metrics and generated glyph data retain **OFL-1.1**; the crate
therefore declares `MIT AND OFL-1.1`. The source is the unmodified
[`fonts/ttf/RobotoMono-Regular.ttf`](https://github.com/googlefonts/RobotoMono/blob/895ec691990d041dd727c7b5afa3ce56525d98e6/fonts/ttf/RobotoMono-Regular.ttf)
at `googlefonts/RobotoMono` revision `895ec691990d041dd727c7b5afa3ce56525d98e6`,
SHA-256 `af0bff7599c3df3831755c16e39b3c496df74b8c8d8a1161b14dc8461be17cb4`.
Copyright 2015 The Roboto Mono Project Authors
(https://github.com/googlefonts/robotomono). The exact upstream
[`OFL.txt`](https://github.com/googlefonts/RobotoMono/blob/895ec691990d041dd727c7b5afa3ce56525d98e6/OFL.txt)
has SHA-256 `50ab8dd54680d3473f649c9db86fece88434d097c7834475c1c72d2f8c429215`
and declares no Reserved Font Names after the copyright statement.

Transformation occurs in the renderer's independently authored MIT tooling:
337 selected common-Latin/symbol scalars are rasterized offline at 20 px with
Pillow 11.1.0 / FreeType 2.13.3, independent scalar masks without shaping or
kerning, into the immutable ATX1 atlas and metrics. Renderer
`assets/manifest.json` records exact source, author, licenses, tool image,
options and output hashes. The synthetic white texture remains MIT. The editor
consumes those immutable resources through the shared renderer and performs no
font lookup, parsing, rasterization, shaping or fetching.

When the locked released graph contains this UI crate, binary packaging copies
its exact upstream notice to `licenses/OFL-RobotoMono.txt`. The font notice
helper uses offline Cargo metadata, exact manifest/lock coordinates, the
registry archive checksum and unpacked Cargo file checksums, plus the reviewed
asset manifest/source/notice hashes in `policy/font-notice.json`. Package
provenance records that registry coordinate and the exact embedded asset and
notice hashes; it refuses missing or altered staged notices. Local candidate
paths/patches are refused by production packaging. The renderer checker owns
source cmap/metrics validation; this editor helper only verifies the immutable
producer assets and attribution. The original neutral-sample asset allowlist
is independent of these dependency assets and remains exact.

The cargo-deny OFL exception uses the exact package specification
`tilewright-render-ui@0.6.0`, following the pinned
[cargo-deny 0.20.2 license exception](https://github.com/EmbarkStudios/cargo-deny/blob/0.20.2/docs/src/checks/licenses/cfg.md#the-exceptions-field-optional)
and [exact PackageSpecs syntax](https://github.com/EmbarkStudios/cargo-deny/blob/0.20.2/docs/src/checks/cfg.md#exact).
No global OFL allowance is added.
