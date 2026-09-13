#!/usr/bin/env bash
# Operator-owned BX0.3 hardware TEE isolation evidence collector.
# Queries GitHub for a Box Actions run and writes TEE_ISOLATION_CERTIFIED only
# when Integration (hardware SEV-SNP) concluded success on the pinned
# box-revision. Never writes simulate=true. Never invents Verified.
# Skipped, failed, cancelled, or SHA-mismatched runs fail closed.
#
# Usage:
#   bash tools/box-conformance/collect_bx0_tee_isolation_evidence.sh \
#     --box-hardware-sev-run https://github.com/A3S-Lab/Box/actions/runs/<id> \
#     --generation milan|genoa \
#     --expected-measurement <96hex> \
#     [--evidence-dir DIR]
#
# Env alternatives:
#   A3S_CLOUD_BX0_TEE_BOX_HARDWARE_SEV_RUN
#   A3S_CLOUD_BX0_TEE_GENERATION
#   A3S_CLOUD_BX0_TEE_EXPECTED_MEASUREMENT
#   A3S_CLOUD_BX0_TEE_EVIDENCE_DIR
#
# Requires: gh authenticated with read access to A3S-Lab/Box Actions.

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
box_revision_file="$tools/box-revision"
validator="$tools/validate_bx0_tee_isolation_evidence.sh"

box_hardware_sev_run=${A3S_CLOUD_BX0_TEE_BOX_HARDWARE_SEV_RUN:-}
generation=${A3S_CLOUD_BX0_TEE_GENERATION:-}
expected_measurement=${A3S_CLOUD_BX0_TEE_EXPECTED_MEASUREMENT:-}
evidence_dir=${A3S_CLOUD_BX0_TEE_EVIDENCE_DIR:-}
job_name='Integration (hardware SEV-SNP)'

usage() {
  cat <<'USAGE' >&2
usage: collect_bx0_tee_isolation_evidence.sh \
  --box-hardware-sev-run URL \
  --generation milan|genoa \
  --expected-measurement <96hex> \
  [--evidence-dir DIR]
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
    --box-hardware-sev-run)
      (($# >= 2)) || { usage; exit 1; }
      box_hardware_sev_run=$2
      shift 2
      ;;
    --generation)
      (($# >= 2)) || { usage; exit 1; }
      generation=$2
      shift 2
      ;;
    --expected-measurement)
      (($# >= 2)) || { usage; exit 1; }
      expected_measurement=$2
      shift 2
      ;;
    --evidence-dir)
      (($# >= 2)) || { usage; exit 1; }
      evidence_dir=$2
      shift 2
      ;;
    -h|--help)
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

reject_placeholder box_hardware_sev_run "$box_hardware_sev_run"
reject_placeholder generation "$generation"
reject_placeholder expected_measurement "$expected_measurement"

case $generation in
  milan|genoa) ;;
  *)
    printf '%s\n' "generation must be milan or genoa: $generation" >&2
    exit 1
    ;;
esac

if [[ ! $expected_measurement =~ ^[0-9a-f]{96}$ ]]; then
  printf '%s\n' "expected_measurement must be 96 lowercase hex" >&2
  exit 1
fi

if [[ ! $box_hardware_sev_run =~ ^https://github.com/A3S-Lab/Box/actions/runs/([0-9]+)$ ]]; then
  printf '%s\n' \
    "box_hardware_sev_run must be https://github.com/A3S-Lab/Box/actions/runs/<id>" >&2
  exit 1
fi
run_id=${BASH_REMATCH[1]}

if ! command -v gh >/dev/null 2>&1; then
  printf '%s\n' "gh CLI required to verify Box hardware SEV job status" >&2
  exit 1
fi

cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
box_revision=$(tr -d '[:space:]' <"$box_revision_file")
if [[ ! $box_revision =~ ^[0-9a-f]{40}$ ]]; then
  printf '%s\n' "invalid box-revision pin: $box_revision" >&2
  exit 1
fi

# shellcheck source=verify_bx0_tee_box_hardware_run.sh
# shellcheck disable=SC1090
source "$tools/verify_bx0_tee_box_hardware_run.sh"
if ! verify_bx0_tee_box_hardware_run "$box_hardware_sev_run" "$box_revision"; then
  exit 1
fi
run_head=$box_revision
run_conclusion=$(gh api "repos/A3S-Lab/Box/actions/runs/$run_id" --jq '.conclusion // ""')
job_name='Integration (hardware SEV-SNP)'
job_conclusion=success

if [[ -z $evidence_dir ]]; then
  evidence_dir=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-tee-evidence.XXXXXX")
fi
mkdir -p -- "$evidence_dir"
cert_file="$evidence_dir/bx0-tee-isolation-certification.txt"

{
  printf '%s\n' \
    "# Collected by collect_bx0_tee_isolation_evidence.sh" \
    "# Box run conclusion=$run_conclusion job=$job_name conclusion=$job_conclusion" \
    "# head_sha=$run_head (matches box-revision)"
  printf '%s\n' \
    "A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED cloud_revision=$cloud_revision box_revision=$box_revision box_hardware_sev_run=$box_hardware_sev_run generation=$generation expected_measurement=$expected_measurement simulate=false"
} >"$cert_file"

# shellcheck source=validate_bx0_tee_isolation_evidence.sh
# shellcheck disable=SC1090
source "$validator"
validate_bx0_tee_isolation_evidence "$cert_file" "$cloud_revision" "$box_revision"

printf '%s\n' \
  "wrote $cert_file" \
  "export A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION=$cert_file" \
  "then: bash tools/box-conformance/run_bx0_tee_isolation_audit.sh"
