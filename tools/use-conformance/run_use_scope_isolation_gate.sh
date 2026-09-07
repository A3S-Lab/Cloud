#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 pinned Use scope/path fail-closed gate.
# Proves managed-scope isolation and package path/symlink escape rejection at
# the Cloud-pinned A3S Use revision without embedding a3s-use in Cloud.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_use_scope_isolation_gate.sh EVIDENCE_DIRECTORY}
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
: >"$evidence_directory/use-scope-isolation.log"
cd -- "$use_checkout"

gate_status=0

echo "===== scope-isolation =====" | tee -a "$evidence_directory/use-scope-isolation.log"
set +e
cargo test --test remote_extension_cli \
  graph_grants::host_scope_isolation::host_manager_binds_same_textual_id_to_exact_scope_kind \
  -- --exact --nocapture --test-threads=1 \
  2>&1 | tee -a "$evidence_directory/use-scope-isolation.log"
rc=${PIPESTATUS[0]}
set -e
if ((rc != 0)); then
  gate_status=$rc
fi

path_filters=(
  source::tests::zip_package_rejects_parent_traversal
  source::tests::tar_package_rejects_symbolic_links
)
for filter in "${path_filters[@]}"; do
  echo "===== path-escape::$filter =====" | tee -a "$evidence_directory/use-scope-isolation.log"
  set +e
  cargo test -p a3s-use-extension --lib "$filter" -- --exact --nocapture \
    2>&1 | tee -a "$evidence_directory/use-scope-isolation.log"
  rc=${PIPESTATUS[0]}
  set -e
  if ((rc != 0)); then
    gate_status=$rc
  fi
done

if ((gate_status != 0)); then
  exit "$gate_status"
fi

passed=$(grep -cE '^test .+ \.\.\. ok$' "$evidence_directory/use-scope-isolation.log" || true)
expected=3
if ((passed != expected)); then
  echo "expected $expected scope/path fail-closed tests to pass, found $passed" >&2
  exit 1
fi

certification="A3S_CLOUD_U0_3_USE_SCOPE_ISOLATION_CERTIFIED revision=$revision checks=$passed/$expected"
printf '%s\n' "$certification" | tee "$evidence_directory/use-scope-isolation-certification.txt"
