#!/usr/bin/env bash
# Operator chain for GA-0 software EXIT on a Docker-free Linux host.
#
# Steps (each fail-closed; never invents digests/EXIT):
#   1) live_prep         — a3s-box compose + control-plane
#   2) live_tenant       — bootstrap org/project/environment
#   3) live_oci          — publish real digests to local registry (crane|skopeo|oras)
#   4) live_agent_release — build/serve node-agent over local CA HTTPS
#   5) live_https        — CA-aware TLS terminate + node.acl + HEALTH probe
#   6) live EXIT         — CREATE_FULL + EXIT harness (needs pin-matched gateway)
#
# Usage:
#   export A3S_CLOUD_GATEWAY_BIN=/abs/a3s-gateway   # pin-matched; GATEWAY-REVISION sidecar
#   bash tools/box-conformance/run_bx0_software_exit_live_chain.sh [EVIDENCE_DIR]

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-live-chain.XXXXXX")
fi
mkdir -p -- "$evidence_directory"

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
# shellcheck disable=SC1090
source "$tools/bx0_ensure_middleware_ports.sh"
bx0_refuse_docker_host || exit 2

# Keep middleware ports bound before prep/OCI — partial maps cause false OCI misses.
box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
compose_acl=${A3S_CLOUD_BX0_COMPOSE_ACL:-$repository_root/deploy/dev/compose.bx0-live.acl}
if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
  bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" || true
fi

bash "$tools/run_bx0_software_exit_live_prep.sh" "$evidence_directory/prep"
# shellcheck disable=SC1090
source "${A3S_CLOUD_BX0_LIVE_PREP_STATE_DIR:-$repository_root/.a3s/cloud/bx0-live-prep}/stack.env"

bash "$tools/run_bx0_software_exit_live_tenant.sh" "$evidence_directory/tenant"
# shellcheck disable=SC1090
source "$evidence_directory/tenant/bx0-live-tenant-env.sh"

bash "$tools/run_bx0_software_exit_live_oci.sh" "$evidence_directory/oci"
# shellcheck disable=SC1090
source "$evidence_directory/oci/bx0-live-oci-env.sh"

bash "$tools/run_bx0_software_exit_live_agent_release.sh" "$evidence_directory/agent"
# shellcheck disable=SC1090
source "$evidence_directory/agent/bx0-live-agent-release-env.sh"

bash "$tools/run_bx0_software_exit_live_https.sh" "$evidence_directory/https"
# shellcheck disable=SC1090
source "$evidence_directory/https/bx0-live-https-env.sh"

if [[ -z ${A3S_CLOUD_BX0_NODE_CONFIG:-} || -z ${A3S_CLOUD_BX0_AGENT_RELEASE_URL:-} || -z ${A3S_CLOUD_BX0_AGENT_RELEASE_SHA256:-} ]]; then
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-chain.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_CHAIN_BLOCKED reason=node_agent_release_incomplete
product_exit=not_claimed
EOF
  exit 2
fi

if [[ -z ${A3S_CLOUD_BX0_HTTPS_URL:-} ]]; then
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-chain.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_CHAIN_BLOCKED reason=https_incomplete
product_exit=not_claimed
EOF
  exit 2
fi

# Gateway pin binary required by clean-host HTTPS preflight (not stubbed).
gateway_bin=${A3S_CLOUD_GATEWAY_BIN:-$(command -v a3s-gateway || true)}
if [[ -z $gateway_bin || ! -x $gateway_bin ]]; then
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-chain.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_CHAIN_BLOCKED reason=gateway_unavailable
honesty=Build/install pin-matched a3s-gateway and export A3S_CLOUD_GATEWAY_BIN (+ GATEWAY-REVISION sidecar).
product_exit=not_claimed
EOF
  exit 2
fi
export A3S_CLOUD_GATEWAY_BIN=$gateway_bin

# Rebind middleware ports before EXIT — agent-release/https can outlive port_map.
if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
  bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" || true
fi

exec bash "$tools/run_bx0_software_exit_live.sh" "$evidence_directory/exit"
