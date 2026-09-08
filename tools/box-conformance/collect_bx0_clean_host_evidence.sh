#!/usr/bin/env bash
# Operator-owned BX0.5 clean-host LOOP evidence collector.
# Writes a validated LOOP_CERTIFIED line plus a checklist. Never writes or
# prints A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED.
#
# Requires the armed gate evidence directory with steps 1–9 *=executed
# (--gate-evidence-dir / A3S_CLOUD_BX0_EVIDENCE_DIR). IDs alone are insufficient.
#
# Usage:
#   bash tools/box-conformance/collect_bx0_clean_host_evidence.sh \
#     --host HOST --service-id ID --node-id ID --artifact-digest DIGEST \
#     --gate-evidence-dir GATE_EVIDENCE_DIR [--evidence-dir DIR]
#
# Required fields may also come from env:
#   A3S_CLOUD_BX0_CLEAN_HOST_HOST, A3S_CLOUD_BX0_CLEAN_HOST_SERVICE_ID,
#   A3S_CLOUD_BX0_CLEAN_HOST_NODE_ID, A3S_CLOUD_BX0_CLEAN_HOST_ARTIFACT_DIGEST
# Gate evidence: A3S_CLOUD_BX0_EVIDENCE_DIR
# Optional output dir: A3S_CLOUD_BX0_CLEAN_HOST_EVIDENCE_DIR

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
box_revision_file="$tools/box-revision"
runtime_revision_file="$repository_root/tools/runtime-conformance/runtime-revision"
gateway_revision_file="$repository_root/tools/gateway-conformance/gateway-revision"
validator="$tools/validate_bx0_clean_host_certification.sh"
# shellcheck source=bx0_clean_host_steps.sh
# shellcheck disable=SC1090
source "$tools/bx0_clean_host_steps.sh"

host=${A3S_CLOUD_BX0_CLEAN_HOST_HOST:-}
service_id=${A3S_CLOUD_BX0_CLEAN_HOST_SERVICE_ID:-}
node_id=${A3S_CLOUD_BX0_CLEAN_HOST_NODE_ID:-}
artifact_digest=${A3S_CLOUD_BX0_CLEAN_HOST_ARTIFACT_DIGEST:-}
gate_evidence_dir=${A3S_CLOUD_BX0_EVIDENCE_DIR:-}
evidence_dir=${A3S_CLOUD_BX0_CLEAN_HOST_EVIDENCE_DIR:-}

usage() {
  cat <<'USAGE' >&2
usage: collect_bx0_clean_host_evidence.sh \
  --host HOST --service-id ID --node-id ID --artifact-digest DIGEST \
  --gate-evidence-dir GATE_EVIDENCE_DIR [--evidence-dir DIR]
USAGE
}

reject_placeholder() {
  local name=$1 value=$2
  if [[ -z $value ]]; then
    printf '%s\n' "required nonempty: $name" >&2
    exit 1
  fi
  if [[ $value == PLACEHOLDER_* ]]; then
    printf '%s\n' "refusing PLACEHOLDER_* for $name: $value" >&2
    exit 1
  fi
}

while (($# > 0)); do
  case $1 in
    --host)
      (($# >= 2)) || { usage; exit 1; }
      host=$2
      shift 2
      ;;
    --service-id)
      (($# >= 2)) || { usage; exit 1; }
      service_id=$2
      shift 2
      ;;
    --node-id)
      (($# >= 2)) || { usage; exit 1; }
      node_id=$2
      shift 2
      ;;
    --artifact-digest)
      (($# >= 2)) || { usage; exit 1; }
      artifact_digest=$2
      shift 2
      ;;
    --gate-evidence-dir)
      (($# >= 2)) || { usage; exit 1; }
      gate_evidence_dir=$2
      shift 2
      ;;
    --evidence-dir)
      (($# >= 2)) || { usage; exit 1; }
      evidence_dir=$2
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      printf '%s\n' "unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

reject_placeholder host "$host"
reject_placeholder service_id "$service_id"
reject_placeholder node_id "$node_id"
reject_placeholder artifact_digest "$artifact_digest"

if [[ -z $gate_evidence_dir ]]; then
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_LOOP_BLOCKED reason=execute_receipts_dir_missing' \
    'Pass --gate-evidence-dir (or A3S_CLOUD_BX0_EVIDENCE_DIR) from an armed EXECUTE gate run.' >&2
  exit 1
fi
if [[ $gate_evidence_dir != /* ]]; then
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_LOOP_BLOCKED reason=execute_receipts_dir_not_absolute' \
    "gate evidence dir must be absolute: $gate_evidence_dir" >&2
  exit 1
fi
if ! bx0_require_execute_receipts_dir \
  "$gate_evidence_dir" "$node_id" "$artifact_digest" "$service_id"
then
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_LOOP_BLOCKED reason=execute_receipts_incomplete' \
    "gate evidence dir=$gate_evidence_dir" >&2
  exit 1
fi

[[ -f $box_revision_file && -f $runtime_revision_file && -f $gateway_revision_file ]] \
  || {
    printf '%s\n' "missing Box/Runtime/Gateway pin files under tools/" >&2
    exit 1
  }

cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
runtime_revision=$(<"$runtime_revision_file")
box_revision=$(<"$box_revision_file")
gateway_revision=$(<"$gateway_revision_file")

for value in "$cloud_revision" "$runtime_revision" "$box_revision" "$gateway_revision"; do
  if [[ ! $value =~ ^[0-9a-f]{40}$ ]]; then
    printf '%s\n' "pin must be 40 lowercase hex: $value" >&2
    exit 1
  fi
done

if [[ -z $evidence_dir ]]; then
  evidence_dir=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-clean-host-evidence.XXXXXX")
fi
mkdir -p -- "$evidence_dir"

cert_file="$evidence_dir/bx0-clean-host-certification.txt"
cert_line="A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED cloud_revision=$cloud_revision runtime_revision=$runtime_revision box_revision=$box_revision gateway_revision=$gateway_revision host=$host service_id=$service_id node_id=$node_id artifact_digest=$artifact_digest"

{
  printf '%s\n' \
    "# Generated by tools/box-conformance/collect_bx0_clean_host_evidence.sh" \
    "# LOOP evidence only — does not claim product EXIT_CERTIFIED." \
    "# Requires gate evidence with steps 1–9 execute receipts." \
    "$cert_line"
} >"$cert_file"

# shellcheck source=validate_bx0_clean_host_certification.sh
# shellcheck disable=SC1090
source "$validator"
validate_bx0_clean_host_certification \
  "$cert_file" "$cloud_revision" "$runtime_revision" "$box_revision" "$gateway_revision"

cat >"$evidence_dir/checklist.txt" <<EOF
BX0.5 clean-host LOOP evidence checklist
host=$host
service_id=$service_id
node_id=$node_id
artifact_digest=$artifact_digest
cloud_revision=$cloud_revision
runtime_revision=$runtime_revision
box_revision=$box_revision
gateway_revision=$gateway_revision
power_revision=UNBOUND reason=pw0_no_pin_file
gate_evidence_dir=$gate_evidence_dir
execute_receipts_complete=1
steps=enroll,oci,deploy,health,https,logs,update,rollback,stop_cleanup
product_exit=not_claimed
EOF

if grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$cert_file" \
  || grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$evidence_dir/checklist.txt"; then
  printf '%s\n' "collector must never claim EXIT_CERTIFIED" >&2
  exit 1
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_LOOP_COLLECTED" \
  "certification=$cert_file" \
  "evidence_dir=$evidence_dir" \
  "gate_evidence_dir=$gate_evidence_dir" \
  "execute_receipts_complete=1" \
  "power=UNBOUND product_exit=not_claimed"
