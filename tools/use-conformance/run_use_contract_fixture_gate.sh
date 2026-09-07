#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 pinned A3S Use golden contract-fixture gate.
# Proves package identity, catalog/host protocol fixtures, plan, confirmation
# binding, enablement plan, and observation validate against the exact Cloud-
# pinned Use revision without embedding a3s-use in Cloud.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_use_contract_fixture_gate.sh EVIDENCE_DIRECTORY}
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
: >"$evidence_directory/use-contract-fixtures.log"
cd -- "$use_checkout"

fixture_filters=(
  "plugin_host_contracts:package_identity_is_typed_and_uses_one_validation_rule"
  "plugin_host_contracts:current_host_protocol_fixtures_are_canonical"
  "plugin_host_contracts:plan_contract_reuses_catalog_plan_and_host_policy_authority"
  "plugin_host_contracts:apply_binds_only_the_stored_plan_and_exact_confirmation"
  "plugin_host_contracts:host_enablement_plan_is_explicit_and_reuses_digest_only_apply"
  "plugin_host_contracts:observation_uses_the_use_owned_state_projection"
  "plugin_catalog_contracts:complete_package_catalog_fixture_is_canonical"
  "plugin_catalog_contracts:canonical_plugin_contract_fixtures_have_cross_sdk_digests"
)

gate_status=0
for entry in "${fixture_filters[@]}"; do
  test_bin=${entry%%:*}
  filter=${entry#*:}
  echo "===== $test_bin::$filter =====" | tee -a "$evidence_directory/use-contract-fixtures.log"
  set +e
  cargo test -p a3s-use-core --test "$test_bin" "$filter" -- --exact --nocapture \
    2>&1 | tee -a "$evidence_directory/use-contract-fixtures.log"
  rc=${PIPESTATUS[0]}
  set -e
  if ((rc != 0)); then
    gate_status=$rc
  fi
done

if ((gate_status != 0)); then
  exit "$gate_status"
fi

passed=$(grep -cE '^test .+ \.\.\. ok$' "$evidence_directory/use-contract-fixtures.log" || true)
expected=${#fixture_filters[@]}
if ((passed != expected)); then
  echo "expected $expected contract fixture tests to pass, found $passed" >&2
  exit 1
fi

certification="A3S_CLOUD_U0_3_USE_CONTRACT_FIXTURES_CERTIFIED revision=$revision checks=$passed/$expected"
printf '%s\n' "$certification" | tee "$evidence_directory/use-contract-fixtures-certification.txt"
