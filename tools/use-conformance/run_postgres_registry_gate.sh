#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_postgres_registry_gate.sh EVIDENCE_DIRECTORY}
: "${A3S_CLOUD_TEST_POSTGRES_URL:?A3S_CLOUD_TEST_POSTGRES_URL is required}"

mkdir -p -- "$evidence_directory"
cd -- "$repository_root"

set +e
cargo test --locked -p a3s-cloud-control-plane \
  --test postgres_integration \
  postgres_plugin_registry_is_atomic_tenant_scoped_and_searchable \
  -- --exact --nocapture --test-threads=1 \
  2>&1 | tee "$evidence_directory/postgres-registry.log"
gate_status=${PIPESTATUS[0]}
set -e

if ((gate_status != 0)); then
  exit "$gate_status"
fi

grep --only-matching --extended-regexp \
  'A3S_CLOUD_U0_POSTGRES_CERTIFIED store=postgresql schema=084 search=085 registries=1 outbox=1 audit=1 idempotency=1 checks=12/12' \
  "$evidence_directory/postgres-registry.log" \
  >"$evidence_directory/postgres-registry-certification.txt"
test "$(wc -l <"$evidence_directory/postgres-registry-certification.txt")" -eq 1
