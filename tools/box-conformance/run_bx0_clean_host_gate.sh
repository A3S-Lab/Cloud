#!/usr/bin/env bash
# BX0.5 / E0 Box re-certification: clean-host release gate entrypoint.
#
# Ordered checklist (must remain exact; automation must not skip steps):
#   1. enroll     — outbound node enrollment on a clean Linux host
#   2. OCI        — build/publish digest-pinned OCI Artifact
#   3. deploy     — deploy one Box-hosted Service via ordinary Runtime path
#   4. health     — require Healthy / ready evidence
#   5. HTTPS      — managed Gateway route + TLS reachability
#   6. logs       — durable ordered log readback
#   7. update     — immutable revision update
#   8. rollback   — cloned prior-revision rollback
#   9. stop/cleanup — stop, remove, restore empty provider + host inventory
#
# This harness does NOT emit A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED.
# Product exit stays open until a joint Cloud+Box+Gateway automation lands.
set -euo pipefail

readonly SCRIPT_DIRECTORY="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly CLOUD_ROOT="$(cd "$SCRIPT_DIRECTORY/../.." && pwd)"
readonly BOX_REVISION_FILE="$SCRIPT_DIRECTORY/box-revision"
readonly RUNTIME_REVISION_FILE="${A3S_CLOUD_BX0_RUNTIME_REVISION_FILE:-$CLOUD_ROOT/tools/runtime-conformance/runtime-revision}"
readonly GATEWAY_REVISION_FILE="${A3S_CLOUD_BX0_GATEWAY_REVISION_FILE:-$CLOUD_ROOT/tools/gateway-conformance/gateway-revision}"
readonly INSTALL_BOX_RELEASE="$SCRIPT_DIRECTORY/install_box_release.sh"

die() {
  printf 'BX0 clean-host gate: %s\n' "$1" >&2
  exit 1
}

require_exact_pin_file() {
  local label=$1
  local path=$2
  local reason_missing=$3
  if [[ ! -f $path ]]; then
    print_checklist
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$reason_missing" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$reason_missing" \
      "missing pin file: $path" >&2
    exit 1
  fi
  local value
  value="$(<"$path")"
  if [[ ! $value =~ ^[0-9a-f]{40}$ ]]; then
    print_checklist
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=${label}_revision_invalid" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=${label}_revision_invalid" \
      "pin file=$path value=$value" >&2
    exit 1
  fi
  printf '%s\n' "$value"
}

print_checklist() {
  cat <<'CHECKLIST'
BX0.5/E0 Box re-cert checklist (ordered):
  1. enroll
  2. OCI (build/publish digest-pinned Artifact)
  3. deploy
  4. health
  5. HTTPS (managed Gateway TLS)
  6. logs
  7. update
  8. rollback
  9. stop/cleanup (empty Box + host inventory)
CHECKLIST
}

print_required_markers() {
  cat <<'MARKERS'
Required certification markers (not emitted by this scaffold):
  A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED
    Must bind exact Cloud, Runtime, Box, Gateway, and Power revisions and
    prove the full enroll→…→stop/cleanup loop with no residue.
  Companion provider/consumer evidence from tools/box-conformance gates
    remains necessary but is not sufficient for clean-host product exit.
MARKERS
}

os_name="$(uname -s)"
if [[ $os_name != Linux ]]; then
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED host_os=$os_name" \
    'requires a supported Linux host (no Docker/compatible daemon fallback)' >&2
  exit 1
fi

box_binary="${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}"
armed="${A3S_CLOUD_BX0_CLEAN_HOST:-}"

if [[ $armed != 1 ]]; then
  print_checklist
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' \
    'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=A3S_CLOUD_BX0_CLEAN_HOST_unset' \
    'Set A3S_CLOUD_BX0_CLEAN_HOST=1 on a clean Linux host with a3s-box to attempt the harness.' >&2
  exit 2
fi

if [[ -z $box_binary || ! -x $box_binary ]]; then
  print_checklist
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' \
    'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=a3s-box_unavailable' \
    "Install pinned Box via: $INSTALL_BOX_RELEASE ABSOLUTE_EMPTY_INSTALL_DIRECTORY" \
    'Then export A3S_CLOUD_BOX_BIN to that install tree a3s-box binary.' >&2
  exit 2
fi

[[ -f $BOX_REVISION_FILE ]] || die "missing pin file: $BOX_REVISION_FILE"
expected_box_revision="$(<"$BOX_REVISION_FILE")"
[[ $expected_box_revision =~ ^[0-9a-f]{40}$ ]] ||
  die "Box pin is not an exact 40-hex revision: $expected_box_revision"

installed_box_revision="${A3S_CLOUD_BOX_REVISION:-}"
box_revision_sidecar="$(dirname "$box_binary")/BOX-REVISION"
if [[ -z $installed_box_revision && -f $box_revision_sidecar ]]; then
  installed_box_revision="$(<"$box_revision_sidecar")"
fi
if [[ -z $installed_box_revision ]]; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=box_revision_missing" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=box_revision_missing" \
    "expected pin=$expected_box_revision" \
    "set A3S_CLOUD_BOX_REVISION or place BOX-REVISION next to a3s-box (install_box_release.sh)" >&2
  exit 1
fi
if [[ $installed_box_revision != "$expected_box_revision" ]]; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=box_revision_mismatch" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=box_revision_mismatch" \
    "expected pin=$expected_box_revision" \
    "installed=$installed_box_revision" >&2
  exit 1
fi

# EXIT requires Cloud+Runtime+Box+Gateway+Power. Bind the pins that exist today;
# Power stays UNBOUND until PW0 lands a pin file (do not invent one).
expected_runtime_revision="$(
  require_exact_pin_file runtime "$RUNTIME_REVISION_FILE" runtime_revision_missing
)"
expected_gateway_revision="$(
  require_exact_pin_file gateway "$GATEWAY_REVISION_FILE" gateway_revision_missing
)"
cloud_revision="$(git -C "$CLOUD_ROOT" rev-parse HEAD 2>/dev/null || true)"
[[ $cloud_revision =~ ^[0-9a-f]{40}$ ]] || die "Cloud HEAD is not an exact revision"

# shellcheck source=bx0_clean_host_steps.sh
source "$SCRIPT_DIRECTORY/bx0_clean_host_steps.sh"

evidence_dir="${A3S_CLOUD_BX0_EVIDENCE_DIR:-}"
if [[ -z $evidence_dir ]]; then
  evidence_dir="$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-clean-host.XXXXXX")"
fi
mkdir -p -- "$evidence_dir"

if ! bx0_step_enroll_preflight "$evidence_dir"; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=node_agent_unavailable" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=node_agent_unavailable" \
    "evidence=$evidence_dir/01-enroll.txt" \
    'Set A3S_CLOUD_NODE_AGENT_BIN to an executable a3s-cloud-node-agent (preflight only; enroll not run).' >&2
  exit 1
fi

BX0_BOX_BINARY="$box_binary"
export BX0_BOX_BINARY
if ! bx0_step_oci_preflight "$evidence_dir"; then
  print_checklist
  oci_reason=oci_unavailable
  if [[ -f $evidence_dir/02-oci.txt ]]; then
    oci_reason="$(
      awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/02-oci.txt" 2>/dev/null || true
    )"
    [[ -n $oci_reason ]] || oci_reason=oci_unavailable
  fi
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=$oci_reason" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$oci_reason" \
    "evidence=$evidence_dir/02-oci.txt" \
    'Require a3s-oci + matching OCI-RUNTIME-REVISION (install_box_release.sh); OCI publish not run.' >&2
  exit 1
fi

if ! bx0_step_deploy_preflight "$evidence_dir"; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=control_plane_unavailable" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=control_plane_unavailable" \
    "evidence=$evidence_dir/03-deploy.txt" \
    'Set A3S_CLOUD_CONTROL_PLANE_BIN or A3S_CLOUD_DEV_API_BIN to a3s-cloud-control-plane (preflight only; deploy not run).' >&2
  exit 1
fi

if ! bx0_step_health_preflight "$evidence_dir"; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=health_probe_unavailable" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=health_probe_unavailable" \
    "evidence=$evidence_dir/04-health.txt" \
    'Set A3S_CLOUD_HEALTH_PROBE_BIN to curl (or equivalent); Ready probe not run.' >&2
  exit 1
fi

if ! bx0_step_https_preflight "$evidence_dir"; then
  print_checklist
  https_reason=gateway_unavailable
  if [[ -f $evidence_dir/05-https.txt ]]; then
    https_reason="$(
      awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/05-https.txt" 2>/dev/null || true
    )"
    [[ -n $https_reason ]] || https_reason=gateway_unavailable
  fi
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=$https_reason" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$https_reason" \
    "evidence=$evidence_dir/05-https.txt" \
    'Require a3s-gateway + matching GATEWAY-REVISION (tools/gateway-conformance/gateway-revision); TLS route not run.' >&2
  exit 1
fi

if ! bx0_step_logs_preflight "$evidence_dir"; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=logs_probe_unavailable" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=logs_probe_unavailable" \
    "evidence=$evidence_dir/06-logs.txt" \
    'Set A3S_CLOUD_LOGS_PROBE_BIN to jq (or equivalent); durable log readback not run.' >&2
  exit 1
fi

if ! bx0_step_update_preflight "$evidence_dir"; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=digest_probe_unavailable" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=digest_probe_unavailable" \
    "evidence=$evidence_dir/07-update.txt" \
    'Set A3S_CLOUD_DIGEST_PROBE_BIN to sha256sum/shasum (or equivalent); immutable update not run.' >&2
  exit 1
fi
bx0_write_remaining_open_steps "$evidence_dir"

printf 'BX0 clean-host gate: armed on Linux with a3s-box=%s\n' "$box_binary"
printf 'bound Cloud revision: %s\n' "$cloud_revision"
printf 'bound Runtime revision: %s\n' "$expected_runtime_revision"
printf 'bound Box revision: %s\n' "$expected_box_revision"
printf 'bound Gateway revision: %s\n' "$expected_gateway_revision"
printf 'power_revision=UNBOUND reason=pw0_no_pin_file\n'
printf 'step1_enroll=preflight_ok enroll=not_run\n'
printf 'step2_oci=preflight_ok oci=not_run\n'
printf 'step3_deploy=preflight_ok deploy=not_run\n'
printf 'step4_health=preflight_ok health=not_run\n'
printf 'step5_https=preflight_ok https=not_run\n'
printf 'step6_logs=preflight_ok logs=not_run\n'
printf 'step7_update=preflight_ok update=not_run\n'
printf 'steps8-9=OPEN not_run=1\n'
printf 'evidence_dir=%s\n' "$evidence_dir"
printf 'Cloud root: %s\n' "$CLOUD_ROOT"
printf 'install helper: %s\n' "$INSTALL_BOX_RELEASE"
print_checklist
print_required_markers

cat <<'OPEN'
A3S_CLOUD_BX0_CLEAN_HOST_OPEN
not yet automated / requires joint Cloud+Box+Gateway harness
This entrypoint refuses to fake EXIT_CERTIFIED.
bound=Cloud+Runtime+Box+Gateway power=UNBOUND
step1=enroll_preflight_ok step2=oci_preflight_ok step3=deploy_preflight_ok step4=health_preflight_ok step5=https_preflight_ok step6=logs_preflight_ok step7=update_preflight_ok steps8-9=not_run
OPEN
exit 3
