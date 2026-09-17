#!/usr/bin/env bash
# GA-0 / BX0.software joint exit harness.
#
# Mission: drive a clean Linux host to
#   A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED … profile=software
# without TEE, without Power, and without inventing receipts.
#
# Modes:
#   default / CI   — refuse-to-fake preflight only; never claims product EXIT
#   LIVE=1         — require a real stack and orchestrate gate→LOOP→EXIT
#                    (fail-closed at the first missing real capability)
#   LIVE=1 + CREATE=1 — run run_bx0_software_loop_create.sh when identities
#                    are incomplete (still never invents digests/EXIT/Power)
#
# Usage:
#   bash tools/box-conformance/run_bx0_software_exit_harness.sh [EVIDENCE_DIR]
#
# Env:
#   A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1    — attempt product EXIT (default: refuse-only)
#   A3S_CLOUD_BX0_SOFTWARE_EXIT_CREATE=1  — create enroll→deploy (…→cleanup if FULL)
#   A3S_CLOUD_BX0_PROBE_STRICT=1          — set automatically in LIVE mode
#
# See docs/ga0-bx0-software-checklist.md.

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
gate="$tools/run_bx0_clean_host_gate.sh"
collector="$tools/collect_bx0_clean_host_evidence.sh"
exit_audit="$tools/run_bx0_clean_host_exit_audit.sh"
probe_logs="$tools/probe_bx0_logs.sh"
probe_digest="$tools/probe_bx0_digest.sh"
probe_rollback="$tools/probe_bx0_rollback.sh"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-software-exit.XXXXXX")
fi
mkdir -p -- "$evidence_directory"
report="$evidence_directory/software-exit-harness-report.txt"
: >"$report"

record() {
  local status=$1
  local item=$2
  printf '%s\t%s\n' "$status" "$item" | tee -a "$report"
}

live=${A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE:-0}
os_name=$(uname -s)
arch_name=$(uname -m)

echo "===== BX0.software exit harness ====="
echo "evidence_dir=$evidence_directory"
echo "live=$live host_os=$os_name arch=$arch_name"

# --- Always-on refuse-to-fake contracts ---
for path in "$gate" "$collector" "$exit_audit" "$probe_logs" "$probe_digest" "$probe_rollback"; do
  [[ -f $path ]] || {
    record FAIL "missing required tool: $path"
    exit 1
  }
  bash -n "$path"
done
record PASS "syntax-check gate/collector/exit-audit/probes"

# Probes must reject PLACEHOLDER_*
set +e
bash "$probe_logs" 'PLACEHOLDER_cursor' >/dev/null 2>&1
logs_ph=$?
bash "$probe_digest" 'PLACEHOLDER_digest' >/dev/null 2>&1
digest_ph=$?
bash "$probe_rollback" 'PLACEHOLDER_digest' >/dev/null 2>&1
rollback_ph=$?
set -e
if ((logs_ph == 0 || digest_ph == 0 || rollback_ph == 0)); then
  record FAIL "probes must reject PLACEHOLDER_*"
  exit 1
fi
record PASS "probes reject PLACEHOLDER_*"

# Digests must reject malformed input
set +e
bash "$probe_digest" 'not-a-digest' >/dev/null 2>&1
bad_digest=$?
set -e
if ((bad_digest == 0)); then
  record FAIL "digest probe must reject malformed digests"
  exit 1
fi
record PASS "digest probe rejects malformed digests"

# Never invent repo Power pin
if [[ -f $repository_root/tools/power-conformance/power-revision ]]; then
  record FAIL "tools/power-conformance/power-revision must not exist for GA-0 software EXIT"
  exit 1
fi
record PASS "no invented Power pin"

# Export probe bins for assisted EXECUTE / LIVE
chmod +x -- "$probe_logs" "$probe_digest" "$probe_rollback" 2>/dev/null || true
export A3S_CLOUD_LOGS_PROBE_BIN="$probe_logs"
export A3S_CLOUD_DIGEST_PROBE_BIN="$probe_digest"
export A3S_CLOUD_ROLLBACK_PROBE_BIN="$probe_rollback"

if [[ $live != 1 ]]; then
  record PASS "LIVE unset: refuse-to-fake harness complete (product EXIT not claimed)"
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_HARNESS_CI_CERTIFIED cloud_revision=$(git -C "$repository_root" rev-parse HEAD) live=0 product_exit=not_claimed" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  cat <<EOF
Harness CI mode finished.
To attempt product EXIT on a clean Linux x86_64 host (no Docker sock):

  export A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1
  # Plus API/token/org/project/environment, pin-matched Box, control-plane, gateway
  bash tools/box-conformance/run_bx0_software_exit_harness.sh

EOF
  exit 0
fi

# --- LIVE path: fail closed; never invent EXIT ---
export A3S_CLOUD_BX0_PROBE_STRICT=1

if [[ $os_name != Linux || $arch_name != x86_64 ]]; then
  record FAIL "LIVE requires Linux x86_64 (got $os_name-$arch_name)"
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=host_unsupported" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  exit 2
fi

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
if ! bx0_refuse_docker_host; then
  record FAIL "LIVE refuses Docker / docker.sock (a3s-box only; no sock override theater)"
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=docker_sock_or_host" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  exit 2
fi
record PASS "clean-host Docker absence"

box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
if [[ -z $box_bin || ! -x $box_bin ]]; then
  record FAIL "LIVE requires pin-matched a3s-box (install_box_release.sh)"
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=a3s-box_unavailable" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  exit 2
fi
export A3S_CLOUD_BOX_BIN=$box_bin
record PASS "a3s-box resolvable"

# Require operator-owned live stack identity. Optionally CREATE enroll→deploy.
create_loop=${A3S_CLOUD_BX0_SOFTWARE_EXIT_CREATE:-0}
loop_create="$tools/run_bx0_software_loop_create.sh"
required_live=(
  A3S_CLOUD_URL
  A3S_CLOUD_TOKEN
  A3S_CLOUD_ORGANIZATION_ID
  A3S_CLOUD_PROJECT_ID
  A3S_CLOUD_ENVIRONMENT_ID
  A3S_CLOUD_BX0_NODE_CONFIG
  A3S_CLOUD_BX0_ENROLL_NODE_ID
  A3S_CLOUD_BX0_ARTIFACT_DIGEST
  A3S_CLOUD_BX0_SERVICE_ID
  A3S_CLOUD_BX0_WORKLOAD_ID
  A3S_CLOUD_BX0_REVISION_ID
  A3S_CLOUD_BX0_HEALTH_URL
  A3S_CLOUD_BX0_HTTPS_URL
  A3S_CLOUD_BX0_LOGS_CURSOR
  A3S_CLOUD_BX0_UPDATE_DIGEST
  A3S_CLOUD_BX0_ROLLBACK_DIGEST
  A3S_CLOUD_BX0_CLEANUP_INSTANCE
  A3S_CLOUD_ENROLLMENT_TOKEN
)
missing=()
for key in "${required_live[@]}"; do
  if [[ -z ${!key:-} || ${!key} == PLACEHOLDER_* ]]; then
    missing+=("$key")
  fi
done
if ((${#missing[@]} > 0)) && [[ $create_loop == 1 ]]; then
  record PASS "LIVE identities incomplete; invoking LOOP create (CREATE=1)"
  [[ -f $loop_create ]] || {
    record FAIL "missing loop create tool: $loop_create"
    exit 1
  }
  bash -n "$loop_create"
  create_dir="$evidence_directory/loop-create"
  mkdir -p -- "$create_dir"
  # CREATE_FULL so health→cleanup identities are produced when URLs/update digests exist.
  export A3S_CLOUD_BX0_CREATE_FULL=${A3S_CLOUD_BX0_CREATE_FULL:-1}
  export A3S_CLOUD_BX0_CREATE_OUT="$create_dir/bx0-software-loop-env.sh"
  set +e
  bash "$loop_create" "$create_dir" \
    >"$evidence_directory/loop-create.out" 2>"$evidence_directory/loop-create.err"
  create_rc=$?
  set -e
  if ((create_rc != 0)); then
    record FAIL "LOOP create failed (exit $create_rc)"
    cat "$evidence_directory/loop-create.out" >&2 || true
    cat "$evidence_directory/loop-create.err" >&2 || true
    printf '%s\n' \
      "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=loop_create_failed create_exit=$create_rc" \
      | tee "$evidence_directory/bx0-software-exit-harness.txt"
    exit 2
  fi
  # shellcheck disable=SC1090
  source "$A3S_CLOUD_BX0_CREATE_OUT"
  record PASS "LOOP create exported identities from $A3S_CLOUD_BX0_CREATE_OUT"
  missing=()
  for key in "${required_live[@]}"; do
    if [[ -z ${!key:-} || ${!key} == PLACEHOLDER_* ]]; then
      missing+=("$key")
    fi
  done
fi
if ((${#missing[@]} > 0)); then
  record FAIL "LIVE missing real enroll→…→cleanup identities: ${missing[*]}"
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-harness.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=live_identities_incomplete
missing=${missing[*]}
honesty=Harness refuses to invent enroll/deploy/HTTPS/log/update/rollback. Run CREATE=1 with a live stack + pre-published OCI digests (run_bx0_software_loop_create.sh), or export identities manually (OPERATOR_CLEAN_HOST.md).
product_exit=not_claimed
EOF
  exit 2
fi
record PASS "LIVE identity env complete (operator-owned or CREATE-produced; not invented)"

# Arm clean-host gate EXECUTE with STRICT probes → receipts
export A3S_CLOUD_BX0_CLEAN_HOST=1
export A3S_CLOUD_BX0_EXECUTE=1
set +e
bash "$gate" >"$evidence_directory/gate.out" 2>"$evidence_directory/gate.err"
gate_rc=$?
set -e
# Gate stays OPEN (exit 3) even with complete receipts — product EXIT is audit-only.
if ((gate_rc != 3)); then
  record FAIL "clean-host gate expected exit 3 OPEN, got $gate_rc"
  cat "$evidence_directory/gate.out" >&2 || true
  cat "$evidence_directory/gate.err" >&2 || true
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=gate_execute_failed gate_exit=$gate_rc" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  exit 2
fi
if ! grep -Fq 'execute_receipts_complete=1' "$evidence_directory/gate.out"; then
  record FAIL "gate did not report execute_receipts_complete=1"
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=execute_receipts_incomplete" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  exit 2
fi
gate_evidence=$(awk -F= '/^evidence_dir=/{print $2; exit}' "$evidence_directory/gate.out")
if [[ -z $gate_evidence || $gate_evidence != /* ]]; then
  record FAIL "gate evidence_dir missing/absolute"
  exit 2
fi
record PASS "clean-host EXECUTE receipts complete under $gate_evidence"

# Collect LOOP
loop_dir="$evidence_directory/loop"
mkdir -p -- "$loop_dir"
set +e
bash "$collector" \
  --host "$(hostname -f 2>/dev/null || hostname)" \
  --service-id "$A3S_CLOUD_BX0_SERVICE_ID" \
  --node-id "$A3S_CLOUD_BX0_ENROLL_NODE_ID" \
  --artifact-digest "$A3S_CLOUD_BX0_ARTIFACT_DIGEST" \
  --gate-evidence-dir "$gate_evidence" \
  --evidence-dir "$loop_dir" \
  >"$evidence_directory/collector.out" 2>"$evidence_directory/collector.err"
collector_rc=$?
set -e
if ((collector_rc != 0)); then
  record FAIL "LOOP collector failed"
  cat "$evidence_directory/collector.out" >&2 || true
  cat "$evidence_directory/collector.err" >&2 || true
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=loop_collection_failed" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  exit 2
fi
loop_cert="$loop_dir/bx0-clean-host-certification.txt"
[[ -f $loop_cert ]] || {
  record FAIL "LOOP certification file missing"
  exit 2
}
record PASS "LOOP certified"

# Software EXIT audit
export A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION=$loop_cert
export A3S_CLOUD_BX0_EVIDENCE_DIR=$gate_evidence
export A3S_CLOUD_BX0_EXIT_PROFILE=software
audit_dir="$evidence_directory/exit-audit"
set +e
bash "$exit_audit" "$audit_dir" \
  >"$evidence_directory/exit-audit.out" 2>"$evidence_directory/exit-audit.err"
audit_rc=$?
set -e
if ((audit_rc != 0)); then
  record FAIL "software EXIT audit failed"
  cat "$evidence_directory/exit-audit.out" >&2 || true
  cat "$evidence_directory/exit-audit.err" >&2 || true
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=exit_audit_failed" \
    | tee "$evidence_directory/bx0-software-exit-harness.txt"
  exit 2
fi
if ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$audit_dir/bx0-exit-certification.txt"; then
  record FAIL "EXIT_CERTIFIED line missing"
  exit 2
fi
if ! grep -Fq 'profile=software' "$audit_dir/bx0-exit-certification.txt"; then
  record FAIL "EXIT profile=software missing"
  exit 2
fi
record PASS "GA-0 software EXIT certified"
cp -- "$audit_dir/bx0-exit-certification.txt" \
  "$evidence_directory/bx0-software-exit-harness.txt"
exit 0
