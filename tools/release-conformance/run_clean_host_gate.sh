#!/usr/bin/env bash

set -Eeuo pipefail
umask 077
export PYTHONDONTWRITEBYTECODE=1

readonly POSTGRES_IMAGE="postgres@sha256:742f40ea20b9ff2ff31db5458d127452988a2164df9e17441e191f3b72252193"
readonly REGISTRY_IMAGE="registry@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373"
readonly WORKLOAD_DIGEST="sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662"
readonly WORKLOAD_IMAGE="docker.io/library/busybox@${WORKLOAD_DIGEST}"
readonly GATEWAY_VERSION="1.0.12"
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
readonly RUNTIME_REVISION_FILE="$script_dir/../runtime-conformance/runtime-revision"

source_root=""
cloud_sha=""
runtime_sha=""
evidence=""
target_dir=""
cargo_bin=""
gateway_bin=""
api_port=18080
node_control_port=18443
gateway_port=19443
gateway_management_port=19090
ttl_seconds=1800

usage() {
    cat <<'USAGE'
Run the process-level A3S Cloud E0 release gate on a clean Linux host.

Usage:
  run_clean_host_gate.sh \
    --source-root PATH \
    --cloud-sha FULL_SHA \
    --gateway PATH \
    [--runtime-sha FULL_SHA] \
    [--evidence-dir PATH] \
    [--target-dir PATH] \
    [--cargo PATH] \
    [--api-port PORT] \
    [--node-control-port PORT] \
    [--gateway-port PORT] \
    [--gateway-management-port PORT] \
    [--ttl-seconds SECONDS]

The source root must contain clean apps/cloud and crates/runtime Git worktrees at
the exact supplied revisions. The runner builds release binaries from scratch,
starts pinned PostgreSQL and registry fixtures, boots A3S Gateway 1.0.12, the
control plane, and one outbound node agent, then drives the public E0 API loop.

The gate requires a dedicated Linux host with Docker and network access. It
does not alter the Docker daemon. Cleanup targets only run-labeled fixtures and
the run-specific A3S Cloud Docker namespace.
USAGE
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

require_value() {
    [[ $# -ge 2 ]] || die "$1 requires a value"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --source-root)
            require_value "$@"
            source_root=$2
            shift 2
            ;;
        --cloud-sha)
            require_value "$@"
            cloud_sha=$2
            shift 2
            ;;
        --runtime-sha)
            require_value "$@"
            runtime_sha=$2
            shift 2
            ;;
        --evidence-dir)
            require_value "$@"
            evidence=$2
            shift 2
            ;;
        --target-dir)
            require_value "$@"
            target_dir=$2
            shift 2
            ;;
        --cargo)
            require_value "$@"
            cargo_bin=$2
            shift 2
            ;;
        --gateway)
            require_value "$@"
            gateway_bin=$2
            shift 2
            ;;
        --api-port)
            require_value "$@"
            api_port=$2
            shift 2
            ;;
        --node-control-port)
            require_value "$@"
            node_control_port=$2
            shift 2
            ;;
        --gateway-port)
            require_value "$@"
            gateway_port=$2
            shift 2
            ;;
        --gateway-management-port)
            require_value "$@"
            gateway_management_port=$2
            shift 2
            ;;
        --ttl-seconds)
            require_value "$@"
            ttl_seconds=$2
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            die "unknown argument: $1"
            ;;
    esac
done

[[ $(uname -s) == Linux ]] || die "the clean-host release gate requires Linux"
[[ $cloud_sha =~ ^[0-9a-f]{40}$ ]] || die "--cloud-sha must be a full lowercase Git SHA"
[[ -f $RUNTIME_REVISION_FILE ]] || die "Runtime revision file is missing"
IFS= read -r pinned_runtime_sha <"$RUNTIME_REVISION_FILE" ||
    die "Runtime revision file is unreadable"
[[ $pinned_runtime_sha =~ ^[0-9a-f]{40}$ ]] ||
    die "Runtime revision file must contain one full lowercase Git SHA"
[[ -n $runtime_sha ]] || runtime_sha=$pinned_runtime_sha
[[ $runtime_sha == "$pinned_runtime_sha" ]] ||
    die "--runtime-sha must match the repository-pinned Runtime revision"
[[ $ttl_seconds =~ ^[0-9]+$ ]] || die "--ttl-seconds must be an integer"
((ttl_seconds >= 600 && ttl_seconds <= 3600)) ||
    die "--ttl-seconds must be between 600 and 3600"

ports=("$api_port" "$node_control_port" "$gateway_port" "$gateway_management_port")
for port in "${ports[@]}"; do
    [[ $port =~ ^[0-9]+$ ]] && ((port >= 1024 && port <= 65535)) ||
        die "release-gate ports must be integers between 1024 and 65535"
done
[[ $(printf '%s\n' "${ports[@]}" | sort -u | wc -l) -eq ${#ports[@]} ]] ||
    die "release-gate ports must be distinct"

for command in awk comm curl docker git grep python3 realpath sha256sum sort timeout; do
    command -v "$command" >/dev/null || die "required command is unavailable: $command"
done
[[ -n $source_root ]] || die "--source-root is required"
source_root=$(realpath "$source_root")
cloud=$source_root/apps/cloud
runtime=$source_root/crates/runtime
[[ -d $cloud ]] || die "Cloud worktree is missing: $cloud"
[[ -d $runtime ]] || die "Runtime worktree is missing: $runtime"
[[ $(git -C "$cloud" rev-parse HEAD) == "$cloud_sha" ]] ||
    die "Cloud worktree is not at $cloud_sha"
[[ $(git -C "$runtime" rev-parse HEAD) == "$runtime_sha" ]] ||
    die "Runtime worktree is not at $runtime_sha"
[[ -z $(git -C "$cloud" status --porcelain=v1) ]] || die "Cloud worktree is dirty"
[[ -z $(git -C "$runtime" status --porcelain=v1) ]] || die "Runtime worktree is dirty"

if [[ -z $cargo_bin ]]; then
    cargo_bin=$(command -v cargo || true)
elif [[ $cargo_bin != */* ]]; then
    cargo_bin=$(command -v "$cargo_bin" || true)
fi
[[ -x $cargo_bin ]] || die "Cargo executable is unavailable: $cargo_bin"
if [[ $gateway_bin != */* ]]; then
    gateway_bin=$(command -v "$gateway_bin" || true)
fi
[[ -x $gateway_bin ]] || die "A3S Gateway executable is unavailable: $gateway_bin"
docker info >/dev/null 2>&1 || die "the host Docker daemon is unavailable"

for port in "${ports[@]}"; do
    if timeout 1 bash -c "exec 3<>/dev/tcp/127.0.0.1/$port" 2>/dev/null; then
        die "release-gate port is already in use: $port"
    fi
done

stamp=$(date -u +%Y%m%dT%H%M%SZ)
suffix=$(printf '%s' "$cloud_sha-$runtime_sha-$stamp-$$-$RANDOM" | sha256sum | cut -c1-12)
run_id="e0-${cloud_sha:0:7}-${runtime_sha:0:7}-${stamp}"
namespace="e0-release-$suffix"
fixture_label="a3s.cloud.release-gate.run-id=$run_id"
postgres="a3s-cloud-e0-postgres-$suffix"
registry="a3s-cloud-e0-registry-$suffix"
run_root="/tmp/a3s-cloud-e0-$suffix"
[[ -n $evidence ]] || evidence="/tmp/a3s-cloud-clean-host-e0-$run_id"
[[ -n $target_dir ]] || target_dir="$run_root/target"
evidence=$(realpath -m "$evidence")
target_dir=$(realpath -m "$target_dir")
config_dir="$run_root/config"
state_dir="$run_root/state"
gateway_certificates="$state_dir/gateway-certificates"
secret_memory_root="/dev/shm/a3s-cloud-e0-$suffix"
context_file="$run_root/release-context.json"

mkdir -p "$evidence" "$config_dir" "$state_dir" "$target_dir"
exec > >(tee -a "$evidence/runner.log") 2>&1

log() {
    printf 'A3S_CLEAN_HOST_GATE %s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"
}

record_inventory() {
    local phase=$1
    docker ps -aq | sort >"$evidence/host-containers-$phase.ids"
    docker volume ls -q | sort >"$evidence/host-volumes-$phase.names"
    docker network ls -q | sort >"$evidence/host-networks-$phase.ids"
}

write_deltas() {
    local prefix=$1 before=$2 after=$3 suffix_name=$4
    comm -13 "$before" "$after" >"$evidence/$prefix-added.$suffix_name"
    comm -23 "$before" "$after" >"$evidence/$prefix-removed.$suffix_name"
}

control_pid=""
node_pid=""
gateway_pid=""
ttl_pid=""
cleanup_failed=0
gate_completed=0
A3S_CLOUD_BOOTSTRAP_TOKEN=""
A3S_CLOUD_ADMIN_TOKEN=""
A3S_CLOUD_ENROLLMENT_TOKEN=""
A3S_CLOUD_GITHUB_WEBHOOK_SECRET=""
A3S_GATEWAY_ADMIN_TOKEN=""

terminate_process() {
    local name=$1 pid=$2
    [[ -n $pid ]] || return 0
    if kill -0 "$pid" >/dev/null 2>&1; then
        kill -TERM "$pid" >/dev/null 2>&1 || true
        for _ in $(seq 1 100); do
            kill -0 "$pid" >/dev/null 2>&1 || break
            sleep 0.1
        done
    fi
    if kill -0 "$pid" >/dev/null 2>&1; then
        kill -KILL "$pid" >/dev/null 2>&1 || true
    fi
    wait "$pid" >/dev/null 2>&1 || true
    kill -0 "$pid" >/dev/null 2>&1 && {
        log "cleanup-error=process-remains name=$name pid=$pid"
        cleanup_failed=1
    }
}

cleanup_resources() {
    set +e
    {
        log "phase=cleanup-start"
        terminate_process node "$node_pid"
        terminate_process control-plane "$control_pid"
        terminate_process gateway "$gateway_pid"

        if docker container inspect "$postgres" >/dev/null 2>&1; then
            docker logs "$postgres" >"$evidence/postgres.log" 2>&1
            docker inspect "$postgres" >"$evidence/postgres-inspect-final.json" 2>&1
        fi
        if docker container inspect "$registry" >/dev/null 2>&1; then
            docker logs "$registry" >"$evidence/registry.log" 2>&1
            docker inspect "$registry" >"$evidence/registry-inspect-final.json" 2>&1
        fi
        managed=$(docker ps -aq --filter "label=a3s.cloud.namespace=$namespace")
        if [[ -n $managed ]]; then
            docker inspect $managed >"$evidence/workload-containers-final.json" 2>&1
            timeout --signal=TERM --kill-after=10s 90s docker rm -fv $managed
        else
            printf '[]\n' >"$evidence/workload-containers-final.json"
        fi
        fixtures=$(docker ps -aq --filter "label=$fixture_label")
        [[ -z $fixtures ]] ||
            timeout --signal=TERM --kill-after=10s 90s docker rm -fv $fixtures
        docker rm -fv "$postgres" "$registry" >/dev/null 2>&1 || true

        rm -rf "$secret_memory_root"
        rm -rf "$run_root"
        [[ ! -e $run_root ]] || {
            log "cleanup-error=run-root-remains"
            cleanup_failed=1
        }
        [[ ! -e $secret_memory_root ]] || {
            log "cleanup-error=secret-directory-remains"
            cleanup_failed=1
        }
        docker ps -aq --filter "label=$fixture_label" |
            sort >"$evidence/target-fixtures-after.ids"
        docker ps -aq --filter "label=a3s.cloud.namespace=$namespace" |
            sort >"$evidence/target-workloads-after.ids"
        [[ ! -s $evidence/target-fixtures-after.ids ]] || cleanup_failed=1
        [[ ! -s $evidence/target-workloads-after.ids ]] || cleanup_failed=1

        record_inventory after
        write_deltas host-containers \
            "$evidence/host-containers-before.ids" \
            "$evidence/host-containers-after.ids" ids
        write_deltas host-volumes \
            "$evidence/host-volumes-before.names" \
            "$evidence/host-volumes-after.names" names
        write_deltas host-networks \
            "$evidence/host-networks-before.ids" \
            "$evidence/host-networks-after.ids" ids
        for delta in \
            "$evidence/host-containers-added.ids" \
            "$evidence/host-containers-removed.ids" \
            "$evidence/host-volumes-added.names" \
            "$evidence/host-volumes-removed.names" \
            "$evidence/host-networks-added.ids" \
            "$evidence/host-networks-removed.ids"; do
            [[ ! -s $delta ]] || {
                log "cleanup-error=host-inventory-drift file=$delta"
                cleanup_failed=1
            }
        done
        log "phase=cleanup-finish cleanup_failed=$cleanup_failed"
    } >>"$evidence/cleanup.log" 2>&1
    set -e
}

scan_sensitive_evidence() {
    local secret
    : >"$evidence/sensitive-scan.txt"
    for secret in \
        "$A3S_CLOUD_BOOTSTRAP_TOKEN" \
        "$A3S_CLOUD_ADMIN_TOKEN" \
        "$A3S_CLOUD_ENROLLMENT_TOKEN" \
        "$A3S_CLOUD_GITHUB_WEBHOOK_SECRET" \
        "$A3S_GATEWAY_ADMIN_TOKEN"; do
        [[ -n $secret ]] || continue
        if grep --recursive --files-with-matches --fixed-strings \
            --binary-files=without-match -- "$secret" "$evidence" \
            >>"$evidence/sensitive-scan.txt"; then
            cleanup_failed=1
        fi
    done
    [[ ! -s $evidence/sensitive-scan.txt ]] || log "cleanup-error=sensitive-evidence"
}

on_exit() {
    local exit_status=$?
    trap - EXIT INT TERM HUP
    if [[ -n $ttl_pid ]]; then
        kill "$ttl_pid" >/dev/null 2>&1 || true
        wait "$ttl_pid" >/dev/null 2>&1 || true
    fi
    cleanup_resources
    scan_sensitive_evidence
    [[ $cleanup_failed -eq 0 ]] || exit_status=1
    if [[ $exit_status -eq 0 && $gate_completed -eq 1 ]]; then
        printf 'A3S_CLOUD_CLEAN_HOST_E0_PASS cloud=%s runtime=%s run_id=%s\n' \
            "$cloud_sha" "$runtime_sha" "$run_id" | tee "$evidence/result.txt"
    else
        rm -f "$evidence/result.txt"
    fi
    printf '%s\n' "$exit_status" >"$evidence/exit-status.txt"
    log "result=exit status=$exit_status evidence=$evidence"
    exit "$exit_status"
}
trap on_exit EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

A3S_CLOUD_BOOTSTRAP_TOKEN=$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')
A3S_CLOUD_ADMIN_TOKEN="a3s_$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
A3S_CLOUD_ENROLLMENT_TOKEN="a3sn_$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
A3S_CLOUD_GITHUB_WEBHOOK_SECRET=$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')
A3S_GATEWAY_ADMIN_TOKEN=$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')
export A3S_CLOUD_BOOTSTRAP_TOKEN A3S_CLOUD_ADMIN_TOKEN
export A3S_CLOUD_ENROLLMENT_TOKEN A3S_CLOUD_GITHUB_WEBHOOK_SECRET
export A3S_GATEWAY_ADMIN_TOKEN

printf '%s\n' "$cloud_sha" >"$evidence/cloud.sha"
printf '%s\n' "$runtime_sha" >"$evidence/runtime.sha"
printf '%s\n' "$POSTGRES_IMAGE" >"$evidence/postgres-image.txt"
printf '%s\n' "$REGISTRY_IMAGE" >"$evidence/registry-image.txt"
printf '%s\n' "$WORKLOAD_IMAGE" >"$evidence/source-workload-image.txt"
printf '%s\n' "$namespace" >"$evidence/docker-namespace.txt"
git -C "$cloud" status --porcelain=v1 >"$evidence/cloud-dirty-before.txt"
git -C "$runtime" status --porcelain=v1 >"$evidence/runtime-dirty-before.txt"
record_inventory before

(
    sleep "$ttl_seconds"
    kill -TERM "$$" >/dev/null 2>&1 || true
) >"$evidence/ttl.log" 2>&1 &
ttl_pid=$!
printf '%s\n' "$ttl_pid" >"$evidence/ttl.pid"

log "phase=build-start"
timeout --signal=TERM --kill-after=30s 1200s \
    env CARGO_TARGET_DIR="$target_dir" \
    "$cargo_bin" build --manifest-path "$cloud/Cargo.toml" --release --locked \
        -p a3s-cloud-control-plane -p a3s-cloud-node-agent \
    2>&1 | tee "$evidence/cargo-release-build.log"
control_bin="$target_dir/release/a3s-cloud-control-plane"
node_bin="$target_dir/release/a3s-cloud-node-agent"
[[ -x $control_bin && -x $node_bin ]] || die "release binaries were not produced"
sha256sum "$control_bin" "$node_bin" >"$evidence/release-binaries.sha256"
log "phase=build-pass"

pull_image() {
    local image=$1 log_file=$2 status=1
    : >"$log_file"
    for attempt in 1 2 3; do
        set +e
        timeout --signal=TERM --kill-after=10s 300s docker pull "$image" \
            2>&1 | tee -a "$log_file"
        status=${PIPESTATUS[0]}
        set -e
        [[ $status -ne 0 ]] || return 0
        sleep $((attempt * 2))
    done
    return "$status"
}

log "phase=fixtures-start"
pull_image "$POSTGRES_IMAGE" "$evidence/postgres-pull.log" ||
    die "could not pull pinned PostgreSQL"
pull_image "$REGISTRY_IMAGE" "$evidence/registry-pull.log" ||
    die "could not pull pinned registry"
pull_image "$WORKLOAD_IMAGE" "$evidence/workload-pull.log" ||
    die "could not pull pinned workload image"

docker run --detach --name "$postgres" --pull=never \
    --label "$fixture_label" \
    --publish 127.0.0.1::5432 \
    --tmpfs /var/lib/postgresql/data:rw,nosuid,nodev,noexec,size=1073741824 \
    --env POSTGRES_DB=a3s_cloud \
    --env POSTGRES_USER=a3s_cloud \
    --env POSTGRES_PASSWORD=a3s_cloud \
    "$POSTGRES_IMAGE" >"$evidence/postgres.id"
docker run --detach --name "$registry" --pull=never \
    --label "$fixture_label" \
    --publish 127.0.0.1::5000 \
    --tmpfs /var/lib/registry:rw,nosuid,nodev,noexec,size=268435456 \
    "$REGISTRY_IMAGE" >"$evidence/registry.id"
postgres_port=$(docker inspect --format \
    '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}' "$postgres")
registry_port=$(docker inspect --format \
    '{{(index (index .NetworkSettings.Ports "5000/tcp") 0).HostPort}}' "$registry")
for attempt in $(seq 1 120); do
    docker exec "$postgres" pg_isready --dbname=a3s_cloud --username=a3s_cloud \
        >/dev/null 2>&1 && break
    [[ $attempt -ne 120 ]] || die "PostgreSQL readiness timed out"
    sleep 0.5
done
for attempt in $(seq 1 120); do
    curl --fail --silent --show-error --max-time 2 \
        "http://127.0.0.1:$registry_port/v2/" >/dev/null && break
    [[ $attempt -ne 120 ]] || die "registry readiness timed out"
    sleep 0.5
done
[[ -z $(docker inspect --format \
    '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' \
    "$postgres" "$registry") ]] || die "release fixtures own anonymous volumes"

local_repository="127.0.0.1:$registry_port/a3s/release-busybox"
docker tag "$WORKLOAD_IMAGE" "$local_repository:acceptance"
docker push "$local_repository:acceptance" 2>&1 | tee "$evidence/workload-push.log"
private_reference=""
while IFS= read -r reference; do
    case "$reference" in
        "$local_repository"@sha256:*)
            private_reference=$reference
            break
            ;;
    esac
done < <(docker image inspect \
    --format '{{range .RepoDigests}}{{println .}}{{end}}' \
    "$local_repository:acceptance")
[[ -n $private_reference ]] || die "local workload push omitted a repository digest"
artifact_digest=${private_reference##*@}
[[ $artifact_digest =~ ^sha256:[0-9a-f]{64}$ ]] ||
    die "local workload digest is invalid"
artifact_uri="oci://$private_reference"
printf '%s\n' "$artifact_uri" >"$evidence/workload-artifact-uri.txt"
local_image_id=$(docker image inspect --format '{{.Id}}' "$local_repository:acceptance")
docker image rm --force "$local_image_id" >"$evidence/workload-local-image-removal.txt"
! docker image inspect "$private_reference" >/dev/null 2>&1 ||
    die "release workload remained cached before node deployment"
log "phase=fixtures-pass postgres_port=$postgres_port registry_port=$registry_port"

cat >"$config_dir/gateway.acl" <<ACL
management {
  enabled = true
  address = "127.0.0.1:$gateway_management_port"
  path_prefix = "/api/gateway"
  auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  allowed_ips = ["127.0.0.1"]
}
ACL

"$gateway_bin" --config "$config_dir/gateway.acl" \
    >"$evidence/gateway.log" 2>&1 &
gateway_pid=$!
for attempt in $(seq 1 120); do
    kill -0 "$gateway_pid" >/dev/null 2>&1 ||
        die "A3S Gateway exited before readiness"
    curl --fail --silent --show-error --max-time 2 \
        --header "authorization: Bearer $A3S_GATEWAY_ADMIN_TOKEN" \
        "http://127.0.0.1:$gateway_management_port/api/gateway/version" \
        >"$evidence/gateway-ready.json" 2>/dev/null && break
    [[ $attempt -ne 120 ]] || die "A3S Gateway readiness timed out"
    sleep 0.25
done
gateway_version=$(
    python3 - "$evidence/gateway-ready.json" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
version = payload.get("version")
if not isinstance(version, str):
    raise SystemExit("A3S Gateway version response omitted a string version")
print(version)
PY
)
[[ $gateway_version == "$GATEWAY_VERSION" ]] ||
    die "A3S Gateway $GATEWAY_VERSION is required, got $gateway_version"
printf '%s\n' "$gateway_version" >"$evidence/gateway-version.txt"

cat >"$config_dir/cloud.acl" <<ACL
server {
  host = "127.0.0.1"
  port = $api_port
  role = "all"
}

node_control {
  host = "127.0.0.1"
  port = $node_control_port
  server_name = "localhost"
  certificate_file = "$state_dir/node-control/server.pem"
  private_key_file = "$state_dir/node-control/server-key.pem"
  client_ca_file = "$state_dir/node-ca/ca.pem"
  max_request_bytes = 20971520
  tls_handshake_timeout_ms = 5000
  request_body_timeout_ms = 10000
}

artifacts {
  store_dir = "$state_dir/artifacts"
  max_blob_bytes = 1073741824
  transfer_timeout_ms = 900000
}

postgres {
  url_env = "A3S_CLOUD_POSTGRES_URL"
  max_connections = 16
}

auth {
  bootstrap_token_env = "A3S_CLOUD_BOOTSTRAP_TOKEN"
}

events {
  provider = "memory"
  nats_url_env = "A3S_CLOUD_NATS_URL"
  stream_name = "A3S_CLOUD_EVENTS"
  batch_size = 100
  poll_interval_ms = 100
  lease_ms = 10000
  publish_timeout_ms = 3000
  retry_initial_ms = 100
  retry_max_ms = 5000
}

operations {
  reconcile_interval_ms = 250
  lease_ms = 10000
}

deployments {
  reconcile_interval_ms = 250
  command_ttl_ms = 180000
  runtime_apply_timeout_ms = 120000
  observation_poll_ms = 250
  convergence_timeout_ms = 300000
  runtime_stop_timeout_ms = 60000
  cleanup_poll_ms = 250
  cleanup_timeout_ms = 120000
}

builds {
  reconcile_interval_ms = 250
  builder_uri = "oci://docker.io/moby/buildkit@sha256:0eeb84626c0cd01aecae7848c5ed8f095aec279dd936d0cdb5a64110f42ca65b"
  builder_digest = "sha256:0eeb84626c0cd01aecae7848c5ed8f095aec279dd936d0cdb5a64110f42ca65b"
  builder_media_type = "application/vnd.oci.image.index.v1+json"
  buildkit_socket_volume_id = "a3s-cloud-buildkit-v0-31-2"
  input_staging_dir = "$state_dir/build-input-staging"
  input_max_entries = 100000
  input_max_bytes = 536870912
  output_staging_dir = "$state_dir/build-output-staging"
  output_max_entries = 100000
  output_max_expanded_bytes = 1073741824
  oci_max_blobs = 10000
  oci_max_bytes = 1073741824
  command_ttl_ms = 900000
  runtime_execution_timeout_ms = 600000
  observation_poll_ms = 250
  convergence_timeout_ms = 1800000
  cleanup_timeout_ms = 300000
  cpu_millis = 2000
  memory_bytes = 1073741824
  pids = 512
  output_max_bytes = 536870912
}

registry {
  request_timeout_ms = 10000
  insecure_hosts = ["127.0.0.1:$registry_port"]
  publication_registry = "127.0.0.1:$registry_port"
  publication_repository_prefix = "a3s-cloud/builds"
  publication_credential_env = ""
  publication_allow_anonymous = true
  publication_timeout_ms = 600000
}

sources {
  github_request_timeout_ms = 10000
  github_webhook_secret_env = "A3S_CLOUD_GITHUB_WEBHOOK_SECRET"
  github_webhook_max_body_bytes = 1048576
  github_app_enabled = false
  github_app_slug = ""
  github_app_client_id = ""
  github_app_client_secret_env = ""
  github_app_private_key_env = ""
  github_app_callback_url = ""
  github_connection_state_ttl_ms = 600000
  checkout_dir = "$state_dir/source-checkouts"
  checkout_timeout_ms = 120000
  checkout_max_files = 100000
  checkout_max_bytes = 268435456
  allowed_repositories = ["https://github.com/A3S-Lab/Cloud"]
  denied_repositories = []
}

logs {
  storage_provider = "local"
  s3_endpoint = ""
  s3_region = "us-east-1"
  s3_bucket = "a3s-cloud-logs"
  s3_prefix = "logs"
  s3_access_key_env = "A3S_CLOUD_S3_ACCESS_KEY_ID"
  s3_secret_key_env = "A3S_CLOUD_S3_SECRET_ACCESS_KEY"
  s3_session_token_env = ""
  s3_allow_http = false
  s3_virtual_hosted_style = false
  s3_request_timeout_ms = 30000
  s3_connect_timeout_ms = 5000
  s3_retry_timeout_ms = 60000
  s3_max_retries = 3
  retention_ms = 604800000
  retention_poll_ms = 60000
  retention_batch_size = 256
  tombstone_retention_ms = 2592000000
  tombstone_compaction_poll_ms = 3600000
  tombstone_compaction_batch_size = 1000
}

edge {
  entrypoint_address = "127.0.0.1:$gateway_port"
  management_address = "127.0.0.1:$gateway_management_port"
  management_path_prefix = "/api/gateway"
  management_auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  domain_verification_timeout_ms = 5000
  certificate_directory = "$gateway_certificates"
  certificate_ttl_ms = 2592000000
  certificate_renewal_window_ms = 604800000
  certificate_reconciliation_interval_ms = 60000
  upstream_request_timeout_ms = 10000
  command_ttl_ms = 180000
}

fleet {
  heartbeat_interval_ms = 1000
  heartbeat_timeout_ms = 10000
  command_long_poll_ms = 2000
  command_lease_ms = 10000
  certificate_ttl_ms = 3600000
  certificate_rotation_window_ms = 900000
}

security {
  profile = "development"
  state_dir = "$state_dir"
  certificate_authority = "local"
  gateway_certificate_authority = "local"
  key_encryption = "local"
  vault_address_env = "A3S_CLOUD_VAULT_ADDR"
  vault_token_env = "A3S_CLOUD_VAULT_TOKEN"
  vault_pki_mount = "pki"
  vault_pki_role = "a3s-cloud-node"
  vault_gateway_pki_mount = "gateway-pki"
  vault_gateway_pki_role = "a3s-cloud-gateway"
  vault_transit_mount = "transit"
  vault_transit_key = "a3s-cloud"
  vault_timeout_ms = 5000
}
ACL

export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:a3s_cloud@127.0.0.1:$postgres_port/a3s_cloud"
export A3S_CLOUD_NATS_URL="nats://127.0.0.1:1"
RUST_LOG=info "$control_bin" "$config_dir/cloud.acl" \
    >"$evidence/control-plane.log" 2>&1 &
control_pid=$!

timeout --signal=TERM --kill-after=10s 180s \
    python3 "$script_dir/release_gate.py" bootstrap \
        --api-origin "http://127.0.0.1:$api_port" \
        --evidence-dir "$evidence" \
        --context "$context_file" \
        --timeout 120

mkdir -p "$secret_memory_root"
chmod 700 "$secret_memory_root"
cat >"$config_dir/node.acl" <<ACL
control_plane {
  enrollment_url = "http://127.0.0.1:$api_port/api/v1/node-control/enroll"
  node_control_url = "https://localhost:$node_control_port"
  enrollment_token_env = "A3S_CLOUD_ENROLLMENT_TOKEN"
  server_ca_file = "$state_dir/node-ca/ca.pem"
  max_response_bytes = 20971520
  connect_timeout_ms = 5000
  request_timeout_ms = 10000
  artifact_transfer_timeout_ms = 900000
  long_poll_margin_ms = 3000
  retry_initial_ms = 100
  retry_max_ms = 5000
}

artifacts {
  max_blob_bytes = 1073741824
  max_entries = 100000
  max_file_bytes = 1073741824
  max_expanded_bytes = 4294967296
}

node {
  name = "e0-release-node"
  state_dir = "$state_dir/node"
}

logs {
  poll_interval_ms = 250
  max_batch_chunks = 256
  max_batch_bytes = 16777216
}

docker {
  socket = "unix:///var/run/docker.sock"
  namespace = "$namespace"
  operation_timeout_ms = 120000
  secret_memory_dir = "$secret_memory_root"
}

gateway {
  management_url = "http://127.0.0.1:$gateway_management_port/api/gateway"
  auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  state_file = "$state_dir/node/gateway-snapshot.json"
  certificate_directory = "$gateway_certificates"
  connect_timeout_ms = 5000
  validation_timeout_ms = 10000
  reload_timeout_ms = 30000
}
ACL

RUST_LOG=info "$node_bin" "$config_dir/node.acl" \
    >"$evidence/node-agent.log" 2>&1 &
node_pid=$!

log "phase=scenario-start"
timeout --signal=TERM --kill-after=10s 900s \
    python3 "$script_dir/release_gate.py" exercise \
        --api-origin "http://127.0.0.1:$api_port" \
        --evidence-dir "$evidence" \
        --context "$context_file" \
        --gateway-port "$gateway_port" \
        --hostname "e0-release.a3s.test" \
        --gateway-ca "$state_dir/gateway-ca/ca.pem" \
        --artifact-uri "$artifact_uri" \
        --artifact-digest "$artifact_digest" \
        --docker-namespace "$namespace" \
        --timeout 240
log "phase=scenario-pass"

git -C "$cloud" status --porcelain=v1 >"$evidence/cloud-dirty-after.txt"
git -C "$runtime" status --porcelain=v1 >"$evidence/runtime-dirty-after.txt"
[[ ! -s $evidence/cloud-dirty-after.txt ]] || die "Cloud worktree changed during release gate"
[[ ! -s $evidence/runtime-dirty-after.txt ]] || die "Runtime worktree changed during release gate"
gate_completed=1
log "phase=gate-pass"
