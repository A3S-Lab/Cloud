#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 Use-side crash points 6–7 gate.
# Runs pinned A3S Use recovery tests that prove:
#   6) candidate prep before capability publication
#   7) capability publication before old-generation drain
# Cloud does not embed CognitivePackageHostManager here; the evidence is the
# exact Use revision already locked by Cargo.toml / use-revision.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_use_recovery_gate.sh EVIDENCE_DIRECTORY}
revision=$(<"$repository_root/tools/use-conformance/use-revision")

[[ $revision =~ ^[0-9a-f]{40}$ ]]
[[ $(grep -Fc "rev = \"$revision\"" "$repository_root/Cargo.toml") -eq 2 ]]

use_checkout=${A3S_USE_CHECKOUT:-}
if [[ -z $use_checkout ]]; then
  candidate=$(cd -- "$repository_root/../.." && pwd)/crates/use
  if [[ -d $candidate/.git || -f $candidate/.git ]]; then
    use_checkout=$candidate
  fi
fi
: "${use_checkout:?set A3S_USE_CHECKOUT to an A3S Use checkout at the Cloud-pinned revision}"

use_head=$(git -C "$use_checkout" rev-parse HEAD)
if [[ $use_head != "$revision" ]]; then
  echo "Use checkout HEAD=$use_head does not match Cloud pin $revision" >&2
  exit 1
fi

mkdir -p -- "$evidence_directory"
: >"$evidence_directory/use-recovery.log"
cd -- "$use_checkout"

recovery_filters=(
  graph_mutation_recovery::interrupted_graph_durably_blocks_enablement_admission_until_recovery
  grant_process_recovery::host_enable::killed_host_protocol_enable_apply_replays_publication_and_grant_cutover
  grant_process_recovery::host_disable::killed_host_protocol_disable_apply_replays_hide_drain_and_grant_retirement
  recovery::schema_v3_uninstall_replays_cutover_drain_and_removal_after_real_process_kill
)

gate_status=0
for filter in "${recovery_filters[@]}"; do
  echo "===== $filter =====" | tee -a "$evidence_directory/use-recovery.log"
  set +e
  cargo test --test remote_extension_cli "$filter" -- --exact --nocapture --test-threads=1 \
    2>&1 | tee -a "$evidence_directory/use-recovery.log"
  status=${PIPESTATUS[0]}
  set -e
  if ((status != 0)); then
    gate_status=$status
  fi
done

if ((gate_status != 0)); then
  exit "$gate_status"
fi

passed=$(grep -cE '^test .+ \.\.\. ok$' "$evidence_directory/use-recovery.log" || true)
expected=${#recovery_filters[@]}
if ((passed != expected)); then
  echo "expected $expected recovery tests to pass, found $passed" >&2
  exit 1
fi

certification="A3S_CLOUD_U0_3_USE_RECOVERY_CERTIFIED revision=$revision crash_points=6,7 checks=$passed/$expected"
printf '%s\n' "$certification" | tee "$evidence_directory/use-recovery-certification.txt"
