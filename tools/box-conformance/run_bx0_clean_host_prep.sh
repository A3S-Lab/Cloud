#!/usr/bin/env bash
# BX0.5 clean-host operator prep (never claims product EXIT or LOOP).
# Resolves pins/binaries where possible and prints the exact enroll recipe.
# Darwin may run this for recipe printing; Fleet long-poll enroll is Linux-only.

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
# shellcheck source=bx0_clean_host_steps.sh
# shellcheck disable=SC1090
source "$tools/bx0_clean_host_steps.sh"

CLOUD_ROOT="$repository_root"
export CLOUD_ROOT

box_revision=$(<"$tools/box-revision")
runtime_revision=$(<"$repository_root/tools/runtime-conformance/runtime-revision")
gateway_revision=$(<"$repository_root/tools/gateway-conformance/gateway-revision")
cloud_revision=$(git -C "$repository_root" rev-parse HEAD)

agent_bin=
agent_bin="$(bx0_resolve_node_agent 2>/dev/null || true)"
control_plane_bin=
control_plane_bin="$(bx0_resolve_control_plane 2>/dev/null || true)"
oci_bin=
oci_bin="$(bx0_resolve_oci_cli 2>/dev/null || true)"
gateway_bin=
gateway_bin="$(bx0_resolve_gateway 2>/dev/null || true)"

os_name="$(uname -s)"

cat <<EOF
A3S_CLOUD_BX0_CLEAN_HOST_PREP
cloud_revision=$cloud_revision
runtime_revision=$runtime_revision
box_revision=$box_revision
gateway_revision=$gateway_revision
power_revision=UNBOUND reason=pw0_no_pin_file
host_os=$os_name
node_agent=${agent_bin:-UNRESOLVED}
control_plane=${control_plane_bin:-UNRESOLVED}
a3s_oci=${oci_bin:-UNRESOLVED}
a3s_gateway=${gateway_bin:-UNRESOLVED}
product_exit=not_claimed
loop_certified=not_claimed

This prep does NOT claim A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED.
This prep does NOT claim A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED.

1) Install pinned Box (Linux worker):
  bash tools/box-conformance/install_box_release.sh

2) Start control-plane (API host; Postgres URLs required):
  export A3S_CLOUD_POSTGRES_MIGRATION_URL=...
  export A3S_CLOUD_POSTGRES_URL=...
  bash tools/dev/run_cloud.sh

3) Bootstrap enrollment credential + node ACL (API must be up):
  export A3S_CLOUD_URL=http://127.0.0.1:8080/api/v1
  export A3S_CLOUD_TOKEN=<api-token-with-node:write>
  # Copy config/node.example.acl to an absolute path ending in .acl
  printf %s "a3sn_<64-lowercase-hex>" | bun run cli/src/main.ts nodes bootstrap worker-1 \\
    --enrollment-token-stdin \\
    --expires-at=<RFC3339> \\
    --agent-release-url=https://<trusted-release>/a3s-cloud-node-agent \\
    --agent-release-sha256=<64-lowercase-hex> \\
    --node-config=/absolute/path/to/node.acl \\
    --idempotency-key=<caller-owned-key>

4) Linux ONLY — start a3s-cloud-node-agent with that absolute .acl (requires a3s-box):
  export A3S_CLOUD_ENROLLMENT_TOKEN=a3sn_<64-lowercase-hex>
  a3s-cloud-node-agent /absolute/path/to/node.acl
  # Capture the enrolled node_id from the agent/API response.

5) Arm gate preflight, then execute steps 1–9 with real receipts (still not EXIT):
  export A3S_CLOUD_BX0_CLEAN_HOST=1
  export A3S_CLOUD_BX0_EXECUTE=1
  export A3S_CLOUD_BX0_NODE_CONFIG=/absolute/path/to/node.acl
  export A3S_CLOUD_BX0_ENROLL_NODE_ID=<real-node-uuid>
  export A3S_CLOUD_BX0_ARTIFACT_DIGEST=sha256:<64-hex>
  export A3S_CLOUD_BX0_SERVICE_ID=<real-service-id>
  export A3S_CLOUD_BX0_HEALTH_URL=http://127.0.0.1:<port>/ready
  export A3S_CLOUD_BX0_HTTPS_URL=https://<managed-host>/
  export A3S_CLOUD_BX0_LOGS_CURSOR=<ordered-log-cursor>
  export A3S_CLOUD_BX0_UPDATE_DIGEST=sha256:<64-hex>
  export A3S_CLOUD_BX0_ROLLBACK_DIGEST=sha256:<64-hex>
  export A3S_CLOUD_BX0_CLEANUP_INSTANCE=<stopped-removed-instance-id>
  export A3S_CLOUD_ENROLLMENT_TOKEN=...
  bash tools/box-conformance/run_bx0_clean_host_gate.sh
  # expected with all receipts: exit 3 OPEN execute_receipts_complete=1 loop_exit=not_certified

6) After full enroll→…→cleanup, collect LOOP evidence; exit audit still needs Power (PW0):
  bash tools/box-conformance/collect_bx0_clean_host_evidence.sh \\
    --host HOST --service-id ID --node-id ID --artifact-digest DIGEST
  See tools/box-conformance/OPERATOR_CLEAN_HOST.md
EOF
