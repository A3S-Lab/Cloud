#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="$ROOT/.a3s-test/services"
mkdir -p "$LOG_DIR"

cd "$ROOT"
cargo build --workspace --all-targets --locked

NODE_RUNNER="$ROOT/target/debug/a3s-workflow-node"
if [[ -f "${NODE_RUNNER}.exe" ]]; then
  NODE_RUNNER="${NODE_RUNNER}.exe"
fi
if command -v cygpath >/dev/null 2>&1; then
  NODE_RUNNER_URI_PATH="$(cygpath -m "$NODE_RUNNER")"
  export A3S_NODE_ARTIFACT_URI="file:///$NODE_RUNNER_URI_PATH"
else
  export A3S_NODE_ARTIFACT_URI="file://$NODE_RUNNER"
fi
export A3S_NODE_ARTIFACT_DIGEST="sha256:$(sha256sum "$NODE_RUNNER" | cut -d' ' -f1)"
export A3S_WORKFLOW_DATABASE_URL="${A3S_WORKFLOW_DATABASE_URL:-postgres://workflow:workflow@127.0.0.1:5432/workflow}"

runtime_pid=""
api_pid=""
worker_pid=""
web_pid=""

cleanup() {
  for pid in "$web_pid" "$worker_pid" "$api_pid" "$runtime_pid"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT INT TERM

"$ROOT/target/debug/a3s-workflow-runtime-provider" >"$LOG_DIR/runtime.log" 2>&1 &
runtime_pid=$!
"$ROOT/target/debug/a3s-workflow-server" >"$LOG_DIR/api.log" 2>&1 &
api_pid=$!
"$ROOT/target/debug/worker" >"$LOG_DIR/worker.log" 2>&1 &
worker_pid=$!
(
  cd "$ROOT/web"
  bun run dev
) >"$LOG_DIR/web.log" 2>&1 &
web_pid=$!

wait_for_url() {
  local url=$1
  for _ in $(seq 1 120); do
    if curl --fail --silent --show-error "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done
  echo "Timed out waiting for $url" >&2
  return 1
}

wait_for_url "http://127.0.0.1:8090/health"
wait_for_url "http://127.0.0.1:8080/api/health"
wait_for_url "http://127.0.0.1:3000"

"$ROOT/scripts/e2e.sh"
