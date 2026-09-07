#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 foundation meta-gate.
# Runs every Use-Registry assignment foundation gate that does not require an
# operator-owned PostgreSQL URL or dual-version a3s-use embed:
#   - Use crash points 6–7 recovery
#   - Use Skill/Ui CognitivePackageHostManager converge
#   - Pinned Use golden contract fixtures (identity/catalog/plan/confirm/observe)
#   - Pinned Use scope isolation and package path/symlink fail-closed
#   - REST/client/CLI/MCP/architecture surface parity
#   - Cloud crash points 1–5 and 8–9
#   - Cloud Flow fail-closed/mutation matrix (deny, ask-timeout, already-
#     converged, uninstall→Removed, upgrade selection/enqueue)
#   - Node Agent shared PluginHostManager journal (Skill/Ui/uninstall/upgrade)
# Optional: set A3S_CLOUD_TEST_POSTGRES_URL to also run the assignment Postgres gate.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_u0_3_foundation_gate.sh EVIDENCE_DIRECTORY}
revision=$(<"$repository_root/tools/use-conformance/use-revision")
tools="$repository_root/tools/use-conformance"

[[ $revision =~ ^[0-9a-f]{40}$ ]]
mkdir -p -- "$evidence_directory"
: >"$evidence_directory/u0-3-foundation.log"

run_child() {
  local label=$1
  local script=$2
  local child_dir=$3
  echo "===== $label =====" | tee -a "$evidence_directory/u0-3-foundation.log"
  mkdir -p -- "$child_dir"
  set +e
  bash "$script" "$child_dir" 2>&1 | tee -a "$evidence_directory/u0-3-foundation.log"
  local status=${PIPESTATUS[0]}
  set -e
  if ((status != 0)); then
    echo "foundation child failed: $label" >&2
    exit "$status"
  fi
}

run_child "use-recovery" "$tools/run_use_recovery_gate.sh" "$evidence_directory/use-recovery"
run_child "use-host-converge" "$tools/run_use_host_converge_gate.sh" "$evidence_directory/use-host"
run_child "use-contract-fixtures" "$tools/run_use_contract_fixture_gate.sh" \
  "$evidence_directory/use-contract-fixtures"
run_child "use-scope-isolation" "$tools/run_use_scope_isolation_gate.sh" \
  "$evidence_directory/use-scope-isolation"
run_child "assignment-surface" "$tools/run_assignment_surface_gate.sh" "$evidence_directory/assignment-surface"

echo "===== cloud-crash-points =====" | tee -a "$evidence_directory/u0-3-foundation.log"
cd -- "$repository_root"
set +e
cargo test --locked -p a3s-cloud-control-plane --lib crash_point_ -- --nocapture \
  2>&1 | tee -a "$evidence_directory/u0-3-foundation.log"
crash_status=${PIPESTATUS[0]}
set -e
if ((crash_status != 0)); then
  exit "$crash_status"
fi
crash_passed=$(grep -cE '^test .+crash_point_.+ \.\.\. ok$' "$evidence_directory/u0-3-foundation.log" || true)
if ((crash_passed < 5)); then
  echo "expected at least 5 Cloud crash_point_ tests, found $crash_passed" >&2
  exit 1
fi

echo "===== flow-mutation-matrix =====" | tee -a "$evidence_directory/u0-3-foundation.log"
cd -- "$repository_root"
mutation_filters=(
  await_confirmation_denies_without_enqueueing_apply
  await_confirmation_ask_times_out_when_plan_expires
  enqueue_plan_short_circuits_when_already_converged
  enqueue_uninstall_plan_when_desired_absent_and_converges_to_removed
  enqueue_upgrade_plan_when_observed_package_drifts_and_converges
  selects_upgrade_when_observed_package_digests_drift
)
mutation_passed=0
for filter in "${mutation_filters[@]}"; do
  echo "----- $filter -----" | tee -a "$evidence_directory/u0-3-foundation.log"
  set +e
  cargo test --locked -p a3s-cloud-control-plane --lib "$filter" -- --nocapture \
    2>&1 | tee -a "$evidence_directory/u0-3-foundation.log"
  mutation_status=${PIPESTATUS[0]}
  set -e
  if ((mutation_status != 0)); then
    echo "flow mutation matrix failed: $filter" >&2
    exit "$mutation_status"
  fi
  filter_ok=$(grep -cE "^test .+${filter} \\.\\.\\. ok$" "$evidence_directory/u0-3-foundation.log" || true)
  if ((filter_ok < 1)); then
    echo "expected passing filter $filter" >&2
    exit 1
  fi
  mutation_passed=$((mutation_passed + 1))
done
if ((mutation_passed < 6)); then
  echo "expected 6 flow mutation matrix filters, found $mutation_passed" >&2
  exit 1
fi

echo "===== node-agent-plugin-host =====" | tee -a "$evidence_directory/u0-3-foundation.log"
cd -- "$repository_root"
node_agent_filters=(
  skill_only_plan_apply_observe_converges_through_shared_manager_port
  ui_only_plan_apply_observe_converges_through_shared_manager_port
  uninstall_plan_apply_observe_converges_through_shared_manager_port
  upgrade_plan_request_journals_exact_action_without_second_channel
  same_generation_plugin_stages_dispatch_through_only_the_shared_manager_port
)
node_agent_passed=0
for filter in "${node_agent_filters[@]}"; do
  echo "----- $filter -----" | tee -a "$evidence_directory/u0-3-foundation.log"
  set +e
  cargo test --locked -p a3s-cloud-node-agent --test plugin_host "$filter" -- --exact --nocapture \
    2>&1 | tee -a "$evidence_directory/u0-3-foundation.log"
  node_agent_status=${PIPESTATUS[0]}
  set -e
  if ((node_agent_status != 0)); then
    echo "node-agent plugin_host journal failed: $filter" >&2
    exit "$node_agent_status"
  fi
  filter_ok=$(grep -cE "^test ${filter} \\.\\.\\. ok$" "$evidence_directory/u0-3-foundation.log" || true)
  if ((filter_ok < 1)); then
    echo "expected passing node-agent filter $filter" >&2
    exit 1
  fi
  node_agent_passed=$((node_agent_passed + 1))
done
if ((node_agent_passed < 5)); then
  echo "expected 5 node-agent plugin_host filters, found $node_agent_passed" >&2
  exit 1
fi

postgres_check=skipped
if [[ -n ${A3S_CLOUD_TEST_POSTGRES_URL:-} ]]; then
  run_child "postgres-assignment" "$tools/run_postgres_assignment_gate.sh" \
    "$evidence_directory/postgres-assignment"
  postgres_check=included
fi

for required in \
  "$evidence_directory/use-recovery/use-recovery-certification.txt" \
  "$evidence_directory/use-host/use-host-converge-certification.txt" \
  "$evidence_directory/use-contract-fixtures/use-contract-fixtures-certification.txt" \
  "$evidence_directory/use-scope-isolation/use-scope-isolation-certification.txt" \
  "$evidence_directory/assignment-surface/assignment-surface-certification.txt"
do
  test -s "$required"
done

certification="A3S_CLOUD_U0_3_FOUNDATION_CERTIFIED revision=$revision crash_points_cloud=$crash_passed mutation_matrix=$mutation_passed node_agent_journal=$node_agent_passed postgres=$postgres_check checks=recovery+host+fixtures+scope+surface+crash+mutation+node-agent"
printf '%s\n' "$certification" | tee "$evidence_directory/u0-3-foundation-certification.txt"
