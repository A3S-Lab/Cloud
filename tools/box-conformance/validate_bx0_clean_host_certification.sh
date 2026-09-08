#!/usr/bin/env bash
# Validate operator-owned BX0.5 clean-host LOOP certification files.
# Source this file and call validate_bx0_clean_host_certification, or run:
#   bash validate_bx0_clean_host_certification.sh CERT_FILE \
#     EXPECTED_CLOUD EXPECTED_RUNTIME EXPECTED_BOX EXPECTED_GATEWAY
#
# Exit codes when executed as a script:
#   0 — valid LOOP certification line
#   1 — invalid / missing / placeholder / revision mismatch
#
# This validator never emits A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED.

validate_bx0_clean_host_certification() {
  local cert_file=$1
  local expected_cloud=$2
  local expected_runtime=$3
  local expected_box=$4
  local expected_gateway=$5
  local line key value
  local cloud_revision= runtime_revision= box_revision= gateway_revision=
  local host= service_id= node_id= artifact_digest=
  local -a tokens
  local found=0

  if [[ ! -s $cert_file ]]; then
    printf '%s\n' "BX0 clean-host certification file missing or empty: $cert_file" >&2
    return 1
  fi

  for value in "$expected_cloud" "$expected_runtime" "$expected_box" "$expected_gateway"; do
    if [[ ! $value =~ ^[0-9a-f]{40}$ ]]; then
      printf '%s\n' "expected pin must be 40 lowercase hex: $value" >&2
      return 1
    fi
  done

  while IFS= read -r line || [[ -n $line ]]; do
    line=${line#"${line%%[![:space:]]*}"}
    line=${line%"${line##*[![:space:]]}"}
    [[ -z $line || $line == \#* ]] && continue

    if [[ $line != A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED\ * ]]; then
      continue
    fi

    found=1
    cloud_revision= runtime_revision= box_revision= gateway_revision=
    host= service_id= node_id= artifact_digest=
    # shellcheck disable=SC2206
    tokens=(${line#A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED })
    for token in "${tokens[@]}"; do
      key=${token%%=*}
      value=${token#*=}
      if [[ $key == "$token" || -z $value ]]; then
        printf '%s\n' "BX0 clean-host certification field empty or malformed: $token" >&2
        return 1
      fi
      case $key in
        cloud_revision) cloud_revision=$value ;;
        runtime_revision) runtime_revision=$value ;;
        box_revision) box_revision=$value ;;
        gateway_revision) gateway_revision=$value ;;
        host) host=$value ;;
        service_id) service_id=$value ;;
        node_id) node_id=$value ;;
        artifact_digest) artifact_digest=$value ;;
        *)
          printf '%s\n' "BX0 clean-host certification unknown field: $key" >&2
          return 1
          ;;
      esac
    done

    if [[ -z $cloud_revision || -z $runtime_revision || -z $box_revision \
      || -z $gateway_revision || -z $host || -z $service_id || -z $node_id \
      || -z $artifact_digest ]]; then
      printf '%s\n' \
        "BX0 clean-host certification missing required fields" \
        "(cloud/runtime/box/gateway revisions, host, service_id, node_id, artifact_digest)" >&2
      return 1
    fi

    for value in "$cloud_revision" "$runtime_revision" "$box_revision" "$gateway_revision"; do
      if [[ ! $value =~ ^[0-9a-f]{40}$ ]]; then
        printf '%s\n' "BX0 clean-host certification revision must be 40 lowercase hex: $value" >&2
        return 1
      fi
    done

    if [[ $cloud_revision != "$expected_cloud" ]]; then
      printf '%s\n' \
        "BX0 clean-host cloud_revision mismatch: got=$cloud_revision expected=$expected_cloud" >&2
      return 1
    fi
    if [[ $runtime_revision != "$expected_runtime" ]]; then
      printf '%s\n' \
        "BX0 clean-host runtime_revision mismatch: got=$runtime_revision expected=$expected_runtime" >&2
      return 1
    fi
    if [[ $box_revision != "$expected_box" ]]; then
      printf '%s\n' \
        "BX0 clean-host box_revision mismatch: got=$box_revision expected=$expected_box" >&2
      return 1
    fi
    if [[ $gateway_revision != "$expected_gateway" ]]; then
      printf '%s\n' \
        "BX0 clean-host gateway_revision mismatch: got=$gateway_revision expected=$expected_gateway" >&2
      return 1
    fi

    for value in "$host" "$service_id" "$node_id" "$artifact_digest" \
      "$cloud_revision" "$runtime_revision" "$box_revision" "$gateway_revision"; do
      if [[ $value == PLACEHOLDER_* ]]; then
        printf '%s\n' "BX0 clean-host certification rejects PLACEHOLDER_* values: $value" >&2
        return 1
      fi
    done

    return 0
  done <"$cert_file"

  if ((found == 0)); then
    printf '%s\n' \
      "BX0 clean-host certification missing A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED line: $cert_file" >&2
  fi
  return 1
}

if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  set -euo pipefail
  if (($# != 5)); then
    printf '%s\n' \
      "usage: validate_bx0_clean_host_certification.sh CERT_FILE CLOUD RUNTIME BOX GATEWAY" >&2
    exit 1
  fi
  validate_bx0_clean_host_certification "$1" "$2" "$3" "$4" "$5"
fi
