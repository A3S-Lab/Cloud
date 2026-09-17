#!/usr/bin/env bash
# Run BX0.software LIVE chain inside an a3s-box Linux MicroVM (Docker-free guest).
#
# Use when the outer host has /var/run/docker.sock (e.g. WSL+Docker Desktop) and
# cannot honestly claim clean-host EXIT. The guest must not see a Docker sock.
# Never invents EXIT on the outer host; product EXIT can only come from the
# guest chain evidence.
#
# Usage:
#   export A3S_CLOUD_BOX_BIN=/path/to/a3s-box
#   export A3S_CLOUD_GATEWAY_BIN=/path/to/a3s-gateway   # optional; built in guest if unset
#   bash tools/box-conformance/run_bx0_software_exit_live_via_box.sh [EVIDENCE_DIR]
#
# Env:
#   A3S_CLOUD_BX0_MONOREPO_ROOT — absolute a3s checkout (auto-detected)
#   A3S_CLOUD_BX0_VIA_BOX_IMAGE — default alpine:3.20
#   A3S_CLOUD_BX0_VIA_BOX_NAME — default bx0-software-exit-live

set -euo pipefail

tools_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cloud_root=$(cd -- "$tools_dir/../.." && pwd)
chain="$tools_dir/run_bx0_software_exit_live_chain.sh"
[[ -f $chain ]]

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-live-via-box.XXXXXX")
fi
mkdir -p -- "$evidence_directory"

fail_blocked() {
  local reason=$1
  shift || true
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-via-box.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_VIA_BOX_BLOCKED reason=$reason
$*
product_exit=not_claimed
loop_certified=not_claimed
honesty=Outer host may have Docker; only guest evidence can claim EXIT.
EOF
  exit 2
}

box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
if [[ -z $box_bin || ! -x $box_bin ]]; then
  fail_blocked a3s_box_unavailable \
    "hint=bash tools/box-conformance/install_box_release.sh /abs/empty/dir"
fi

monorepo_root=${A3S_CLOUD_BX0_MONOREPO_ROOT:-}
if [[ -z $monorepo_root ]]; then
  monorepo_root=$(
    git -C "$cloud_root" rev-parse --show-superproject-working-tree 2>/dev/null || true
  )
fi
if [[ -z $monorepo_root || $monorepo_root != /* || ! -d $monorepo_root/apps/cloud ]]; then
  fail_blocked monorepo_root_unavailable \
    "Set A3S_CLOUD_BX0_MONOREPO_ROOT to the absolute a3s checkout."
fi
if [[ $(cd -- "$monorepo_root/apps/cloud" && pwd -P) != "$(cd -- "$cloud_root" && pwd -P)" ]]; then
  fail_blocked monorepo_cloud_mismatch "monorepo apps/cloud must be this Cloud tree"
fi

image=${A3S_CLOUD_BX0_VIA_BOX_IMAGE:-alpine:3.20}
box_name=${A3S_CLOUD_BX0_VIA_BOX_NAME:-bx0-software-exit-live}
guest_mount=/a3s
guest_cloud=$guest_mount/apps/cloud
guest_evidence=/tmp/bx0-live-via-box-evidence

cleanup_box() {
  "$box_bin" rm -f "$box_name" >/dev/null 2>&1 || true
}
trap cleanup_box EXIT HUP INT TERM

cleanup_box
# Writable mount so guest can build, write state, and retain evidence.
# Do NOT mount host docker.sock.
set +e
"$box_bin" run -d \
  --name "$box_name" \
  --cpus "${A3S_CLOUD_BX0_VIA_BOX_CPUS:-6}" \
  --memory "${A3S_CLOUD_BX0_VIA_BOX_MEMORY:-8g}" \
  -v "$monorepo_root:$guest_mount" \
  -w "$guest_cloud" \
  "$image" -- sleep 14400 \
  >"$evidence_directory/box-run.out" 2>"$evidence_directory/box-run.err"
run_rc=$?
set -e
if ((run_rc != 0)); then
  # Fall back to shared-kernel sandbox only when MicroVM cannot boot (still no
  # docker.sock mount). Fail closed with the original error if both fail.
  set +e
  "$box_bin" run -d \
    --isolation sandbox \
    --name "$box_name" \
    --cpus "${A3S_CLOUD_BX0_VIA_BOX_CPUS:-6}" \
    --memory "${A3S_CLOUD_BX0_VIA_BOX_MEMORY:-8g}" \
    -v "$monorepo_root:$guest_mount" \
    -w "$guest_cloud" \
    "$image" -- sleep 14400 \
    >"$evidence_directory/box-run-sandbox.out" 2>"$evidence_directory/box-run-sandbox.err"
  sandbox_rc=$?
  set -e
  if ((sandbox_rc != 0)); then
    fail_blocked a3s_box_run_failed \
      "microvm_log=$evidence_directory/box-run.err sandbox_log=$evidence_directory/box-run-sandbox.err hint=Need KVM+permissions or Sandbox runtime/cgroupv2; do not override docker.sock on the outer host."
  fi
fi

"$box_bin" exec --timeout 60 "$box_name" -- uname -s | grep -Fxq Linux \
  || fail_blocked guest_not_linux

# Prove guest has no Docker sock (canonical path).
if "$box_bin" exec --timeout 30 "$box_name" -- sh -lc \
  'test -e /var/run/docker.sock || test -S /var/run/docker.sock'; then
  fail_blocked guest_docker_sock_present
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_VIA_BOX_ARMED" \
  "a3s_box=$box_bin" \
  "monorepo_root=$monorepo_root" \
  "box_name=$box_name" \
  "image=$image" \
  "product_exit=not_claimed" \
  "loop_certified=not_claimed" \
  | tee "$evidence_directory/bx0-software-exit-live-via-box-armed.txt"

"$box_bin" exec --timeout "${A3S_CLOUD_BX0_VIA_BOX_APT_TIMEOUT:-1800}" "$box_name" -- sh -lc "
set -eu
if command -v apk >/dev/null 2>&1; then
  apk add --no-cache bash git curl jq ca-certificates coreutils diffutils \
    openssl python3 cargo rust rustup build-base \
    >/dev/null || true
  # Prefer full toolchain if rustup present after apk
  if ! command -v cargo >/dev/null 2>&1; then
    echo 'guest cargo unavailable after apk' >&2
    exit 1
  fi
elif command -v apt-get >/dev/null 2>&1; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq
  apt-get install -y -qq bash git curl jq ca-certificates diffutils coreutils \
    openssl python3 build-essential >/dev/null
else
  echo 'via-box guest needs apk or apt-get' >&2
  exit 1
fi
# OCI publisher (best-effort)
if ! command -v crane >/dev/null 2>&1 && ! command -v skopeo >/dev/null 2>&1 \
  && ! command -v oras >/dev/null 2>&1; then
  if command -v apk >/dev/null 2>&1; then
    apk add --no-cache skopeo >/dev/null 2>&1 || true
  fi
fi
mkdir -p $guest_evidence
printf '%s\\n' '[safe]' '	directory = *' > /tmp/bx0-gitconfig
# Nested a3s-box: use host-provided binary via PATH if copied; else install later.
if [[ -x $guest_mount/crates/box/src/target/release/a3s-box ]]; then
  ln -sf $guest_mount/crates/box/src/target/release/a3s-box /usr/local/bin/a3s-box
elif [[ -x $guest_mount/crates/box/src/target/debug/a3s-box ]]; then
  ln -sf $guest_mount/crates/box/src/target/debug/a3s-box /usr/local/bin/a3s-box
fi
" || fail_blocked guest_bootstrap_failed

# Export host gateway into guest when provided
gateway_env=()
if [[ -n ${A3S_CLOUD_GATEWAY_BIN:-} && -x ${A3S_CLOUD_GATEWAY_BIN} ]]; then
  gateway_env+=(-e "A3S_CLOUD_GATEWAY_BIN=$A3S_CLOUD_GATEWAY_BIN")
fi
if [[ -n ${A3S_CLOUD_BOX_BIN:-} ]]; then
  gateway_env+=(-e "A3S_CLOUD_BOX_BIN=${A3S_CLOUD_BOX_BIN}")
fi

set +e
"$box_bin" exec --timeout "${A3S_CLOUD_BX0_VIA_BOX_LIVE_TIMEOUT:-14400}" \
  -e GIT_CONFIG_GLOBAL=/tmp/bx0-gitconfig \
  -e "A3S_CLOUD_BX0_MONOREPO_ROOT=$guest_mount" \
  "${gateway_env[@]}" \
  -w "$guest_cloud" "$box_name" -- \
  bash -lc "mkdir -p $guest_evidence && bash tools/box-conformance/run_bx0_software_exit_live_chain.sh $guest_evidence" \
  >"$evidence_directory/chain.out" 2>"$evidence_directory/chain.err"
chain_rc=$?
set -e

# Best-effort copy of guest evidence (virtiofs already shared if under monorepo;
# guest_evidence is under /tmp — try box cp if available)
if "$box_bin" exec --timeout 60 "$box_name" -- test -d "$guest_evidence"; then
  "$box_bin" exec --timeout 120 "$box_name" -- \
    sh -lc "tar -C $guest_evidence -cf - ." \
    >"$evidence_directory/guest-evidence.tar" 2>/dev/null || true
fi

if ((chain_rc != 0)); then
  fail_blocked live_chain_failed \
    "rc=$chain_rc see $evidence_directory/chain.err (nested virt / missing pins may block)"
fi

if grep -E --quiet \
  '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
  "$evidence_directory/chain.out" \
  "$evidence_directory/chain.err" 2>/dev/null; then
  grep -E \
    '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
    "$evidence_directory/chain.out" \
    "$evidence_directory/chain.err" \
    | tee "$evidence_directory/bx0-software-exit-live-via-box.txt"
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_VIA_BOX_OK" \
    "guest_os=Linux" \
    "honesty=EXIT line above is guest-retained evidence; copy into ROADMAP only with pin SHAs." \
    | tee -a "$evidence_directory/bx0-software-exit-live-via-box.txt"
  exit 0
fi

fail_blocked exit_not_emitted \
  "chain succeeded without EXIT_CERTIFIED — inspect guest evidence"
