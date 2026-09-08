#!/usr/bin/env bash
# BX0.5 / E0 Box re-certification: clean-host release gate entrypoint.
#
# Ordered checklist (must remain exact; automation must not skip steps):
#   1. enroll     — outbound node enrollment on a clean Linux host
#   2. OCI        — build/publish digest-pinned OCI Artifact
#   3. deploy     — deploy one Box-hosted Service via ordinary Runtime path
#   4. health     — require Healthy / ready evidence
#   5. HTTPS      — managed Gateway route + TLS reachability
#   6. logs       — durable ordered log readback
#   7. update     — immutable revision update
#   8. rollback   — cloned prior-revision rollback
#   9. stop/cleanup — stop, remove, restore empty provider + host inventory
#
# This harness does NOT emit A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED.
# Product exit stays open until a joint Cloud+Box+Gateway automation lands.
set -euo pipefail

readonly SCRIPT_DIRECTORY="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly CLOUD_ROOT="$(cd "$SCRIPT_DIRECTORY/../.." && pwd)"
readonly BOX_REVISION_FILE="$SCRIPT_DIRECTORY/box-revision"
readonly INSTALL_BOX_RELEASE="$SCRIPT_DIRECTORY/install_box_release.sh"

die() {
  printf 'BX0 clean-host gate: %s\n' "$1" >&2
  exit 1
}

print_checklist() {
  cat <<'CHECKLIST'
BX0.5/E0 Box re-cert checklist (ordered):
  1. enroll
  2. OCI (build/publish digest-pinned Artifact)
  3. deploy
  4. health
  5. HTTPS (managed Gateway TLS)
  6. logs
  7. update
  8. rollback
  9. stop/cleanup (empty Box + host inventory)
CHECKLIST
}

print_required_markers() {
  cat <<'MARKERS'
Required certification markers (not emitted by this scaffold):
  A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED
    Must bind exact Cloud, Runtime, Box, Gateway, and Power revisions and
    prove the full enroll→…→stop/cleanup loop with no residue.
  Companion provider/consumer evidence from tools/box-conformance gates
    remains necessary but is not sufficient for clean-host product exit.
MARKERS
}

os_name="$(uname -s)"
if [[ $os_name != Linux ]]; then
  printf '%s\n' \
    "BX0 clean-host gate: FAIL_CLOSED host_os=$os_name" \
    'requires a supported Linux host (no Docker/compatible daemon fallback)' >&2
  exit 1
fi

box_binary="${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}"
armed="${A3S_CLOUD_BX0_CLEAN_HOST:-}"

if [[ $armed != 1 ]]; then
  print_checklist
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' \
    'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=A3S_CLOUD_BX0_CLEAN_HOST_unset' \
    'Set A3S_CLOUD_BX0_CLEAN_HOST=1 on a clean Linux host with a3s-box to attempt the harness.' >&2
  exit 2
fi

if [[ -z $box_binary || ! -x $box_binary ]]; then
  print_checklist
  printf '%s\n' \
    'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' \
    'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED reason=a3s-box_unavailable' \
    "Install pinned Box via: $INSTALL_BOX_RELEASE ABSOLUTE_EMPTY_INSTALL_DIRECTORY" \
    'Then export A3S_CLOUD_BOX_BIN to that install tree a3s-box binary.' >&2
  exit 2
fi

expected_box_revision=
if [[ -f $BOX_REVISION_FILE ]]; then
  expected_box_revision="$(<"$BOX_REVISION_FILE")"
fi

printf 'BX0 clean-host gate: armed on Linux with a3s-box=%s\n' "$box_binary"
if [[ -n $expected_box_revision ]]; then
  printf 'pinned Box revision: %s\n' "$expected_box_revision"
fi
printf 'Cloud root: %s\n' "$CLOUD_ROOT"
printf 'install helper: %s\n' "$INSTALL_BOX_RELEASE"
print_checklist
print_required_markers

cat <<'OPEN'
A3S_CLOUD_BX0_CLEAN_HOST_OPEN
not yet automated / requires joint Cloud+Box+Gateway harness
This entrypoint refuses to fake EXIT_CERTIFIED.
OPEN
exit 3
