#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 product-exit audit.
# Verifies foundation-adjacent evidence that can run without a full meta-gate,
# then fails closed unless operator-owned live Cloud↔host certification exists.
# This gate must not claim A3S_CLOUD_U0_3_EXIT_CERTIFIED until live host evidence
# is present. It never embeds a3s-use.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_u0_3_exit_audit.sh EVIDENCE_DIRECTORY}
revision=$(<"$repository_root/tools/use-conformance/use-revision")
tools="$repository_root/tools/use-conformance"

[[ $revision =~ ^[0-9a-f]{40}$ ]]
[[ $(grep -Fc "rev = \"$revision\"" "$repository_root/Cargo.toml") -eq 2 ]]

mkdir -p -- "$evidence_directory"
audit_log="$evidence_directory/u0-3-exit-audit.log"
: >"$audit_log"
report="$evidence_directory/u0-3-exit-audit-report.txt"
: >"$report"

record() {
  local status=$1
  local item=$2
  printf '%s\t%s\n' "$status" "$item" | tee -a "$report" | tee -a "$audit_log"
}

echo "===== pin =====" | tee -a "$audit_log"
record PASS "use-revision and Cargo.toml pin=$revision"

echo "===== architecture-ratchet =====" | tee -a "$audit_log"
cd -- "$repository_root"
set +e
cargo test --locked -p a3s-cloud-control-plane --lib \
  plugins_u0_assignment_surface_owns_no_second_use_platform -- --exact --nocapture \
  2>&1 | tee -a "$audit_log"
arch_status=${PIPESTATUS[0]}
set -e
if ((arch_status != 0)); then
  record FAIL "no-second-Use-platform architecture ratchet"
  exit "$arch_status"
fi
record PASS "no-second-Use-platform architecture ratchet"

echo "===== contract-fixtures =====" | tee -a "$audit_log"
bash "$tools/run_use_contract_fixture_gate.sh" "$evidence_directory/use-contract-fixtures" \
  2>&1 | tee -a "$audit_log"
record PASS "pinned Use golden contract fixtures"

echo "===== scope-isolation =====" | tee -a "$audit_log"
bash "$tools/run_use_scope_isolation_gate.sh" "$evidence_directory/use-scope-isolation" \
  2>&1 | tee -a "$audit_log"
record PASS "pinned Use scope/path/symlink fail-closed"

echo "===== node-agent-enablement-journal =====" | tee -a "$audit_log"
cd -- "$repository_root"
set +e
cargo test --locked -p a3s-cloud-node-agent --test plugin_host \
  same_generation_plugin_stages_dispatch_through_only_the_shared_manager_port \
  -- --exact --nocapture 2>&1 | tee -a "$audit_log"
enable_status=${PIPESTATUS[0]}
set -e
if ((enable_status != 0)); then
  record FAIL "Node Agent enablement journal (same-generation plan/apply/enablement)"
  exit "$enable_status"
fi
record PASS "Node Agent enablement journal (same-generation plan/apply/enablement)"

live_status=OPEN
live_detail="operator-owned live Cloud↔host certification missing"
if [[ -n ${A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION:-} ]]; then
  if [[ -s $A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION ]] \
    && grep -Fq 'A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED' "$A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION"
  then
    live_status=PASS
    live_detail="live Cloud↔host certified via $A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION"
    cp -- "$A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION" \
      "$evidence_directory/live-host-certification.txt"
  else
    live_status=FAIL
    live_detail="A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION set but missing A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED"
  fi
fi
record "$live_status" "$live_detail"

if [[ $live_status != PASS ]]; then
  printf '%s\n' \
    "A3S_CLOUD_U0_3_EXIT_BLOCKED revision=$revision reason=live_cloud_host_unavailable" \
    | tee "$evidence_directory/u0-3-exit-certification.txt"
  echo "U0.3 product exit blocked: live Cloud↔host evidence required" >&2
  echo "Set A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION to an operator certification file containing A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED" >&2
  exit 2
fi

printf '%s\n' \
  "A3S_CLOUD_U0_3_EXIT_CERTIFIED revision=$revision live_host=included" \
  | tee "$evidence_directory/u0-3-exit-certification.txt"
