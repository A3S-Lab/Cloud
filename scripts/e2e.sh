#!/usr/bin/env bash
set -euo pipefail

A3S_TEST_BIN="${A3S_TEST_BIN:-a3s-test}"
A3S_TEST_BROWSER="${A3S_TEST_BROWSER:-agent-browser}"
MANIFEST="${A3S_TEST_MANIFEST:-tests/e2e/workflow-studio.acl}"
COMMAND_TIMEOUT_MS="${A3S_TEST_COMMAND_TIMEOUT_MS:-30000}"
IDLE_TIMEOUT_MS="${A3S_TEST_IDLE_TIMEOUT_MS:-30000}"

"$A3S_TEST_BIN" capabilities \
  --browser-driver standalone \
  --browser-executable "$A3S_TEST_BROWSER" \
  --json

"$A3S_TEST_BIN" check "$MANIFEST" --json

"$A3S_TEST_BIN" run "$MANIFEST" \
  --browser-driver standalone \
  --browser-executable "$A3S_TEST_BROWSER" \
  --command-timeout-ms "$COMMAND_TIMEOUT_MS" \
  --idle-timeout-ms "$IDLE_TIMEOUT_MS" \
  --cleanup-timeout-ms 10000 \
  --infrastructure-retries 1 \
  --json
