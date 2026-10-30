# Third-party notices

The editor source is MIT. Its exact dependency graph is recorded by
`Cargo.lock`, checked by `cargo-deny`, and included in each CycloneDX release
SBOM.

- `tilewright-source`, `tilewright-edit` and `tilewright-jsonc` 0.1.0
  (tilewright-world): MIT.
- `tilewright-scene` and `tilewright-render-*` 0.5.1 (tilewright-renderer): MIT.
- `sdl3` Rust bindings: MIT.
- SDL native library: Zlib License.
- `libloading` (native graphics loading): ISC License; its notice remains in
  the dependency source and generated SBOM.
- `unicode-ident` (Rust macro identifier tables): MIT/Apache-2.0 and Unicode
  License v3; its Unicode data notice remains in the dependency source.
- `wgpu` and graphics/shader support: MIT or Apache-2.0, with exact versions
  and selected alternatives recorded by the dependency license check/SBOM.
- `png`, compression support, SHA-256 and JSON encoding: MIT-compatible
  alternatives recorded by the exact locked dependency graph.

The bundled `samples/prototype-neutral` data and standalone ATX1 resources are
original MIT geometric fixtures by Seraphina Studios. Exact source, author,
license, transformation and SHA-256 records are in `provenance/assets.json`.
No external Classic map or original game artwork is included.

Authored content opened by the editor is not part of this software and retains
its own license and attribution.
