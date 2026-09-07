#!/usr/bin/env bash
# Validate operator-owned U0.3 live Cloud↔host certification files.
# Source this file and call validate_live_host_certification, or run:
#   bash validate_live_host_certification.sh CERT_FILE EXPECTED_REVISION
#
# Exit codes when executed as a script:
#   0 — valid certification line
#   1 — invalid / missing / placeholder / revision mismatch

validate_live_host_certification() {
  local cert_file=$1
  local expected_revision=$2
  local line key value revision host assignment package plan_digest
  local -a tokens
  local found=0

  if [[ ! -s $cert_file ]]; then
    printf '%s\n' "live-host certification file missing or empty: $cert_file" >&2
    return 1
  fi

  if [[ ! $expected_revision =~ ^[0-9a-f]{40}$ ]]; then
    printf '%s\n' "expected revision must be 40 lowercase hex: $expected_revision" >&2
    return 1
  fi

  while IFS= read -r line || [[ -n $line ]]; do
    # Strip leading/trailing whitespace; skip blank and comment lines.
    line=${line#"${line%%[![:space:]]*}"}
    line=${line%"${line##*[![:space:]]}"}
    [[ -z $line || $line == \#* ]] && continue

    if [[ $line != A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED\ * ]]; then
      continue
    fi

    found=1
    revision= host= assignment= package= plan_digest=
    # shellcheck disable=SC2206
    tokens=(${line#A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED })
    for token in "${tokens[@]}"; do
      key=${token%%=*}
      value=${token#*=}
      if [[ $key == "$token" || -z $value ]]; then
        printf '%s\n' "live-host certification field empty or malformed: $token" >&2
        return 1
      fi
      case $key in
        revision) revision=$value ;;
        host) host=$value ;;
        assignment) assignment=$value ;;
        package) package=$value ;;
        plan_digest) plan_digest=$value ;;
        *)
          printf '%s\n' "live-host certification unknown field: $key" >&2
          return 1
          ;;
      esac
    done

    if [[ -z $revision || -z $host || -z $assignment || -z $package || -z $plan_digest ]]; then
      printf '%s\n' \
        "live-host certification missing required fields (revision/host/assignment/package/plan_digest)" >&2
      return 1
    fi

    if [[ ! $revision =~ ^[0-9a-f]{40}$ ]]; then
      printf '%s\n' "live-host certification revision must be 40 lowercase hex: $revision" >&2
      return 1
    fi

    if [[ $revision != "$expected_revision" ]]; then
      printf '%s\n' \
        "live-host certification revision mismatch: got=$revision expected=$expected_revision" >&2
      return 1
    fi

    for value in "$host" "$assignment" "$package" "$plan_digest" "$revision"; do
      if [[ $value == PLACEHOLDER_* ]]; then
        printf '%s\n' "live-host certification rejects PLACEHOLDER_* values: $value" >&2
        return 1
      fi
    done

    return 0
  done <"$cert_file"

  if ((found == 0)); then
    printf '%s\n' \
      "live-host certification missing A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED line: $cert_file" >&2
  fi
  return 1
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  set -euo pipefail
  if (($# != 2)); then
    printf '%s\n' "usage: validate_live_host_certification.sh CERT_FILE EXPECTED_REVISION" >&2
    exit 1
  fi
  validate_live_host_certification "$1" "$2"
fi
