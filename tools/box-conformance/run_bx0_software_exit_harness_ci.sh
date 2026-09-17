#!/usr/bin/env bash
# Fail-closed CI for run_bx0_software_exit_harness.sh.
# Never claims product A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED.

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
harness="$tools/run_bx0_software_exit_harness.sh"
[[ -f $harness ]]

evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-software-exit-ci.XXXXXX")
cleanup() {
  rm -rf -- "$evidence_directory"
}
trap cleanup EXIT

echo "===== bash -n software exit tools ====="
bash -n "$harness"
bash -n "$tools/probe_bx0_logs.sh"
bash -n "$tools/probe_bx0_digest.sh"
bash -n "$tools/probe_bx0_rollback.sh"
bash -n "$tools/run_bx0_software_loop_create.sh"
bash -n "$tools/run_bx0_software_exit_live.sh"
bash -n "$tools/run_bx0_software_exit_live_prep.sh"
bash -n "$tools/run_bx0_software_exit_live_tenant.sh"
bash -n "$tools/run_bx0_software_exit_live_oci.sh"
bash -n "$tools/run_bx0_software_exit_live_agent_release.sh"
bash -n "$tools/run_bx0_software_exit_live_https.sh"
bash -n "$tools/run_bx0_software_exit_live_chain.sh"
bash -n "$tools/run_bx0_software_exit_live_via_box.sh"
bash -n "$tools/bx0_refuse_docker_host.sh"
bash -n "$tools/install_gateway_pin.sh"

echo "===== LIVE binder refuses Docker sock (no sock override theater) ====="
live_bind_dir="$evidence_directory/live-binder"
mkdir -p -- "$live_bind_dir"
# Point at a present path so the binder fail-closes even on Docker-free hosts.
printf '' >"$live_bind_dir/fake.sock"
set +e
A3S_CLOUD_BX0_DOCKER_SOCK_PATH="$live_bind_dir/fake.sock" \
  bash "$tools/run_bx0_software_exit_live.sh" "$live_bind_dir/out" \
  >"$evidence_directory/live-binder.out" 2>"$evidence_directory/live-binder.err"
live_bind_rc=$?
set -e
if ((live_bind_rc != 2)); then
  printf '%s\n' "expected live binder docker refuse exit 2, got $live_bind_rc" >&2
  cat "$evidence_directory/live-binder.out" >&2 || true
  cat "$evidence_directory/live-binder.err" >&2 || true
  exit 1
fi
grep -Fq 'docker_sock_present' "$evidence_directory/live-binder.err"
if grep -E --quiet \
  '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
  "$evidence_directory/live-binder.out" \
  "$evidence_directory/live-binder.err" 2>/dev/null; then
  printf '%s\n' "live binder must never emit product EXIT_CERTIFIED" >&2
  exit 1
fi

echo "===== LIVE binder refuses override theater when canonical sock exists ====="
if [[ -e /var/run/docker.sock || -S /var/run/docker.sock ]]; then
  override_dir="$evidence_directory/live-override-theater"
  mkdir -p -- "$override_dir"
  set +e
  A3S_CLOUD_BX0_DOCKER_SOCK_PATH="$override_dir/missing.sock" \
    bash "$tools/run_bx0_software_exit_live.sh" "$override_dir/out" \
    >"$evidence_directory/live-override.out" 2>"$evidence_directory/live-override.err"
  override_rc=$?
  set -e
  if ((override_rc != 2)); then
    printf '%s\n' "expected override theater refuse exit 2, got $override_rc" >&2
    exit 1
  fi
  grep -Fq 'docker_sock_present' "$evidence_directory/live-override.err"
  grep -Fq '/var/run/docker.sock' "$evidence_directory/live-override.err"
fi

echo "===== LIVE prep refuses Docker sock (no Docker middleware theater) ====="
prep_dir="$evidence_directory/live-prep-refuse"
mkdir -p -- "$prep_dir"
printf '' >"$prep_dir/fake.sock"
set +e
A3S_CLOUD_BX0_DOCKER_SOCK_PATH="$prep_dir/fake.sock" \
  bash "$tools/run_bx0_software_exit_live_prep.sh" "$prep_dir/out" \
  >"$evidence_directory/live-prep.out" 2>"$evidence_directory/live-prep.err"
prep_rc=$?
set -e
if ((prep_rc != 2)); then
  printf '%s\n' "expected live prep docker refuse exit 2, got $prep_rc" >&2
  cat "$evidence_directory/live-prep.out" >&2 || true
  cat "$evidence_directory/live-prep.err" >&2 || true
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_PREP_BLOCKED' \
  "$prep_dir/out/bx0-software-exit-live-prep.txt"
grep -Fq 'docker_sock_present' \
  "$prep_dir/out/bx0-software-exit-live-prep.txt"
if grep -E --quiet \
  '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
  "$evidence_directory/live-prep.out" \
  "$evidence_directory/live-prep.err" \
  "$prep_dir/out/bx0-software-exit-live-prep.txt" 2>/dev/null; then
  printf '%s\n' "live prep must never emit product EXIT_CERTIFIED" >&2
  exit 1
fi

echo "===== LIVE tenant/oci/agent/https/chain refuse Docker sock ====="
for script_name in \
  run_bx0_software_exit_live_tenant.sh \
  run_bx0_software_exit_live_oci.sh \
  run_bx0_software_exit_live_agent_release.sh \
  run_bx0_software_exit_live_https.sh \
  run_bx0_software_exit_live_chain.sh; do
  refuse_dir="$evidence_directory/refuse-${script_name%.sh}"
  mkdir -p -- "$refuse_dir"
  printf '' >"$refuse_dir/fake.sock"
  set +e
  A3S_CLOUD_BX0_DOCKER_SOCK_PATH="$refuse_dir/fake.sock" \
    bash "$tools/$script_name" "$refuse_dir/out" \
    >"$refuse_dir/stdout" 2>"$refuse_dir/stderr"
  refuse_rc=$?
  set -e
  if ((refuse_rc != 2)); then
    printf '%s\n' "expected $script_name docker refuse exit 2, got $refuse_rc" >&2
    cat "$refuse_dir/stdout" >&2 || true
    cat "$refuse_dir/stderr" >&2 || true
    exit 1
  fi
  if grep -E --quiet \
    '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
    "$refuse_dir/stdout" "$refuse_dir/stderr" 2>/dev/null; then
    printf '%s\n' "$script_name must never emit product EXIT_CERTIFIED" >&2
    exit 1
  fi
done

echo "===== LOOP create refuse path (no prereqs) must BLOCK ====="
create_dir="$evidence_directory/loop-create-refuse"
set +e
env -u A3S_CLOUD_URL -u A3S_CLOUD_TOKEN \
  bash "$tools/run_bx0_software_loop_create.sh" "$create_dir" \
  >"$evidence_directory/create-refuse.out" 2>"$evidence_directory/create-refuse.err"
create_rc=$?
set -e
if ((create_rc != 2)); then
  printf '%s\n' "expected loop create refuse exit 2, got $create_rc" >&2
  cat "$evidence_directory/create-refuse.out" >&2 || true
  cat "$evidence_directory/create-refuse.err" >&2 || true
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_SOFTWARE_LOOP_CREATE_BLOCKED' \
  "$create_dir/bx0-software-loop-create.txt"
if grep -E --quiet \
  '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
  "$evidence_directory/create-refuse.out" \
  "$evidence_directory/create-refuse.err" \
  "$create_dir/bx0-software-loop-create.txt" 2>/dev/null; then
  printf '%s\n' "loop create must never emit product EXIT_CERTIFIED" >&2
  exit 1
fi

echo "===== LOOP create must reject URI/digest mismatch (no invent) ====="
mismatch_dir="$evidence_directory/loop-create-mismatch"
mismatch_acl="$mismatch_dir/node.acl"
mkdir -p -- "$mismatch_dir"
cat >"$mismatch_acl" <<'EOF'
control_plane {
  enrollment_url = "http://127.0.0.1:8080/api/v1/node-control/enroll"
  node_control_url = "https://localhost:8443"
  enrollment_token_env = "A3S_CLOUD_ENROLLMENT_TOKEN"
}
node {
  name = "worker-bx0-software"
  state_dir = "/tmp/a3s-bx0-create-mismatch"
}
EOF
# Provide enough fake prereqs to pass early checks until digest mismatch on Linux;
# on non-Linux the script still fail-closes before inventing EXIT.
set +e
A3S_CLOUD_URL='http://127.0.0.1:8080/api/v1' \
  A3S_CLOUD_TOKEN='test-token' \
  A3S_CLOUD_ORGANIZATION_ID='11111111-1111-1111-1111-111111111111' \
  A3S_CLOUD_PROJECT_ID='22222222-2222-2222-2222-222222222222' \
  A3S_CLOUD_ENVIRONMENT_ID='33333333-3333-3333-3333-333333333333' \
  A3S_CLOUD_BX0_NODE_CONFIG="$mismatch_acl" \
  A3S_CLOUD_BX0_AGENT_RELEASE_URL='https://example.com/a3s-cloud-node-agent' \
  A3S_CLOUD_BX0_AGENT_RELEASE_SHA256='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
  A3S_CLOUD_BX0_ARTIFACT_URI='oci://registry.example/app@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' \
  A3S_CLOUD_BX0_ARTIFACT_DIGEST='sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc' \
  A3S_CLOUD_BX0_DOCKER_SOCK_PATH="$mismatch_dir/no-docker.sock" \
  bash "$tools/run_bx0_software_loop_create.sh" "$mismatch_dir" \
  >"$evidence_directory/create-mismatch.out" 2>"$evidence_directory/create-mismatch.err"
mismatch_rc=$?
set -e
if ((mismatch_rc != 2)); then
  printf '%s\n' "expected digest mismatch / host refuse exit 2, got $mismatch_rc" >&2
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_SOFTWARE_LOOP_CREATE_BLOCKED' \
  "$mismatch_dir/bx0-software-loop-create.txt"
# On Linux x86_64 without docker sock this should be artifact_uri_digest_mismatch;
# elsewhere host_unsupported / docker_sock / etc. — never CREATE_OK or EXIT.
if grep -Fq 'A3S_CLOUD_BX0_SOFTWARE_LOOP_CREATE_OK' \
  "$mismatch_dir/bx0-software-loop-create.txt"; then
  printf '%s\n' "mismatch path must not emit CREATE_OK" >&2
  exit 1
fi

echo "===== harness CI mode (LIVE unset) must not claim product EXIT ====="
set +e
env -u A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE \
  bash "$harness" "$evidence_directory/ci" \
  >"$evidence_directory/ci.out" 2>"$evidence_directory/ci.err"
ci_rc=$?
set -e
if ((ci_rc != 0)); then
  printf '%s\n' "expected harness CI mode exit 0, got $ci_rc" >&2
  cat "$evidence_directory/ci.out" >&2 || true
  cat "$evidence_directory/ci.err" >&2 || true
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_SOFTWARE_EXIT_HARNESS_CI_CERTIFIED' \
  "$evidence_directory/ci/bx0-software-exit-harness.txt"
grep -Fq 'product_exit=not_claimed' \
  "$evidence_directory/ci/bx0-software-exit-harness.txt"
if grep -E --quiet \
  '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
  "$evidence_directory/ci.out" \
  "$evidence_directory/ci.err" \
  "$evidence_directory/ci/bx0-software-exit-harness.txt" 2>/dev/null; then
  printf '%s\n' "CI harness must never emit product EXIT_CERTIFIED claim" >&2
  exit 1
fi

echo "===== LIVE without identities must BLOCK (not invent EXIT) ====="
live_dir="$evidence_directory/live-incomplete"
set +e
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1 \
  env -u A3S_CLOUD_BX0_ENROLL_NODE_ID \
  bash "$harness" "$live_dir" \
  >"$evidence_directory/live.out" 2>"$evidence_directory/live.err"
live_rc=$?
set -e
if ((live_rc != 2)); then
  printf '%s\n' "expected LIVE incomplete to exit 2, got $live_rc" >&2
  cat "$evidence_directory/live.out" >&2 || true
  cat "$evidence_directory/live.err" >&2 || true
  exit 1
fi
grep -Fq 'A3S_CLOUD_BX0_SOFTWARE_EXIT_BLOCKED' \
  "$live_dir/bx0-software-exit-harness.txt"
if grep -E --quiet \
  '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
  "$evidence_directory/live.out" \
  "$evidence_directory/live.err" \
  "$live_dir/bx0-software-exit-harness.txt" 2>/dev/null; then
  printf '%s\n' "LIVE incomplete must never emit product EXIT_CERTIFIED" >&2
  exit 1
fi

echo "===== no invented Power pin ====="
if [[ -f $repository_root/tools/power-conformance/power-revision ]]; then
  printf '%s\n' "tools/power-conformance/power-revision must remain absent" >&2
  exit 1
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_SOFTWARE_EXIT_HARNESS_CI_CERTIFIED revision=$(git -C "$repository_root" rev-parse HEAD) fail_closed=1"
