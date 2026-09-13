#!/usr/bin/env bash
# BX0.3 hardware TEE isolation audit.
# Requires a validated A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED file bound to the
# exact Cloud tip and tools/box-conformance/box-revision, produced only after a
# green Box Integration (hardware SEV-SNP) job (simulate=false).
# Without that evidence, prints A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED and exits 2.
# Never fakes TEE_ISOLATION_CERTIFIED. Never accepts simulated SEV-SNP.
#
# Usage:
#   bash tools/box-conformance/run_bx0_tee_isolation_audit.sh [EVIDENCE_DIR]
#
# Env:
#   A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION — path to TEE cert file

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
box_revision_file="$tools/box-revision"
validator="$tools/validate_bx0_tee_isolation_evidence.sh"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-tee-audit.XXXXXX")
fi
mkdir -p -- "$evidence_directory"

cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
box_revision=$(<"$box_revision_file")

# shellcheck source=validate_bx0_tee_isolation_evidence.sh
# shellcheck disable=SC1090
source "$validator"
# shellcheck source=verify_bx0_tee_box_hardware_run.sh
# shellcheck disable=SC1090
source "$tools/verify_bx0_tee_box_hardware_run.sh"

extract_tee_run_url() {
  local cert_file=$1 line
  while IFS= read -r line || [[ -n $line ]]; do
    line=${line#"${line%%[![:space:]]*}"}
    line=${line%"${line##*[![:space:]]}"}
    [[ -z $line || $line == \#* ]] && continue
    if [[ $line == A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED\ * ]]; then
      # shellcheck disable=SC2206
      local tokens=(${line#A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED })
      local token key value
      for token in "${tokens[@]}"; do
        key=${token%%=*}
        value=${token#*=}
        if [[ $key == box_hardware_sev_run ]]; then
          printf '%s\n' "$value"
          return 0
        fi
      done
    fi
  done <"$cert_file"
  return 1
}

tee_status=OPEN
tee_detail="operator-bound hardware TEE isolation evidence missing"
if [[ -n ${A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION:-} ]]; then
  if validate_bx0_tee_isolation_evidence \
    "$A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION" \
    "$cloud_revision" "$box_revision"
  then
    run_url=$(extract_tee_run_url "$A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION" || true)
    if [[ -z ${run_url:-} ]]; then
      tee_status=FAIL
      tee_detail="certification line missing box_hardware_sev_run"
    elif ! verify_bx0_tee_box_hardware_run "$run_url" "$box_revision"; then
      tee_status=FAIL
      tee_detail="Box hardware SEV remote verification failed for $run_url"
    else
      tee_status=PASS
      tee_detail="TEE isolation certified via $A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION"
      cp -- "$A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION" \
        "$evidence_directory/bx0-tee-isolation-certification.txt"
    fi
  else
    tee_status=FAIL
    tee_detail="A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION set but certification line invalid"
  fi
fi

printf 'tee_status=%s detail=%s\n' "$tee_status" "$tee_detail" \
  | tee "$evidence_directory/tee-status.txt"

if [[ $tee_status != PASS ]]; then
  printf '%s\n' \
    "A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED cloud_revision=$cloud_revision box_revision=$box_revision reason=hardware_sev_evidence_unavailable" \
    | tee "$evidence_directory/bx0-tee-isolation-audit.txt"
  printf '%s\n' \
    "BX0 TEE isolation blocked: green Box Integration (hardware SEV-SNP) evidence required" \
    "Arm per Box docs/ci-kvm-runner.md; bind a TEE_ISOLATION_CERTIFIED line via" \
    "A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION (simulate=false only)" >&2
  exit 2
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED cloud_revision=$cloud_revision box_revision=$box_revision source=$A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION" \
  | tee "$evidence_directory/bx0-tee-isolation-audit.txt"
