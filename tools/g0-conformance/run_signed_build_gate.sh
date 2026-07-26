#!/usr/bin/env bash
set -euo pipefail

cloud_root=$(pwd -P)
[[ -f "$cloud_root/Cargo.toml" ]]
[[ -f "$cloud_root/tools/g0-conformance/run_signed_build_gate.sh" ]]

required_commands=(cargo cut docker git mktemp rm seq sha256sum sleep sudo tar wc)
for required_command in "${required_commands[@]}"; do
  command -v "$required_command" >/dev/null
done

required_environment=(
  A3S_CLOUD_TEST_REGISTRY_URL
  A3S_CLOUD_TEST_REGISTRY_USERNAME
  A3S_CLOUD_TEST_REGISTRY_PASSWORD
  A3S_CLOUD_TEST_VAULT_ADDR
  A3S_CLOUD_TEST_VAULT_TOKEN
  A3S_CLOUD_TEST_VAULT_TRANSIT_MOUNT
  A3S_CLOUD_TEST_VAULT_TRANSIT_KEY
  A3S_CLOUD_TEST_G0_EVIDENCE_DIR
)
for environment_name in "${required_environment[@]}"; do
  [[ -n ${!environment_name:-} ]]
done
[[ $A3S_CLOUD_TEST_REGISTRY_URL == https://* ]]
[[ $A3S_CLOUD_TEST_VAULT_ADDR == https://* ]]
[[ $A3S_CLOUD_TEST_G0_EVIDENCE_DIR == /* ]]

gate_id=${A3S_CLOUD_TEST_GATE_ID:-manual}
[[ $gate_id =~ ^[a-zA-Z0-9][a-zA-Z0-9-]{0,62}$ ]]
gate_namespace="g0-signed-build-$gate_id"
buildkit_container="a3s-cloud-g0-buildkit-$gate_id"
busybox_container="a3s-cloud-g0-busybox-$gate_id"
postgres_container="a3s-cloud-g0-postgres-$gate_id"
buildkit_volume_id=a3s-cloud-buildkit-v0-31-2
buildkit_volume="a3s-${gate_namespace}-volume-$(printf '%s' "$buildkit_volume_id" | sha256sum | cut -c1-16)"
buildkit_image=docker.io/moby/buildkit@sha256:0eeb84626c0cd01aecae7848c5ed8f095aec279dd936d0cdb5a64110f42ca65b
busybox_image=docker.io/library/busybox@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662
postgres_image=docker.io/library/postgres:17-alpine@sha256:742f40ea20b9ff2ff31db5458d127452988a2164df9e17441e191f3b72252193
postgres_database=a3s_cloud
postgres_user=a3s_cloud
postgres_password="g0-$gate_id-postgres"
gate_temp=$(mktemp -d "${RUNNER_TEMP:-/tmp}/a3s-cloud-g0-signed-build.XXXXXX")
busybox_rootfs="$gate_temp/busybox-rootfs.tar"

cleanup() {
  managed_containers=$(docker ps --all --quiet \
    --filter label=a3s.cloud.managed=true \
    --filter "label=a3s.cloud.namespace=$gate_namespace" || true)
  if [[ -n $managed_containers ]]; then
    docker rm --force $managed_containers >/dev/null 2>&1 || true
  fi
  docker rm --force \
    "$buildkit_container" "$busybox_container" "$postgres_container" \
    >/dev/null 2>&1 || true
  docker volume rm "$buildkit_volume" >/dev/null 2>&1 || true
  if [[ -n ${gate_temp:-} && -d $gate_temp ]]; then
    rm -rf "$gate_temp"
  fi
}
trap cleanup EXIT

docker info >/dev/null
restriction=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
if [[ -r $restriction ]] && [[ $(<"$restriction") == 1 ]]; then
  sudo sysctl --write kernel.apparmor_restrict_unprivileged_userns=0
fi
[[ ! -r $restriction ]] || [[ $(<"$restriction") == 0 ]]

docker pull "$buildkit_image"
docker pull --platform linux/amd64 "$busybox_image"
docker pull "$postgres_image"
docker run --detach --rm \
  --name "$postgres_container" \
  --env "POSTGRES_DB=$postgres_database" \
  --env "POSTGRES_PASSWORD=$postgres_password" \
  --env "POSTGRES_USER=$postgres_user" \
  --publish 127.0.0.1::5432 \
  "$postgres_image" >/dev/null
postgres_ready=false
for _ in $(seq 1 90); do
  if docker exec "$postgres_container" \
    pg_isready --dbname "$postgres_database" --username "$postgres_user" \
    >/dev/null 2>&1; then
    postgres_ready=true
    break
  fi
  sleep 1
done
[[ $postgres_ready == true ]]
postgres_endpoint=$(docker port "$postgres_container" 5432/tcp)
postgres_port=${postgres_endpoint##*:}
[[ $postgres_port =~ ^[0-9]+$ ]]
export A3S_CLOUD_TEST_G0_POSTGRES_URL="postgres://$postgres_user:$postgres_password@127.0.0.1:$postgres_port/$postgres_database"

docker create --name "$busybox_container" --platform linux/amd64 "$busybox_image"
docker export --output "$busybox_rootfs" "$busybox_container"
docker rm "$busybox_container"
rootfs_bytes=$(wc -c <"$busybox_rootfs")
((rootfs_bytes > 0 && rootfs_bytes <= 16777216))
tar --list --file "$busybox_rootfs" \
  bin/busybox lib/ld-linux-x86-64.so.2 lib/libc.so.6 lib64 >/dev/null

docker volume create "$buildkit_volume" >/dev/null
docker run --rm --user 0 --entrypoint /bin/sh \
  --volume "$buildkit_volume:/run/user/1000/a3s-buildkit" \
  "$buildkit_image" \
  -ceu 'chown 1000:1000 /run/user/1000/a3s-buildkit'
docker run --detach --privileged --rm \
  --name "$buildkit_container" \
  --volume "$buildkit_volume:/run/user/1000/a3s-buildkit" \
  "$buildkit_image" \
  --addr unix:///run/user/1000/a3s-buildkit/buildkitd.sock \
  --oci-worker-no-process-sandbox >/dev/null

buildkit_ready=false
for _ in $(seq 1 90); do
  buildkit_state=$(docker inspect --format '{{.State.Status}}' "$buildkit_container")
  if [[ $buildkit_state == exited || $buildkit_state == dead ]]; then
    docker logs "$buildkit_container"
    exit 1
  fi
  if docker exec "$buildkit_container" /usr/bin/buildctl \
    --addr unix:///run/user/1000/a3s-buildkit/buildkitd.sock \
    debug workers >/dev/null 2>&1; then
    buildkit_ready=true
    break
  fi
  sleep 1
done
[[ $buildkit_ready == true ]]
[[ $(docker exec "$buildkit_container" id -u) == 1000 ]]

export A3S_CLOUD_TEST_CLOUD_REVISION
A3S_CLOUD_TEST_CLOUD_REVISION=$(git rev-parse HEAD)
[[ $A3S_CLOUD_TEST_CLOUD_REVISION =~ ^[0-9a-f]{40}$ ]]
export A3S_CLOUD_TEST_BUILDKIT_CONTAINER="$buildkit_container"
export A3S_CLOUD_TEST_BUSYBOX_ROOTFS="$busybox_rootfs"
export A3S_CLOUD_TEST_DOCKER_SOCKET=unix:///var/run/docker.sock
export A3S_CLOUD_TEST_G0_OPERATOR=1
export A3S_CLOUD_TEST_RUNTIME_BUILDKIT=1
export A3S_CLOUD_TEST_RUNTIME_BUILDKIT_NAMESPACE="$gate_namespace"
export A3S_CLOUD_TEST_RUNTIME_BUILDKIT_VOLUME_ID="$buildkit_volume_id"

cargo test -p a3s-cloud-control-plane --lib --locked \
  modules::artifacts::infrastructure::build_flow::tests::runtime_gate::real_runtime_task_builds_publishes_and_rejects_network_access \
  -- --ignored --exact --nocapture --test-threads=1

test -s "$A3S_CLOUD_TEST_G0_EVIDENCE_DIR/signed-build-evidence.json"
managed_containers=$(docker ps --all --quiet \
  --filter label=a3s.cloud.managed=true \
  --filter "label=a3s.cloud.namespace=$gate_namespace")
[[ -z $managed_containers ]]

cleanup
trap - EXIT
! docker container inspect "$buildkit_container" >/dev/null 2>&1
! docker container inspect "$postgres_container" >/dev/null 2>&1
! docker volume inspect "$buildkit_volume" >/dev/null 2>&1
