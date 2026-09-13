#!/usr/bin/env bash
# Verify a Box Actions run URL is completed success for
# Integration (hardware SEV-SNP) on the expected box-revision.
# Source and call verify_bx0_tee_box_hardware_run RUN_URL EXPECTED_BOX_SHA
# or execute: bash verify_bx0_tee_box_hardware_run.sh RUN_URL EXPECTED_BOX_SHA
#
# Exit 0 on green hardware job; 1 otherwise. Requires gh.

verify_bx0_tee_box_hardware_run() {
  local run_url=$1
  local expected_box=$2
  local run_id job_name job_line job_status job_conclusion run_head run_status

  job_name='Integration (hardware SEV-SNP)'

  if [[ ! $run_url =~ ^https://github.com/A3S-Lab/Box/actions/runs/([0-9]+)$ ]]; then
    printf '%s\n' "invalid Box Actions run URL: $run_url" >&2
    return 1
  fi
  run_id=${BASH_REMATCH[1]}

  if [[ ! $expected_box =~ ^[0-9a-f]{40}$ ]]; then
    printf '%s\n' "expected box revision must be 40 lowercase hex" >&2
    return 1
  fi

  if ! command -v gh >/dev/null 2>&1; then
    printf '%s\n' "gh CLI required to verify Box hardware SEV job" >&2
    return 1
  fi

  run_head=$(gh api "repos/A3S-Lab/Box/actions/runs/$run_id" --jq .head_sha)
  run_status=$(gh api "repos/A3S-Lab/Box/actions/runs/$run_id" --jq .status)

  if [[ $run_head != "$expected_box" ]]; then
    printf '%s\n' \
      "Box run head_sha mismatch: $run_head != expected $expected_box" >&2
    return 1
  fi

  if [[ $run_status != completed ]]; then
    printf '%s\n' "Box run $run_id is not completed (status=$run_status)" >&2
    return 1
  fi

  job_line=$(
    gh api "repos/A3S-Lab/Box/actions/runs/$run_id/jobs" \
      --jq ".jobs[] | select(.name == \"$job_name\") | \"\(.status) \(.conclusion // \"\")\""
  )
  if [[ -z $job_line ]]; then
    printf '%s\n' "Box run $run_id missing job: $job_name" >&2
    return 1
  fi

  job_status=${job_line%% *}
  job_conclusion=${job_line#"$job_status"}
  job_conclusion=${job_conclusion## }

  if [[ $job_status != completed || $job_conclusion != success ]]; then
    printf '%s\n' \
      "Box hardware SEV job not green: status=$job_status conclusion=$job_conclusion" \
      "skipped/failure/cancelled are not TEE certification" >&2
    return 1
  fi

  return 0
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  set -euo pipefail
  if [[ $# -ne 2 ]]; then
    printf '%s\n' "usage: $0 RUN_URL EXPECTED_BOX_SHA" >&2
    exit 2
  fi
  verify_bx0_tee_box_hardware_run "$1" "$2"
fi
