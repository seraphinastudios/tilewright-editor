#!/usr/bin/env bash
set -euo pipefail

metadata=$(mktemp /tmp/tilewright-editor-metadata.XXXXXX)
trap 'rm -f -- "${metadata}"' EXIT
cargo metadata --locked --offline --format-version 1 >"${metadata}"

jq -e '
  def deps($name): [.packages[] | select(.name == $name) | .dependencies[].name] | sort;
  deps("tilewright-editor-project") == [] and
  deps("tilewright-editor-document") == ["atrinik-source"] and
  deps("tilewright-editor-commands") == ["atrinik-source","atrinik-transaction"] and
  deps("tilewright-editor-ui") == ["tilewright-editor-project"] and
  deps("tilewright-editor-preview") == ["atrinik-render-api","atrinik-render-resources","atrinik-render-testkit","atrinik-scene"] and
  deps("tilewright-editor-testkit") == ["tilewright-editor-project"] and
  deps("tilewright-editor") == ["sdl3","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-preview","tilewright-editor-project","tilewright-editor-ui"] and
  ([.packages[].name | select(test("(client|protocol|server)"; "i"))] | length == 0) and
  all(.packages[].dependencies[];
    (.source // "") as $source |
    ($source == "" or
      $source == "registry+https://github.com/rust-lang/crates.io-index"))
' "${metadata}" >/dev/null

if grep -RnE --include='*.rs' \
  'std::(fs|net|process::Command)|tokio::|fn[[:space:]]+(parse|serialize|write|save)[[:space:]]*\(' \
  crates >/dev/null; then
  echo "editor source duplicates a forbidden parser/writer or direct I/O authority" >&2
  exit 1
fi
