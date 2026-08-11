#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_registry_gate.sh EVIDENCE_DIRECTORY}
revision=$(<"$repository_root/tools/use-conformance/use-revision")
expected_root=$(<"$repository_root/tools/use-conformance/plugin-v3-root.sha256")

[[ $revision =~ ^[0-9a-f]{40}$ ]]
[[ $expected_root =~ ^sha256:[0-9a-f]{64}$ ]]
[[ $(grep -Fc "rev = \"$revision\"" "$repository_root/Cargo.toml") -eq 2 ]]
grep -Fq "#$revision" "$repository_root/Cargo.lock"

mkdir -p -- "$evidence_directory"
cd -- "$repository_root"

set +e
cargo test --locked -p a3s-cloud-control-plane \
  --test plugin_registry_provider \
  real_public_https_use_registry_refreshes_and_replays_bounded_catalog_reads \
  -- --ignored --exact --nocapture --test-threads=1 \
  2>&1 | tee "$evidence_directory/provider.log"
gate_status=${PIPESTATUS[0]}
set -e

if (( gate_status != 0 )); then
  exit "$gate_status"
fi

grep --extended-regexp \
  '^A3S_CLOUD_U0_PROVIDER_CERTIFIED revision=[0-9a-f]{40} root=sha256:[0-9a-f]{64} timestamp_version=[1-9][0-9]* snapshot_version=[1-9][0-9]* targets_version=[1-9][0-9]* package=acme/research checks=9/9$' \
  "$evidence_directory/provider.log" \
  >"$evidence_directory/provider-certification.txt"
test "$(wc -l <"$evidence_directory/provider-certification.txt")" -eq 1
