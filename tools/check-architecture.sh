#!/usr/bin/env bash
set -euo pipefail

metadata=$(mktemp /tmp/tilewright-editor-metadata.XXXXXX)
trap 'rm -f -- "${metadata}"' EXIT
cargo metadata --locked --offline --format-version 1 >"${metadata}"

jq -e '
  def deps($name): [.packages[] | select(.name == $name) | .dependencies[].name] | sort;
  deps("tilewright-editor-project") == [] and
  deps("tilewright-editor-document") == ["tilewright-editor-testkit","tilewright-source"] and
  deps("tilewright-editor-commands") == ["tempfile","tilewright-edit","tilewright-editor-document","tilewright-editor-testkit","tilewright-source"] and
  deps("tilewright-editor-ui") == ["getrandom","serde_json","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-preview","tilewright-editor-project","tilewright-editor-testkit","tilewright-jsonc","tilewright-source","unicode-segmentation"] and
  deps("tilewright-editor-observe") == ["serde_json","tempfile","tilewright-editor-document","tilewright-editor-preview","tilewright-editor-testkit","tilewright-editor-ui","tilewright-source"] and
  deps("tilewright-editor-mcp") == ["serde_json","tempfile","tilewright-editor-document","tilewright-editor-observe","tilewright-editor-preview","tilewright-editor-testkit","tilewright-jsonc","tilewright-source"] and
  deps("tilewright-editor-preview") == ["serde_json","sha2","tempfile","tilewright-jsonc","tilewright-render-api","tilewright-render-resources","tilewright-render-testkit","tilewright-render-world","tilewright-scene","tilewright-source"] and
  deps("tilewright-editor-testkit") == ["serde_json","sha2","tilewright-editor-project","tilewright-source"] and
  deps("tilewright-editor-proposal") == ["serde_json","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-preview","tilewright-editor-testkit","tilewright-editor-ui","tilewright-jsonc","tilewright-source"] and
  deps("tilewright-editor-sandbox") == ["serde_json","tempfile","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-observe","tilewright-editor-preview","tilewright-editor-proposal","tilewright-editor-recovery","tilewright-editor-testkit","tilewright-editor-ui","tilewright-jsonc","tilewright-source"] and
  deps("tilewright-editor-mcp-sandbox") == ["serde_json","tempfile","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-mcp","tilewright-editor-observe","tilewright-editor-preview","tilewright-editor-proposal","tilewright-editor-recovery","tilewright-editor-sandbox","tilewright-editor-testkit","tilewright-editor-ui","tilewright-jsonc","tilewright-source"] and
  deps("tilewright-editor-recovery") == ["sha2","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-testkit","tilewright-source"] and
  deps("tilewright-editor") == ["png","pollster","sdl3","serde_json","sha2","tempfile","tilewright-edit","tilewright-editor-commands","tilewright-editor-document","tilewright-editor-mcp","tilewright-editor-mcp-sandbox","tilewright-editor-observe","tilewright-editor-preview","tilewright-editor-proposal","tilewright-editor-recovery","tilewright-editor-sandbox","tilewright-editor-testkit","tilewright-editor-ui","tilewright-render-api","tilewright-render-resources","tilewright-render-sdl3","tilewright-render-testkit","tilewright-render-ui","tilewright-render-wgpu","tilewright-render-world","tilewright-scene","tilewright-source"] and
  ([.packages[].name | select(test("(client|protocol|server)"; "i"))] | length == 0) and
  all(.packages[].dependencies[];
    (.source // "") as $source |
    ($source == "" or
      $source == "registry+https://github.com/rust-lang/crates.io-index"))
' "${metadata}" >/dev/null

# Integration tests alone may create disposable source trees and inject faults.
# Production crates (including the testkit library) retain the no-I/O contract.
if grep -RnE --include='*.rs' --exclude-dir=tests --exclude=tests.rs \
  'std::(fs|net|process::Command)|tokio::|fn[[:space:]]+(parse|serialize|write|save)[[:space:]]*\(' \
  crates >/dev/null; then
  echo "editor source duplicates a forbidden parser/writer or direct I/O authority" >&2
  exit 1
fi

# The read-only reports, and the MCP server that serves them, never reach
# an editing session, history, recovery, proposals, the sandbox runner or
# provisioning: not even through the ui crate that exports them. A missing
# source directory fails rather than passing an empty search.
read_only=(crates/tilewright-editor-observe/src crates/tilewright-editor-mcp/src)
for directory in "${read_only[@]}"; do
  test -d "${directory}" || {
    echo "missing ${directory}" >&2
    exit 1
  }
done
if grep -RnE --include='*.rs' \
  '\b(Session|History|DurableRecovery|RecoveryStore|Proposal|ProposalStore|Sandbox)\b|tilewright_edit::|tilewright_editor_(commands|proposal|recovery|sandbox|ui)\b|provision' \
  crates/tilewright-editor-mcp/src | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' >/dev/null; then
  echo "the read-only MCP server reaches the ui crate, editing, history, recovery, the sandbox or provisioning" >&2
  exit 1
fi
if grep -RnE --include='*.rs' \
  '\b(Session|History|DurableRecovery|RecoveryStore|Proposal|Sandbox)\b|tilewright_edit::|tilewright_editor_(commands|proposal|recovery|sandbox)|provision' \
  "${read_only[@]}" | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' >/dev/null; then
  echo "the read-only observe crate or MCP server reaches editing, history, recovery, proposals, the sandbox or provisioning" >&2
  exit 1
fi
# The sandbox runner is held to the same import rule below: it reaches files
# only through the World roots the binary hands it (`SourceRoot`, and the
# world's path, which it passes to `WorldDocument::open_prototype`), and it
# names no file, network or process module itself. It needs no allowance.
sandbox=crates/tilewright-editor-sandbox/src
test -d "${sandbox}" || {
  echo "missing ${sandbox}" >&2
  exit 1
}
# The MCP editing server reaches the sandbox only through the runner's
# typed entry point and the world only through the observe tools: it is
# held to the sandbox runner's import rule, and below it opens no root or
# directory and writes nothing itself, as the read-only crates.
mcp_editing=crates/tilewright-editor-mcp-sandbox/src
test -d "${mcp_editing}" || {
  echo "missing ${mcp_editing}" >&2
  exit 1
}
# Nor do they name the standard library's file, network or process
# modules, its file types, or a directory-creating call, however the import
# is spelled. Each file is read with its comment lines dropped and its lines
# joined (a file that cannot be read stops the script), so a grouped import
# over several lines is caught. Inside a `use ...;` statement a bare `fs`,
# `net` or `process` followed by `::`, `,`, `}`, `;` or `as` is refused
# (`use std::{io, fs};`); elsewhere only `fs::`, `net::` or `process::` is,
# so a variable or enum variant may be called `process` or `Command`.
# String literals and trailing comments are not stripped: one that spells
# `fs::`, `File` or such fails the check too, and is better reworded.
while IFS= read -r -d '' file; do
  text=$(sed -e '/^[[:space:]]*\/\//d' -- "${file}" | tr '\n' ' ')
  if grep -qE \
    '\buse\b[^;]*\b(fs|net|process)\b[[:space:]]*(::|,|\}|;|as[[:space:]])|\b(fs|net|process)[[:space:]]*::|\b(File|OpenOptions|DirBuilder)\b|\bcreate_dir(_all)?\b' \
    <<<"${text}"; then
    echo "${file} names file, network or process access" >&2
    exit 1
  fi
done < <(find "${read_only[@]}" "${sandbox}" "${mcp_editing}" -name '*.rs' -print0 | sort -z)
# Nor do they write: `render` returns pixels, the binary writes the one
# file `--out` names, and the MCP server returns the image in its reply. World's confined access writes only through a
# directory it opens (`write`, or `create` for a file only made new), and
# its lock creates a file in the root on Windows;
# the observe crate names no root, directory or lock type, resolves no path,
# and calls no method or function (`.f(` or `path::f(`) that opens a
# directory, writes, locks, renames or removes. To allow a lock the crate
# needs later (a Mutex, say), strip that exact call before matching, with
# an anchored pattern such as `sed -E 's/\bstate\.lock\(\)//g'`, rather
# than loosening the pattern below.
if grep -RnE --include='*.rs' \
  '\b(SourceRoot|SourceDirectory|SourceLock|canonicalize|make_directory)\b|\.(directory|lock)[[:space:]]*\(|(\.|::)(rename|remove_file|remove_directory|set_own_access|make_directory|write|create)[[:space:]]*\(' \
  "${read_only[@]}" "${mcp_editing}" | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' >/dev/null; then
  echo "the read-only observe crate, the MCP server or the MCP editing server opens a root or directory, locks, resolves a path or writes" >&2
  exit 1
fi

# The sandbox runner reads no environment, and opens only the directories
# it is given: a request carries no path, so the module that reads requests
# names no path, root or OS string type. The crate's tests pin each
# request's members, which is what keeps paths out of requests.
if grep -RnE --include='*.rs' '\benv[[:space:]]*(::|!|;|,|\}|as[[:space:]])' "${sandbox}" "${mcp_editing}" |
  grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' >/dev/null; then
  echo "the sandbox runner or the MCP editing server reads the environment" >&2
  exit 1
fi
sandbox_requests=crates/tilewright-editor-sandbox/src/request.rs
test -f "${sandbox_requests}" || {
  echo "missing ${sandbox_requests}" >&2
  exit 1
}
if grep -nE '\b(Path|PathBuf|OsStr|OsString|SourceRoot|SourceDirectory)\b|std::(path|env)' \
  "${sandbox_requests}" | grep -vE '^[0-9]+:[[:space:]]*//' >/dev/null; then
  echo "the sandbox runner's requests name a path, root or environment" >&2
  exit 1
fi
# Nor does the MCP editing server take a path from a tool call: the module
# that reads tool arguments names no path, root or OS string type.
mcp_editing_tools=crates/tilewright-editor-mcp-sandbox/src/tools.rs
test -f "${mcp_editing_tools}" || {
  echo "missing ${mcp_editing_tools}" >&2
  exit 1
}
if grep -nE '\b(Path|PathBuf|OsStr|OsString|SourceRoot|SourceDirectory)\b|std::(path|env)' \
  "${mcp_editing_tools}" "${mcp_editing}/action.rs" | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' >/dev/null; then
  echo "the MCP editing server's tools name a path, root or environment" >&2
  exit 1
fi
