#!/usr/bin/env bash

set -Eeuo pipefail
umask 077

readonly PROVIDER_IMAGE="docker@sha256:66d292e5c26bd33a6f6f61cacb880de2186339a524ecba1ce098dbbaceed6515"
readonly REGISTRY_IMAGE="registry@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373"
readonly POSTGRES_IMAGE="postgres@sha256:742f40ea20b9ff2ff31db5458d127452988a2164df9e17441e191f3b72252193"
readonly NATS_IMAGE="nats@sha256:e4bf19f15fd3218814a4e3c9e0064e1334bd8aa20d5984b9f1a0afd084f8cc00"
readonly WORKLOAD_DIGEST="sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662"
readonly WORKLOAD_IMAGE="docker.io/library/busybox@${WORKLOAD_DIGEST}"
readonly EXPECTED_MANIFESTS=18
readonly EXPECTED_BLOBS=34
readonly PROVIDER_DISK_BYTES=$((4 * 1024 * 1024 * 1024))
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
readonly RUNTIME_REVISION_FILE="$script_dir/runtime-revision"

source_root=""
cloud_sha=""
runtime_sha=""
registry_data=""
evidence=""
target_dir=""
cargo_bin=""
ttl_seconds=2400
registry_data_owned=0
suite=provider

usage() {
    cat <<'USAGE'
Run the complete Docker Runtime certification gate in an isolated nested daemon.

Usage:
  run_isolated_docker_gate.sh \
    --source-root PATH \
    --cloud-sha FULL_SHA \
    [--runtime-sha FULL_SHA] \
    [--registry-data PATH] \
    [--evidence-dir PATH] \
    [--target-dir PATH] \
    [--cargo PATH] \
    [--suite provider|cloud] \
    [--ttl-seconds SECONDS]

The source root must contain clean apps/cloud and crates/runtime Git worktrees at
the exact supplied commits. The Runtime commit defaults to the repository-pinned
tools/runtime-conformance/runtime-revision value; an explicit --runtime-sha must
match it. If --registry-data is omitted, the runner creates a temporary registry
store and copies the pinned BusyBox OCI index from Docker Hub. An existing
registry store is mounted read-only and audited before use.

This command requires Linux, root, an already-running host Docker daemon, loop
devices, ext4 tools, nsenter, and the two pinned runner images already present in
the host image store. It never restarts the host Docker daemon.

The default provider suite runs Runtime conformance. The cloud suite also
requires the pinned PostgreSQL and NATS images and runs the A3S Cloud restart,
redelivery, reconciliation, cancellation, log transport, and cleanup E2E gates.
USAGE
    printf '\nProvider image: %s\nRegistry image: %s\nPostgreSQL image: %s\nNATS image: %s\nWorkload image: %s\n' \
        "$PROVIDER_IMAGE" "$REGISTRY_IMAGE" "$POSTGRES_IMAGE" "$NATS_IMAGE" "$WORKLOAD_IMAGE"
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --source-root)
            [[ $# -ge 2 ]] || die "--source-root requires a value"
            source_root=$2
            shift 2
            ;;
        --cloud-sha)
            [[ $# -ge 2 ]] || die "--cloud-sha requires a value"
            cloud_sha=$2
            shift 2
            ;;
        --runtime-sha)
            [[ $# -ge 2 ]] || die "--runtime-sha requires a value"
            runtime_sha=$2
            shift 2
            ;;
        --registry-data)
            [[ $# -ge 2 ]] || die "--registry-data requires a value"
            registry_data=$2
            shift 2
            ;;
        --evidence-dir)
            [[ $# -ge 2 ]] || die "--evidence-dir requires a value"
            evidence=$2
            shift 2
            ;;
        --target-dir)
            [[ $# -ge 2 ]] || die "--target-dir requires a value"
            target_dir=$2
            shift 2
            ;;
        --cargo)
            [[ $# -ge 2 ]] || die "--cargo requires a value"
            cargo_bin=$2
            shift 2
            ;;
        --suite)
            [[ $# -ge 2 ]] || die "--suite requires a value"
            suite=$2
            shift 2
            ;;
        --ttl-seconds)
            [[ $# -ge 2 ]] || die "--ttl-seconds requires a value"
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

[[ $(uname -s) == Linux ]] || die "the isolated Docker gate requires Linux"
[[ $(id -u) -eq 0 ]] || die "the isolated Docker gate must run as root"
[[ $cloud_sha =~ ^[0-9a-f]{40}$ ]] || die "--cloud-sha must be a full lowercase Git SHA"
[[ -f $RUNTIME_REVISION_FILE ]] || die "Runtime revision file is missing: $RUNTIME_REVISION_FILE"
IFS= read -r pinned_runtime_sha <"$RUNTIME_REVISION_FILE" ||
    die "Runtime revision file is unreadable: $RUNTIME_REVISION_FILE"
[[ $pinned_runtime_sha =~ ^[0-9a-f]{40}$ ]] ||
    die "Runtime revision file must contain one full lowercase Git SHA"
[[ -n $runtime_sha ]] || runtime_sha=$pinned_runtime_sha
[[ $runtime_sha =~ ^[0-9a-f]{40}$ ]] || die "--runtime-sha must be a full lowercase Git SHA"
[[ $runtime_sha == "$pinned_runtime_sha" ]] ||
    die "--runtime-sha must match the repository-pinned Runtime revision $pinned_runtime_sha"
[[ $suite == provider || $suite == cloud ]] || die "--suite must be provider or cloud"
[[ $ttl_seconds =~ ^[0-9]+$ ]] || die "--ttl-seconds must be an integer"
((ttl_seconds >= 600 && ttl_seconds <= 3600)) || die "--ttl-seconds must be between 600 and 3600"
[[ -n $source_root ]] || die "--source-root is required"
source_root=$(realpath "$source_root")

for command in awk basename comm curl dirname docker find findmnt git losetup \
    mkfs.ext4 mount mountpoint nsenter pkill python3 realpath sha256sum sort \
    stat tee timeout umount; do
    command -v "$command" >/dev/null || die "required command is unavailable: $command"
done

if [[ -z $cargo_bin ]]; then
    cargo_bin=$(command -v cargo || true)
    [[ -n $cargo_bin ]] || [[ ! -x /root/.cargo/bin/cargo ]] || cargo_bin=/root/.cargo/bin/cargo
fi
if [[ $cargo_bin != */* ]]; then
    cargo_bin=$(command -v "$cargo_bin" || true)
fi
[[ -x $cargo_bin ]] || die "Cargo executable is unavailable: $cargo_bin"
cargo_dir=$(cd "$(dirname "$cargo_bin")" && pwd -P)
cargo_bin=$cargo_dir/$(basename "$cargo_bin")
cargo_path=$cargo_dir:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

cloud=$source_root/apps/cloud
runtime=$source_root/crates/runtime
[[ -d $cloud ]] || die "Cloud worktree is missing: $cloud"
[[ -d $runtime ]] || die "Runtime worktree is missing: $runtime"
[[ $(git -C "$cloud" rev-parse HEAD) == "$cloud_sha" ]] || die "Cloud worktree is not at $cloud_sha"
[[ $(git -C "$runtime" rev-parse HEAD) == "$runtime_sha" ]] || die "Runtime worktree is not at $runtime_sha"
[[ -z $(git -C "$cloud" status --porcelain=v1) ]] || die "Cloud worktree is dirty"
[[ -z $(git -C "$runtime" status --porcelain=v1) ]] || die "Runtime worktree is dirty"
docker info >/dev/null 2>&1 || die "the host Docker daemon is unavailable"
docker image inspect "$PROVIDER_IMAGE" >/dev/null || die "the pinned Docker provider image is not present"
docker image inspect "$REGISTRY_IMAGE" >/dev/null || die "the pinned registry image is not present"
if [[ $suite == cloud ]]; then
    docker image inspect "$POSTGRES_IMAGE" >/dev/null || die "the pinned PostgreSQL image is not present"
    docker image inspect "$NATS_IMAGE" >/dev/null || die "the pinned NATS image is not present"
fi

stamp=$(date -u +%Y%m%dT%H%M%SZ)
if [[ $suite == provider ]]; then
    run_id="full-${cloud_sha:0:7}-${runtime_sha:0:7}-${stamp}"
else
    run_id="cloud-r15-${cloud_sha:0:7}-${runtime_sha:0:7}-${stamp}"
fi
suffix=$(printf '%s' "$run_id-$$-$RANDOM" | sha256sum | cut -c1-12)
provider_root=/var/tmp/a3s-runtime-provider/$run_id
data_dir=$provider_root/data
socket_dir=$provider_root/socket
disk_image=$provider_root/provider-data.ext4
provider=a3s-runtime-provider-$suffix
keeper=a3s-runtime-network-keeper-$suffix
registry=a3s-runtime-registry-$suffix
postgres=a3s-runtime-postgres-$suffix
nats=a3s-runtime-nats-$suffix
network=a3s-runtime-provider-net-$suffix
provider_host=unix://$socket_dir/docker.sock
secret_memory_dir=""
provider_secret_mount=()
if [[ -z $evidence ]]; then
    if [[ $suite == provider ]]; then
        evidence=/tmp/a3s-cloud-docker-full-isolated-$run_id
    else
        evidence=/tmp/a3s-cloud-runtime-e2e-isolated-$run_id
    fi
fi
[[ -n $target_dir ]] || target_dir=/var/tmp/a3s-runtime-build-cache/${cloud_sha:0:7}-${runtime_sha:0:7}
evidence=$(realpath -m "$evidence")
target_dir=$(realpath -m "$target_dir")

if [[ -z $registry_data ]]; then
    registry_data=$provider_root/registry
    registry_data_owned=1
else
    registry_data=$(realpath "$registry_data")
fi

loop_device=""
ttl_pid=""
cleanup_failed=0
gate_completed=0
mkdir -p "$evidence" "$provider_root" "$data_dir" "$socket_dir" "$target_dir" "$registry_data"
if [[ $suite == cloud ]]; then
    secret_memory_dir=/dev/shm/a3s-cloud/$run_id
    mkdir -p "$secret_memory_dir"
    chmod 700 "$secret_memory_dir"
    [[ $(findmnt -rn -T "$secret_memory_dir" -o FSTYPE) == tmpfs ]] ||
        die "Cloud Secret material directory is not tmpfs-backed"
    provider_secret_mount=(--mount "type=bind,source=$secret_memory_dir,target=$secret_memory_dir")
fi
exec > >(tee -a "$evidence/runner.log") 2>&1

log() {
    printf 'A3S_ISOLATED_GATE %s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"
}

record_host_inventory() {
    local phase=$1
    docker ps -aq | sort >"$evidence/host-containers-$phase.ids"
    docker volume ls -q | sort >"$evidence/host-volumes-$phase.names"
    docker network ls -q | sort >"$evidence/host-networks-$phase.ids"
    losetup -l -n -O NAME,BACK-FILE 2>/dev/null | sort >"$evidence/host-loops-$phase.txt" || true
    findmnt -rn -S '/dev/loop*' -o SOURCE,TARGET,FSTYPE,OPTIONS 2>/dev/null | sort >"$evidence/host-loop-mounts-$phase.txt" || true
    find /run/docker/netns -mindepth 1 -maxdepth 1 -printf '%f\n' 2>/dev/null | sort >"$evidence/host-docker-netns-$phase.names" || true
}

record_provider_inventory() {
    local phase=$1
    docker --host "$provider_host" ps -aq | sort >"$evidence/provider-containers-$phase.ids"
    docker --host "$provider_host" volume ls -q | sort >"$evidence/provider-volumes-$phase.names"
    docker --host "$provider_host" network ls -q | sort >"$evidence/provider-networks-$phase.ids"
    docker --host "$provider_host" network ls \
        --format '{{.Name}}\t{{.Driver}}\t{{.Scope}}\t{{.Internal}}\t{{.IPv6}}' | \
        sort >"$evidence/provider-networks-$phase.semantic"
}

write_deltas() {
    local prefix=$1 before=$2 after=$3 added=$4 removed=$5
    comm -13 "$before" "$after" >"$evidence/$prefix-added.$added"
    comm -23 "$before" "$after" >"$evidence/$prefix-removed.$removed"
}

audit_registry_fixture() {
    local storage_root=$registry_data/docker/registry/v2
    local repository=$storage_root/repositories/library/busybox
    local digest_hex=${WORKLOAD_DIGEST#sha256:}
    local root_link=$repository/_manifests/revisions/sha256/$digest_hex/link
    local digest_file=$evidence/oci-fixture-digests.txt
    [[ -f $root_link ]] || die "registry fixture omits the pinned root manifest"
    [[ $(<"$root_link") == "$WORKLOAD_DIGEST" ]] || die "registry fixture root digest is invalid"

    find "$repository" -type f -name link -print0 |
        while IFS= read -r -d '' link; do
            local value hex data actual
            value=$(<"$link")
            [[ $value =~ ^sha256:[0-9a-f]{64}$ ]] || die "invalid registry link: $link"
            hex=${value#sha256:}
            data=$storage_root/blobs/sha256/${hex:0:2}/$hex/data
            [[ -f $data ]] || die "registry fixture omits blob $value"
            actual=$(sha256sum "$data" | awk '{print $1}')
            [[ $actual == "$hex" ]] || die "registry fixture blob digest mismatch: $value"
            printf '%s\n' "$value"
        done | sort -u >"$digest_file"

    local manifests blobs unique total_bytes value hex data
    manifests=$(find "$repository/_manifests/revisions/sha256" -mindepth 2 -maxdepth 2 -type f -name link | wc -l)
    blobs=$(find "$repository/_layers/sha256" -mindepth 2 -maxdepth 2 -type f -name link | wc -l)
    unique=$(wc -l <"$digest_file")
    [[ $manifests -eq $EXPECTED_MANIFESTS ]] || die "registry fixture manifest count is $manifests"
    [[ $blobs -eq $EXPECTED_BLOBS ]] || die "registry fixture blob count is $blobs"
    [[ $unique -eq $((EXPECTED_MANIFESTS + EXPECTED_BLOBS)) ]] || die "registry fixture object count is $unique"
    total_bytes=0
    while IFS= read -r value; do
        hex=${value#sha256:}
        data=$storage_root/blobs/sha256/${hex:0:2}/$hex/data
        total_bytes=$((total_bytes + $(stat -c %s "$data")))
    done <"$digest_file"
    printf 'OCI_FIXTURE_AUDIT_PASS root=%s manifests=%s blobs=%s unique=%s total_bytes=%s\n' \
        "$WORKLOAD_DIGEST" "$manifests" "$blobs" "$unique" "$total_bytes" |
        tee "$evidence/oci-fixture-audit.txt"
}

cleanup_resources() {
    set +e
    {
        log "phase=cleanup-start"
        if docker container inspect "$provider" >/dev/null 2>&1; then
            timeout --signal=TERM --kill-after=10s 60s docker logs "$provider" >"$evidence/provider.log" 2>&1
            docker inspect "$provider" >"$evidence/provider-inspect-final.json" 2>&1
            timeout --signal=TERM --kill-after=10s 90s docker rm -fv "$provider"
        fi
        if docker container inspect "$keeper" >/dev/null 2>&1; then
            docker inspect "$keeper" >"$evidence/keeper-inspect-final.json" 2>&1
            timeout --signal=TERM --kill-after=10s 60s docker rm -fv "$keeper"
        fi
        if docker container inspect "$postgres" >/dev/null 2>&1; then
            timeout --signal=TERM --kill-after=10s 60s docker logs "$postgres" >"$evidence/postgres.log" 2>&1
            docker inspect "$postgres" >"$evidence/postgres-inspect-final.json" 2>&1
            timeout --signal=TERM --kill-after=10s 60s docker rm -fv "$postgres"
        fi
        if docker container inspect "$nats" >/dev/null 2>&1; then
            timeout --signal=TERM --kill-after=10s 60s docker logs "$nats" >"$evidence/nats.log" 2>&1
            docker inspect "$nats" >"$evidence/nats-inspect-final.json" 2>&1
            timeout --signal=TERM --kill-after=10s 60s docker rm -fv "$nats"
        fi
        if docker container inspect "$registry" >/dev/null 2>&1; then
            timeout --signal=TERM --kill-after=10s 60s docker logs "$registry" >"$evidence/registry.log" 2>&1
            docker inspect "$registry" >"$evidence/registry-inspect-final.json" 2>&1
            timeout --signal=TERM --kill-after=10s 90s docker rm -fv "$registry"
        fi
        if docker network inspect "$network" >/dev/null 2>&1; then
            timeout --signal=TERM --kill-after=10s 60s docker network rm "$network"
        fi
        local leftovers
        leftovers=$(docker ps -aq --filter "label=a3s.runtime.conformance.run-id=$run_id")
        [[ -z $leftovers ]] || timeout --signal=TERM --kill-after=10s 90s docker rm -fv $leftovers
        if docker network inspect "$network" >/dev/null 2>&1; then
            timeout --signal=TERM --kill-after=10s 60s docker network rm "$network"
        fi

        if mountpoint -q "$data_dir"; then
            for _ in 1 2 3 4 5; do
                umount "$data_dir" && break
                sleep 1
            done
        fi
        if mountpoint -q "$data_dir"; then
            cleanup_failed=1
            log "cleanup-error=provider-data-still-mounted"
        fi
        if [[ -n $loop_device ]] && losetup "$loop_device" >/dev/null 2>&1; then
            losetup -d "$loop_device" || cleanup_failed=1
        fi
        if [[ -n $loop_device ]] && losetup "$loop_device" >/dev/null 2>&1; then
            cleanup_failed=1
            log "cleanup-error=loop-still-attached loop=$loop_device"
        fi
        if [[ -n $secret_memory_dir ]]; then
            find "$secret_memory_dir" -type f -print >"$evidence/secret-files-after-test.paths"
            if [[ -s $evidence/secret-files-after-test.paths ]]; then
                cleanup_failed=1
                log "cleanup-error=secret-files-remain"
            fi
            rm -rf "$secret_memory_dir"
            if [[ -e $secret_memory_dir ]]; then
                cleanup_failed=1
                log "cleanup-error=secret-memory-directory-remains"
            fi
        fi
        mountpoint -q "$data_dir" || rm -rf "$provider_root"

        docker ps -aq --filter "label=a3s.runtime.conformance.run-id=$run_id" | sort >"$evidence/target-containers-after.ids"
        docker network inspect "$network" >/dev/null 2>&1 && printf '%s\n' "$network" >"$evidence/target-networks-after.names" || : >"$evidence/target-networks-after.names"
        [[ ! -d $provider_root ]] && : >"$evidence/target-directories-after.paths" || printf '%s\n' "$provider_root" >"$evidence/target-directories-after.paths"
        record_host_inventory after

        write_deltas host-containers "$evidence/host-containers-before.ids" "$evidence/host-containers-after.ids" ids ids
        write_deltas host-volumes "$evidence/host-volumes-before.names" "$evidence/host-volumes-after.names" names names
        write_deltas host-networks "$evidence/host-networks-before.ids" "$evidence/host-networks-after.ids" ids ids
        write_deltas host-loops "$evidence/host-loops-before.txt" "$evidence/host-loops-after.txt" txt txt
        write_deltas host-loop-mounts "$evidence/host-loop-mounts-before.txt" "$evidence/host-loop-mounts-after.txt" txt txt
        write_deltas host-docker-netns "$evidence/host-docker-netns-before.names" "$evidence/host-docker-netns-after.names" names names

        if [[ -s $evidence/target-containers-after.ids ||
              -s $evidence/target-networks-after.names ||
              -s $evidence/target-directories-after.paths ]]; then
            cleanup_failed=1
        fi
        log "phase=cleanup-finish cleanup_failed=$cleanup_failed"
    } >>"$evidence/cleanup.log" 2>&1
    set -e
}

on_exit() {
    local exit_status=$?
    trap - EXIT INT TERM HUP
    if [[ -n $ttl_pid ]]; then
        pkill -TERM -P "$ttl_pid" >/dev/null 2>&1 || true
        kill "$ttl_pid" >/dev/null 2>&1 || true
        wait "$ttl_pid" >/dev/null 2>&1 || true
    fi
    cleanup_resources
    [[ $cleanup_failed -eq 0 ]] || exit_status=1
    if [[ $exit_status -eq 0 && $gate_completed -eq 1 ]]; then
        if [[ $suite == provider ]]; then
            printf 'A3S_DOCKER_CERTIFICATION_PASS cloud=%s runtime=%s run_id=%s\n' \
                "$cloud_sha" "$runtime_sha" "$run_id" | tee "$evidence/result.txt"
        else
            printf 'A3S_CLOUD_RUNTIME_E2E_PASS cloud=%s runtime=%s run_id=%s\n' \
                "$cloud_sha" "$runtime_sha" "$run_id" | tee "$evidence/result.txt"
        fi
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

printf '%s\n' "$cloud_sha" >"$evidence/cloud.sha"
printf '%s\n' "$runtime_sha" >"$evidence/runtime.sha"
printf '%s\n' "$suite" >"$evidence/suite.txt"
printf '%s\n' "$PROVIDER_IMAGE" >"$evidence/provider-image.txt"
printf '%s\n' "$REGISTRY_IMAGE" >"$evidence/registry-image.txt"
printf '%s\n' "$POSTGRES_IMAGE" >"$evidence/postgres-image.txt"
printf '%s\n' "$NATS_IMAGE" >"$evidence/nats-image.txt"
printf '%s\n' "$WORKLOAD_IMAGE" >"$evidence/workload-image.txt"
printf '%s\n' "$source_root" >"$evidence/source-root.txt"
printf '%s\n' "$target_dir" >"$evidence/cargo-target-dir.txt"
printf '%s\n' "$secret_memory_dir" >"$evidence/secret-memory-dir.txt"
git -C "$cloud" status --porcelain=v1 >"$evidence/cloud-dirty-before.txt"
git -C "$runtime" status --porcelain=v1 >"$evidence/runtime-dirty-before.txt"
record_host_inventory before
[[ -z $(docker ps -aq --filter "label=a3s.runtime.conformance.run-id=$run_id") ]] || die "run ID already owns containers"

log "phase=build-start"
if [[ $suite == provider ]]; then
    timeout --signal=TERM --kill-after=30s 1200s \
        env PATH="$cargo_path" CARGO_TARGET_DIR="$target_dir" \
        "$cargo_bin" test --manifest-path "$cloud/Cargo.toml" --locked \
        -p a3s-cloud-node-agent --test docker_conformance --no-run \
        2>&1 | tee "$evidence/cargo-build.log"
else
    timeout --signal=TERM --kill-after=30s 1200s \
        env PATH="$cargo_path" CARGO_TARGET_DIR="$target_dir" \
        "$cargo_bin" test --manifest-path "$cloud/Cargo.toml" --locked \
        -p a3s-cloud-control-plane --test postgres_integration --no-run \
        2>&1 | tee "$evidence/cargo-postgres-build.log"
    timeout --signal=TERM --kill-after=30s 1200s \
        env PATH="$cargo_path" CARGO_TARGET_DIR="$target_dir" \
        "$cargo_bin" test --manifest-path "$cloud/Cargo.toml" --locked \
        -p a3s-cloud-control-plane --test docker_deployment --no-run \
        2>&1 | tee "$evidence/cargo-docker-deployment-build.log"
fi
log "phase=build-pass"

docker network create --label "a3s.runtime.conformance.run-id=$run_id" "$network" >"$evidence/network.id"
registry_mount="type=bind,source=$registry_data,target=/var/lib/registry"
[[ $registry_data_owned -eq 1 ]] || registry_mount+=",readonly"
docker run -d --pull=never --name "$registry" --network "$network" \
    --network-alias a3s-runtime-registry \
    --label "a3s.runtime.conformance.run-id=$run_id" \
    --cpus 0.25 --memory 128m --pids-limit 64 \
    --publish 127.0.0.1::5000 --mount "$registry_mount" \
    "$REGISTRY_IMAGE" >"$evidence/registry.id"
registry_port=$(docker inspect --format '{{(index (index .NetworkSettings.Ports "5000/tcp") 0).HostPort}}' "$registry")
for attempt in $(seq 1 60); do
    curl --fail --silent --show-error --max-time 2 "http://127.0.0.1:$registry_port/v2/" >/dev/null && break
    [[ $attempt -ne 60 ]] || die "registry readiness timed out"
    sleep 0.5
done
[[ -z $(docker inspect --format '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' "$registry") ]] || die "registry unexpectedly owns an anonymous volume"

if [[ $registry_data_owned -eq 1 ]]; then
    copy_status=1
    : >"$evidence/oci-copy.log"
    for attempt in 1 2 3; do
        set +e
        python3 "$cloud/tools/runtime-conformance/copy_oci_image.py" \
            --source https://registry-1.docker.io \
            --token-url https://auth.docker.io/token \
            --service registry.docker.io \
            --repository library/busybox \
            --digest "$WORKLOAD_DIGEST" \
            --target "http://127.0.0.1:$registry_port" \
            --tag conformance 2>&1 | tee -a "$evidence/oci-copy.log"
        copy_status=${PIPESTATUS[0]}
        set -e
        [[ $copy_status -ne 0 ]] || break
        sleep $((attempt * 2))
    done
    [[ $copy_status -eq 0 ]] || die "OCI fixture copy failed after three attempts"
fi
audit_registry_fixture
curl --fail --silent --show-error --head --max-time 10 \
    --header 'Accept: application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json' \
    "http://127.0.0.1:$registry_port/v2/library/busybox/manifests/$WORKLOAD_DIGEST" \
    >"$evidence/registry-manifest-head.txt"
grep -Fi "Docker-Content-Digest: $WORKLOAD_DIGEST" "$evidence/registry-manifest-head.txt" >/dev/null
log "phase=registry-pass"

if [[ $suite == cloud ]]; then
    docker run -d --pull=never --name "$postgres" --network "$network" \
        --network-alias a3s-runtime-postgres \
        --label "a3s.runtime.conformance.run-id=$run_id" \
        --cpus 1 --memory 1g --pids-limit 512 --shm-size 128m \
        --tmpfs /var/lib/postgresql/data:rw,nosuid,nodev,noexec,size=2147483648 \
        --env POSTGRES_DB=postgres \
        --env POSTGRES_USER=a3s_cloud \
        --env POSTGRES_PASSWORD=a3s_cloud \
        "$POSTGRES_IMAGE" >"$evidence/postgres.id"
    docker run -d --pull=never --name "$nats" --network "$network" \
        --network-alias a3s-runtime-nats \
        --label "a3s.runtime.conformance.run-id=$run_id" \
        --cpus 0.5 --memory 256m --pids-limit 128 \
        --tmpfs /data:rw,nosuid,nodev,noexec,size=268435456 \
        "$NATS_IMAGE" --jetstream --store_dir=/data --http_port=8222 \
        >"$evidence/nats.id"
    for attempt in $(seq 1 90); do
        docker exec "$postgres" pg_isready --dbname=postgres --username=a3s_cloud >/dev/null 2>&1 && break
        [[ $attempt -ne 90 ]] || die "PostgreSQL readiness timed out"
        sleep 0.5
    done
    for attempt in $(seq 1 90); do
        docker exec "$nats" wget --quiet --output-document=- http://127.0.0.1:8222/healthz 2>/dev/null | grep -q ok && break
        [[ $attempt -ne 90 ]] || die "NATS readiness timed out"
        sleep 0.5
    done
    [[ -z $(docker inspect --format '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' "$postgres") ]] || die "PostgreSQL unexpectedly owns an anonymous volume"
    [[ -z $(docker inspect --format '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' "$nats") ]] || die "NATS unexpectedly owns an anonymous volume"
    docker inspect "$postgres" >"$evidence/postgres-inspect-initial.json"
    docker inspect "$nats" >"$evidence/nats-inspect-initial.json"
    log "phase=cloud-services-pass"
fi

truncate -s "$PROVIDER_DISK_BYTES" "$disk_image"
loop_device=$(losetup --find --show "$disk_image")
printf '%s\n' "$loop_device" >"$evidence/loop-device.txt"
mkfs.ext4 -F -m 0 -L a3s-runtime "$loop_device" >"$evidence/mkfs.log" 2>&1
mount -t ext4 -o noatime,nodiratime "$loop_device" "$data_dir"
findmnt -rn "$data_dir" -o SOURCE,TARGET,FSTYPE,OPTIONS >"$evidence/provider-data-mount.txt"

keeper_lifetime=$((ttl_seconds + 300))
docker run -d --pull=never --name "$keeper" --network "$network" \
    --label "a3s.runtime.conformance.run-id=$run_id" \
    --cpus 0.10 --memory 32m --pids-limit 32 \
    --entrypoint /bin/sh "$PROVIDER_IMAGE" -ceu 'exec sleep "$1"' sh "$keeper_lifetime" \
    >"$evidence/keeper.id"
docker run -d --pull=never --name "$provider" --network "container:$keeper" \
    --label a3s.runtime.conformance.provider=true \
    --label "a3s.runtime.conformance.run-id=$run_id" \
    --privileged --cpus 2 --memory 4g --pids-limit 2048 \
    --mount "type=bind,source=$data_dir,target=/var/lib/docker" \
    --mount "type=bind,source=$socket_dir,target=/run/a3s-provider" \
    "${provider_secret_mount[@]}" \
    --entrypoint /bin/sh "$PROVIDER_IMAGE" \
    -ceu '
        root=/sys/fs/cgroup
        mkdir -p "$root/a3s-init" "$root/a3s-workloads"
        printf "%s\n" "$$" >"$root/a3s-init/cgroup.procs"
        enabled=
        available=" $(cat "$root/cgroup.controllers") "
        for controller in cpuset cpu io memory pids; do
            case "$available" in *" $controller "*) enabled="$enabled +$controller" ;; esac
        done
        [ -z "$enabled" ] || printf "%s\n" "$enabled" >"$root/cgroup.subtree_control"
        rm -rf /run/a3s-provider/exec
        rm -f /run/a3s-provider/docker.pid /run/a3s-provider/docker.sock
        exec dockerd --cgroup-parent=/a3s-workloads "$@"
    ' sh \
    --host=unix:///run/a3s-provider/docker.sock \
    --storage-driver=vfs \
    --registry-mirror=http://a3s-runtime-registry:5000 \
    --insecure-registry=a3s-runtime-registry:5000 \
    --data-root=/var/lib/docker \
    --exec-root=/run/a3s-provider/exec \
    --pidfile=/run/a3s-provider/docker.pid >"$evidence/provider.id"

(
    sleep "$ttl_seconds"
    timeout --signal=TERM --kill-after=10s 90s \
        docker rm -fv "$provider" "$keeper" "$postgres" "$nats" "$registry" || true
    timeout --signal=TERM --kill-after=10s 60s docker network rm "$network" || true
    mountpoint -q "$data_dir" && umount "$data_dir" || true
    [[ -z $loop_device ]] || ! losetup "$loop_device" >/dev/null 2>&1 || losetup -d "$loop_device" || true
    [[ -z $secret_memory_dir ]] || rm -rf "$secret_memory_dir"
    mountpoint -q "$data_dir" || rm -rf "$provider_root"
) >"$evidence/ttl.log" 2>&1 &
ttl_pid=$!
printf '%s\n' "$ttl_pid" >"$evidence/ttl.pid"

for attempt in $(seq 1 120); do
    docker --host "$provider_host" version >/dev/null 2>&1 && break
    [[ $attempt -ne 120 ]] || die "provider readiness timed out"
    sleep 0.5
done
docker inspect "$provider" >"$evidence/provider-inspect-initial.json"
docker inspect "$keeper" >"$evidence/keeper-inspect-initial.json"
docker --host "$provider_host" info >"$evidence/provider-info-initial.txt"
timeout --signal=TERM --kill-after=10s 300s docker --host "$provider_host" pull "$WORKLOAD_IMAGE" \
    2>&1 | tee "$evidence/provider-pull.log"
docker --host "$provider_host" image inspect "$WORKLOAD_IMAGE" >"$evidence/workload-image-inspect.json"

run_resource_probe() {
    local name=$1
    docker --host "$provider_host" run --rm --name "$name" \
        --cpus 0.333 --memory 48m --memory-swap 48m --pids-limit 17 \
        "$WORKLOAD_IMAGE" /bin/sh -ceu '
            set -- $(cat /sys/fs/cgroup/cpu.max)
            test "$1" != max
            test "$(cat /sys/fs/cgroup/memory.max)" = 50331648
            test "$(cat /sys/fs/cgroup/pids.max)" = 17
        '
}

run_resource_probe "a3s-runtime-restart-preflight-$suffix"
keeper_pid=$(docker inspect --format '{{.State.Pid}}' "$keeper")
provider_pid_before=$(docker inspect --format '{{.State.Pid}}' "$provider")
keeper_netns=$(readlink "/proc/$keeper_pid/ns/net")
provider_netns_before=$(readlink "/proc/$provider_pid_before/ns/net")
printf '%s\n' "$keeper_netns" >"$evidence/keeper-netns.txt"
printf '%s\n' "$provider_netns_before" >"$evidence/provider-netns-before.txt"
[[ $keeper_netns == "$provider_netns_before" ]] || die "provider does not share the keeper network namespace"
timeout --signal=TERM --kill-after=10s 90s docker restart --timeout 10 "$provider" >"$evidence/provider-preflight-restart.txt"
for attempt in $(seq 1 120); do
    docker --host "$provider_host" version >/dev/null 2>&1 && break
    [[ $attempt -ne 120 ]] || die "provider restart readiness timed out"
    sleep 0.5
done
provider_pid_after=$(docker inspect --format '{{.State.Pid}}' "$provider")
provider_netns_after=$(readlink "/proc/$provider_pid_after/ns/net")
printf '%s\n' "$provider_netns_after" >"$evidence/provider-netns-after.txt"
[[ $keeper_netns == "$provider_netns_after" ]] || die "provider restart replaced the network namespace"
run_resource_probe "a3s-runtime-restart-postflight-$suffix"
record_provider_inventory before
log "phase=provider-preflight-pass netns=$keeper_netns"

if [[ $suite == provider ]]; then
    set +e
    timeout --signal=TERM --kill-after=30s 2100s \
        nsenter -t "$keeper_pid" -n -- \
        env PATH="$cargo_path" \
            A3S_CLOUD_TEST_DOCKER=1 \
            A3S_CLOUD_TEST_DOCKER_SOCKET="$provider_host" \
            A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER="$provider" \
            CARGO_TARGET_DIR="$target_dir" \
            "$cargo_bin" test --manifest-path "$cloud/Cargo.toml" --locked \
                -p a3s-cloud-node-agent \
                --test docker_conformance \
                real_docker_passes_all_advertised_runtime_profiles \
                -- --ignored --exact --nocapture --test-threads=1 \
        2>&1 | tee "$evidence/cargo-test.log"
    test_status=${PIPESTATUS[0]}
    set -e
    printf '%s\n' "$test_status" >"$evidence/cargo-test-status.txt"
else
    postgres_ip=$(docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$postgres")
    nats_ip=$(docker inspect --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$nats")
    [[ -n $postgres_ip ]] || die "PostgreSQL container has no network address"
    [[ -n $nats_ip ]] || die "NATS container has no network address"
    printf '%s\n' "$postgres_ip" >"$evidence/postgres.ip"
    printf '%s\n' "$nats_ip" >"$evidence/nats.ip"

    set +e
    timeout --signal=TERM --kill-after=30s 1800s \
        nsenter -t "$keeper_pid" -n -- \
        env PATH="$cargo_path" \
            RUST_BACKTRACE=1 \
            A3S_CLOUD_TEST_DOCKER=1 \
            A3S_CLOUD_TEST_DOCKER_SOCKET="$provider_host" \
            A3S_CLOUD_TEST_SECRET_MEMORY_DIR="$secret_memory_dir" \
            A3S_CLOUD_TEST_POSTGRES_URL="postgres://a3s_cloud:a3s_cloud@$postgres_ip:5432/postgres" \
            A3S_CLOUD_TEST_NATS_URL="nats://$nats_ip:4222" \
            CARGO_TARGET_DIR="$target_dir" \
            "$cargo_bin" test --manifest-path "$cloud/Cargo.toml" --locked \
                -p a3s-cloud-control-plane \
                --test postgres_integration \
                postgres_foundation_is_migrated_atomic_and_idempotent \
                -- --exact --nocapture --test-threads=1 \
        2>&1 | tee "$evidence/cargo-postgres-test.log"
    postgres_test_status=${PIPESTATUS[0]}

    timeout --signal=TERM --kill-after=30s 900s \
        nsenter -t "$keeper_pid" -n -- \
        env PATH="$cargo_path" \
            RUST_BACKTRACE=1 \
            A3S_CLOUD_TEST_DOCKER=1 \
            A3S_CLOUD_TEST_DOCKER_SOCKET="$provider_host" \
            A3S_CLOUD_TEST_SECRET_MEMORY_DIR="$secret_memory_dir" \
            CARGO_TARGET_DIR="$target_dir" \
            "$cargo_bin" test --manifest-path "$cloud/Cargo.toml" --locked \
                -p a3s-cloud-control-plane \
                --test docker_deployment \
                permanently_unhealthy_real_docker_update_preserves_healthy_revision \
                -- --exact --nocapture --test-threads=1 \
        2>&1 | tee "$evidence/cargo-docker-deployment-test.log"
    docker_deployment_test_status=${PIPESTATUS[0]}
    set -e
    printf '%s\n' "$postgres_test_status" >"$evidence/cargo-postgres-test-status.txt"
    printf '%s\n' "$docker_deployment_test_status" >"$evidence/cargo-docker-deployment-test-status.txt"
    if [[ $postgres_test_status -eq 0 && $docker_deployment_test_status -eq 0 ]]; then
        test_status=0
    else
        test_status=1
    fi
    printf '%s\n' "$test_status" >"$evidence/cargo-test-status.txt"

    docker exec "$postgres" psql --username=a3s_cloud --dbname=postgres --tuples-only --no-align \
        --command="select count(*) from pg_database where datname like 'a3s_cloud_test_%'" \
        >"$evidence/postgres-test-database-count.txt"
    [[ $(tr -d '[:space:]' <"$evidence/postgres-test-database-count.txt") == 0 ]] || \
        die "Cloud E2E left an isolated PostgreSQL test database"
fi

for attempt in $(seq 1 120); do
    docker --host "$provider_host" version >/dev/null 2>&1 && break
    [[ $attempt -ne 120 ]] || die "provider post-test readiness timed out"
    sleep 0.5
done
provider_pid_final=$(docker inspect --format '{{.State.Pid}}' "$provider")
provider_netns_final=$(readlink "/proc/$provider_pid_final/ns/net")
printf '%s\n' "$provider_netns_final" >"$evidence/provider-netns-final.txt"
[[ $keeper_netns == "$provider_netns_final" ]] || die "conformance replaced the provider network namespace"
record_provider_inventory after

write_deltas provider-containers "$evidence/provider-containers-before.ids" "$evidence/provider-containers-after.ids" ids ids
write_deltas provider-volumes "$evidence/provider-volumes-before.names" "$evidence/provider-volumes-after.names" names names
write_deltas provider-network-ids "$evidence/provider-networks-before.ids" "$evidence/provider-networks-after.ids" ids ids
write_deltas provider-networks "$evidence/provider-networks-before.semantic" "$evidence/provider-networks-after.semantic" semantic semantic
git -C "$cloud" status --porcelain=v1 >"$evidence/cloud-dirty-after.txt"
git -C "$runtime" status --porcelain=v1 >"$evidence/runtime-dirty-after.txt"

if [[ $suite == provider ]]; then
    [[ $test_status -eq 0 ]] || die "Docker Runtime certification test failed"
else
    [[ $test_status -eq 0 ]] || die "A3S Cloud Runtime E2E test failed"
fi
for empty_file in \
    "$evidence/provider-containers-added.ids" \
    "$evidence/provider-containers-removed.ids" \
    "$evidence/provider-volumes-added.names" \
    "$evidence/provider-volumes-removed.names" \
    "$evidence/provider-networks-added.semantic" \
    "$evidence/provider-networks-removed.semantic" \
    "$evidence/cloud-dirty-after.txt" \
    "$evidence/runtime-dirty-after.txt"; do
    [[ ! -s $empty_file ]] || die "post-test inventory changed: $empty_file"
done
gate_completed=1
log "phase=gate-pass"
