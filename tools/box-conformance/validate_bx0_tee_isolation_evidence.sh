#!/usr/bin/env bash
# Validate operator-bound BX0.3 hardware TEE isolation evidence.
# Source this file and call validate_bx0_tee_isolation_evidence, or run:
#   bash validate_bx0_tee_isolation_evidence.sh CERT_FILE EXPECTED_CLOUD EXPECTED_BOX
#
# Exit codes when executed as a script:
#   0 — valid TEE isolation certification line
#   1 — invalid / missing / simulated / revision mismatch
#
# Never accepts simulate=true. Never invents certification.

validate_bx0_tee_isolation_evidence() {
  local cert_file=$1
  local expected_cloud=$2
  local expected_box=$3
  local line key value
  local cloud_revision= box_revision= box_hardware_sev_run=
  local generation= expected_measurement= simulate=
  local -a tokens
  local found=0

  if [[ ! -s $cert_file ]]; then
    printf '%s\n' "BX0 TEE isolation evidence file missing or empty: $cert_file" >&2
    return 1
  fi

  for value in "$expected_cloud" "$expected_box"; do
    if [[ ! $value =~ ^[0-9a-f]{40}$ ]]; then
      printf '%s\n' "expected pin must be 40 lowercase hex: $value" >&2
      return 1
    fi
  done

  while IFS= read -r line || [[ -n $line ]]; do
    line=${line#"${line%%[![:space:]]*}"}
    line=${line%"${line##*[![:space:]]}"}
    [[ -z $line || $line == \#* ]] && continue

    if [[ $line != A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED\ * ]]; then
      continue
    fi

    found=1
    cloud_revision= box_revision= box_hardware_sev_run=
    generation= expected_measurement= simulate=
    # shellcheck disable=SC2206
    tokens=(${line#A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED })
    for token in "${tokens[@]}"; do
      key=${token%%=*}
      value=${token#*=}
      if [[ $key == "$token" || -z $value ]]; then
        printf '%s\n' "BX0 TEE isolation field empty or malformed: $token" >&2
        return 1
      fi
      case $key in
        cloud_revision) cloud_revision=$value ;;
        box_revision) box_revision=$value ;;
        box_hardware_sev_run) box_hardware_sev_run=$value ;;
        generation) generation=$value ;;
        expected_measurement) expected_measurement=$value ;;
        simulate) simulate=$value ;;
        *)
          printf '%s\n' "BX0 TEE isolation unknown field: $key" >&2
          return 1
          ;;
      esac
    done

    if [[ -z $cloud_revision || -z $box_revision || -z $box_hardware_sev_run \
      || -z $generation || -z $expected_measurement || -z $simulate ]]; then
      printf '%s\n' \
        "BX0 TEE isolation missing required fields" \
        "(cloud_revision, box_revision, box_hardware_sev_run, generation," \
        "expected_measurement, simulate)" >&2
      return 1
    fi

    if [[ $simulate != false ]]; then
      printf '%s\n' \
        "BX0 TEE isolation rejects simulate=$simulate (hardware mode only)" >&2
      return 1
    fi

    for value in "$cloud_revision" "$box_revision" "$box_hardware_sev_run" \
      "$generation" "$expected_measurement" "$simulate"; do
      if [[ $value == PLACEHOLDER_* ]]; then
        printf '%s\n' "BX0 TEE isolation rejects PLACEHOLDER_* field values" >&2
        return 1
      fi
    done

    case $generation in
      milan|genoa) ;;
      *)
        printf '%s\n' "BX0 TEE isolation generation must be milan or genoa: $generation" >&2
        return 1
        ;;
    esac

    if [[ ! $expected_measurement =~ ^[0-9a-f]{96}$ ]]; then
      printf '%s\n' \
        "BX0 TEE isolation expected_measurement must be 96 lowercase hex" >&2
      return 1
    fi

    if [[ ! $cloud_revision =~ ^[0-9a-f]{40}$ || ! $box_revision =~ ^[0-9a-f]{40}$ ]]; then
      printf '%s\n' "BX0 TEE isolation revisions must be 40 lowercase hex" >&2
      return 1
    fi

    if [[ $cloud_revision != "$expected_cloud" ]]; then
      printf '%s\n' \
        "BX0 TEE isolation cloud_revision mismatch: $cloud_revision != $expected_cloud" >&2
      return 1
    fi

    if [[ $box_revision != "$expected_box" ]]; then
      printf '%s\n' \
        "BX0 TEE isolation box_revision mismatch: $box_revision != $expected_box" >&2
      return 1
    fi

    if [[ ! $box_hardware_sev_run =~ ^https://github.com/A3S-Lab/Box/actions/runs/[0-9]+$ ]]; then
      printf '%s\n' \
        "BX0 TEE isolation box_hardware_sev_run must be an A3S-Lab/Box Actions run URL" >&2
      return 1
    fi

    return 0
  done <"$cert_file"

  if [[ $found -eq 0 ]]; then
    printf '%s\n' \
      "BX0 TEE isolation certification line missing" \
      "(expected A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED ...)" >&2
    return 1
  fi

  return 1
}

# When sourced, only define the function. When executed, validate argv.
if [[ ${BASH_SOURCE[0]} == "$0" ]]; then
  if [[ $# -ne 3 ]]; then
    printf '%s\n' \
      "usage: $0 CERT_FILE EXPECTED_CLOUD EXPECTED_BOX" >&2
    exit 2
  fi
  validate_bx0_tee_isolation_evidence "$1" "$2" "$3"
fi
