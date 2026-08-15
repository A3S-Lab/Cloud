#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_s3_namespace_gate.sh EVIDENCE_DIRECTORY}
[[ $evidence_directory == /* ]]

required=(
  A3S_CLOUD_TEST_S3_ENDPOINT
  A3S_CLOUD_TEST_S3_BUCKET
  A3S_CLOUD_TEST_S3_ACCESS_KEY_ID
  A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY
)
for name in "${required[@]}"; do
  [[ -n ${!name:-} ]]
done
[[ $A3S_CLOUD_TEST_S3_ENDPOINT == https://* ]]

mkdir -p -- "$evidence_directory"
cd -- "$repository_root"
git rev-parse HEAD >"$evidence_directory/cloud-revision.txt"

set +e
cargo test --locked -p a3s-cloud-control-plane \
  modules::data::infrastructure::shared_object_namespace::tests::real_s3_compatible_namespace_passes_destructive_cas_conformance \
  -- --ignored --exact --nocapture --test-threads=1 \
  2>&1 | tee "$evidence_directory/provider.log"
gate_status=${PIPESTATUS[0]}
set -e

EVIDENCE_LOG="$evidence_directory/provider.log" python3 - <<'PY'
import os
from pathlib import Path

log = Path(os.environ["EVIDENCE_LOG"]).read_bytes()
names = [
    "A3S_CLOUD_TEST_S3_ACCESS_KEY_ID",
    "A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY",
    "A3S_CLOUD_TEST_S3_SESSION_TOKEN",
]
for name in names:
    value = os.environ.get(name, "").encode()
    if value and value in log:
        raise SystemExit(f"retained S0 provider evidence contains {name}")
PY

if (( gate_status != 0 )); then
  exit "$gate_status"
fi

marker='A3S_CLOUD_S0_NAMESPACE_PROVIDER_CERTIFIED provider=s3-compatible protocol=a3s.s0.object-namespace.v1 checks=7/7 cleanup=verified'
grep --fixed-strings --line-regexp "$marker" \
  "$evidence_directory/provider.log" \
  >"$evidence_directory/provider-certification.txt"
test "$(wc -l <"$evidence_directory/provider-certification.txt")" -eq 1

(
  cd -- "$evidence_directory"
  sha256sum cloud-revision.txt provider.log provider-certification.txt \
    >evidence-sha256.txt
)
