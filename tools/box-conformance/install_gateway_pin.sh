#!/usr/bin/env bash
# Install pin-matched a3s-gateway for BX0.software HTTPS preflight / EXIT.
#
# Builds from the exact SHA in tools/gateway-conformance/gateway-revision and
# writes a GATEWAY-REVISION sidecar next to the binary. Never claims LOOP/EXIT.
#
# Usage:
#   bash tools/box-conformance/install_gateway_pin.sh /abs/empty/install/dir
#   export A3S_CLOUD_GATEWAY_BIN=/abs/empty/install/dir/a3s-gateway

set -euo pipefail

readonly SCRIPT_DIRECTORY="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLOUD_ROOT="$(cd "$SCRIPT_DIRECTORY/../.." && pwd)"
# When this script is LF-copied under /tmp, BASH_SOURCE no longer points at apps/cloud.
if [[ ! -f $CLOUD_ROOT/tools/gateway-conformance/gateway-revision ]]; then
  CLOUD_ROOT=/mnt/d/code/a3s/apps/cloud
fi
readonly CLOUD_ROOT
readonly GATEWAY_PIN_FILE="$CLOUD_ROOT/tools/gateway-conformance/gateway-revision"

install_root=${1:-}
if [[ -z $install_root || $install_root != /* ]]; then
  printf 'usage: %s ABSOLUTE_EMPTY_INSTALL_DIRECTORY\n' "$0" >&2
  exit 2
fi
if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
  printf '%s\n' 'gateway pin install requires Linux x86_64' >&2
  exit 1
fi
[[ -f $GATEWAY_PIN_FILE ]] || {
  printf '%s\n' "gateway pin missing: $GATEWAY_PIN_FILE" >&2
  exit 1
}
gateway_revision=$(<"$GATEWAY_PIN_FILE")
[[ $gateway_revision =~ ^[0-9a-f]{40}$ ]] || {
  printf '%s\n' 'gateway pin is invalid' >&2
  exit 1
}
for command_name in cargo git; do
  command -v "$command_name" >/dev/null 2>&1 || {
    printf 'required command is unavailable: %s\n' "$command_name" >&2
    exit 1
  }
done
if [[ -e $install_root ]] &&
  [[ ! -d $install_root || -n $(find "$install_root" -mindepth 1 -maxdepth 1 -print -quit) ]]; then
  printf '%s\n' 'gateway install directory must be absent or empty' >&2
  exit 1
fi

install -d -m 0755 "$install_root"
source_root="$install_root/.gateway-source"
target_root="$install_root/.gateway-target"
cleanup_sources() {
  rm -rf "$source_root" "$target_root"
}
trap cleanup_sources EXIT HUP INT TERM

git init --quiet "$source_root"
git -C "$source_root" remote add origin https://github.com/A3S-Lab/Gateway.git
git -C "$source_root" fetch --quiet --depth=1 origin "$gateway_revision"
git -C "$source_root" checkout --quiet --detach FETCH_HEAD
CARGO_TARGET_DIR="$target_root" cargo build --locked --manifest-path "$source_root/Cargo.toml" \
  -p a3s-gateway --release
install -m 0755 "$target_root/release/a3s-gateway" "$install_root/a3s-gateway"
printf '%s\n' "$gateway_revision" >"$install_root/GATEWAY-REVISION"

trap - EXIT HUP INT TERM
cleanup_sources

cat <<EOF
A3S_CLOUD_BX0_GATEWAY_PIN_INSTALLED
gateway_bin=$install_root/a3s-gateway
gateway_revision=$gateway_revision
export A3S_CLOUD_GATEWAY_BIN=$install_root/a3s-gateway
product_exit=not_claimed
loop_certified=not_claimed
EOF
