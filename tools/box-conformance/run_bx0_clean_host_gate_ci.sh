#!/usr/bin/env bash
set -euo pipefail

# Cloud BX0.5 clean-host gate CI fail-closed harness.
# Proves run_bx0_clean_host_gate.sh refuse-closes without claiming product
# A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED. Does NOT unlock product EXIT.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
gate="$tools/run_bx0_clean_host_gate.sh"
revision=$(<"$tools/box-revision")
[[ $revision =~ ^[0-9a-f]{40}$ ]]
[[ -x $gate || -f $gate ]]

evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-clean-host-ci.XXXXXX")
cleanup() {
  rm -rf -- "$evidence_directory"
}
trap cleanup EXIT

forbid_exit_certified_claim() {
  local path=$1
  # Documentation may list the bare marker name (checklist / refuses-to-fake).
  # A real product claim always continues with fields (cloud_revision=…).
  if grep -E --quiet \
    '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
    "$path" 2>/dev/null; then
    printf '%s\n' \
      "CI harness must never emit product EXIT_CERTIFIED claim: $path" >&2
    exit 1
  fi
}

echo "===== static: gate sources + fail-closed contracts ====="
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$gate"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_OPEN' "$gate"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' "$gate"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED' "$gate"
grep -Fq 'refuses to fake EXIT_CERTIFIED' "$gate"
grep -Fq 'box-revision' "$gate"
grep -Fq 'BOX-REVISION' "$gate"
grep -Fq 'box_revision_missing' "$gate"
grep -Fq 'box_revision_mismatch' "$gate"
grep -Fq 'runtime-revision' "$gate"
grep -Fq 'gateway-revision' "$gate"
grep -Fq 'runtime_revision_missing' "$gate"
grep -Fq 'gateway_revision_missing' "$gate"
grep -Fq 'pw0_no_pin_file' "$gate"
grep -Fq 'bound=Cloud+Runtime+Box+Gateway' "$gate"
grep -Fq 'bx0_clean_host_steps.sh' "$gate"
grep -Fq 'node_agent_unavailable' "$gate"
grep -Fq 'enroll=not_run' "$gate"
grep -Fq 'step1_status=enroll_preflight_ok' "$gate"
grep -Fq 'enroll_executed' "$gate"
grep -Fq 'A3S_CLOUD_BX0_EXECUTE' "$gate"
grep -Fq 'A3S_CLOUD_BX0_ENROLL_NODE_ID' "$gate"
grep -Fq 'execute_receipts_complete=' "$gate"
grep -Fq 'loop_exit=not_certified' "$gate"
grep -Fq 'oci_executed' "$gate"
grep -Fq 'deploy_executed' "$gate"
grep -Fq 'health_executed' "$gate"
grep -Fq 'https_executed' "$gate"
grep -Fq 'logs_executed' "$gate"
grep -Fq 'update_executed' "$gate"
grep -Fq 'rollback_executed' "$gate"
grep -Fq 'stop_cleanup_executed' "$gate"
grep -Fq 'A3S_CLOUD_BX0_ARTIFACT_DIGEST' "$gate"
grep -Fq 'A3S_CLOUD_BX0_SERVICE_ID' "$gate"
grep -Fq 'A3S_CLOUD_BX0_HEALTH_URL' "$gate"
grep -Fq 'A3S_CLOUD_BX0_HTTPS_URL' "$gate"
grep -Fq 'A3S_CLOUD_BX0_LOGS_CURSOR' "$gate"
grep -Fq 'A3S_CLOUD_BX0_UPDATE_DIGEST' "$gate"
grep -Fq 'A3S_CLOUD_BX0_ROLLBACK_DIGEST' "$gate"
grep -Fq 'A3S_CLOUD_BX0_CLEANUP_INSTANCE' "$gate"
grep -Fq 'step2_status=oci_preflight_ok' "$gate"
grep -Fq 'step3_status=deploy_preflight_ok' "$gate"
grep -Fq 'step4_status=health_preflight_ok' "$gate"
grep -Fq 'step5_status=https_preflight_ok' "$gate"
grep -Fq 'step6_status=logs_preflight_ok' "$gate"
grep -Fq 'step7_status=update_preflight_ok' "$gate"
grep -Fq 'step8_status=rollback_preflight_ok' "$gate"
grep -Fq 'step9_status=stop_cleanup_preflight_ok' "$gate"
grep -Fq 'oci=not_run' "$gate"
grep -Fq 'deploy=not_run' "$gate"
grep -Fq 'health=not_run' "$gate"
grep -Fq 'https=not_run' "$gate"
grep -Fq 'logs=not_run' "$gate"
grep -Fq 'update=not_run' "$gate"
grep -Fq 'rollback=not_run' "$gate"
grep -Fq 'stop_cleanup=not_run' "$gate"
grep -Fq 'control_plane_unavailable' "$gate"
grep -Fq 'health_probe_unavailable' "$gate"
grep -Fq 'gateway_unavailable' "$gate"
grep -Fq 'logs_probe_unavailable' "$gate"
grep -Fq 'digest_probe_unavailable' "$gate"
grep -Fq 'rollback_probe_unavailable' "$gate"
grep -Fq 'cleanup_box_unavailable' "$gate"
grep -Eq 'exit 1' "$gate"
grep -Eq 'exit 2' "$gate"
grep -Eq 'exit 3' "$gate"
steps="$tools/bx0_clean_host_steps.sh"
[[ -f $steps ]]
grep -Fq 'oci_unavailable' "$steps"
grep -Fq 'oci_runtime_revision_mismatch' "$steps"
grep -Fq 'control_plane_unavailable' "$steps"
grep -Fq 'health_probe_unavailable' "$steps"
grep -Fq 'gateway_unavailable' "$steps"
grep -Fq 'gateway_revision_mismatch' "$steps"
grep -Fq 'logs_probe_unavailable' "$steps"
grep -Fq 'digest_probe_unavailable' "$steps"
grep -Fq 'rollback_probe_unavailable' "$steps"
grep -Fq 'cleanup_box_unavailable' "$steps"
grep -Fq 'enroll_node_id_missing' "$steps"
grep -Fq 'bx0_step_enroll_execute' "$steps"
grep -Fq 'artifact_digest_missing' "$steps"
grep -Fq 'bx0_step_oci_execute' "$steps"
grep -Fq 'service_id_missing' "$steps"
grep -Fq 'bx0_step_deploy_execute' "$steps"
grep -Fq 'health_url_missing' "$steps"
grep -Fq 'bx0_step_health_execute' "$steps"
grep -Fq 'https_url_missing' "$steps"
grep -Fq 'bx0_step_https_execute' "$steps"
grep -Fq 'logs_cursor_missing' "$steps"
grep -Fq 'bx0_step_logs_execute' "$steps"
grep -Fq 'update_digest_missing' "$steps"
grep -Fq 'bx0_step_update_execute' "$steps"
grep -Fq 'rollback_digest_missing' "$steps"
grep -Fq 'bx0_step_rollback_execute' "$steps"
grep -Fq 'cleanup_instance_missing' "$steps"
grep -Fq 'bx0_step_stop_cleanup_execute' "$steps"
bash -n "$steps"
bash -n "$gate"
bash -n "$tools/run_bx0_clean_host_prep.sh"
runtime_pin="$tools/../runtime-conformance/runtime-revision"
gateway_pin="$tools/../gateway-conformance/gateway-revision"
oci_pin="$tools/oci-runtime-revision"
[[ -f $runtime_pin && -f $gateway_pin && -f $oci_pin ]]
runtime_revision=$(<"$runtime_pin")
gateway_revision=$(<"$gateway_pin")
oci_runtime_revision=$(<"$oci_pin")
[[ $runtime_revision =~ ^[0-9a-f]{40}$ ]]
[[ $gateway_revision =~ ^[0-9a-f]{40}$ ]]
[[ $oci_runtime_revision =~ ^[0-9a-f]{40}$ ]]
# No success emission of EXIT_CERTIFIED (printf/echo/cat claiming it).
if grep -E '^([[:space:]]*)(printf|echo|cat).*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$gate" \
  | grep -Ev 'Required certification markers|not emitted|refuses to fake|Must bind'; then
  printf '%s\n' "gate must not printf/echo product EXIT_CERTIFIED" >&2
  exit 1
fi

echo "===== step library: enroll preflight (Darwin-safe) ====="
# shellcheck source=bx0_clean_host_steps.sh
# shellcheck disable=SC1090
source "$steps"
steps_evidence="$evidence_directory/steps-lib"
mkdir -p -- "$steps_evidence"
unset A3S_CLOUD_NODE_AGENT_BIN || true
# Negative case: empty Cloud root + stripped PATH so cargo/PATH agents cannot resolve.
set +e
CLOUD_ROOT="$steps_evidence/empty-cloud-root" \
  PATH="/usr/bin:/bin" \
  env -u A3S_CLOUD_NODE_AGENT_BIN \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_enroll_preflight "$1"
  ' "$steps" "$steps_evidence/missing"
missing_enroll=$?
set -e
if ((missing_enroll != 1)); then
  printf '%s\n' "expected enroll preflight fail without node-agent, got $missing_enroll" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/missing/01-enroll.txt"
grep -Fq 'enroll=not_run' "$steps_evidence/missing/01-enroll.txt"
forbid_exit_certified_claim "$steps_evidence/missing/01-enroll.txt"

stub_agent="$steps_evidence/stub-a3s-cloud-node-agent"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_agent"
chmod +x "$stub_agent"
CLOUD_ROOT="$repository_root" \
  A3S_CLOUD_NODE_AGENT_BIN="$stub_agent" \
  bx0_step_enroll_preflight "$steps_evidence/ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/ok/01-enroll.txt"
grep -Fq 'enroll=not_run' "$steps_evidence/ok/01-enroll.txt"
grep -Fq "$stub_agent" "$steps_evidence/ok/01-enroll.txt"
forbid_exit_certified_claim "$steps_evidence/ok/01-enroll.txt"

echo "===== step library: enroll execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_NODE_AGENT_BIN="$stub_agent" \
  env -u A3S_CLOUD_BX0_NODE_CONFIG -u A3S_CLOUD_BX0_ENROLL_NODE_ID \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_enroll_execute "$1"
  ' "$steps" "$steps_evidence/enroll-exec-no-config"
missing_exec_config=$?
set -e
if ((missing_exec_config != 1)); then
  printf '%s\n' "expected enroll execute fail without node config, got $missing_exec_config" >&2
  exit 1
fi
grep -Fq 'node_config_unavailable' "$steps_evidence/enroll-exec-no-config/01-enroll.txt"
grep -Fq 'enroll=not_run' "$steps_evidence/enroll-exec-no-config/01-enroll.txt"
forbid_exit_certified_claim "$steps_evidence/enroll-exec-no-config/01-enroll.txt"

stub_node_acl="$steps_evidence/stub-node.acl"
cat >"$stub_node_acl" <<'ACL'
control_plane {
  enrollment_url = "http://127.0.0.1:8080/api/v1/node-control/enroll"
  node_control_url = "https://localhost:8443"
  enrollment_token_env = "A3S_CLOUD_ENROLLMENT_TOKEN"
}
ACL
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_NODE_AGENT_BIN="$stub_agent" \
  A3S_CLOUD_BX0_NODE_CONFIG="$stub_node_acl" \
  A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_test_token_not_placeholder' \
  env -u A3S_CLOUD_BX0_ENROLL_NODE_ID \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_enroll_execute "$1"
  ' "$steps" "$steps_evidence/enroll-exec-no-node"
missing_exec_node=$?
set -e
if ((missing_exec_node != 1)); then
  printf '%s\n' "expected enroll execute fail without node_id, got $missing_exec_node" >&2
  exit 1
fi
grep -Fq 'enroll_node_id_missing' "$steps_evidence/enroll-exec-no-node/01-enroll.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_NODE_AGENT_BIN="$stub_agent" \
  A3S_CLOUD_BX0_NODE_CONFIG="$stub_node_acl" \
  A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_test_token_not_placeholder' \
  A3S_CLOUD_BX0_ENROLL_NODE_ID='11111111-2222-3333-4444-555555555555' \
  bx0_step_enroll_execute "$steps_evidence/enroll-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/enroll-exec-ok/01-enroll.txt"
grep -Fq 'enroll=executed' "$steps_evidence/enroll-exec-ok/01-enroll.txt"
grep -Fq '11111111-2222-3333-4444-555555555555' "$steps_evidence/enroll-exec-ok/01-enroll.txt"
forbid_exit_certified_claim "$steps_evidence/enroll-exec-ok/01-enroll.txt"

echo "===== prep script prints recipe without EXIT ====="
bash "$tools/run_bx0_clean_host_prep.sh" \
  >"$evidence_directory/prep.out" 2>"$evidence_directory/prep.err"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_PREP' "$evidence_directory/prep.out"
grep -Fq 'product_exit=not_claimed' "$evidence_directory/prep.out"
forbid_exit_certified_claim "$evidence_directory/prep.out"
forbid_exit_certified_claim "$evidence_directory/prep.err"

echo "===== step library: OCI preflight (Darwin-safe) ====="
set +e
CLOUD_ROOT="$repository_root" \
  PATH="/usr/bin:/bin" \
  env -u A3S_CLOUD_OCI_BIN -u A3S_CLOUD_BOX_BIN -u BX0_BOX_BINARY \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_oci_preflight "$1"
  ' "$steps" "$steps_evidence/oci-missing"
missing_oci=$?
set -e
if ((missing_oci != 1)); then
  printf '%s\n' "expected OCI preflight fail without a3s-oci, got $missing_oci" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/oci-missing/02-oci.txt"
grep -Fq 'oci_unavailable' "$steps_evidence/oci-missing/02-oci.txt"
grep -Fq 'oci=not_run' "$steps_evidence/oci-missing/02-oci.txt"
forbid_exit_certified_claim "$steps_evidence/oci-missing/02-oci.txt"

oci_install="$steps_evidence/oci-install"
mkdir -p -- "$oci_install"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$oci_install/a3s-oci"
chmod +x "$oci_install/a3s-oci"
printf '%s\n' '0000000000000000000000000000000000000000' \
  >"$oci_install/OCI-RUNTIME-REVISION"
set +e
CLOUD_ROOT="$repository_root" \
  A3S_CLOUD_OCI_BIN="$oci_install/a3s-oci" \
  env -u A3S_CLOUD_OCI_RUNTIME_REVISION \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_oci_preflight "$1"
  ' "$steps" "$steps_evidence/oci-mismatch"
mismatch_oci=$?
set -e
if ((mismatch_oci != 1)); then
  printf '%s\n' "expected OCI pin mismatch fail, got $mismatch_oci" >&2
  exit 1
fi
grep -Fq 'oci_runtime_revision_mismatch' "$steps_evidence/oci-mismatch/02-oci.txt"

printf '%s\n' "$oci_runtime_revision" >"$oci_install/OCI-RUNTIME-REVISION"
unset A3S_CLOUD_OCI_RUNTIME_REVISION || true
CLOUD_ROOT="$repository_root" \
  A3S_CLOUD_OCI_BIN="$oci_install/a3s-oci" \
  bx0_step_oci_preflight "$steps_evidence/oci-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/oci-ok/02-oci.txt"
grep -Fq 'oci=not_run' "$steps_evidence/oci-ok/02-oci.txt"
grep -Fq "$oci_runtime_revision" "$steps_evidence/oci-ok/02-oci.txt"
forbid_exit_certified_claim "$steps_evidence/oci-ok/02-oci.txt"

echo "===== step library: OCI execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_OCI_BIN="$oci_install/a3s-oci" \
  env -u A3S_CLOUD_BX0_ARTIFACT_DIGEST \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_oci_execute "$1"
  ' "$steps" "$steps_evidence/oci-exec-missing"
missing_oci_exec=$?
set -e
if ((missing_oci_exec != 1)); then
  printf '%s\n' "expected OCI execute fail without digest, got $missing_oci_exec" >&2
  exit 1
fi
grep -Fq 'artifact_digest_missing' "$steps_evidence/oci-exec-missing/02-oci.txt"
grep -Fq 'oci=not_run' "$steps_evidence/oci-exec-missing/02-oci.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_OCI_BIN="$oci_install/a3s-oci" \
  A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
  bx0_step_oci_execute "$steps_evidence/oci-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/oci-exec-ok/02-oci.txt"
grep -Fq 'oci=executed' "$steps_evidence/oci-exec-ok/02-oci.txt"
forbid_exit_certified_claim "$steps_evidence/oci-exec-ok/02-oci.txt"

echo "===== step library: deploy preflight (Darwin-safe) ====="
set +e
CLOUD_ROOT="$steps_evidence/empty-cloud-root" \
  PATH="/usr/bin:/bin" \
  env -u A3S_CLOUD_CONTROL_PLANE_BIN -u A3S_CLOUD_DEV_API_BIN \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_deploy_preflight "$1"
  ' "$steps" "$steps_evidence/deploy-missing"
missing_deploy=$?
set -e
if ((missing_deploy != 1)); then
  printf '%s\n' "expected deploy preflight fail without control-plane, got $missing_deploy" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/deploy-missing/03-deploy.txt"
grep -Fq 'control_plane_unavailable' "$steps_evidence/deploy-missing/03-deploy.txt"
grep -Fq 'deploy=not_run' "$steps_evidence/deploy-missing/03-deploy.txt"
forbid_exit_certified_claim "$steps_evidence/deploy-missing/03-deploy.txt"

stub_cp="$steps_evidence/stub-a3s-cloud-control-plane"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_cp"
chmod +x "$stub_cp"
unset A3S_CLOUD_DEV_API_BIN || true
CLOUD_ROOT="$repository_root" \
  A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp" \
  bx0_step_deploy_preflight "$steps_evidence/deploy-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/deploy-ok/03-deploy.txt"
grep -Fq 'deploy=not_run' "$steps_evidence/deploy-ok/03-deploy.txt"
grep -Fq "$stub_cp" "$steps_evidence/deploy-ok/03-deploy.txt"
forbid_exit_certified_claim "$steps_evidence/deploy-ok/03-deploy.txt"

echo "===== step library: deploy execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp" \
  env -u A3S_CLOUD_BX0_SERVICE_ID -u A3S_CLOUD_DEV_API_BIN \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_deploy_execute "$1"
  ' "$steps" "$steps_evidence/deploy-exec-missing"
missing_deploy_exec=$?
set -e
if ((missing_deploy_exec != 1)); then
  printf '%s\n' "expected deploy execute fail without service_id, got $missing_deploy_exec" >&2
  exit 1
fi
grep -Fq 'service_id_missing' "$steps_evidence/deploy-exec-missing/03-deploy.txt"
grep -Fq 'deploy=not_run' "$steps_evidence/deploy-exec-missing/03-deploy.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp" \
  A3S_CLOUD_BX0_SERVICE_ID='svc-execute-1' \
  bx0_step_deploy_execute "$steps_evidence/deploy-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/deploy-exec-ok/03-deploy.txt"
grep -Fq 'deploy=executed' "$steps_evidence/deploy-exec-ok/03-deploy.txt"
grep -Fq 'svc-execute-1' "$steps_evidence/deploy-exec-ok/03-deploy.txt"
forbid_exit_certified_claim "$steps_evidence/deploy-exec-ok/03-deploy.txt"

echo "===== step library: health preflight (Darwin-safe) ====="
set +e
PATH="/usr/bin:/bin" \
  A3S_CLOUD_HEALTH_PROBE_BIN="$steps_evidence/does-not-exist-health-probe" \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_health_preflight "$1"
  ' "$steps" "$steps_evidence/health-missing"
missing_health=$?
set -e
if ((missing_health != 1)); then
  printf '%s\n' "expected health preflight fail without probe, got $missing_health" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/health-missing/04-health.txt"
grep -Fq 'health_probe_unavailable' "$steps_evidence/health-missing/04-health.txt"
grep -Fq 'health=not_run' "$steps_evidence/health-missing/04-health.txt"
forbid_exit_certified_claim "$steps_evidence/health-missing/04-health.txt"

stub_probe="$steps_evidence/stub-health-probe"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_probe"
chmod +x "$stub_probe"
A3S_CLOUD_HEALTH_PROBE_BIN="$stub_probe" \
  bx0_step_health_preflight "$steps_evidence/health-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/health-ok/04-health.txt"
grep -Fq 'health=not_run' "$steps_evidence/health-ok/04-health.txt"
grep -Fq "$stub_probe" "$steps_evidence/health-ok/04-health.txt"
forbid_exit_certified_claim "$steps_evidence/health-ok/04-health.txt"

echo "===== step library: health execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_HEALTH_PROBE_BIN="$stub_probe" \
  env -u A3S_CLOUD_BX0_HEALTH_URL \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_health_execute "$1"
  ' "$steps" "$steps_evidence/health-exec-missing"
missing_health_exec=$?
set -e
if ((missing_health_exec != 1)); then
  printf '%s\n' "expected health execute fail without URL, got $missing_health_exec" >&2
  exit 1
fi
grep -Fq 'health_url_missing' "$steps_evidence/health-exec-missing/04-health.txt"
grep -Fq 'health=not_run' "$steps_evidence/health-exec-missing/04-health.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_HEALTH_PROBE_BIN="$stub_probe" \
  A3S_CLOUD_BX0_HEALTH_URL='http://127.0.0.1:18080/ready' \
  bx0_step_health_execute "$steps_evidence/health-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/health-exec-ok/04-health.txt"
grep -Fq 'health=executed' "$steps_evidence/health-exec-ok/04-health.txt"
grep -Fq 'http://127.0.0.1:18080/ready' "$steps_evidence/health-exec-ok/04-health.txt"
forbid_exit_certified_claim "$steps_evidence/health-exec-ok/04-health.txt"

echo "===== step library: HTTPS / Gateway preflight (Darwin-safe) ====="
set +e
CLOUD_ROOT="$steps_evidence/empty-cloud-root" \
  PATH="/usr/bin:/bin" \
  env -u A3S_CLOUD_GATEWAY_BIN -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_https_preflight "$1"
  ' "$steps" "$steps_evidence/https-missing"
missing_https=$?
set -e
if ((missing_https != 1)); then
  printf '%s\n' "expected HTTPS preflight fail without gateway, got $missing_https" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/https-missing/05-https.txt"
grep -Fq 'https=not_run' "$steps_evidence/https-missing/05-https.txt"
forbid_exit_certified_claim "$steps_evidence/https-missing/05-https.txt"

gw_install="$steps_evidence/gw-install"
mkdir -p -- "$gw_install"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$gw_install/a3s-gateway"
chmod +x "$gw_install/a3s-gateway"
printf '%s\n' '0000000000000000000000000000000000000000' \
  >"$gw_install/GATEWAY-REVISION"
set +e
CLOUD_ROOT="$repository_root" \
  A3S_CLOUD_GATEWAY_BIN="$gw_install/a3s-gateway" \
  env -u A3S_CLOUD_GATEWAY_REVISION -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_https_preflight "$1"
  ' "$steps" "$steps_evidence/https-mismatch"
mismatch_https=$?
set -e
if ((mismatch_https != 1)); then
  printf '%s\n' "expected Gateway pin mismatch fail, got $mismatch_https" >&2
  exit 1
fi
grep -Fq 'gateway_revision_mismatch' "$steps_evidence/https-mismatch/05-https.txt"

printf '%s\n' "$gateway_revision" >"$gw_install/GATEWAY-REVISION"
unset A3S_CLOUD_GATEWAY_REVISION A3S_CLOUD_TEST_GATEWAY_REVISION || true
CLOUD_ROOT="$repository_root" \
  A3S_CLOUD_GATEWAY_BIN="$gw_install/a3s-gateway" \
  bx0_step_https_preflight "$steps_evidence/https-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/https-ok/05-https.txt"
grep -Fq 'https=not_run' "$steps_evidence/https-ok/05-https.txt"
grep -Fq "$gateway_revision" "$steps_evidence/https-ok/05-https.txt"
forbid_exit_certified_claim "$steps_evidence/https-ok/05-https.txt"

echo "===== step library: HTTPS execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_GATEWAY_BIN="$gw_install/a3s-gateway" \
  env -u A3S_CLOUD_BX0_HTTPS_URL \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_https_execute "$1"
  ' "$steps" "$steps_evidence/https-exec-missing"
missing_https_exec=$?
set -e
if ((missing_https_exec != 1)); then
  printf '%s\n' "expected HTTPS execute fail without URL, got $missing_https_exec" >&2
  exit 1
fi
grep -Fq 'https_url_missing' "$steps_evidence/https-exec-missing/05-https.txt"
grep -Fq 'https=not_run' "$steps_evidence/https-exec-missing/05-https.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_GATEWAY_BIN="$gw_install/a3s-gateway" \
  A3S_CLOUD_BX0_HTTPS_URL='https://svc.example.test/' \
  bx0_step_https_execute "$steps_evidence/https-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/https-exec-ok/05-https.txt"
grep -Fq 'https=executed' "$steps_evidence/https-exec-ok/05-https.txt"
grep -Fq 'https://svc.example.test/' "$steps_evidence/https-exec-ok/05-https.txt"
forbid_exit_certified_claim "$steps_evidence/https-exec-ok/05-https.txt"

echo "===== step library: logs preflight (Darwin-safe) ====="
set +e
PATH="/usr/bin:/bin" \
  A3S_CLOUD_LOGS_PROBE_BIN="$steps_evidence/does-not-exist-logs-probe" \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_logs_preflight "$1"
  ' "$steps" "$steps_evidence/logs-missing"
missing_logs=$?
set -e
if ((missing_logs != 1)); then
  printf '%s\n' "expected logs preflight fail without probe, got $missing_logs" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/logs-missing/06-logs.txt"
grep -Fq 'logs_probe_unavailable' "$steps_evidence/logs-missing/06-logs.txt"
grep -Fq 'logs=not_run' "$steps_evidence/logs-missing/06-logs.txt"
forbid_exit_certified_claim "$steps_evidence/logs-missing/06-logs.txt"

stub_logs="$steps_evidence/stub-logs-probe"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_logs"
chmod +x "$stub_logs"
A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs" \
  bx0_step_logs_preflight "$steps_evidence/logs-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/logs-ok/06-logs.txt"
grep -Fq 'logs=not_run' "$steps_evidence/logs-ok/06-logs.txt"
grep -Fq "$stub_logs" "$steps_evidence/logs-ok/06-logs.txt"
forbid_exit_certified_claim "$steps_evidence/logs-ok/06-logs.txt"

echo "===== step library: logs execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs" \
  env -u A3S_CLOUD_BX0_LOGS_CURSOR \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_logs_execute "$1"
  ' "$steps" "$steps_evidence/logs-exec-missing"
missing_logs_exec=$?
set -e
if ((missing_logs_exec != 1)); then
  printf '%s\n' "expected logs execute fail without cursor, got $missing_logs_exec" >&2
  exit 1
fi
grep -Fq 'logs_cursor_missing' "$steps_evidence/logs-exec-missing/06-logs.txt"
grep -Fq 'logs=not_run' "$steps_evidence/logs-exec-missing/06-logs.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs" \
  A3S_CLOUD_BX0_LOGS_CURSOR='cursor-execute-1' \
  bx0_step_logs_execute "$steps_evidence/logs-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/logs-exec-ok/06-logs.txt"
grep -Fq 'logs=executed' "$steps_evidence/logs-exec-ok/06-logs.txt"
grep -Fq 'cursor-execute-1' "$steps_evidence/logs-exec-ok/06-logs.txt"
forbid_exit_certified_claim "$steps_evidence/logs-exec-ok/06-logs.txt"

echo "===== step library: update / digest preflight (Darwin-safe) ====="
set +e
PATH="/usr/bin:/bin" \
  A3S_CLOUD_DIGEST_PROBE_BIN="$steps_evidence/does-not-exist-digest-probe" \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_update_preflight "$1"
  ' "$steps" "$steps_evidence/update-missing"
missing_update=$?
set -e
if ((missing_update != 1)); then
  printf '%s\n' "expected update preflight fail without digest probe, got $missing_update" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/update-missing/07-update.txt"
grep -Fq 'digest_probe_unavailable' "$steps_evidence/update-missing/07-update.txt"
grep -Fq 'update=not_run' "$steps_evidence/update-missing/07-update.txt"
forbid_exit_certified_claim "$steps_evidence/update-missing/07-update.txt"

stub_digest="$steps_evidence/stub-digest-probe"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_digest"
chmod +x "$stub_digest"
A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest" \
  bx0_step_update_preflight "$steps_evidence/update-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/update-ok/07-update.txt"
grep -Fq 'update=not_run' "$steps_evidence/update-ok/07-update.txt"
grep -Fq "$stub_digest" "$steps_evidence/update-ok/07-update.txt"
forbid_exit_certified_claim "$steps_evidence/update-ok/07-update.txt"

echo "===== step library: update execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest" \
  env -u A3S_CLOUD_BX0_UPDATE_DIGEST \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_update_execute "$1"
  ' "$steps" "$steps_evidence/update-exec-missing"
missing_update_exec=$?
set -e
if ((missing_update_exec != 1)); then
  printf '%s\n' "expected update execute fail without digest, got $missing_update_exec" >&2
  exit 1
fi
grep -Fq 'update_digest_missing' "$steps_evidence/update-exec-missing/07-update.txt"
grep -Fq 'update=not_run' "$steps_evidence/update-exec-missing/07-update.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest" \
  A3S_CLOUD_BX0_UPDATE_DIGEST='sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210' \
  bx0_step_update_execute "$steps_evidence/update-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/update-exec-ok/07-update.txt"
grep -Fq 'update=executed' "$steps_evidence/update-exec-ok/07-update.txt"
grep -Fq 'sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210' "$steps_evidence/update-exec-ok/07-update.txt"
forbid_exit_certified_claim "$steps_evidence/update-exec-ok/07-update.txt"

echo "===== step library: rollback preflight (Darwin-safe) ====="
set +e
PATH="/usr/bin:/bin" \
  A3S_CLOUD_ROLLBACK_PROBE_BIN="$steps_evidence/does-not-exist-rollback-probe" \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_rollback_preflight "$1"
  ' "$steps" "$steps_evidence/rollback-missing"
missing_rollback=$?
set -e
if ((missing_rollback != 1)); then
  printf '%s\n' "expected rollback preflight fail without probe, got $missing_rollback" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/rollback-missing/08-rollback.txt"
grep -Fq 'rollback_probe_unavailable' "$steps_evidence/rollback-missing/08-rollback.txt"
grep -Fq 'rollback=not_run' "$steps_evidence/rollback-missing/08-rollback.txt"
forbid_exit_certified_claim "$steps_evidence/rollback-missing/08-rollback.txt"

stub_rollback="$steps_evidence/stub-rollback-probe"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_rollback"
chmod +x "$stub_rollback"
A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback" \
  bx0_step_rollback_preflight "$steps_evidence/rollback-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/rollback-ok/08-rollback.txt"
grep -Fq 'rollback=not_run' "$steps_evidence/rollback-ok/08-rollback.txt"
grep -Fq "$stub_rollback" "$steps_evidence/rollback-ok/08-rollback.txt"
forbid_exit_certified_claim "$steps_evidence/rollback-ok/08-rollback.txt"

echo "===== step library: rollback execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback" \
  env -u A3S_CLOUD_BX0_ROLLBACK_DIGEST \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_rollback_execute "$1"
  ' "$steps" "$steps_evidence/rollback-exec-missing"
missing_rollback_exec=$?
set -e
if ((missing_rollback_exec != 1)); then
  printf '%s\n' "expected rollback execute fail without digest, got $missing_rollback_exec" >&2
  exit 1
fi
grep -Fq 'rollback_digest_missing' "$steps_evidence/rollback-exec-missing/08-rollback.txt"
grep -Fq 'rollback=not_run' "$steps_evidence/rollback-exec-missing/08-rollback.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback" \
  A3S_CLOUD_BX0_ROLLBACK_DIGEST='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
  bx0_step_rollback_execute "$steps_evidence/rollback-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/rollback-exec-ok/08-rollback.txt"
grep -Fq 'rollback=executed' "$steps_evidence/rollback-exec-ok/08-rollback.txt"
grep -Fq 'sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' "$steps_evidence/rollback-exec-ok/08-rollback.txt"
forbid_exit_certified_claim "$steps_evidence/rollback-exec-ok/08-rollback.txt"

echo "===== step library: stop/cleanup preflight (Darwin-safe) ====="
set +e
PATH="/usr/bin:/bin" \
  env -u A3S_CLOUD_BOX_BIN -u BX0_BOX_BINARY \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_stop_cleanup_preflight "$1"
  ' "$steps" "$steps_evidence/cleanup-missing"
missing_cleanup=$?
set -e
if ((missing_cleanup != 1)); then
  printf '%s\n' "expected cleanup preflight fail without box, got $missing_cleanup" >&2
  exit 1
fi
grep -Fq 'status=preflight_failed' "$steps_evidence/cleanup-missing/09-stop_cleanup.txt"
grep -Fq 'cleanup_box_unavailable' "$steps_evidence/cleanup-missing/09-stop_cleanup.txt"
grep -Fq 'stop_cleanup=not_run' "$steps_evidence/cleanup-missing/09-stop_cleanup.txt"
forbid_exit_certified_claim "$steps_evidence/cleanup-missing/09-stop_cleanup.txt"

stub_cleanup_box="$steps_evidence/stub-cleanup-box"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_cleanup_box"
chmod +x "$stub_cleanup_box"
A3S_CLOUD_BOX_BIN="$stub_cleanup_box" \
  bx0_step_stop_cleanup_preflight "$steps_evidence/cleanup-ok"
grep -Fq 'status=preflight_ok' "$steps_evidence/cleanup-ok/09-stop_cleanup.txt"
grep -Fq 'stop_cleanup=not_run' "$steps_evidence/cleanup-ok/09-stop_cleanup.txt"
grep -Fq "$stub_cleanup_box" "$steps_evidence/cleanup-ok/09-stop_cleanup.txt"
forbid_exit_certified_claim "$steps_evidence/cleanup-ok/09-stop_cleanup.txt"

echo "===== step library: stop/cleanup execute (Darwin-safe) ====="
set +e
A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_BOX_BIN="$stub_cleanup_box" \
  env -u A3S_CLOUD_BX0_CLEANUP_INSTANCE -u BX0_BOX_BINARY \
  bash -c '
    set -euo pipefail
    # shellcheck disable=SC1090
    source "$0"
    bx0_step_stop_cleanup_execute "$1"
  ' "$steps" "$steps_evidence/cleanup-exec-missing"
missing_cleanup_exec=$?
set -e
if ((missing_cleanup_exec != 1)); then
  printf '%s\n' "expected cleanup execute fail without instance, got $missing_cleanup_exec" >&2
  exit 1
fi
grep -Fq 'cleanup_instance_missing' "$steps_evidence/cleanup-exec-missing/09-stop_cleanup.txt"
grep -Fq 'stop_cleanup=not_run' "$steps_evidence/cleanup-exec-missing/09-stop_cleanup.txt"

A3S_CLOUD_BX0_EXECUTE=1 \
  A3S_CLOUD_BOX_BIN="$stub_cleanup_box" \
  A3S_CLOUD_BX0_CLEANUP_INSTANCE='box-instance-cleanup-1' \
  bx0_step_stop_cleanup_execute "$steps_evidence/cleanup-exec-ok"
grep -Fq 'status=execute_ok' "$steps_evidence/cleanup-exec-ok/09-stop_cleanup.txt"
grep -Fq 'stop_cleanup=executed' "$steps_evidence/cleanup-exec-ok/09-stop_cleanup.txt"
grep -Fq 'box-instance-cleanup-1' "$steps_evidence/cleanup-exec-ok/09-stop_cleanup.txt"
forbid_exit_certified_claim "$steps_evidence/cleanup-exec-ok/09-stop_cleanup.txt"

echo "===== unarmed gate must fail-close (no product EXIT) ====="
unset A3S_CLOUD_BX0_CLEAN_HOST || true
unset A3S_CLOUD_BOX_BIN || true
os_name="$(uname -s)"
set +e
bash "$gate" \
  >"$evidence_directory/unarmed.out" 2>"$evidence_directory/unarmed.err"
unarmed_status=$?
set -e

case $os_name in
  Linux)
    if ((unarmed_status != 2)); then
      printf '%s\n' "expected Linux unarmed exit 2, got $unarmed_status" >&2
      cat "$evidence_directory/unarmed.out" >&2 || true
      cat "$evidence_directory/unarmed.err" >&2 || true
      exit 1
    fi
    if ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' "$evidence_directory/unarmed.err" \
      && ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' "$evidence_directory/unarmed.out"; then
      printf '%s\n' "expected A3S_CLOUD_BX0_CLEAN_HOST_SKIP when unarmed" >&2
      exit 1
    fi
    if ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED' "$evidence_directory/unarmed.err" \
      && ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED' "$evidence_directory/unarmed.out"; then
      printf '%s\n' "expected A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED when unarmed" >&2
      exit 1
    fi
    ;;
  *)
    if ((unarmed_status != 1)); then
      printf '%s\n' "expected non-Linux FAIL_CLOSED exit 1, got $unarmed_status" >&2
      cat "$evidence_directory/unarmed.out" >&2 || true
      cat "$evidence_directory/unarmed.err" >&2 || true
      exit 1
    fi
    if ! grep -Fq 'FAIL_CLOSED' "$evidence_directory/unarmed.err"; then
      printf '%s\n' "expected FAIL_CLOSED on non-Linux host" >&2
      exit 1
    fi
    ;;
esac
forbid_exit_certified_claim "$evidence_directory/unarmed.out"
forbid_exit_certified_claim "$evidence_directory/unarmed.err"

if [[ $os_name == Linux ]]; then
  echo "===== armed without a3s-box must exit 2 ====="
  set +e
  env -u A3S_CLOUD_BOX_BIN \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    bash "$gate" \
    >"$evidence_directory/armed-no-box.out" 2>"$evidence_directory/armed-no-box.err"
  armed_status=$?
  set -e
  if ((armed_status != 2)); then
    printf '%s\n' "expected armed-without-box exit 2, got $armed_status" >&2
    cat "$evidence_directory/armed-no-box.out" >&2 || true
    cat "$evidence_directory/armed-no-box.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'a3s-box_unavailable' "$evidence_directory/armed-no-box.err" \
    && ! grep -Fq 'a3s-box_unavailable' "$evidence_directory/armed-no-box.out"; then
    printf '%s\n' "expected a3s-box_unavailable when armed without box" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-box.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-box.err"

  echo "===== armed stub without BOX-REVISION must exit 1 ====="
  stub_root="$evidence_directory/stub-box-missing"
  mkdir -p -- "$stub_root"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_root/a3s-box"
  chmod +x "$stub_root/a3s-box"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_root/a3s-box" \
    bash "$gate" \
    >"$evidence_directory/armed-missing.out" 2>"$evidence_directory/armed-missing.err"
  missing_status=$?
  set -e
  if ((missing_status != 1)); then
    printf '%s\n' "expected missing BOX-REVISION exit 1, got $missing_status" >&2
    cat "$evidence_directory/armed-missing.out" >&2 || true
    cat "$evidence_directory/armed-missing.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'box_revision_missing' "$evidence_directory/armed-missing.err"; then
    printf '%s\n' "expected box_revision_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-missing.out"
  forbid_exit_certified_claim "$evidence_directory/armed-missing.err"

  echo "===== armed stub with mismatched BOX-REVISION must exit 1 ====="
  stub_mismatch="$evidence_directory/stub-box-mismatch"
  mkdir -p -- "$stub_mismatch"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_mismatch/a3s-box"
  chmod +x "$stub_mismatch/a3s-box"
  printf '%s\n' '0000000000000000000000000000000000000000' \
    >"$stub_mismatch/BOX-REVISION"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_mismatch/a3s-box" \
    bash "$gate" \
    >"$evidence_directory/armed-mismatch.out" 2>"$evidence_directory/armed-mismatch.err"
  mismatch_status=$?
  set -e
  if ((mismatch_status != 1)); then
    printf '%s\n' "expected mismatch exit 1, got $mismatch_status" >&2
    cat "$evidence_directory/armed-mismatch.out" >&2 || true
    cat "$evidence_directory/armed-mismatch.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'box_revision_mismatch' "$evidence_directory/armed-mismatch.err"; then
    printf '%s\n' "expected box_revision_mismatch" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-mismatch.out"
  forbid_exit_certified_claim "$evidence_directory/armed-mismatch.err"

  echo "===== armed stub missing Runtime pin must exit 1 ====="
  stub_rt_missing="$evidence_directory/stub-box-rt-missing"
  mkdir -p -- "$stub_rt_missing"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_rt_missing/a3s-box"
  chmod +x "$stub_rt_missing/a3s-box"
  printf '%s\n' "$revision" >"$stub_rt_missing/BOX-REVISION"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_rt_missing/a3s-box" \
    A3S_CLOUD_BX0_RUNTIME_REVISION_FILE="$evidence_directory/does-not-exist-runtime-revision" \
    bash "$gate" \
    >"$evidence_directory/armed-rt-missing.out" 2>"$evidence_directory/armed-rt-missing.err"
  rt_missing_status=$?
  set -e
  if ((rt_missing_status != 1)); then
    printf '%s\n' "expected runtime_revision_missing exit 1, got $rt_missing_status" >&2
    cat "$evidence_directory/armed-rt-missing.out" >&2 || true
    cat "$evidence_directory/armed-rt-missing.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'runtime_revision_missing' "$evidence_directory/armed-rt-missing.err"; then
    printf '%s\n' "expected runtime_revision_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-rt-missing.out"
  forbid_exit_certified_claim "$evidence_directory/armed-rt-missing.err"

  echo "===== armed stub missing node-agent must exit 1 ====="
  stub_no_agent="$evidence_directory/stub-box-no-agent"
  mkdir -p -- "$stub_no_agent"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_agent/a3s-box"
  chmod +x "$stub_no_agent/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_agent/BOX-REVISION"
  set +e
  # Point NODE_AGENT_BIN at a missing path so CLOUD_ROOT/target fallbacks cannot
  # false-pass when a local (or host-mounted) build tree is present.
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_agent/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$evidence_directory/does-not-exist-node-agent" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-agent-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-agent.out" 2>"$evidence_directory/armed-no-agent.err"
  no_agent_status=$?
  set -e
  if ((no_agent_status != 1)); then
    printf '%s\n' "expected node_agent_unavailable exit 1, got $no_agent_status" >&2
    cat "$evidence_directory/armed-no-agent.out" >&2 || true
    cat "$evidence_directory/armed-no-agent.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'node_agent_unavailable' "$evidence_directory/armed-no-agent.err"; then
    printf '%s\n' "expected node_agent_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-agent.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-agent.err"

  echo "===== armed stub missing OCI must exit 1 ====="
  stub_no_oci="$evidence_directory/stub-box-no-oci"
  mkdir -p -- "$stub_no_oci"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_oci/a3s-box"
  chmod +x "$stub_no_oci/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_oci/BOX-REVISION"
  stub_node_agent_oci="$evidence_directory/stub-node-agent-for-oci"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent_oci"
  chmod +x "$stub_node_agent_oci"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_oci/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent_oci" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-oci-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-oci.out" 2>"$evidence_directory/armed-no-oci.err"
  no_oci_status=$?
  set -e
  if ((no_oci_status != 1)); then
    printf '%s\n' "expected oci_unavailable exit 1, got $no_oci_status" >&2
    cat "$evidence_directory/armed-no-oci.out" >&2 || true
    cat "$evidence_directory/armed-no-oci.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'oci_unavailable' "$evidence_directory/armed-no-oci.err"; then
    printf '%s\n' "expected oci_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-oci.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-oci.err"

  echo "===== armed stub missing control-plane must exit 1 ====="
  stub_no_cp="$evidence_directory/stub-box-no-cp"
  mkdir -p -- "$stub_no_cp"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_cp/a3s-box"
  chmod +x "$stub_no_cp/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_cp/BOX-REVISION"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_cp/a3s-oci"
  chmod +x "$stub_no_cp/a3s-oci"
  printf '%s\n' "$oci_runtime_revision" >"$stub_no_cp/OCI-RUNTIME-REVISION"
  stub_node_agent_cp="$evidence_directory/stub-node-agent-for-cp"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent_cp"
  chmod +x "$stub_node_agent_cp"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_cp/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent_cp" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$evidence_directory/does-not-exist-control-plane" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-cp-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-cp.out" 2>"$evidence_directory/armed-no-cp.err"
  no_cp_status=$?
  set -e
  if ((no_cp_status != 1)); then
    printf '%s\n' "expected control_plane_unavailable exit 1, got $no_cp_status" >&2
    cat "$evidence_directory/armed-no-cp.out" >&2 || true
    cat "$evidence_directory/armed-no-cp.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'control_plane_unavailable' "$evidence_directory/armed-no-cp.err"; then
    printf '%s\n' "expected control_plane_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-cp.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-cp.err"

  echo "===== armed stub missing health probe must exit 1 ====="
  stub_no_health="$evidence_directory/stub-box-no-health"
  mkdir -p -- "$stub_no_health"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_health/a3s-box"
  chmod +x "$stub_no_health/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_health/BOX-REVISION"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_health/a3s-oci"
  chmod +x "$stub_no_health/a3s-oci"
  printf '%s\n' "$oci_runtime_revision" >"$stub_no_health/OCI-RUNTIME-REVISION"
  stub_node_agent_h="$evidence_directory/stub-node-agent-for-health"
  stub_cp_h="$evidence_directory/stub-cp-for-health"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent_h"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_cp_h"
  chmod +x "$stub_node_agent_h" "$stub_cp_h"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_health/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent_h" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp_h" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$evidence_directory/does-not-exist-health-probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-health-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-health.out" 2>"$evidence_directory/armed-no-health.err"
  no_health_status=$?
  set -e
  if ((no_health_status != 1)); then
    printf '%s\n' "expected health_probe_unavailable exit 1, got $no_health_status" >&2
    cat "$evidence_directory/armed-no-health.out" >&2 || true
    cat "$evidence_directory/armed-no-health.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'health_probe_unavailable' "$evidence_directory/armed-no-health.err"; then
    printf '%s\n' "expected health_probe_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-health.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-health.err"

  echo "===== armed stub missing gateway must exit 1 ====="
  stub_no_gw="$evidence_directory/stub-box-no-gw"
  mkdir -p -- "$stub_no_gw"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_gw/a3s-box"
  chmod +x "$stub_no_gw/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_gw/BOX-REVISION"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_gw/a3s-oci"
  chmod +x "$stub_no_gw/a3s-oci"
  printf '%s\n' "$oci_runtime_revision" >"$stub_no_gw/OCI-RUNTIME-REVISION"
  stub_node_agent_g="$evidence_directory/stub-node-agent-for-gw"
  stub_cp_g="$evidence_directory/stub-cp-for-gw"
  stub_probe_g="$evidence_directory/stub-probe-for-gw"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent_g"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_cp_g"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_probe_g"
  chmod +x "$stub_node_agent_g" "$stub_cp_g" "$stub_probe_g"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_gw/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent_g" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp_g" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_probe_g" \
    A3S_CLOUD_GATEWAY_BIN="$evidence_directory/does-not-exist-gateway" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-gw-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-gw.out" 2>"$evidence_directory/armed-no-gw.err"
  no_gw_status=$?
  set -e
  if ((no_gw_status != 1)); then
    printf '%s\n' "expected gateway_unavailable exit 1, got $no_gw_status" >&2
    cat "$evidence_directory/armed-no-gw.out" >&2 || true
    cat "$evidence_directory/armed-no-gw.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'gateway_unavailable' "$evidence_directory/armed-no-gw.err"; then
    printf '%s\n' "expected gateway_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-gw.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-gw.err"

  echo "===== armed stub missing logs probe must exit 1 ====="
  stub_no_logs="$evidence_directory/stub-box-no-logs"
  mkdir -p -- "$stub_no_logs"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_logs/a3s-box"
  chmod +x "$stub_no_logs/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_logs/BOX-REVISION"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_logs/a3s-oci"
  chmod +x "$stub_no_logs/a3s-oci"
  printf '%s\n' "$oci_runtime_revision" >"$stub_no_logs/OCI-RUNTIME-REVISION"
  stub_node_agent_l="$evidence_directory/stub-node-agent-for-logs"
  stub_cp_l="$evidence_directory/stub-cp-for-logs"
  stub_probe_l="$evidence_directory/stub-probe-for-logs"
  stub_gw_l="$evidence_directory/stub-gw-for-logs"
  mkdir -p -- "$stub_gw_l"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent_l"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_cp_l"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_probe_l"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_gw_l/a3s-gateway"
  chmod +x "$stub_node_agent_l" "$stub_cp_l" "$stub_probe_l" "$stub_gw_l/a3s-gateway"
  printf '%s\n' "$gateway_revision" >"$stub_gw_l/GATEWAY-REVISION"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_logs/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent_l" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp_l" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_probe_l" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gw_l/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$evidence_directory/does-not-exist-logs-probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-logs-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-logs.out" 2>"$evidence_directory/armed-no-logs.err"
  no_logs_status=$?
  set -e
  if ((no_logs_status != 1)); then
    printf '%s\n' "expected logs_probe_unavailable exit 1, got $no_logs_status" >&2
    cat "$evidence_directory/armed-no-logs.out" >&2 || true
    cat "$evidence_directory/armed-no-logs.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'logs_probe_unavailable' "$evidence_directory/armed-no-logs.err"; then
    printf '%s\n' "expected logs_probe_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-logs.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-logs.err"

  echo "===== armed stub missing digest probe must exit 1 ====="
  stub_no_digest="$evidence_directory/stub-box-no-digest"
  mkdir -p -- "$stub_no_digest"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_digest/a3s-box"
  chmod +x "$stub_no_digest/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_digest/BOX-REVISION"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_digest/a3s-oci"
  chmod +x "$stub_no_digest/a3s-oci"
  printf '%s\n' "$oci_runtime_revision" >"$stub_no_digest/OCI-RUNTIME-REVISION"
  stub_node_agent_d="$evidence_directory/stub-node-agent-for-digest"
  stub_cp_d="$evidence_directory/stub-cp-for-digest"
  stub_probe_d="$evidence_directory/stub-probe-for-digest"
  stub_logs_d="$evidence_directory/stub-logs-for-digest"
  stub_gw_d="$evidence_directory/stub-gw-for-digest"
  mkdir -p -- "$stub_gw_d"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent_d"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_cp_d"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_probe_d"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_logs_d"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_gw_d/a3s-gateway"
  chmod +x "$stub_node_agent_d" "$stub_cp_d" "$stub_probe_d" "$stub_logs_d" "$stub_gw_d/a3s-gateway"
  printf '%s\n' "$gateway_revision" >"$stub_gw_d/GATEWAY-REVISION"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_digest/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent_d" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp_d" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_probe_d" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gw_d/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_d" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$evidence_directory/does-not-exist-digest-probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-digest-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-digest.out" 2>"$evidence_directory/armed-no-digest.err"
  no_digest_status=$?
  set -e
  if ((no_digest_status != 1)); then
    printf '%s\n' "expected digest_probe_unavailable exit 1, got $no_digest_status" >&2
    cat "$evidence_directory/armed-no-digest.out" >&2 || true
    cat "$evidence_directory/armed-no-digest.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'digest_probe_unavailable' "$evidence_directory/armed-no-digest.err"; then
    printf '%s\n' "expected digest_probe_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-digest.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-digest.err"

  echo "===== armed stub missing rollback probe must exit 1 ====="
  stub_no_rb="$evidence_directory/stub-box-no-rb"
  mkdir -p -- "$stub_no_rb"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_rb/a3s-box"
  chmod +x "$stub_no_rb/a3s-box"
  printf '%s\n' "$revision" >"$stub_no_rb/BOX-REVISION"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_no_rb/a3s-oci"
  chmod +x "$stub_no_rb/a3s-oci"
  printf '%s\n' "$oci_runtime_revision" >"$stub_no_rb/OCI-RUNTIME-REVISION"
  stub_node_agent_r="$evidence_directory/stub-node-agent-for-rb"
  stub_cp_r="$evidence_directory/stub-cp-for-rb"
  stub_probe_r="$evidence_directory/stub-probe-for-rb"
  stub_logs_r="$evidence_directory/stub-logs-for-rb"
  stub_digest_r="$evidence_directory/stub-digest-for-rb"
  stub_gw_r="$evidence_directory/stub-gw-for-rb"
  mkdir -p -- "$stub_gw_r"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent_r"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_cp_r"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_probe_r"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_logs_r"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_digest_r"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_gw_r/a3s-gateway"
  chmod +x "$stub_node_agent_r" "$stub_cp_r" "$stub_probe_r" "$stub_logs_r" "$stub_digest_r" "$stub_gw_r/a3s-gateway"
  printf '%s\n' "$gateway_revision" >"$stub_gw_r/GATEWAY-REVISION"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_rb/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent_r" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_cp_r" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_probe_r" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gw_r/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_r" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_r" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$evidence_directory/does-not-exist-rollback-probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/no-rb-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-no-rb.out" 2>"$evidence_directory/armed-no-rb.err"
  no_rb_status=$?
  set -e
  if ((no_rb_status != 1)); then
    printf '%s\n' "expected rollback_probe_unavailable exit 1, got $no_rb_status" >&2
    cat "$evidence_directory/armed-no-rb.out" >&2 || true
    cat "$evidence_directory/armed-no-rb.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'rollback_probe_unavailable' "$evidence_directory/armed-no-rb.err"; then
    printf '%s\n' "expected rollback_probe_unavailable" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-rb.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-rb.err"

  echo "===== armed stub with matching Box+OCI+Gateway pins must stay OPEN (exit 3) ====="
  stub_match="$evidence_directory/stub-box-match"
  mkdir -p -- "$stub_match"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_match/a3s-box"
  chmod +x "$stub_match/a3s-box"
  printf '%s\n' "$revision" >"$stub_match/BOX-REVISION"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_match/a3s-oci"
  chmod +x "$stub_match/a3s-oci"
  printf '%s\n' "$oci_runtime_revision" >"$stub_match/OCI-RUNTIME-REVISION"
  stub_node_agent="$evidence_directory/stub-node-agent"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_node_agent"
  chmod +x "$stub_node_agent"
  stub_control_plane="$evidence_directory/stub-control-plane"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_control_plane"
  chmod +x "$stub_control_plane"
  stub_health_probe="$evidence_directory/stub-health-probe"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_health_probe"
  chmod +x "$stub_health_probe"
  stub_gateway="$evidence_directory/stub-gateway"
  mkdir -p -- "$stub_gateway"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_gateway/a3s-gateway"
  chmod +x "$stub_gateway/a3s-gateway"
  printf '%s\n' "$gateway_revision" >"$stub_gateway/GATEWAY-REVISION"
  stub_logs_probe="$evidence_directory/stub-logs-probe"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_logs_probe"
  chmod +x "$stub_logs_probe"
  stub_digest_probe="$evidence_directory/stub-digest-probe"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_digest_probe"
  chmod +x "$stub_digest_probe"
  stub_rollback_probe="$evidence_directory/stub-rollback-probe"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_rollback_probe"
  chmod +x "$stub_rollback_probe"
  runtime_revision=$(<"$tools/../runtime-conformance/runtime-revision")
  gateway_revision=$(<"$tools/../gateway-conformance/gateway-revision")
  cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
  match_evidence="$evidence_directory/match-evidence"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$match_evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-match.out" 2>"$evidence_directory/armed-match.err"
  match_status=$?
  set -e
  if ((match_status != 3)); then
    printf '%s\n' "expected armed-match exit 3 OPEN, got $match_status" >&2
    cat "$evidence_directory/armed-match.out" >&2 || true
    cat "$evidence_directory/armed-match.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_OPEN' "$evidence_directory/armed-match.out" \
    && ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_OPEN' "$evidence_directory/armed-match.err"; then
    printf '%s\n' "expected A3S_CLOUD_BX0_CLEAN_HOST_OPEN for pin-matched stub" >&2
    exit 1
  fi
  combined_match="$(
    cat "$evidence_directory/armed-match.out" "$evidence_directory/armed-match.err"
  )"
  for needle in \
    "$revision" \
    "$runtime_revision" \
    "$gateway_revision" \
    "$cloud_revision" \
    'pw0_no_pin_file' \
    'bound=Cloud+Runtime+Box+Gateway' \
    'step1=enroll_preflight_ok' \
    'step2=oci_preflight_ok' \
    'step3=deploy_preflight_ok' \
    'step4=health_preflight_ok' \
    'step5=https_preflight_ok' \
    'step6=logs_preflight_ok' \
    'step7=update_preflight_ok' \
    'step8=rollback_preflight_ok' \
    'step9=stop_cleanup_preflight_ok' \
    'execute_receipts_complete=0' \
    'loop_exit=not_certified'; do
    if ! grep -Fq "$needle" <<<"$combined_match"; then
      printf '%s\n' "expected armed OPEN output to include: $needle" >&2
      exit 1
    fi
  done
  grep -Fq 'status=preflight_ok' "$match_evidence/01-enroll.txt"
  grep -Fq 'enroll=not_run' "$match_evidence/01-enroll.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/02-oci.txt"
  grep -Fq 'oci=not_run' "$match_evidence/02-oci.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/03-deploy.txt"
  grep -Fq 'deploy=not_run' "$match_evidence/03-deploy.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/04-health.txt"
  grep -Fq 'health=not_run' "$match_evidence/04-health.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/05-https.txt"
  grep -Fq 'https=not_run' "$match_evidence/05-https.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/06-logs.txt"
  grep -Fq 'logs=not_run' "$match_evidence/06-logs.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/07-update.txt"
  grep -Fq 'update=not_run' "$match_evidence/07-update.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/08-rollback.txt"
  grep -Fq 'rollback=not_run' "$match_evidence/08-rollback.txt"
  grep -Fq 'status=preflight_ok' "$match_evidence/09-stop_cleanup.txt"
  grep -Fq 'stop_cleanup=not_run' "$match_evidence/09-stop_cleanup.txt"
  forbid_exit_certified_claim "$evidence_directory/armed-match.out"
  forbid_exit_certified_claim "$evidence_directory/armed-match.err"
  forbid_exit_certified_claim "$match_evidence/01-enroll.txt"
  forbid_exit_certified_claim "$match_evidence/02-oci.txt"
  forbid_exit_certified_claim "$match_evidence/03-deploy.txt"
  forbid_exit_certified_claim "$match_evidence/04-health.txt"
  forbid_exit_certified_claim "$match_evidence/05-https.txt"
  forbid_exit_certified_claim "$match_evidence/06-logs.txt"
  forbid_exit_certified_claim "$match_evidence/07-update.txt"
  forbid_exit_certified_claim "$match_evidence/08-rollback.txt"
  forbid_exit_certified_claim "$match_evidence/09-stop_cleanup.txt"

  echo "===== armed EXECUTE without enroll node_id must exit 1 ====="
  exec_acl="$evidence_directory/exec-node.acl"
  cat >"$exec_acl" <<'ACL'
control_plane {
  enrollment_url = "http://127.0.0.1:8080/api/v1/node-control/enroll"
  node_control_url = "https://localhost:8443"
  enrollment_token_env = "A3S_CLOUD_ENROLLMENT_TOKEN"
}
ACL
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_ENROLL_NODE_ID \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-node-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-node.out" 2>"$evidence_directory/armed-exec-no-node.err"
  exec_no_node_status=$?
  set -e
  if ((exec_no_node_status != 1)); then
    printf '%s\n' "expected EXECUTE without node_id exit 1, got $exec_no_node_status" >&2
    cat "$evidence_directory/armed-exec-no-node.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-node.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'enroll_node_id_missing' "$evidence_directory/armed-exec-no-node.err"; then
    printf '%s\n' "expected enroll_node_id_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-node.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-node.err"

  echo "===== armed EXECUTE without artifact digest must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_ARTIFACT_DIGEST \
    -u A3S_CLOUD_BX0_SERVICE_ID \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-digest-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-digest.out" 2>"$evidence_directory/armed-exec-no-digest.err"
  exec_no_digest_status=$?
  set -e
  if ((exec_no_digest_status != 1)); then
    printf '%s\n' "expected EXECUTE without digest exit 1, got $exec_no_digest_status" >&2
    cat "$evidence_directory/armed-exec-no-digest.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-digest.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'artifact_digest_missing' "$evidence_directory/armed-exec-no-digest.err"; then
    printf '%s\n' "expected artifact_digest_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-digest.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-digest.err"

  echo "===== armed EXECUTE without service_id must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_SERVICE_ID \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-svc-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-svc.out" 2>"$evidence_directory/armed-exec-no-svc.err"
  exec_no_svc_status=$?
  set -e
  if ((exec_no_svc_status != 1)); then
    printf '%s\n' "expected EXECUTE without service_id exit 1, got $exec_no_svc_status" >&2
    cat "$evidence_directory/armed-exec-no-svc.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-svc.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'service_id_missing' "$evidence_directory/armed-exec-no-svc.err"; then
    printf '%s\n' "expected service_id_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-svc.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-svc.err"

  echo "===== armed EXECUTE without health URL must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_HEALTH_URL \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BX0_SERVICE_ID='svc-execute-ci-1' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-health-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-health.out" 2>"$evidence_directory/armed-exec-no-health.err"
  exec_no_health_status=$?
  set -e
  if ((exec_no_health_status != 1)); then
    printf '%s\n' "expected EXECUTE without health URL exit 1, got $exec_no_health_status" >&2
    cat "$evidence_directory/armed-exec-no-health.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-health.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'health_url_missing' "$evidence_directory/armed-exec-no-health.err"; then
    printf '%s\n' "expected health_url_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-health.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-health.err"

  echo "===== armed EXECUTE without HTTPS URL must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_HTTPS_URL \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BX0_SERVICE_ID='svc-execute-ci-1' \
    A3S_CLOUD_BX0_HEALTH_URL='http://127.0.0.1:18080/ready' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-https-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-https.out" 2>"$evidence_directory/armed-exec-no-https.err"
  exec_no_https_status=$?
  set -e
  if ((exec_no_https_status != 1)); then
    printf '%s\n' "expected EXECUTE without HTTPS URL exit 1, got $exec_no_https_status" >&2
    cat "$evidence_directory/armed-exec-no-https.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-https.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'https_url_missing' "$evidence_directory/armed-exec-no-https.err"; then
    printf '%s\n' "expected https_url_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-https.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-https.err"

  echo "===== armed EXECUTE without logs cursor must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_LOGS_CURSOR \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BX0_SERVICE_ID='svc-execute-ci-1' \
    A3S_CLOUD_BX0_HEALTH_URL='http://127.0.0.1:18080/ready' \
    A3S_CLOUD_BX0_HTTPS_URL='https://svc.example.test/' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-logs-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-logs.out" 2>"$evidence_directory/armed-exec-no-logs.err"
  exec_no_logs_status=$?
  set -e
  if ((exec_no_logs_status != 1)); then
    printf '%s\n' "expected EXECUTE without logs cursor exit 1, got $exec_no_logs_status" >&2
    cat "$evidence_directory/armed-exec-no-logs.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-logs.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'logs_cursor_missing' "$evidence_directory/armed-exec-no-logs.err"; then
    printf '%s\n' "expected logs_cursor_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-logs.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-logs.err"

  echo "===== armed EXECUTE without update digest must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_UPDATE_DIGEST \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BX0_SERVICE_ID='svc-execute-ci-1' \
    A3S_CLOUD_BX0_HEALTH_URL='http://127.0.0.1:18080/ready' \
    A3S_CLOUD_BX0_HTTPS_URL='https://svc.example.test/' \
    A3S_CLOUD_BX0_LOGS_CURSOR='cursor-ci-1' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-update-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-update.out" 2>"$evidence_directory/armed-exec-no-update.err"
  exec_no_update_status=$?
  set -e
  if ((exec_no_update_status != 1)); then
    printf '%s\n' "expected EXECUTE without update digest exit 1, got $exec_no_update_status" >&2
    cat "$evidence_directory/armed-exec-no-update.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-update.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'update_digest_missing' "$evidence_directory/armed-exec-no-update.err"; then
    printf '%s\n' "expected update_digest_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-update.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-update.err"

  echo "===== armed EXECUTE without rollback digest must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_ROLLBACK_DIGEST \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BX0_SERVICE_ID='svc-execute-ci-1' \
    A3S_CLOUD_BX0_HEALTH_URL='http://127.0.0.1:18080/ready' \
    A3S_CLOUD_BX0_HTTPS_URL='https://svc.example.test/' \
    A3S_CLOUD_BX0_LOGS_CURSOR='cursor-ci-1' \
    A3S_CLOUD_BX0_UPDATE_DIGEST='sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-rollback-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-rollback.out" 2>"$evidence_directory/armed-exec-no-rollback.err"
  exec_no_rollback_status=$?
  set -e
  if ((exec_no_rollback_status != 1)); then
    printf '%s\n' "expected EXECUTE without rollback digest exit 1, got $exec_no_rollback_status" >&2
    cat "$evidence_directory/armed-exec-no-rollback.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-rollback.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'rollback_digest_missing' "$evidence_directory/armed-exec-no-rollback.err"; then
    printf '%s\n' "expected rollback_digest_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-rollback.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-rollback.err"

  echo "===== armed EXECUTE without cleanup instance must exit 1 ====="
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    -u A3S_CLOUD_BX0_CLEANUP_INSTANCE \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BX0_SERVICE_ID='svc-execute-ci-1' \
    A3S_CLOUD_BX0_HEALTH_URL='http://127.0.0.1:18080/ready' \
    A3S_CLOUD_BX0_HTTPS_URL='https://svc.example.test/' \
    A3S_CLOUD_BX0_LOGS_CURSOR='cursor-ci-1' \
    A3S_CLOUD_BX0_UPDATE_DIGEST='sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210' \
    A3S_CLOUD_BX0_ROLLBACK_DIGEST='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$evidence_directory/exec-no-cleanup-evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-no-cleanup.out" 2>"$evidence_directory/armed-exec-no-cleanup.err"
  exec_no_cleanup_status=$?
  set -e
  if ((exec_no_cleanup_status != 1)); then
    printf '%s\n' "expected EXECUTE without cleanup instance exit 1, got $exec_no_cleanup_status" >&2
    cat "$evidence_directory/armed-exec-no-cleanup.out" >&2 || true
    cat "$evidence_directory/armed-exec-no-cleanup.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'cleanup_instance_missing' "$evidence_directory/armed-exec-no-cleanup.err"; then
    printf '%s\n' "expected cleanup_instance_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-cleanup.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-no-cleanup.err"

  echo "===== armed EXECUTE with enroll through cleanup receipts must OPEN ====="
  exec_ok_evidence="$evidence_directory/exec-ok-evidence"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_TEST_GATEWAY_BIN \
    -u A3S_CLOUD_GATEWAY_REVISION \
    -u A3S_CLOUD_TEST_GATEWAY_REVISION \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BX0_EXECUTE=1 \
    A3S_CLOUD_BX0_NODE_CONFIG="$exec_acl" \
    A3S_CLOUD_ENROLLMENT_TOKEN='a3sn_ci_token_not_placeholder' \
    A3S_CLOUD_BX0_ENROLL_NODE_ID='aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee' \
    A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef' \
    A3S_CLOUD_BX0_SERVICE_ID='svc-execute-ci-1' \
    A3S_CLOUD_BX0_HEALTH_URL='http://127.0.0.1:18080/ready' \
    A3S_CLOUD_BX0_HTTPS_URL='https://svc.example.test/' \
    A3S_CLOUD_BX0_LOGS_CURSOR='cursor-ci-1' \
    A3S_CLOUD_BX0_UPDATE_DIGEST='sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210' \
    A3S_CLOUD_BX0_ROLLBACK_DIGEST='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
    A3S_CLOUD_BX0_CLEANUP_INSTANCE='box-instance-ci-1' \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
    A3S_CLOUD_HEALTH_PROBE_BIN="$stub_health_probe" \
    A3S_CLOUD_GATEWAY_BIN="$stub_gateway/a3s-gateway" \
    A3S_CLOUD_LOGS_PROBE_BIN="$stub_logs_probe" \
    A3S_CLOUD_DIGEST_PROBE_BIN="$stub_digest_probe" \
    A3S_CLOUD_ROLLBACK_PROBE_BIN="$stub_rollback_probe" \
    A3S_CLOUD_BX0_EVIDENCE_DIR="$exec_ok_evidence" \
    bash "$gate" \
    >"$evidence_directory/armed-exec-ok.out" 2>"$evidence_directory/armed-exec-ok.err"
  exec_ok_status=$?
  set -e
  if ((exec_ok_status != 3)); then
    printf '%s\n' "expected EXECUTE enroll through cleanup exit 3 OPEN, got $exec_ok_status" >&2
    cat "$evidence_directory/armed-exec-ok.out" >&2 || true
    cat "$evidence_directory/armed-exec-ok.err" >&2 || true
    exit 1
  fi
  combined_exec="$(
    cat "$evidence_directory/armed-exec-ok.out" "$evidence_directory/armed-exec-ok.err"
  )"
  grep -Fq 'step1=enroll_executed' <<<"$combined_exec"
  grep -Fq 'step2=oci_executed' <<<"$combined_exec"
  grep -Fq 'step3=deploy_executed' <<<"$combined_exec"
  grep -Fq 'step4=health_executed' <<<"$combined_exec"
  grep -Fq 'step5=https_executed' <<<"$combined_exec"
  grep -Fq 'step6=logs_executed' <<<"$combined_exec"
  grep -Fq 'step7=update_executed' <<<"$combined_exec"
  grep -Fq 'step8=rollback_executed' <<<"$combined_exec"
  grep -Fq 'step9=stop_cleanup_executed' <<<"$combined_exec"
  grep -Fq 'execute_receipts_complete=1' <<<"$combined_exec"
  grep -Fq 'loop_exit=not_certified' <<<"$combined_exec"
  grep -Fq 'enroll=executed' "$exec_ok_evidence/01-enroll.txt"
  grep -Fq 'oci=executed' "$exec_ok_evidence/02-oci.txt"
  grep -Fq 'deploy=executed' "$exec_ok_evidence/03-deploy.txt"
  grep -Fq 'health=executed' "$exec_ok_evidence/04-health.txt"
  grep -Fq 'https=executed' "$exec_ok_evidence/05-https.txt"
  grep -Fq 'logs=executed' "$exec_ok_evidence/06-logs.txt"
  grep -Fq 'update=executed' "$exec_ok_evidence/07-update.txt"
  grep -Fq 'rollback=executed' "$exec_ok_evidence/08-rollback.txt"
  grep -Fq 'stop_cleanup=executed' "$exec_ok_evidence/09-stop_cleanup.txt"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-ok.out"
  forbid_exit_certified_claim "$evidence_directory/armed-exec-ok.err"
  forbid_exit_certified_claim "$exec_ok_evidence/01-enroll.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/02-oci.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/03-deploy.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/04-health.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/05-https.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/06-logs.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/07-update.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/08-rollback.txt"
  forbid_exit_certified_claim "$exec_ok_evidence/09-stop_cleanup.txt"
fi

echo "===== LOOP certification validator refuse-to-fake ====="
validator="$tools/validate_bx0_clean_host_certification.sh"
collector="$tools/collect_bx0_clean_host_evidence.sh"
exit_audit="$tools/run_bx0_clean_host_exit_audit.sh"
example="$tools/bx0-clean-host-certification.example.txt"
[[ -f $validator && -f $collector && -f $exit_audit && -f $example ]]
bash -n "$validator"
bash -n "$collector"
bash -n "$exit_audit"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED' "$validator"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED' "$exit_audit"
grep -Fq 'power_unbound' "$exit_audit"
# EXIT_CERTIFIED may appear in exit_audit source as the success path, but CI
# must prove it is never emitted without Power+LOOP. Collector must never claim it.
if grep -E '^([[:space:]]*)(printf|echo|cat).*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$collector"; then
  printf '%s\n' "collector must not printf/echo product EXIT_CERTIFIED" >&2
  exit 1
fi

cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
set +e
bash "$validator" "$example" "$cloud_revision" "$runtime_revision" "$revision" "$gateway_revision" \
  >"$evidence_directory/validator-example.out" 2>"$evidence_directory/validator-example.err"
example_status=$?
set -e
if ((example_status == 0)); then
  printf '%s\n' "expected PLACEHOLDER example certification to FAIL" >&2
  exit 1
fi
forbid_exit_certified_claim "$evidence_directory/validator-example.out"
forbid_exit_certified_claim "$evidence_directory/validator-example.err"

good_loop="$evidence_directory/good-loop-certification.txt"
printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED cloud_revision=$cloud_revision runtime_revision=$runtime_revision box_revision=$revision gateway_revision=$gateway_revision host=ci-host-1 service_id=svc-1 node_id=node-1 artifact_digest=sha256:deadbeef" \
  >"$good_loop"
bash "$validator" "$good_loop" "$cloud_revision" "$runtime_revision" "$revision" "$gateway_revision" \
  >"$evidence_directory/validator-good.out" 2>"$evidence_directory/validator-good.err"
forbid_exit_certified_claim "$evidence_directory/validator-good.out"
forbid_exit_certified_claim "$evidence_directory/validator-good.err"

echo "===== collector writes LOOP only (never EXIT) ====="
collect_dir="$evidence_directory/collector"
bash "$collector" \
  --host ci-host-1 \
  --service-id svc-1 \
  --node-id node-1 \
  --artifact-digest sha256:deadbeef \
  --evidence-dir "$collect_dir" \
  >"$evidence_directory/collector.out" 2>"$evidence_directory/collector.err"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_LOOP_COLLECTED' "$evidence_directory/collector.out"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED' "$collect_dir/bx0-clean-host-certification.txt"
grep -Fq 'product_exit=not_claimed' "$evidence_directory/collector.out"
forbid_exit_certified_claim "$evidence_directory/collector.out"
forbid_exit_certified_claim "$evidence_directory/collector.err"
forbid_exit_certified_claim "$collect_dir/bx0-clean-host-certification.txt"
forbid_exit_certified_claim "$collect_dir/checklist.txt"

echo "===== exit audit without LOOP must exit 2 EXIT_BLOCKED ====="
unset A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION || true
unset A3S_CLOUD_BX0_POWER_REVISION_FILE || true
audit_dir="$evidence_directory/exit-audit-no-loop"
set +e
bash "$exit_audit" "$audit_dir" \
  >"$evidence_directory/exit-audit-no-loop.out" 2>"$evidence_directory/exit-audit-no-loop.err"
no_loop_status=$?
set -e
if ((no_loop_status != 2)); then
  printf '%s\n' "expected exit audit without LOOP to exit 2, got $no_loop_status" >&2
  cat "$evidence_directory/exit-audit-no-loop.out" >&2 || true
  cat "$evidence_directory/exit-audit-no-loop.err" >&2 || true
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED' "$audit_dir/bx0-exit-certification.txt"
grep -Fq 'loop_certification_unavailable' "$audit_dir/bx0-exit-certification.txt"
forbid_exit_certified_claim "$evidence_directory/exit-audit-no-loop.out"
forbid_exit_certified_claim "$evidence_directory/exit-audit-no-loop.err"
forbid_exit_certified_claim "$audit_dir/bx0-exit-certification.txt"

echo "===== exit audit with LOOP but Power UNBOUND must exit 2 ====="
audit_dir_power="$evidence_directory/exit-audit-no-power"
set +e
A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION="$collect_dir/bx0-clean-host-certification.txt" \
  env -u A3S_CLOUD_BX0_POWER_REVISION_FILE \
  bash "$exit_audit" "$audit_dir_power" \
  >"$evidence_directory/exit-audit-no-power.out" 2>"$evidence_directory/exit-audit-no-power.err"
no_power_status=$?
set -e
if ((no_power_status != 2)); then
  printf '%s\n' "expected exit audit with LOOP but no Power to exit 2, got $no_power_status" >&2
  cat "$evidence_directory/exit-audit-no-power.out" >&2 || true
  cat "$evidence_directory/exit-audit-no-power.err" >&2 || true
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED' "$audit_dir_power/bx0-exit-certification.txt"
grep -Fq 'power_unbound' "$audit_dir_power/bx0-exit-certification.txt"
forbid_exit_certified_claim "$evidence_directory/exit-audit-no-power.out"
forbid_exit_certified_claim "$evidence_directory/exit-audit-no-power.err"
forbid_exit_certified_claim "$audit_dir_power/bx0-exit-certification.txt"

printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_CI_CERTIFIED revision=$revision host_os=$os_name fail_closed=1"
