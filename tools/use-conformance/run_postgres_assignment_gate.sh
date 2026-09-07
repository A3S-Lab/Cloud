#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_postgres_assignment_gate.sh EVIDENCE_DIRECTORY}
: "${A3S_CLOUD_TEST_POSTGRES_URL:?A3S_CLOUD_TEST_POSTGRES_URL is required}"

mkdir -p -- "$evidence_directory"
cd -- "$repository_root"

set +e
cargo test --locked -p a3s-cloud-control-plane \
  --test postgres_integration \
  postgres_plugin_assignments_are_atomic_tenant_scoped_and_confirmed \
  -- --exact --nocapture --test-threads=1 \
  2>&1 | tee "$evidence_directory/postgres-assignment.log"
gate_status=${PIPESTATUS[0]}
set -e

if ((gate_status != 0)); then
  exit "$gate_status"
fi

grep --only-matching --extended-regexp \
  'A3S_CLOUD_U0_3_POSTGRES_CERTIFIED store=postgresql schema=189,190,191 assignments=1 projections=1 confirmation=1 outbox=1 audit=1 idempotency=1 checks=12/12' \
  "$evidence_directory/postgres-assignment.log" \
  >"$evidence_directory/postgres-assignment-certification.txt"
test "$(wc -l <"$evidence_directory/postgres-assignment-certification.txt")" -eq 1
