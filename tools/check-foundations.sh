#!/usr/bin/env bash
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"

jq -e '.schema_version == 1 and .grant_used == false and all(.records[]; .status == "excluded")' provenance/reuse.json >/dev/null
python3 tools/check-prototype-assets.py

if grep -RhE '^[[:space:]]*uses:' .github/workflows 2>/dev/null \
  | grep -Ev '@[0-9a-f]{40}([[:space:]]|$)' >/dev/null; then
  echo "workflow action is not pinned to an immutable commit" >&2
  exit 1
fi
for required in CONTRIBUTING.md PROVENANCE.md SECURITY.md THIRD_PARTY_NOTICES.md \
  decisions/0001-editor-architecture.md docs/THREAT_MODEL.md docs/MUTABLE_PATHS.md; do
  test -s "${required}"
done
tools/check-architecture.sh
