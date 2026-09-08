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
if [[ $evidence_dir != /* ]]; then
  die "A3S_CLOUD_BX0_EVIDENCE_DIR must be an absolute path (got: $evidence_dir)"
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

if ! bx0_step_rollback_preflight "$evidence_dir"; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=rollback_probe_unavailable" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=rollback_probe_unavailable" \
    "evidence=$evidence_dir/08-rollback.txt" \
    'Set A3S_CLOUD_ROLLBACK_PROBE_BIN to diff/cmp (or equivalent); cloned rollback not run.' >&2
  exit 1
fi

if ! bx0_step_stop_cleanup_preflight "$evidence_dir"; then
  print_checklist
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED reason=cleanup_box_unavailable" \
    "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=cleanup_box_unavailable" \
    "evidence=$evidence_dir/09-stop_cleanup.txt" \
    'Require a resolvable a3s-box for stop/remove; cleanup not run.' >&2
  exit 1
fi

step1_status=enroll_preflight_ok
step1_enroll=not_run
step2_status=oci_preflight_ok
step2_oci=not_run
step3_status=deploy_preflight_ok
step3_deploy=not_run
step4_status=health_preflight_ok
step4_health=not_run
step5_status=https_preflight_ok
step5_https=not_run
step6_status=logs_preflight_ok
step6_logs=not_run
step7_status=update_preflight_ok
step7_update=not_run
step8_status=rollback_preflight_ok
step8_rollback=not_run
step9_status=stop_cleanup_preflight_ok
step9_stop_cleanup=not_run
if [[ ${A3S_CLOUD_BX0_EXECUTE:-} == 1 ]]; then
  if ! bx0_step_enroll_execute "$evidence_dir"; then
    print_checklist
    enroll_reason=enroll_execute_failed
    if [[ -f $evidence_dir/01-enroll.txt ]]; then
      enroll_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/01-enroll.txt" 2>/dev/null || true
      )"
      [[ -n $enroll_reason ]] || enroll_reason=enroll_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$enroll_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$enroll_reason" \
      "evidence=$evidence_dir/01-enroll.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires absolute node .acl, enrollment token, and A3S_CLOUD_BX0_ENROLL_NODE_ID from a real enroll.' >&2
    exit 1
  fi
  step1_status=enroll_executed
  step1_enroll=executed

  if ! bx0_step_oci_execute "$evidence_dir"; then
    print_checklist
    oci_reason=oci_execute_failed
    if [[ -f $evidence_dir/02-oci.txt ]]; then
      oci_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/02-oci.txt" 2>/dev/null || true
      )"
      [[ -n $oci_reason ]] || oci_reason=oci_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$oci_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$oci_reason" \
      "evidence=$evidence_dir/02-oci.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_ARTIFACT_DIGEST (sha256:64hex) from a real OCI publish.' >&2
    exit 1
  fi
  step2_status=oci_executed
  step2_oci=executed

  if ! bx0_step_deploy_execute "$evidence_dir"; then
    print_checklist
    deploy_reason=deploy_execute_failed
    if [[ -f $evidence_dir/03-deploy.txt ]]; then
      deploy_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/03-deploy.txt" 2>/dev/null || true
      )"
      [[ -n $deploy_reason ]] || deploy_reason=deploy_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$deploy_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$deploy_reason" \
      "evidence=$evidence_dir/03-deploy.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_SERVICE_ID from a real Box-hosted deploy.' >&2
    exit 1
  fi
  step3_status=deploy_executed
  step3_deploy=executed

  if ! bx0_step_health_execute "$evidence_dir"; then
    print_checklist
    health_reason=health_execute_failed
    if [[ -f $evidence_dir/04-health.txt ]]; then
      health_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/04-health.txt" 2>/dev/null || true
      )"
      [[ -n $health_reason ]] || health_reason=health_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$health_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$health_reason" \
      "evidence=$evidence_dir/04-health.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_HEALTH_URL (http/https) from a real Ready probe.' >&2
    exit 1
  fi
  step4_status=health_executed
  step4_health=executed

  if ! bx0_step_https_execute "$evidence_dir"; then
    print_checklist
    https_reason=https_execute_failed
    if [[ -f $evidence_dir/05-https.txt ]]; then
      https_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/05-https.txt" 2>/dev/null || true
      )"
      [[ -n $https_reason ]] || https_reason=https_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$https_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$https_reason" \
      "evidence=$evidence_dir/05-https.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_HTTPS_URL (https://) from a real managed-TLS reach.' >&2
    exit 1
  fi
  step5_status=https_executed
  step5_https=executed

  if ! bx0_step_logs_execute "$evidence_dir"; then
    print_checklist
    logs_reason=logs_execute_failed
    if [[ -f $evidence_dir/06-logs.txt ]]; then
      logs_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/06-logs.txt" 2>/dev/null || true
      )"
      [[ -n $logs_reason ]] || logs_reason=logs_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$logs_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$logs_reason" \
      "evidence=$evidence_dir/06-logs.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_LOGS_CURSOR from a real ordered-log read.' >&2
    exit 1
  fi
  step6_status=logs_executed
  step6_logs=executed

  if ! bx0_step_update_execute "$evidence_dir"; then
    print_checklist
    update_reason=update_execute_failed
    if [[ -f $evidence_dir/07-update.txt ]]; then
      update_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/07-update.txt" 2>/dev/null || true
      )"
      [[ -n $update_reason ]] || update_reason=update_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$update_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$update_reason" \
      "evidence=$evidence_dir/07-update.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_UPDATE_DIGEST (sha256:64hex) from a real immutable update.' >&2
    exit 1
  fi
  step7_status=update_executed
  step7_update=executed

  if ! bx0_step_rollback_execute "$evidence_dir"; then
    print_checklist
    rollback_reason=rollback_execute_failed
    if [[ -f $evidence_dir/08-rollback.txt ]]; then
      rollback_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/08-rollback.txt" 2>/dev/null || true
      )"
      [[ -n $rollback_reason ]] || rollback_reason=rollback_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$rollback_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$rollback_reason" \
      "evidence=$evidence_dir/08-rollback.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_ROLLBACK_DIGEST (sha256:64hex) from a real cloned rollback.' >&2
    exit 1
  fi
  step8_status=rollback_executed
  step8_rollback=executed

  if ! bx0_step_stop_cleanup_execute "$evidence_dir"; then
    print_checklist
    cleanup_reason=cleanup_execute_failed
    if [[ -f $evidence_dir/09-stop_cleanup.txt ]]; then
      cleanup_reason="$(
        awk -F= '/^reason=/{print $2; exit}' "$evidence_dir/09-stop_cleanup.txt" 2>/dev/null || true
      )"
      [[ -n $cleanup_reason ]] || cleanup_reason=cleanup_execute_failed
    fi
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=$cleanup_reason" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=$cleanup_reason" \
      "evidence=$evidence_dir/09-stop_cleanup.txt" \
      'A3S_CLOUD_BX0_EXECUTE=1 requires A3S_CLOUD_BX0_CLEANUP_INSTANCE from a real Box stop/remove.' >&2
    exit 1
  fi
  step9_status=stop_cleanup_executed
  step9_stop_cleanup=executed
fi

execute_receipts_complete=0
if [[ $step9_status == stop_cleanup_executed ]]; then
  # Re-verify on-disk steps 1–9 *=executed before claiming complete (status
  # alone is insufficient for LOOP/exit-audit consumers).
  if ! bx0_require_execute_receipts_dir "$evidence_dir"; then
    print_checklist
    printf '%s\n' \
      "BX0 clean-host gate: FAIL_CLOSED reason=execute_receipts_incomplete" \
      "A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=execute_receipts_incomplete" \
      "evidence_dir=$evidence_dir" \
      'EXECUTE claimed stop/cleanup but gate evidence is missing *=executed receipts.' >&2
    exit 1
  fi
  execute_receipts_complete=1
fi

printf 'BX0 clean-host gate: armed on Linux with a3s-box=%s\n' "$box_binary"
printf 'bound Cloud revision: %s\n' "$cloud_revision"
printf 'bound Runtime revision: %s\n' "$expected_runtime_revision"
printf 'bound Box revision: %s\n' "$expected_box_revision"
printf 'bound Gateway revision: %s\n' "$expected_gateway_revision"
printf 'power_revision=UNBOUND reason=pw0_no_pin_file\n'
printf 'step1_enroll=%s enroll=%s\n' "$step1_status" "$step1_enroll"
printf 'step2_oci=%s oci=%s\n' "$step2_status" "$step2_oci"
printf 'step3_deploy=%s deploy=%s\n' "$step3_status" "$step3_deploy"
printf 'step4_health=%s health=%s\n' "$step4_status" "$step4_health"
printf 'step5_https=%s https=%s\n' "$step5_status" "$step5_https"
printf 'step6_logs=%s logs=%s\n' "$step6_status" "$step6_logs"
printf 'step7_update=%s update=%s\n' "$step7_status" "$step7_update"
printf 'step8_rollback=%s rollback=%s\n' "$step8_status" "$step8_rollback"
printf 'step9_stop_cleanup=%s stop_cleanup=%s\n' "$step9_status" "$step9_stop_cleanup"
printf 'execute_receipts_complete=%s\n' "$execute_receipts_complete"
printf 'evidence_dir=%s\n' "$evidence_dir"
if ((execute_receipts_complete == 1)); then
  printf '%s\n' \
    'next_loop=collect_bx0_clean_host_evidence.sh' \
    "next_loop_gate_evidence_dir=$evidence_dir" \
    'next_exit=run_bx0_clean_host_exit_audit.sh' \
    'next_exit_requires=LOOP+gate_evidence+Power' \
    'product_exit=not_claimed'
fi
printf 'Cloud root: %s\n' "$CLOUD_ROOT"
printf 'install helper: %s\n' "$INSTALL_BOX_RELEASE"
print_checklist
print_required_markers

cat <<OPEN
A3S_CLOUD_BX0_CLEAN_HOST_OPEN
not yet automated / requires joint Cloud+Box+Gateway harness
This entrypoint refuses to fake EXIT_CERTIFIED.
bound=Cloud+Runtime+Box+Gateway power=UNBOUND
step1=${step1_status} step2=${step2_status} step3=${step3_status} step4=${step4_status} step5=${step5_status} step6=${step6_status} step7=${step7_status} step8=${step8_status} step9=${step9_status} execute_receipts_complete=${execute_receipts_complete} loop_exit=not_certified
OPEN
exit 3
