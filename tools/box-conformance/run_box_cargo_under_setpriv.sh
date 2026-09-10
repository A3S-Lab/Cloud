#!/usr/bin/env bash
# Compile Cloud Box-provider cargo tests as the runner, then execute the
# harness under the Linux Sandbox setpriv identity.
#
# setpriv with ruid != euid enables AT_SECURE, which strips LD_LIBRARY_PATH and
# breaks rustup's rustc. Match Box scripts/run-linux-sandbox-cargo-test.sh:
# compile outside setpriv, run only the test binary under Sandbox identity.
#
# Usage (same shape as cargo test):
#   bash run_box_cargo_under_setpriv.sh test --manifest-path apps/cloud/Cargo.toml \
#     --locked -p a3s-cloud-node-agent --test box_lifecycle FILTER \
#     -- --ignored --exact --nocapture --test-threads=1
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

if [[ "${1:-}" == "test" ]]; then
  shift
fi

cargo_args=()
harness_args=()
seeing_harness=0
for arg in "$@"; do
  if [[ "${seeing_harness}" -eq 0 && "${arg}" == "--" ]]; then
    seeing_harness=1
    continue
  fi
  if [[ "${seeing_harness}" -eq 0 ]]; then
    cargo_args+=("${arg}")
  else
    harness_args+=("${arg}")
  fi
done

if [[ "${#cargo_args[@]}" -eq 0 ]]; then
  echo "usage: $0 test <cargo test args...> [-- <harness args...>]" >&2
  exit 2
fi

# When callers place the cargo filter before "--", promote bare filter tokens
# into harness args so --exact still selects the intended ignored test.
filters=()
compact_cargo=()
expect_value=0
for arg in "${cargo_args[@]}"; do
  if [[ "${expect_value}" -eq 1 ]]; then
    compact_cargo+=("${arg}")
    expect_value=0
    continue
  fi
  case "${arg}" in
    --manifest-path|--locked|-p|--package|--test|--features|--target-dir)
      compact_cargo+=("${arg}")
      if [[ "${arg}" != "--locked" ]]; then
        expect_value=1
      fi
      ;;
    --lib|--doc|--bins|--examples|--tests|--all-targets|--all-features|--no-default-features)
      compact_cargo+=("${arg}")
      ;;
    -*)
      compact_cargo+=("${arg}")
      ;;
    *)
      filters+=("${arg}")
      ;;
  esac
done
cargo_args=("${compact_cargo[@]}")
if [[ "${#filters[@]}" -gt 0 ]]; then
  harness_args=("${filters[@]}" "${harness_args[@]}")
fi
if [[ "${#harness_args[@]}" -eq 0 ]]; then
  echo "harness args are required (test filter and/or flags after --)" >&2
  exit 2
fi

cargo_bin="$(command -v cargo)"
cargo_home="${CARGO_HOME:-$(dirname "$(dirname "${cargo_bin}")")}"
rustup_home="$(rustup show home)"
target_dir="${CARGO_TARGET_DIR:-${RUNNER_TEMP:-/tmp}/cloud-box-target}"
deps_dir="${target_dir}/debug/deps"
mkdir -p "${deps_dir}"

unset A3S_BOX_CI_SETPRIV_MATCHED_CREDS
unset A3S_BOX_CI_SETPRIV_WRAPPER

before="$(mktemp)"
after="$(mktemp)"
trap 'rm -f -- "${before}" "${after}"' EXIT
find "${deps_dir}" -maxdepth 1 -type f -executable ! -name '*.d' -printf '%p\n' 2>/dev/null \
  | sort >"${before}" || true

env \
  PATH="${PATH}" \
  HOME="${HOME}" \
  CARGO_HOME="${cargo_home}" \
  CARGO_TARGET_DIR="${target_dir}" \
  RUSTUP_HOME="${rustup_home}" \
  "${cargo_bin}" test --no-run "${cargo_args[@]}"

find "${deps_dir}" -maxdepth 1 -type f -executable ! -name '*.d' -printf '%p\n' \
  | sort >"${after}"

mapfile -t new_bins < <(comm -13 "${before}" "${after}")
if [[ "${#new_bins[@]}" -eq 0 ]]; then
  mapfile -t new_bins < <(
    find "${deps_dir}" -maxdepth 1 -type f -executable ! -name '*.d' -printf '%T@\t%p\n' \
      | sort -nr \
      | head -n 8 \
      | cut -f2-
  )
fi
if [[ "${#new_bins[@]}" -eq 0 ]]; then
  echo "no cargo test harness found under ${deps_dir}" >&2
  exit 1
fi

prefer=""
for ((i = 0; i < ${#cargo_args[@]}; i++)); do
  case "${cargo_args[$i]}" in
    --test)
      prefer="${cargo_args[$((i + 1))]-}"
      ;;
    -p|--package)
      pkg="${cargo_args[$((i + 1))]-}"
      prefer="${pkg//-/_}"
      ;;
  esac
done

test_bin=""
if [[ -n "${prefer}" ]]; then
  for candidate in "${new_bins[@]}"; do
    base="$(basename "${candidate}")"
    if [[ "${base}" == "${prefer}-"* ]]; then
      test_bin="${candidate}"
      break
    fi
  done
fi
if [[ -z "${test_bin}" ]]; then
  test_bin="${new_bins[0]}"
fi

echo "Running Sandbox-identity Cloud harness: ${test_bin}" >&2
exec bash "${sandbox_ci}" env \
  PATH="${PATH}" \
  HOME="${HOME}" \
  A3S_HOME="${A3S_HOME}" \
  A3S_BOX_OCI_AGENT_PATH="${A3S_BOX_OCI_AGENT_PATH:-}" \
  A3S_BOX_OCI_RUNTIME_PATH="${A3S_BOX_OCI_RUNTIME_PATH:-}" \
  A3S_BOX_SANDBOX_OCI_LAUNCHER="${A3S_BOX_SANDBOX_OCI_LAUNCHER:-}" \
  A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT="${A3S_BOX_SANDBOX_DELEGATED_CGROUP_ROOT}" \
  A3S_BOX_CI_SANDBOX_UID="${A3S_BOX_CI_SANDBOX_UID}" \
  A3S_BOX_CI_SANDBOX_GID="${A3S_BOX_CI_SANDBOX_GID}" \
  A3S_BOX_RUNTIME_CONFORMANCE_IMAGE="${A3S_BOX_RUNTIME_CONFORMANCE_IMAGE:-}" \
  A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE="${A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE:-}" \
  A3S_CLOUD_TEST_BOX="${A3S_CLOUD_TEST_BOX:-}" \
  A3S_REGISTRY_PROTOCOL="${A3S_REGISTRY_PROTOCOL:-}" \
  A3S_DEPS_STUB="${A3S_DEPS_STUB:-}" \
  RUST_MIN_STACK="${RUST_MIN_STACK:-33554432}" \
  "${test_bin}" "${harness_args[@]}"
