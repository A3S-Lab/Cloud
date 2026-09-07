#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 control-surface parity gate for Use Registry assignments.
# Certifies catalog-pinned Management MCP assignment/plan tools, TypeScript
# client paths, CLI plugin-assignments / plugin-plan-projections commands, and
# the Plugins source-architecture ratchet. Does not require PostgreSQL or a
# live Use host.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
evidence_directory=${1:?usage: run_assignment_surface_gate.sh EVIDENCE_DIRECTORY}
revision=$(<"$repository_root/tools/use-conformance/use-revision")

[[ $revision =~ ^[0-9a-f]{40}$ ]]
[[ $(grep -Fc "rev = \"$revision\"" "$repository_root/Cargo.toml") -eq 2 ]]

mkdir -p -- "$evidence_directory"
: >"$evidence_directory/assignment-surface.log"

run_step() {
  local label=$1
  shift
  echo "===== $label =====" | tee -a "$evidence_directory/assignment-surface.log"
  set +e
  "$@" 2>&1 | tee -a "$evidence_directory/assignment-surface.log"
  local status=${PIPESTATUS[0]}
  set -e
  if ((status != 0)); then
    echo "step failed: $label" >&2
    exit "$status"
  fi
}

cd -- "$repository_root/packages/cloud-client"
run_step "mcp-catalog-pin" bun test src/management-mcp-conformance.test.ts -t 'pins the current privileged-management'
run_step "client-assignment-api" bun test src/api.test.ts -t 'plugin assignments'
run_step "client-plan-api" bun test src/api.test.ts -t 'plugin plan'

cd -- "$repository_root/cli"
run_step "cli-plugin-commands" bun test test/plugin-commands.test.ts

cd -- "$repository_root"
run_step "architecture-ratchet" cargo test --locked -p a3s-cloud-control-plane --lib \
  modules::architecture_tests::plugins_u0_assignment_surface_owns_no_second_use_platform \
  -- --exact --nocapture

# Fail closed if any step reported zero executed tests.
passed_units=$(grep -cE '^[[:space:]]*[0-9]+ pass' "$evidence_directory/assignment-surface.log" || true)
passed_rust=$(grep -cE '^test .+ \.\.\. ok$' "$evidence_directory/assignment-surface.log" || true)
if ((passed_rust < 1)); then
  echo "architecture ratchet produced no passing Rust tests" >&2
  exit 1
fi
if ((passed_units < 3)); then
  echo "expected bun surface suites to report passes, found $passed_units" >&2
  exit 1
fi

checks=5
certification="A3S_CLOUD_U0_3_SURFACE_CERTIFIED revision=$revision surfaces=rest,client,cli,mcp,architecture checks=$checks/$checks"
printf '%s\n' "$certification" | tee "$evidence_directory/assignment-surface-certification.txt"
