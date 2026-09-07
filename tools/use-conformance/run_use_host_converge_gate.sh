#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 real-host Skill/UI converge gate.
# Proves CognitivePackageHostManager plan/apply/observe (including Skill + Ui
# surfaces) at the Cloud-pinned A3S Use revision. Cloud does not embed a3s-use
# while Flow/Runtime pins diverge (see development-plan U0.3).

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_use_host_converge_gate.sh EVIDENCE_DIRECTORY}
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
: >"$evidence_directory/use-host-converge.log"
cd -- "$use_checkout"

# Skill+Ui surfaces on the real Host Manager (six-surface package includes both).
# Scope lifecycle proves plan/apply/observe replay across User and Workspace.
host_filters=(
  graph_grants::host_six_surface_lifecycle::host_manager_replays_the_complete_six_surface_lifecycle
  graph_grants::host_scope_lifecycle::host_manager_replays_the_full_lifecycle_for_each_scope_kind
)

gate_status=0
for filter in "${host_filters[@]}"; do
  echo "===== $filter =====" | tee -a "$evidence_directory/use-host-converge.log"
  set +e
  cargo test --test remote_extension_cli "$filter" -- --exact --nocapture --test-threads=1 \
    2>&1 | tee -a "$evidence_directory/use-host-converge.log"
  status=${PIPESTATUS[0]}
  set -e
  if ((status != 0)); then
    gate_status=$status
  fi
done

if ((gate_status != 0)); then
  exit "$gate_status"
fi

passed=$(grep -cE '^test .+ \.\.\. ok$' "$evidence_directory/use-host-converge.log" || true)
expected=${#host_filters[@]}
if ((passed != expected)); then
  echo "expected $expected host converge tests to pass, found $passed" >&2
  exit 1
fi

certification="A3S_CLOUD_U0_3_USE_HOST_CERTIFIED revision=$revision surfaces=Skill,Ui checks=$passed/$expected"
printf '%s\n' "$certification" | tee "$evidence_directory/use-host-converge-certification.txt"
