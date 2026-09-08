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
  # Product EXIT marker may appear only as a documented required/refused marker.
  # Any line that claims certification (…EXIT_CERTIFIED without OPEN/required/
  # refuses/not emitted/Must bind scaffolding context) is forbidden in harness output.
  if grep -E --quiet \
    '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]*$' \
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
grep -Fq 'step1=enroll_preflight_ok' "$gate"
grep -Fq 'step2=oci_preflight_ok' "$gate"
grep -Fq 'step3=deploy_preflight_ok' "$gate"
grep -Fq 'oci=not_run' "$gate"
grep -Fq 'deploy=not_run' "$gate"
grep -Fq 'control_plane_unavailable' "$gate"
grep -Eq 'exit 1' "$gate"
grep -Eq 'exit 2' "$gate"
grep -Eq 'exit 3' "$gate"
steps="$tools/bx0_clean_host_steps.sh"
[[ -f $steps ]]
grep -Fq 'oci_unavailable' "$steps"
grep -Fq 'oci_runtime_revision_mismatch' "$steps"
grep -Fq 'control_plane_unavailable' "$steps"
bash -n "$steps"
bash -n "$gate"
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
bx0_write_remaining_open_steps "$steps_evidence/deploy-ok"
[[ -f $steps_evidence/deploy-ok/04-health.txt ]]
[[ -f $steps_evidence/deploy-ok/09-stop_cleanup.txt ]]
grep -Fq 'status=OPEN' "$steps_evidence/deploy-ok/04-health.txt"
forbid_exit_certified_claim "$steps_evidence/deploy-ok/03-deploy.txt"

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
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_NODE_AGENT_BIN \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_no_agent/a3s-box" \
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

  echo "===== armed stub with matching Box+OCI pins must stay OPEN (exit 3) ====="
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
  runtime_revision=$(<"$tools/../runtime-conformance/runtime-revision")
  gateway_revision=$(<"$tools/../gateway-conformance/gateway-revision")
  cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
  match_evidence="$evidence_directory/match-evidence"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    -u A3S_CLOUD_OCI_BIN \
    -u A3S_CLOUD_OCI_RUNTIME_REVISION \
    -u A3S_CLOUD_DEV_API_BIN \
    -u A3S_CLOUD_BX0_RUNTIME_REVISION_FILE \
    -u A3S_CLOUD_BX0_GATEWAY_REVISION_FILE \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    A3S_CLOUD_NODE_AGENT_BIN="$stub_node_agent" \
    A3S_CLOUD_CONTROL_PLANE_BIN="$stub_control_plane" \
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
  combined_match="$evidence_directory/armed-match.out"$'\n'"$(cat "$evidence_directory/armed-match.err")"
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
    'steps4-9=not_run'; do
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
  [[ -f $match_evidence/09-stop_cleanup.txt ]]
  forbid_exit_certified_claim "$evidence_directory/armed-match.out"
  forbid_exit_certified_claim "$evidence_directory/armed-match.err"
  forbid_exit_certified_claim "$match_evidence/01-enroll.txt"
  forbid_exit_certified_claim "$match_evidence/02-oci.txt"
  forbid_exit_certified_claim "$match_evidence/03-deploy.txt"
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_CI_CERTIFIED revision=$revision host_os=$os_name fail_closed=1"
