#!/usr/bin/env bash
set -euo pipefail

script_directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
hold_seconds=137

set +e
output="$(
  A3S_CLOUD_POSTGRES_URL='postgres://unused-for-supervision-test' \
  A3S_CLOUD_BOOTSTRAP_TOKEN='supervision-test-bootstrap-token' \
  A3S_CLOUD_GITHUB_WEBHOOK_SECRET='supervision-test-webhook-secret' \
  A3S_CLOUD_DEV_TEST_SKIP_PREPARE=true \
  A3S_CLOUD_DEV_API_BIN=/usr/bin/false \
  A3S_CLOUD_DEV_WEB_BIN=/bin/sleep \
  A3S_CLOUD_DEV_WEB_ARGUMENT="$hold_seconds" \
  "$script_directory/run_cloud.sh" dev 2>&1
)"
exit_code=$?
set -e

if [[ $exit_code -eq 0 ]]; then
  printf '%s\n' "$output" >&2
  printf '%s\n' 'supervision fixture unexpectedly succeeded' >&2
  exit 1
fi
if [[ $output != *'Cloud API exited with status 1'* ]]; then
  printf '%s\n' "$output" >&2
  printf '%s\n' 'launcher did not report the failed API process' >&2
  exit 1
fi
if pgrep -f "[/]bin/sleep $hold_seconds" >/dev/null 2>&1; then
  printf '%s\n' 'launcher left the web fixture running' >&2
  exit 1
fi

printf 'A3S_CLOUD_DEV_SUPERVISION_PASS\n'
