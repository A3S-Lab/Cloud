#!/usr/bin/env bash
# One-command GA-0 / BX0.software LIVE EXIT binder for a clean Linux host.
#
# Arms LIVE + CREATE + CREATE_FULL and runs the software EXIT harness.
# Fail-closes on Docker sock / DOCKER_HOST — never overrides the sock path to
# fake a clean host.
#
# Required (same as run_bx0_software_loop_create.sh CREATE_FULL):
#   Cloud URL/token/org/project/environment
#   A3S_CLOUD_BX0_NODE_CONFIG, agent release URL+sha256
#   Pre-published ARTIFACT_URI + DIGEST (+ UPDATE_* for revision flip)
#   A3S_CLOUD_BX0_HEALTH_URL, A3S_CLOUD_BX0_HTTPS_URL
#   Pin-matched a3s-box (install_box_release.sh)
#
# Usage:
#   bash tools/box-conformance/run_bx0_software_exit_live.sh [EVIDENCE_DIR]

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
harness="$tools/run_bx0_software_exit_harness.sh"

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
bx0_refuse_docker_host || exit 2

if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
  printf '%s\n' \
    "A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED reason=host_unsupported got=$(uname -s)-$(uname -m)" >&2
  exit 2
fi

export A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1
export A3S_CLOUD_BX0_SOFTWARE_EXIT_CREATE=1
export A3S_CLOUD_BX0_CREATE_FULL=${A3S_CLOUD_BX0_CREATE_FULL:-1}
export A3S_CLOUD_BX0_EXIT_PROFILE=software

exec bash "$harness" "$@"
