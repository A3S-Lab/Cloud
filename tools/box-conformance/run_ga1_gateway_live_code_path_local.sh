#!/usr/bin/env bash
# GA-1 Gateway LIVE Code-path smoke (Docker-free, pin-matched).
#
# Fail-closed: refuses PLACEHOLDER_*, docker.sock, stub gateways, and BX0 Python
# TLS theater (:18444). Public traffic must traverse pin-matched a3s-gateway to
# the published Agent Service (/health/ready). Conversations/events use the
# management plane against the same release.
#
# Emits A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED on success.
#
# Usage:
#   bash tools/box-conformance/run_ga1_gateway_live_code_path_local.sh [EVIDENCE_DIR]
set -euo pipefail

repository_root=${A3S_CLOUD_ROOT:-}
if [[ -z $repository_root ]]; then
  repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
fi
if [[ ! -f $repository_root/tools/box-conformance/box-revision ]]; then
  echo "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_BLOCKED reason=cloud_root_missing path=$repository_root" >&2
  exit 3
fi
tools=$repository_root/tools/box-conformance
gateway_pin_file=$repository_root/tools/gateway-conformance/gateway-revision
monorepo_root=${A3S_CLOUD_MONOREPO_ROOT:-$(cd -- "$repository_root/../.." && pwd)}

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-ga1-gateway-live.XXXXXX")
fi
mkdir -p -- "$evidence_directory"
export A3S_CLOUD_GA1_EVIDENCE_DIR=$evidence_directory

box_revision=$(<"$tools/box-revision")
gateway_revision=$(<"$gateway_pin_file")

marker='A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED'
blocked() {
  local reason=$1
  echo "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_BLOCKED reason=$reason" \
    | tee "$evidence_directory/blocked.txt"
  exit 3
}

if [[ $(uname -s) != Linux ]]; then
  blocked "host_not_linux uname=$(uname -s)"
fi
if [[ -S /var/run/docker.sock || -S /var/run/docker.sock.raw ]]; then
  blocked docker_sock_present
fi
# Box Agent / warm-pool VMs need KVM on Linux. OrbStack Apple Silicon guests
# never expose /dev/kvm (no nested virt) and must not invent CERTIFIED.
if [[ ! -e /dev/kvm ]]; then
  blocked "box_vm_requires_kvm path=/dev/kvm"
fi

# Refuse placeholder env theater.
for var in A3S_CLOUD_GATEWAY_BIN A3S_CLOUD_TEST_GATEWAY_BIN A3S_CLOUD_GA1_GATEWAY_URL \
  A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE A3S_CLOUD_TEST_POSTGRES_URL; do
  val=${!var:-}
  if [[ $val == PLACEHOLDER_* || $val == *PLACEHOLDER* ]]; then
    blocked "placeholder_env:$var"
  fi
done

# Require retained A0.4 image env (exact published digest).
durable=${A3S_CLOUD_A0_4_DURABLE:-$HOME/a0-4-durable}
img_env=${A3S_CLOUD_A0_4_IMAGE_ENV:-$durable/a0-4-image.env}
if [[ ! -f $img_env ]]; then
  img_env=$repository_root/docs/evidence/a0-4-real-box-2026-09-17/a0-4-image.env
fi
[[ -f $img_env ]] || blocked missing_a0_4_image_env
# shellcheck disable=SC1090
source "$img_env"
[[ -n ${A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE:-} ]] || blocked missing_agent_runtime_image
printf 'a0_4_image=%s\n' "$A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE" \
  | tee "$evidence_directory/a0-4-image-bind.txt"

# Postgres + registry
postgres_url=${A3S_CLOUD_TEST_POSTGRES_URL:-postgresql://a3s_cloud:a3s_cloud@127.0.0.1:54320/postgres}
export A3S_CLOUD_TEST_POSTGRES_URL=$postgres_url
ss -ltn | grep -q ':54320' || blocked postgres_port_down
curl -fsS --max-time 3 http://127.0.0.1:50020/v2/ >/dev/null \
  || blocked registry_down

# Pin-matched gateway binary
gateway_install=${A3S_CLOUD_GATEWAY_INSTALL_DIR:-$durable/gateway-pin-install}
gateway_bin=${A3S_CLOUD_GATEWAY_BIN:-}
if [[ -z $gateway_bin || ! -x $gateway_bin ]]; then
  if [[ -x $gateway_install/a3s-gateway \
    && -f $gateway_install/GATEWAY-REVISION \
    && $(<"$gateway_install/GATEWAY-REVISION") == "$gateway_revision" ]]; then
    gateway_bin=$gateway_install/a3s-gateway
  else
    echo "=== install gateway pin $gateway_revision ===" \
      | tee "$evidence_directory/gateway-install.log"
    rm -rf -- "$gateway_install"
    bash "$tools/install_gateway_pin.sh" "$gateway_install" \
      | tee -a "$evidence_directory/gateway-install.log"
    gateway_bin=$gateway_install/a3s-gateway
  fi
fi
[[ -x $gateway_bin ]] || blocked gateway_bin_missing
got_gw=$(<"${gateway_bin%/*}/GATEWAY-REVISION" 2>/dev/null || true)
if [[ -z $got_gw && -f $gateway_install/GATEWAY-REVISION ]]; then
  got_gw=$(<"$gateway_install/GATEWAY-REVISION")
fi
[[ $got_gw == "$gateway_revision" ]] || blocked "gateway_pin_mismatch want=$gateway_revision got=${got_gw:-missing}"
export A3S_CLOUD_GATEWAY_BIN=$gateway_bin
export A3S_CLOUD_TEST_GATEWAY_BIN=$gateway_bin

# Box pin — require an explicit install dir or A3S_CLOUD_BOX_BIN (no silent /home hardcode).
install_dir=${A3S_CLOUD_BOX_INSTALL_DIR:-}
if [[ -z $install_dir && -n ${A3S_CLOUD_BOX_BIN:-} ]]; then
  install_dir=$(cd -- "$(dirname -- "$A3S_CLOUD_BOX_BIN")" && pwd)
fi
[[ -n $install_dir ]] || blocked missing_box_install_dir
if [[ -f $install_dir/BOX-REVISION ]]; then
  install_box_rev=$(<"$install_dir/BOX-REVISION")
  [[ $install_box_rev == "$box_revision" ]] \
    || blocked "box_pin_mismatch cloud=$box_revision install=$install_box_rev"
else
  blocked missing_box_revision_sidecar
fi
export A3S_CLOUD_BOX_BIN=${A3S_CLOUD_BOX_BIN:-$install_dir/a3s-box}
[[ -x $A3S_CLOUD_BOX_BIN ]] || blocked box_bin_missing

printf 'box_revision=%s\ngateway_revision=%s\ngateway_bin=%s\nbox_bin=%s\nmonorepo_root=%s\n' \
  "$box_revision" "$gateway_revision" "$gateway_bin" "$A3S_CLOUD_BOX_BIN" "$monorepo_root" \
  | tee "$evidence_directory/ga1-preflight.txt"

# Sandbox CI identity (same as A0.4) for real Box harness.
export GITHUB_WORKSPACE=$monorepo_root
export SUDO_UID=${SUDO_UID:-1000}
export SUDO_GID=${SUDO_GID:-1000}
export SUDO_USER=${SUDO_USER:-${USER:-roylin}}
export RUST_MIN_STACK=${RUST_MIN_STACK:-33554432}
export A3S_CLOUD_TEST_BOX=1
export A3S_DEPS_STUB=1
export A3S_REGISTRY_PROTOCOL=http

# Run ignored integration test under setpriv when available.
set +e
if [[ -x $tools/run_box_cargo_under_setpriv.sh ]]; then
  bash "$tools/run_box_cargo_under_setpriv.sh" test \
    --manifest-path "$repository_root/Cargo.toml" --locked \
    -p a3s-cloud-control-plane --features persistence-conformance \
    --test ga1_gateway_live_code_path \
    ga1_gateway_live_code_path_agent_release_through_real_gateway \
    -- --ignored --exact --nocapture --test-threads=1 \
    >"$evidence_directory/ga1-gateway-live.log" 2>&1
  test_rc=$?
else
  (
    cd "$repository_root"
    cargo test --manifest-path "$repository_root/Cargo.toml" --locked \
      -p a3s-cloud-control-plane --features persistence-conformance \
      --test ga1_gateway_live_code_path \
      ga1_gateway_live_code_path_agent_release_through_real_gateway \
      -- --ignored --exact --nocapture --test-threads=1
  ) >"$evidence_directory/ga1-gateway-live.log" 2>&1
  test_rc=$?
fi
set -e

tail -n 80 "$evidence_directory/ga1-gateway-live.log" || true

# Fail closed: CERTIFIED requires both public traffic and management-plane markers.
if ! grep -Fq 'A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN' \
  "$evidence_directory/ga1-gateway-live.log"; then
  echo "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_BLOCKED reason=gateway_public_missing test_rc=$test_rc" \
    | tee "$evidence_directory/blocked.txt"
  exit 4
fi
if ! grep -Fq 'A3S_CLOUD_GA1_MANAGEMENT_PLANE_PROVEN' \
  "$evidence_directory/ga1-gateway-live.log"; then
  echo "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_BLOCKED reason=management_plane_missing test_rc=$test_rc" \
    | tee "$evidence_directory/blocked.txt"
  exit 4
fi

if grep -Fq "$marker" "$evidence_directory/ga1-gateway-live.log"; then
  grep -F "$marker" "$evidence_directory/ga1-gateway-live.log" | head -n 1 \
    | tee "$evidence_directory/ga1-gateway-live-certification.txt"
  grep -F 'A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN' \
    "$evidence_directory/ga1-gateway-live.log" | head -n 1 \
    | tee "$evidence_directory/gateway-public-traffic.txt"
  grep -F 'A3S_CLOUD_GA1_MANAGEMENT_PLANE_PROVEN' \
    "$evidence_directory/ga1-gateway-live.log" | head -n 1 \
    | tee "$evidence_directory/management-plane.txt"
  {
    printf 'box_revision=%s\n' "$box_revision"
    printf 'gateway_revision=%s\n' "$gateway_revision"
    printf 'gateway_bin=%s\n' "$gateway_bin"
    printf 'box_bin=%s\n' "$A3S_CLOUD_BOX_BIN"
    printf 'a0_4_image=%s\n' "$A3S_CLOUD_A0_4_AGENT_RUNTIME_IMAGE"
  } | tee "$evidence_directory/pins.txt"
  cp -- "$img_env" "$evidence_directory/a0-4-image.env"
  if [[ -f ${gateway_bin%/*}/GATEWAY-REVISION ]]; then
    cp -- "${gateway_bin%/*}/GATEWAY-REVISION" "$evidence_directory/GATEWAY-REVISION"
  fi
  cat >"$evidence_directory/README.md" <<EOF
# GA-1 Gateway LIVE Code-path evidence

**Status:** \`A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED\`

Pins and markers are captured from a Docker-free Linux software Box run.
Public traffic and management-plane conversation/execution/events were proven
in the same smoke against the published Agent release.

See checklist: \`docs/ga1-agent-availability-checklist.md\`.
EOF
  retain_to=${A3S_CLOUD_GA1_RETAIN_TO:-}
  if [[ -n $retain_to ]]; then
    mkdir -p -- "$retain_to"
    cp -a -- "$evidence_directory"/. "$retain_to"/
    echo "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_RETAINED path=$retain_to"
  fi
  echo "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_OK evidence=$evidence_directory"
  exit 0
fi

echo "A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_BLOCKED reason=certification_missing test_rc=$test_rc" \
  | tee "$evidence_directory/blocked.txt"
exit 4
