#!/usr/bin/env bash
# Install the named pinned Linux validation tools into DIRECTORY/bin, each
# checked against its release digest. Tools: cargo-deny, cargo-auditable, syft.
set -euo pipefail

directory=${1:?usage: install-linux-ci-tools.sh DIRECTORY TOOL...}
shift
install -d "${directory}/bin"
fetch() {
  curl --fail --silent --show-error --location "$1" --output "$2"
  printf '%s  %s\n' "$3" "$2" | sha256sum --check --strict
}
for tool in "$@"; do
  case ${tool} in
    cargo-deny)
      install -d "${directory}/tools/cargo-deny"
      fetch https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz \
        "${directory}/cargo-deny.tar.gz" 9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f
      tar -xzf "${directory}/cargo-deny.tar.gz" -C "${directory}/tools/cargo-deny" --strip-components=1
      install "${directory}/tools/cargo-deny/cargo-deny" "${directory}/bin/cargo-deny"
      ;;
    cargo-auditable)
      install -d "${directory}/tools/cargo-auditable"
      fetch https://github.com/rust-secure-code/cargo-auditable/releases/download/v0.7.5/cargo-auditable-x86_64-unknown-linux-gnu.tar.xz \
        "${directory}/cargo-auditable.tar.xz" 322eda09f24e5dba97371c6c2c6949569c411ddf893852e88135cc5b51d5a719
      tar -xJf "${directory}/cargo-auditable.tar.xz" -C "${directory}/tools/cargo-auditable" --strip-components=1
      install "${directory}/tools/cargo-auditable/cargo-auditable" "${directory}/bin/cargo-auditable"
      ;;
    syft)
      fetch https://github.com/anchore/syft/releases/download/v1.50.0/syft_1.50.0_linux_amd64.tar.gz \
        "${directory}/syft.tar.gz" bf7b29ff57f06da30918266a0e1c2885a8f99784798d1bdb1628886aa015d788
      tar -xzf "${directory}/syft.tar.gz" -C "${directory}/bin" syft
      ;;
    *)
      echo "unknown tool: ${tool}" >&2
      exit 2
      ;;
  esac
done
