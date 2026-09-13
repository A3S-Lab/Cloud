#!/usr/bin/env bash
set -euo pipefail

# Cloud BX0.3 TEE isolation CI fail-closed harness.
# Proves validator/audit refuse-to-fake behavior. Never claims BX0 Verified and
# never emits A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED as a product unlock without
# operator-bound hardware evidence (simulate=false + green Box SEV job).

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
validator="$tools/validate_bx0_tee_isolation_evidence.sh"
audit="$tools/run_bx0_tee_isolation_audit.sh"
collector="$tools/collect_bx0_tee_isolation_evidence.sh"
example="$tools/bx0-tee-isolation-certification.example.txt"
box_revision=$(tr -d '[:space:]' <"$tools/box-revision")
cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
meas=$(printf 'a%.0s' {1..96})

[[ $box_revision =~ ^[0-9a-f]{40}$ ]]
[[ -f $validator && -f $audit && -f $collector && -f $example ]]

evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-tee-audit-ci.XXXXXX")
cleanup() {
  rm -rf -- "$evidence_directory"
}
trap cleanup EXIT


echo "===== bash -n TEE binder scripts ====="
bash -n "$validator"
bash -n "$audit"
bash -n "$collector"
bash -n "$tools/verify_bx0_tee_box_hardware_run.sh"
bash -n "$0"

echo "===== scripts refuse simulate / require green SEV ====="
grep -Fq 'simulate != false' "$validator"
grep -Fq 'hardware mode only' "$validator"
grep -Fq 'A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED' "$audit"
grep -Fq 'Integration (hardware SEV-SNP)' "$tools/verify_bx0_tee_box_hardware_run.sh"
grep -Fq 'skipped/failure/cancelled are not TEE certification' "$tools/verify_bx0_tee_box_hardware_run.sh"
grep -Fq 'verify_bx0_tee_box_hardware_run' "$collector"
grep -Fq 'verify_bx0_tee_box_hardware_run' "$audit"

echo "===== validator: example PLACEHOLDER must FAIL ====="
set +e
bash "$validator" "$example" "$cloud_revision" "$box_revision" \
  >"$evidence_directory/validator-example.out" 2>"$evidence_directory/validator-example.err"
example_status=$?
set -e
if ((example_status == 0)); then
  printf '%s\n' "expected PLACEHOLDER example to FAIL" >&2
  exit 1
fi

echo "===== validator: simulate=true must FAIL ====="
sim="$evidence_directory/simulate-true.txt"
printf '%s\n' \
  "A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED cloud_revision=$cloud_revision box_revision=$box_revision box_hardware_sev_run=https://github.com/A3S-Lab/Box/actions/runs/1 generation=milan expected_measurement=$meas simulate=true" \
  >"$sim"
set +e
bash "$validator" "$sim" "$cloud_revision" "$box_revision" \
  >"$evidence_directory/validator-sim.out" 2>"$evidence_directory/validator-sim.err"
sim_status=$?
set -e
if ((sim_status == 0)); then
  printf '%s\n' "expected simulate=true to FAIL" >&2
  exit 1
fi
grep -Fq 'hardware mode only' "$evidence_directory/validator-sim.err"

echo "===== validator: good hardware line must PASS ====="
good="$evidence_directory/good-tee-certification.txt"
printf '%s\n' \
  "A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED cloud_revision=$cloud_revision box_revision=$box_revision box_hardware_sev_run=https://github.com/A3S-Lab/Box/actions/runs/1 generation=milan expected_measurement=$meas simulate=false" \
  >"$good"
bash "$validator" "$good" "$cloud_revision" "$box_revision" \
  >"$evidence_directory/validator-good.out" 2>"$evidence_directory/validator-good.err"

echo "===== audit without certification must exit 2 BLOCKED ====="
unset A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION || true
audit_evidence="$evidence_directory/audit-blocked"
mkdir -p -- "$audit_evidence"
set +e
bash "$audit" "$audit_evidence" \
  >"$evidence_directory/audit-blocked.out" 2>"$evidence_directory/audit-blocked.err"
audit_status=$?
set -e
if ((audit_status != 2)); then
  printf '%s\n' "expected TEE audit exit 2 without evidence, got $audit_status" >&2
  cat "$evidence_directory/audit-blocked.out" >&2 || true
  cat "$evidence_directory/audit-blocked.err" >&2 || true
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED' \
  "$audit_evidence/bx0-tee-isolation-audit.txt"
if grep -Fq 'A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED' \
  "$audit_evidence/bx0-tee-isolation-audit.txt"
then
  printf '%s\n' "blocked audit must not emit TEE_ISOLATION_CERTIFIED" >&2
  exit 1
fi

echo "===== audit rejects hand-edited cert for skipped pin run ====="
if gh api "repos/A3S-Lab/Box/actions/runs/34723450680" --jq .id >/dev/null 2>&1; then
  hand="$evidence_directory/hand-edited-skipped.txt"
  printf '%s\n' \
    "A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED cloud_revision=$cloud_revision box_revision=$box_revision box_hardware_sev_run=https://github.com/A3S-Lab/Box/actions/runs/34723450680 generation=milan expected_measurement=$meas simulate=false" \
    >"$hand"
  # Format validates, but remote job is skipped → audit must FAIL/BLOCKED
  unset A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION || true
  export A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION=$hand
  hand_evidence="$evidence_directory/audit-hand-edited"
  mkdir -p -- "$hand_evidence"
  set +e
  bash "$audit" "$hand_evidence" \
    >"$evidence_directory/audit-hand.out" 2>"$evidence_directory/audit-hand.err"
  hand_status=$?
  set -e
  unset A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION || true
  if ((hand_status == 0)); then
    printf '%s\n' "expected audit to reject skipped SEV run behind hand-edited cert" >&2
    exit 1
  fi
  grep -Fq 'A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED' \
    "$hand_evidence/bx0-tee-isolation-audit.txt"
  echo "hand-edited skipped-run reject: OK"
else
  echo "Box Actions API unavailable; skipped hand-edited audit check"
fi

echo "===== collector: PLACEHOLDER / bad shape must FAIL before gh ====="
set +e
bash "$collector" \
  --box-hardware-sev-run PLACEHOLDER_RUN \
  --generation milan \
  --expected-measurement "$meas" \
  --evidence-dir "$evidence_directory/collector-placeholder" \
  >"$evidence_directory/collector-placeholder.out" 2>"$evidence_directory/collector-placeholder.err"
ph_status=$?
set -e
if ((ph_status == 0)); then
  printf '%s\n' "expected collector PLACEHOLDER reject" >&2
  exit 1
fi

set +e
bash "$collector" \
  --box-hardware-sev-run https://github.com/A3S-Lab/Box/actions/runs/1 \
  --generation milan \
  --expected-measurement deadbeef \
  --evidence-dir "$evidence_directory/collector-bad-meas" \
  >"$evidence_directory/collector-bad-meas.out" 2>"$evidence_directory/collector-bad-meas.err"
bad_meas_status=$?
set -e
if ((bad_meas_status == 0)); then
  printf '%s\n' "expected collector bad measurement reject" >&2
  exit 1
fi

echo "===== optional: collector rejects skipped pin run when Box API readable ====="
pin_run=34723450680
if gh api "repos/A3S-Lab/Box/actions/runs/$pin_run" --jq .id >/dev/null 2>&1; then
  set +e
  bash "$collector" \
    --box-hardware-sev-run "https://github.com/A3S-Lab/Box/actions/runs/$pin_run" \
    --generation milan \
    --expected-measurement "$meas" \
    --evidence-dir "$evidence_directory/collector-skipped" \
    >"$evidence_directory/collector-skipped.out" 2>"$evidence_directory/collector-skipped.err"
  skipped_status=$?
  set -e
  if ((skipped_status == 0)); then
    printf '%s\n' "expected collector to reject skipped SEV job on pin" >&2
    exit 1
  fi
  if ! grep -Eq 'not green|skipped|head_sha mismatch' "$evidence_directory/collector-skipped.err"; then
    printf '%s\n' "expected skipped/not-green/pin-mismatch error from collector" >&2
    cat "$evidence_directory/collector-skipped.err" >&2
    exit 1
  fi
  echo "collector skipped-job reject: OK"
else
  echo "Box Actions API unavailable in this environment; skipped live collector check"
fi

echo "A3S_CLOUD_BX0_TEE_ISOLATION_CI_HARNESS_PASSED"
echo "Harness does not certify BX0 Verified; hardware SEV evidence remains required."
