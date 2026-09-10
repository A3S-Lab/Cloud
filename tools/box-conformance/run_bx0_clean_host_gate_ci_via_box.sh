#!/usr/bin/env bash
# Run run_bx0_clean_host_gate_ci.sh inside an a3s-box Linux MicroVM.
# Use this on Darwin (or any host with a3s-box) so Linux-only armed stubs are
# exercised without Docker. Never claims product EXIT or LOOP.
#
# Usage:
#   bash tools/box-conformance/run_bx0_clean_host_gate_ci_via_box.sh
#
# Env:
#   A3S_CLOUD_BOX_BIN — a3s-box binary (default: PATH)
#   A3S_CLOUD_BX0_MONOREPO_ROOT — absolute a3s monorepo root (auto-detected when
#     Cloud is checked out as apps/cloud submodule)
#   A3S_CLOUD_BX0_VIA_BOX_IMAGE — OCI image (default: ubuntu:24.04)
#   A3S_CLOUD_BX0_VIA_BOX_NAME — box name (default: bx0-clean-host-ci)

set -euo pipefail

tools_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cloud_root=$(cd -- "$tools_dir/../.." && pwd)
gate_ci="$tools_dir/run_bx0_clean_host_gate_ci.sh"
[[ -f $gate_ci ]]

box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
if [[ -z $box_bin || ! -x $box_bin ]]; then
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_CI_VIA_BOX_BLOCKED reason=a3s-box_unavailable' \
    'Install a3s-box (crates/box/install.sh) and export A3S_CLOUD_BOX_BIN.' >&2
  exit 2
fi

monorepo_root=${A3S_CLOUD_BX0_MONOREPO_ROOT:-}
if [[ -z $monorepo_root ]]; then
  monorepo_root=$(
    git -C "$cloud_root" rev-parse --show-superproject-working-tree 2>/dev/null || true
  )
fi
if [[ -z $monorepo_root || $monorepo_root != /* || ! -d $monorepo_root/apps/cloud ]]; then
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_CI_VIA_BOX_BLOCKED reason=monorepo_root_unavailable' \
    'Set A3S_CLOUD_BX0_MONOREPO_ROOT to the absolute a3s checkout (contains apps/cloud).' >&2
  exit 2
fi
if [[ $(cd -- "$monorepo_root/apps/cloud" && pwd -P) != "$(cd -- "$cloud_root" && pwd -P)" ]]; then
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_CI_VIA_BOX_BLOCKED reason=monorepo_cloud_mismatch' \
    "monorepo apps/cloud must be this Cloud tree: $cloud_root" >&2
  exit 2
fi

image=${A3S_CLOUD_BX0_VIA_BOX_IMAGE:-alpine:3.20}
box_name=${A3S_CLOUD_BX0_VIA_BOX_NAME:-bx0-clean-host-ci}
guest_mount=/a3s
guest_cloud=$guest_mount/apps/cloud

cleanup_box() {
  "$box_bin" rm -f "$box_name" >/dev/null 2>&1 || true
}
trap cleanup_box EXIT HUP INT TERM

cleanup_box
"$box_bin" run -d \
  --name "$box_name" \
  --cpus "${A3S_CLOUD_BX0_VIA_BOX_CPUS:-4}" \
  --memory "${A3S_CLOUD_BX0_VIA_BOX_MEMORY:-2g}" \
  -v "$monorepo_root:$guest_mount:ro" \
  -w "$guest_cloud" \
  "$image" -- sleep 7200

"$box_bin" exec --timeout 60 "$box_name" -- uname -s | grep -Fxq Linux

printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_CI_VIA_BOX_ARMED" \
  "a3s_box=$box_bin" \
  "monorepo_root=$monorepo_root" \
  "box_name=$box_name" \
  "image=$image" \
  "product_exit=not_claimed" \
  "loop_certified=not_claimed"

# Keep package install and the harness in separate execs. Prefer Alpine apk
# (faster than apt on first boot). a3s-box --timeout 0 means "immediate", not
# "disabled". Avoid `git config --global` (can hang on some virtiofs homes).
"$box_bin" exec --timeout "${A3S_CLOUD_BX0_VIA_BOX_APT_TIMEOUT:-1800}" "$box_name" -- sh -lc "
set -eu
if command -v apk >/dev/null 2>&1; then
  if ! command -v git >/dev/null 2>&1 || ! command -v jq >/dev/null 2>&1 || ! command -v bash >/dev/null 2>&1; then
    apk add --no-cache bash git curl jq ca-certificates coreutils diffutils >/dev/null
  fi
elif command -v apt-get >/dev/null 2>&1; then
  export DEBIAN_FRONTEND=noninteractive
  if ! command -v git >/dev/null 2>&1 || ! command -v jq >/dev/null 2>&1; then
    apt-get update -qq
    apt-get install -y -qq git curl jq ca-certificates diffutils coreutils >/dev/null
  fi
else
  echo 'via-box guest needs apk or apt-get' >&2
  exit 1
fi
printf '%s\n' '[safe]' '	directory = *' > /tmp/bx0-gitconfig
"

"$box_bin" exec --timeout "${A3S_CLOUD_BX0_VIA_BOX_CI_TIMEOUT:-600}" \
  -e GIT_CONFIG_GLOBAL=/tmp/bx0-gitconfig \
  -w "$guest_cloud" "$box_name" -- \
  bash tools/box-conformance/run_bx0_clean_host_gate_ci.sh

printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_CI_VIA_BOX_CERTIFIED" \
  "host_os=$(uname -s)" \
  "guest_os=Linux" \
  "product_exit=not_claimed" \
  "loop_certified=not_claimed"
