#!/usr/bin/env bash
# BX0.5 clean-host product exit audit.
# Requires a validated LOOP certification file, matching gate execute receipts
# (steps 1–9 *=executed), and a bound Power pin.
# Without any of those, prints A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED and exits 2.
# Never fakes EXIT_CERTIFIED.
#
# Usage:
#   bash tools/box-conformance/run_bx0_clean_host_exit_audit.sh [EVIDENCE_DIR]
#
# Env:
#   A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION — path to LOOP cert file
#   A3S_CLOUD_BX0_EVIDENCE_DIR — absolute gate evidence with execute receipts
#   A3S_CLOUD_BX0_POWER_REVISION_FILE — optional Power pin (40 hex); absent → BLOCKED

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
box_revision_file="$tools/box-revision"
runtime_revision_file="$repository_root/tools/runtime-conformance/runtime-revision"
gateway_revision_file="$repository_root/tools/gateway-conformance/gateway-revision"
power_revision_file="${A3S_CLOUD_BX0_POWER_REVISION_FILE:-$repository_root/tools/power-conformance/power-revision}"
validator="$tools/validate_bx0_clean_host_certification.sh"
# shellcheck source=bx0_clean_host_steps.sh
# shellcheck disable=SC1090
source "$tools/bx0_clean_host_steps.sh"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-exit-audit.XXXXXX")
fi
mkdir -p -- "$evidence_directory"

cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
runtime_revision=$(<"$runtime_revision_file")
box_revision=$(<"$box_revision_file")
gateway_revision=$(<"$gateway_revision_file")

# shellcheck source=validate_bx0_clean_host_certification.sh
# shellcheck disable=SC1090
source "$validator"

bx0_loop_cert_field() {
  local cert_file=$1 field=$2 line token key value
  while IFS= read -r line || [[ -n $line ]]; do
    line=${line#"${line%%[![:space:]]*}"}
    line=${line%"${line##*[![:space:]]}"}
    [[ $line == A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED\ * ]] || continue
    # shellcheck disable=SC2206
    for token in ${line#A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED }; do
      key=${token%%=*}
      value=${token#*=}
      if [[ $key == "$field" ]]; then
        printf '%s\n' "$value"
        return 0
      fi
    done
  done <"$cert_file"
  return 1
}

loop_status=OPEN
loop_detail="operator-owned clean-host LOOP certification missing"
if [[ -n ${A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION:-} ]]; then
  if validate_bx0_clean_host_certification \
    "$A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION" \
    "$cloud_revision" "$runtime_revision" "$box_revision" "$gateway_revision"
  then
    loop_status=PASS
    loop_detail="LOOP certified via $A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION"
    cp -- "$A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION" \
      "$evidence_directory/bx0-clean-host-certification.txt"
  else
    loop_status=FAIL
    loop_detail="A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION set but certification line invalid"
  fi
fi

printf 'loop_status=%s detail=%s\n' "$loop_status" "$loop_detail" \
  | tee "$evidence_directory/loop-status.txt"

if [[ $loop_status != PASS ]]; then
  printf '%s\n' \
    "A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED cloud_revision=$cloud_revision reason=loop_certification_unavailable" \
    | tee "$evidence_directory/bx0-exit-certification.txt"
  printf '%s\n' \
    "BX0 product exit blocked: clean-host LOOP evidence required" \
    "Set A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION to a LOOP_CERTIFIED file" >&2
  exit 2
fi

gate_evidence_dir=${A3S_CLOUD_BX0_EVIDENCE_DIR:-}
receipts_status=OPEN
receipts_detail="gate execute receipts missing"
if [[ -z $gate_evidence_dir ]]; then
  receipts_status=FAIL
  receipts_detail="A3S_CLOUD_BX0_EVIDENCE_DIR unset"
elif [[ $gate_evidence_dir != /* ]]; then
  receipts_status=FAIL
  receipts_detail="gate evidence dir not absolute: $gate_evidence_dir"
else
  loop_node_id=$(bx0_loop_cert_field \
    "$A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION" node_id)
  loop_artifact_digest=$(bx0_loop_cert_field \
    "$A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION" artifact_digest)
  loop_service_id=$(bx0_loop_cert_field \
    "$A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION" service_id)
  if bx0_require_execute_receipts_dir \
    "$gate_evidence_dir" "$loop_node_id" "$loop_artifact_digest" "$loop_service_id"
  then
    receipts_status=PASS
    receipts_detail="execute receipts complete under $gate_evidence_dir"
  else
    receipts_status=FAIL
    receipts_detail="execute receipts incomplete under $gate_evidence_dir"
  fi
fi

printf 'receipts_status=%s detail=%s\n' "$receipts_status" "$receipts_detail" \
  | tee "$evidence_directory/receipts-status.txt"

if [[ $receipts_status != PASS ]]; then
  printf '%s\n' \
    "A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED cloud_revision=$cloud_revision reason=execute_receipts_incomplete" \
    "loop=PASS receipts=FAIL" \
    | tee "$evidence_directory/bx0-exit-certification.txt"
  printf '%s\n' \
    "BX0 product exit blocked: gate execute receipts required (steps 1–9 *=executed)" \
    "Set A3S_CLOUD_BX0_EVIDENCE_DIR to the armed EXECUTE gate evidence directory" >&2
  exit 2
fi

# First principles: missing pin and invalid pin are different fail-closed
# contracts. Collapsing both to power_unbound hides forged/typo pins.
power_status=OPEN
power_detail="power pin missing (PW0)"
power_revision=
power_block_reason=power_unbound
if [[ -f $power_revision_file ]]; then
  power_revision=$(<"$power_revision_file")
  if [[ $power_revision =~ ^[0-9a-f]{40}$ ]]; then
    power_status=PASS
    power_detail="power_revision=$power_revision"
    power_block_reason=
  else
    power_status=FAIL
    power_detail="power pin invalid: $power_revision"
    power_block_reason=power_pin_invalid
  fi
fi

printf 'power_status=%s detail=%s\n' "$power_status" "$power_detail" \
  | tee "$evidence_directory/power-status.txt"

if [[ $power_status != PASS ]]; then
  power_label=UNBOUND
  if [[ $power_block_reason == power_pin_invalid ]]; then
    power_label=INVALID
  fi
  printf '%s\n' \
    "A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED cloud_revision=$cloud_revision reason=$power_block_reason" \
    "loop=PASS receipts=PASS power=$power_label" \
    | tee "$evidence_directory/bx0-exit-certification.txt"
  if [[ $power_block_reason == power_pin_invalid ]]; then
    printf '%s\n' \
      "BX0 product exit blocked: Power pin present but not exact lowercase 40-hex (PW0)" >&2
  else
    printf '%s\n' \
      "BX0 product exit blocked: Power pin required (PW0); LOOP alone is insufficient" >&2
  fi
  exit 2
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED cloud_revision=$cloud_revision runtime_revision=$runtime_revision box_revision=$box_revision gateway_revision=$gateway_revision power_revision=$power_revision loop=included receipts=included" \
  | tee "$evidence_directory/bx0-exit-certification.txt"
