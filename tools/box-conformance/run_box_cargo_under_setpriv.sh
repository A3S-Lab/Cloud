#!/usr/bin/env bash
# Run Cloud Box-provider cargo under the Linux Sandbox setpriv harness.
#
# Full sudo (UID 0/0) fails OCI rootless device-policy bootstrap. Match Box
# composition: matched setpriv credentials for the harness, elevate only the
# owner child via A3S_BOX_CI_SETPRIV_WRAPPER.
set -euo pipefail

: "${A3S_BOX_CI_SANDBOX_UID:?A3S_BOX_CI_SANDBOX_UID is required}"
: "${A3S_BOX_CI_SANDBOX_GID:?A3S_BOX_CI_SANDBOX_GID is required}"
: "${A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT:?A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT is required}"
: "${A3S_HOME:?A3S_HOME is required}"

workspace_root="${GITHUB_WORKSPACE:-}"
if [[ -z "${workspace_root}" ]]; then
  workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fi

sandbox_ci="${workspace_root}/crates/box/scripts/run-linux-sandbox-ci.sh"
test -f "${sandbox_ci}"
test -d "${A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT}"

cargo_bin="$(command -v cargo)"
cargo_home="${CARGO_HOME:-$(dirname "$(dirname "${cargo_bin}")")}"
rustup_home="$(rustup show home)"
target_dir="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/cloud-box-target}"

export A3S_BOX_CI_SETPRIV_MATCHED_CREDS=1
export A3S_BOX_CI_SETPRIV_WRAPPER="${sandbox_ci}"

exec bash "${sandbox_ci}" env \
  PATH="${PATH}" \
  HOME="${HOME}" \
  CARGO_HOME="${cargo_home}" \
  CARGO_TARGET_DIR="${target_dir}" \
  RUSTUP_HOME="${rustup_home}" \
  A3S_HOME="${A3S_HOME}" \
  A3S_BOX_OCI_AGENT_PATH="${A3S_BOX_OCI_AGENT_PATH:-}" \
  A3S_BOX_OCI_RUNTIME_PATH="${A3S_BOX_OCI_RUNTIME_PATH:-}" \
  A3S_BOX_SANDBOX_OCI_LAUNCHER="${A3S_BOX_SANDBOX_OCI_LAUNCHER:-}" \
  A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT="${A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT}" \
  A3S_BOX_CI_SANDBOX_UID="${A3S_BOX_CI_SANDBOX_UID}" \
  A3S_BOX_CI_SANDBOX_GID="${A3S_BOX_CI_SANDBOX_GID}" \
  A3S_BOX_CI_SETPRIV_MATCHED_CREDS=1 \
  A3S_BOX_CI_SETPRIV_WRAPPER="${A3S_BOX_CI_SETPRIV_WRAPPER}" \
  A3S_BOX_RUNTIME_CONFORMANCE_IMAGE="${A3S_BOX_RUNTIME_CONFORMANCE_IMAGE:-}" \
  A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE="${A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE:-}" \
  A3S_CLOUD_TEST_BOX="${A3S_CLOUD_TEST_BOX:-}" \
  A3S_REGISTRY_PROTOCOL="${A3S_REGISTRY_PROTOCOL:-}" \
  A3S_DEPS_STUB="${A3S_DEPS_STUB:-}" \
  RUST_MIN_STACK="${RUST_MIN_STACK:-33554432}" \
  "${cargo_bin}" "$@"
