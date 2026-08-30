#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_provider_gate.sh EVIDENCE_DIRECTORY}

mkdir -p -- "$evidence_directory"
cd -- "$repository_root"

set +e
cargo test --locked -p a3s-cloud-control-plane \
  --test workload_identity_provider \
  real_tls_spiffe_https_web_provider_is_exact_bounded_and_drift_safe \
  -- --ignored --exact --nocapture --test-threads=1 \
  2>&1 | tee "$evidence_directory/provider.log"
gate_status=${PIPESTATUS[0]}
set -e

if (( gate_status != 0 )); then
  exit "$gate_status"
fi

grep --extended-regexp \
  '^A3S_CLOUD_WI1_PROVIDER_CERTIFIED profile=sha256:[0-9a-f]{64} bundle=sha256:[0-9a-f]{64} checks=7/7$' \
  "$evidence_directory/provider.log" \
  >"$evidence_directory/provider-certification.txt"
test "$(wc -l <"$evidence_directory/provider-certification.txt")" -eq 1
