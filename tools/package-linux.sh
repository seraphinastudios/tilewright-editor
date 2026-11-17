#!/usr/bin/env bash
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"
output=${1:-dist}
version=${2:-0.1.0-dev}
font_pack=${3:?usage: package-linux.sh OUTPUT VERSION PREPARED_FONT_PACK}
if [[ ! ${version} =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release version: ${version}" >&2
  exit 2
fi
if [[ -e ${output} ]]; then
  echo "release output already exists: ${output}" >&2
  exit 1
fi
for command in cargo-auditable syft strip python3; do command -v "${command}" >/dev/null || { echo "missing required tool: ${command}" >&2; exit 1; }; done

stage=$(mktemp -d /tmp/tilewright-editor-stage.XXXXXX)
render_smoke=$(mktemp -d /tmp/tilewright-editor-render.XXXXXX)
# The sandbox provisioner refuses volatile storage: its smoke test runs on
# the checked test root when one is given.
sandbox_smoke=$(mktemp -d "${TILEWRIGHT_CHECKED_TEST_ROOT:-${TMPDIR:-/tmp}}/tilewright-editor-mcp-sandbox.XXXXXX")
trap 'rm -rf -- "${stage}" "${render_smoke}" "${sandbox_smoke}"' EXIT
# The packaged binary reports the release version for itself (`version`,
# the MCP servers, proposals): the build reads it at compile time and
# refuses an unusable one.
TILEWRIGHT_EDITOR_RELEASE_VERSION="${version}" cargo auditable build --locked --release --package tilewright-editor
target=$(cargo metadata --locked --offline --format-version 1 --no-deps | jq -r .target_directory)
install -d "${stage}/tilewright-editor-${version}/bin" "${output}"
install "${target}/release/tilewright-editor" "${stage}/tilewright-editor-${version}/bin/tilewright-editor"
strip "${stage}/tilewright-editor-${version}/bin/tilewright-editor"
cp LICENSE PROVENANCE.md THIRD_PARTY_NOTICES.md \
  "${stage}/tilewright-editor-${version}/"
python3 tools/check-prototype-assets.py
# Every sample file must be on the package allowlist. The staged sample is
# opened as a World below, which does not read the preview catalog or its
# art; this test covers those too.
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools -p 'test_package_assets.py' >/dev/null
python3 tools/package-assets.py --root . --stage "${stage}/tilewright-editor-${version}"
python3 tools/package-font-notice.py --root . --stage "${stage}/tilewright-editor-${version}" >/dev/null
python3 tools/prepare-font-pack.py stage --root . --font-pack "${font_pack}" \
  --stage "${stage}/tilewright-editor-${version}" >/dev/null
reported=$("${stage}/tilewright-editor-${version}/bin/tilewright-editor" version)
if [[ ${reported} != "tilewright-editor ${version} "* ]]; then
  echo "the packaged binary reports itself as ${reported@Q}, not as ${version}" >&2
  exit 1
fi
# The packaged sample must open as a World with the packaged binary: a fixture
# file left out of policy/package-assets.json would ship a sample that does not
# load.
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" open \
  "${stage}/tilewright-editor-${version}/samples/prototype-neutral" >/dev/null
# The read-only commands need no editing support: the packaged sample's catalog
# and art load, and lint finds no error in it.
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" overview --json \
  "${stage}/tilewright-editor-${version}/samples/prototype-neutral" >/dev/null
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" lint \
  "${stage}/tilewright-editor-${version}/samples/prototype-neutral" >/dev/null
# render writes one PNG, outside the sample and the staged package: its
# signature and size must be what its report says.
"${stage}/tilewright-editor-${version}/bin/tilewright-editor" render --json \
  --out "${render_smoke}/sample.png" \
  "${stage}/tilewright-editor-${version}/samples/prototype-neutral" >"${render_smoke}/report.json"
python3 -I - "${render_smoke}/sample.png" "${render_smoke}/report.json" <<'PYTHON'
import json, struct, sys
with open(sys.argv[1], "rb") as image:
    png = image.read()
with open(sys.argv[2], encoding="utf-8") as report:
    report = json.load(report)
assert png[:8] == b"\x89PNG\r\n\x1a\n" and png[12:16] == b"IHDR", "render did not write a PNG"
size = struct.unpack(">II", png[16:24])
assert size == (256, 192), size
assert size == (report["image"]["width"], report["image"]["height"]), report["image"]
assert len(png) == report["png"]["bytes"], report["png"]
PYTHON
test "$(ls -A "${render_smoke}" | wc -l)" -eq 2
# The MCP server answers over stdio with the packaged binary: discovery, and
# render, whose image is the PNG render wrote, byte for byte.
mcp_meta='"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}'
printf '%s\n' \
  "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"server/discover\",\"params\":{${mcp_meta}}}" \
  "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"render\",\"arguments\":{},${mcp_meta}}}" |
  "${stage}/tilewright-editor-${version}/bin/tilewright-editor" mcp \
    "${stage}/tilewright-editor-${version}/samples/prototype-neutral" 2>/dev/null |
  python3 -I -c '
import base64, json, sys
replies = [json.loads(line) for line in sys.stdin]
assert [reply["id"] for reply in replies] == [1, 2], replies
assert replies[0]["result"]["supportedVersions"] == ["2026-07-28"], replies[0]
assert replies[0]["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["version"] == sys.argv[2], replies[0]
result = replies[1]["result"]
assert result["isError"] is False, result["content"][0]
image = result["content"][1]
assert image["type"] == "image" and image["mimeType"] == "image/png", image["type"]
with open(sys.argv[1], "rb") as written:
    assert base64.b64decode(image["data"], validate=True) == written.read(), "the MCP image is not the PNG render writes"
' "${render_smoke}/sample.png" "${version}"
# The MCP editing server starts through the sandbox provisioner with the
# packaged binary on a fresh copy of the packaged sample: discovery, the
# sandbox's status and an edit refused for its form, over stdio.
printf '%s\n' \
  "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"server/discover\",\"params\":{${mcp_meta}}}" \
  "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"status\",\"arguments\":{},${mcp_meta}}}" \
  "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"edit\",\"arguments\":{\"action\":{\"kind\":\"x\"}},${mcp_meta}}}" |
  python3 -I tools/provision-sandbox.py --world "${stage}/tilewright-editor-${version}/samples/prototype-neutral" \
    --destination-parent "${sandbox_smoke}" --actor agent:package-smoke --mcp \
    --editor "${stage}/tilewright-editor-${version}/bin/tilewright-editor" 2>/dev/null |
  python3 -I -c '
import json, sys
replies = [json.loads(line) for line in sys.stdin]
assert [reply["id"] for reply in replies] == [1, 2, 3], replies
assert replies[0]["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["title"] == "Tilewright editor (sandbox editing)", replies[0]
assert replies[0]["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["version"] == sys.argv[1], replies[0]
status = replies[1]["result"]
assert status["isError"] is False and status["structuredContent"]["actor"] == "agent:package-smoke", status
assert status["structuredContent"]["editor"] == sys.argv[1], status
refused = replies[2]["result"]
assert refused["isError"] is True and refused["structuredContent"]["code"] == "proposal.invalid", refused
' "${version}"
test "$(ls -A "${sandbox_smoke}" | wc -l)" -eq 1
SYFT_CHECK_FOR_APP_UPDATE=false syft \
  "${stage}/tilewright-editor-${version}/bin/tilewright-editor" \
  --source-name tilewright-editor --source-version "${version}" \
  --output "cyclonedx-json=${stage}/tilewright-editor-${version}/sbom.cdx.json"
sbom="${stage}/tilewright-editor-${version}/sbom.cdx.json"
normalized="${sbom}.normalized"
digest=$(printf 'tilewright-editor:linux-amd64:%s' "${version}" | sha256sum | cut -d' ' -f1)
serial="urn:uuid:${digest:0:8}-${digest:8:4}-8${digest:13:3}-8${digest:17:3}-${digest:20:12}"
jq --arg version "${version}" --arg serial "${serial}" '
  .serialNumber = $serial |
  .metadata.timestamp = "1970-01-01T00:00:00Z" |
  .metadata.component["bom-ref"] = ("tilewright-editor-linux-amd64@" + $version) |
  (.components[] | select(.type == "file") | .name) = "/tilewright-editor"
' "${sbom}" >"${normalized}"
mv "${normalized}" "${sbom}"
jq -e '(.components // []) | length >= 10' "${sbom}" >/dev/null
python3 tools/prepare-font-pack.py sbom --root . --stage "${stage}/tilewright-editor-${version}" \
  --sbom "${sbom}" >/dev/null
python3 tools/package-provenance.py --root . --version "${version}" \
  --output "${stage}/tilewright-editor-${version}/provenance.json"

archive="${output}/tilewright-editor-${version}-linux-amd64.tar.gz"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='UTC 1970-01-01' \
  --mode='u+rwX,go+rX,go-w' \
  -C "${stage}" -cf - "tilewright-editor-${version}" | gzip -n >"${archive}"
git archive --format=tar --prefix="tilewright-editor-${version}/" HEAD \
  | gzip -n >"${output}/tilewright-editor-${version}-source.tar.gz"
(cd "${output}" && sha256sum ./* >SHA256SUMS)
