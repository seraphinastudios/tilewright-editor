#!/usr/bin/env bash
# The full editor validation. With no argument it runs every stage, in order.
# Name stages to run only those (CI runs each stage in its own job):
#   checks   tool tests, formatting, Clippy, dependency and foundation gates
#   tests    workspace and doc tests, and the CLI smoke runs
#   package  two Linux packages of one version, compared byte for byte
# Every stage checks the toolchain first, and last `git diff --check` and that
# it left the checkout as it found it (`git status --porcelain`).
set -euo pipefail

repository=$(git rev-parse --show-toplevel)
cd "${repository}"
font_pack=${TILEWRIGHT_PREPARED_FONT_PACK:?set explicit prepared immutable font pack; preparation is separate}
# Explicit input for cfg(test) font readers; native runtime uses --font-pack.
export TILEWRIGHT_UNICODE_FONT_ROOT="${font_pack}"
checks=false tests=false package=false
if [[ $# -eq 0 ]]; then
  set -- checks tests package
fi
for stage in "$@"; do
  case ${stage} in
    checks) checks=true ;;
    tests) tests=true ;;
    package) package=true ;;
    *) echo "unknown validation stage: ${stage} (checks, tests, package)" >&2; exit 2 ;;
  esac
done
tools=(cargo)
if ${checks}; then tools+=(cargo-deny jq); fi
if ${package}; then tools+=(cargo-auditable jq syft); fi

before=$(git status --porcelain --untracked-files=all)
if ${checks}; then python3 -m unittest discover -s tools -p 'test_*.py'; fi
test "$(rustc --version | awk '{print $2}')" = 1.97.1
for command in "${tools[@]}"; do command -v "${command}" >/dev/null || { echo "missing required tool: ${command}" >&2; exit 1; }; done
if ${checks}; then
  cargo fmt --all --check
  cargo clippy --locked --workspace --all-targets -- -D warnings
fi
if ${tests}; then
  cargo test --locked --workspace --all-targets
  cargo test --locked --workspace --doc
fi
if ${checks}; then
  cargo deny --locked check
  tools/check-foundations.sh
fi
if ${tests}; then
  cargo run --locked --quiet --package tilewright-editor -- version
  cargo run --locked --quiet --package tilewright-editor -- open crates/tilewright-editor-testkit/fixtures/meadow
  cargo run --locked --quiet --package tilewright-editor -- headless crates/tilewright-editor-testkit/fixtures/meadow
  cargo run --locked --quiet --package tilewright-editor -- overview crates/tilewright-editor-testkit/fixtures/prototype-neutral
  cargo run --locked --quiet --package tilewright-editor -- lint --json crates/tilewright-editor-testkit/fixtures/prototype-neutral
  SDL_VIDEO_DRIVER=dummy cargo run --locked --quiet --package tilewright-editor -- window
fi
if ${package}; then
  first=$(mktemp -d /tmp/tilewright-editor-release-first.XXXXXX); rmdir "${first}"
  second=$(mktemp -d /tmp/tilewright-editor-release-second.XXXXXX); rmdir "${second}"
  trap 'rm -rf -- "${first}" "${second}"' EXIT
  tools/package-linux.sh "${first}" 0.1.0-test.1 "${font_pack}"
  tools/package-linux.sh "${second}" 0.1.0-test.1 "${font_pack}"
  cmp "${first}/tilewright-editor-0.1.0-test.1-linux-amd64.tar.gz" "${second}/tilewright-editor-0.1.0-test.1-linux-amd64.tar.gz"
  cmp "${first}/tilewright-editor-0.1.0-test.1-source.tar.gz" "${second}/tilewright-editor-0.1.0-test.1-source.tar.gz"
fi
git diff --check
# Nothing a stage runs may add, change or remove files in the checkout: CI runs
# each stage in a fresh checkout, so a later stage would not notice them. Work
# in progress that was there before is allowed; in CI the checkout starts clean.
after=$(git status --porcelain --untracked-files=all)
if [[ ${after} != "${before}" ]]; then
  echo 'validation changed the checkout (git status --porcelain, before and after):' >&2
  diff <(printf '%s\n' "${before}") <(printf '%s\n' "${after}") >&2 || true
  exit 1
fi
