#!/usr/bin/env bash
# BX0.software ordered-log probe for clean-host execute step 6.
# Usage: probe_bx0_logs.sh <ordered-log-cursor>
#
# Always rejects PLACEHOLDER_* and empty cursors.
# With A3S_CLOUD_BX0_PROBE_STRICT=1 (LIVE harness): requires Cloud API env and
# verifies the cursor against workloads logs (fail-closed).
# Without STRICT: accepts a non-placeholder cursor only after the operator has
# already read durable logs (assisted EXECUTE). Never invents log content.

set -euo pipefail

cursor=${1:-}
if [[ -z $cursor || $cursor == PLACEHOLDER_* ]]; then
  printf '%s\n' "bx0 logs probe: cursor missing or PLACEHOLDER_*" >&2
  exit 1
fi

if [[ ${A3S_CLOUD_BX0_PROBE_STRICT:-} != 1 ]]; then
  printf '%s\n' "bx0 logs probe: accepted cursor (non-strict assisted mode)"
  exit 0
fi

: "${A3S_CLOUD_URL:?A3S_CLOUD_URL required in STRICT mode}"
: "${A3S_CLOUD_TOKEN:?A3S_CLOUD_TOKEN required in STRICT mode}"
: "${A3S_CLOUD_ORGANIZATION_ID:?A3S_CLOUD_ORGANIZATION_ID required in STRICT mode}"
: "${A3S_CLOUD_BX0_WORKLOAD_ID:?A3S_CLOUD_BX0_WORKLOAD_ID required in STRICT mode}"
: "${A3S_CLOUD_BX0_REVISION_ID:?A3S_CLOUD_BX0_REVISION_ID required in STRICT mode}"

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cli=(bun run --cwd "$repository_root/cli" src/main.ts)

set +e
out=$("${cli[@]}" workloads logs \
  "$A3S_CLOUD_BX0_WORKLOAD_ID" \
  "$A3S_CLOUD_BX0_REVISION_ID" \
  --cursor="$cursor" \
  --output=json 2>&1)
rc=$?
set -e
if ((rc != 0)); then
  printf '%s\n' "bx0 logs probe: Cloud workloads logs failed" "$out" >&2
  exit 1
fi
if grep -Fq 'PLACEHOLDER_' <<<"$out"; then
  printf '%s\n' "bx0 logs probe: response contains PLACEHOLDER_*" >&2
  exit 1
fi
printf '%s\n' "bx0 logs probe: STRICT Cloud verification ok"
exit 0
