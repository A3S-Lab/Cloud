#!/usr/bin/env bash
# BX0.software rollback probe for clean-host execute step 8.
# Usage: probe_bx0_rollback.sh <sha256-digest>
#
# Same contract as probe_bx0_digest.sh: reject PLACEHOLDER/malformed;
# STRICT mode requires the rollback target digest to appear on the workload.

set -euo pipefail

digest=${1:-}
if [[ -z $digest || $digest == PLACEHOLDER_* ]]; then
  printf '%s\n' "bx0 rollback probe: digest missing or PLACEHOLDER_*" >&2
  exit 1
fi
if [[ ! $digest =~ ^(sha256:)?[0-9a-f]{64}$ ]]; then
  printf '%s\n' "bx0 rollback probe: digest must be sha256: + 64 hex (or bare 64 hex)" >&2
  exit 1
fi
if [[ $digest != sha256:* ]]; then
  digest="sha256:$digest"
fi

if [[ ${A3S_CLOUD_BX0_PROBE_STRICT:-} != 1 ]]; then
  printf '%s\n' "bx0 rollback probe: accepted $digest (non-strict assisted mode)"
  exit 0
fi

: "${A3S_CLOUD_URL:?A3S_CLOUD_URL required in STRICT mode}"
: "${A3S_CLOUD_TOKEN:?A3S_CLOUD_TOKEN required in STRICT mode}"
: "${A3S_CLOUD_ORGANIZATION_ID:?A3S_CLOUD_ORGANIZATION_ID required in STRICT mode}"
: "${A3S_CLOUD_BX0_WORKLOAD_ID:?A3S_CLOUD_BX0_WORKLOAD_ID required in STRICT mode}"

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cli=(bun run --cwd "$repository_root/cli" src/main.ts)

set +e
out=$("${cli[@]}" workloads get "$A3S_CLOUD_BX0_WORKLOAD_ID" --output=json 2>&1)
rc=$?
set -e
if ((rc != 0)); then
  printf '%s\n' "bx0 rollback probe: workloads get failed" "$out" >&2
  exit 1
fi
if ! grep -Fq "$digest" <<<"$out"; then
  printf '%s\n' "bx0 rollback probe: workload JSON does not contain rollback digest $digest" >&2
  exit 1
fi
printf '%s\n' "bx0 rollback probe: STRICT Cloud verification ok for $digest"
exit 0
