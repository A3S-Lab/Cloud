#!/usr/bin/env bash
# GA-0 / BX0.software LIVE stack prep on a Docker-free Linux host.
#
# Brings up Postgres/NATS/registry via a3s-box compose only, migrates, and
# starts the control-plane API. Never uses Docker fallback. Never claims
# LOOP or EXIT.
#
# Usage:
#   bash tools/box-conformance/run_bx0_software_exit_live_prep.sh [EVIDENCE_DIR]
#
# After prep, set org/project/environment + OCI digests + HEALTH/HTTPS, then:
#   bash tools/box-conformance/run_bx0_software_exit_live.sh

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
compose_acl="${A3S_CLOUD_BX0_LIVE_COMPOSE_ACL:-$repository_root/deploy/dev/compose.bx0-live.acl}"
if [[ ! -f $compose_acl ]]; then
  compose_acl="$repository_root/deploy/dev/compose.acl"
fi
state_dir="${A3S_CLOUD_BX0_LIVE_PREP_STATE_DIR:-$repository_root/.a3s/cloud/bx0-live-prep}"
pid_file="$state_dir/control-plane.pid"
log_file="$state_dir/control-plane.log"
env_file="$state_dir/stack.env"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-live-prep.XXXXXX")
fi
mkdir -p -- "$evidence_directory" "$state_dir"

fail_blocked() {
  local reason=$1
  shift || true
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-prep.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_PREP_BLOCKED reason=$reason
$*
product_exit=not_claimed
loop_certified=not_claimed
EOF
  exit 2
}

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
bx0_refuse_docker_host
if [[ $(uname -s) != Linux || $(uname -m) != x86_64 ]]; then
  fail_blocked host_unsupported "got=$(uname -s)-$(uname -m)"
fi
if [[ -f $repository_root/tools/power-conformance/power-revision ]]; then
  fail_blocked invented_power_pin
fi

box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
if [[ -z $box_bin || ! -x $box_bin ]]; then
  fail_blocked a3s_box_unavailable \
    "hint=bash tools/box-conformance/install_box_release.sh /abs/empty/dir && export A3S_CLOUD_BOX_BIN=..."
fi
export A3S_CLOUD_BOX_BIN=$box_bin
if ! "$box_bin" info >/dev/null 2>&1; then
  fail_blocked a3s_box_info_failed
fi
if [[ ! -f $compose_acl ]]; then
  fail_blocked compose_acl_missing "path=$compose_acl"
fi
for cmd in cargo openssl; do
  command -v "$cmd" >/dev/null 2>&1 || fail_blocked "${cmd}_unavailable"
done

pg_user=${A3S_CLOUD_POSTGRES_USER:-a3s_cloud}
pg_password=${A3S_CLOUD_POSTGRES_PASSWORD:-a3s_cloud}
pg_db=${A3S_CLOUD_POSTGRES_DB:-a3s_cloud}
postgres_port=${A3S_CLOUD_POSTGRES_PORT:-54320}
api_port=${A3S_CLOUD_API_PORT:-8080}

printf '===== BX0.software live prep (a3s-box compose only) =====\n'
printf 'box=%s\n' "$box_bin"
printf 'compose=%s\n' "$compose_acl"

"$box_bin" compose --file "$compose_acl" up --detach --timeout 120 \
  >"$evidence_directory/compose-up.out" 2>"$evidence_directory/compose-up.err" \
  || fail_blocked a3s_box_compose_failed

port_reachable() {
  local host=$1
  local port=$2
  if command -v nc >/dev/null 2>&1; then
    nc -z "$host" "$port" >/dev/null 2>&1
  else
    (echo >"/dev/tcp/${host}/${port}") >/dev/null 2>&1
  fi
}

# Host port_map can drop while boxes stay healthy; compose restart rebinds it.
wait_host_postgres() {
  local seconds=$1
  local deadline=$((SECONDS + seconds))
  while ((SECONDS < deadline)); do
    port_reachable 127.0.0.1 "$postgres_port" && return 0
    sleep 1
  done
  return 1
}

postgres_host=127.0.0.1
postgres_connect_port=$postgres_port
# Prefer loopback after an explicit rebind — WSL→guest-CNI Postgres often hangs.
if ! wait_host_postgres 15; then
  printf '%s\n' 'host postgres port missing; restarting compose to rebind port_map' \
    | tee "$evidence_directory/compose-port-rebind.txt"
  "$box_bin" compose --file "$compose_acl" restart \
    >"$evidence_directory/compose-restart.out" 2>"$evidence_directory/compose-restart.err" \
    || fail_blocked a3s_box_compose_restart_failed
fi
if ! wait_host_postgres 60; then
  printf '%s\n' 'host postgres still missing after restart; compose down/up' \
    | tee -a "$evidence_directory/compose-port-rebind.txt"
  "$box_bin" compose --file "$compose_acl" down \
    >>"$evidence_directory/compose-restart.out" 2>>"$evidence_directory/compose-restart.err" || true
  "$box_bin" compose --file "$compose_acl" up --detach --timeout 180 \
    >>"$evidence_directory/compose-restart.out" 2>>"$evidence_directory/compose-restart.err" \
    || fail_blocked a3s_box_compose_up_failed
fi
if ! wait_host_postgres 90; then
  fail_blocked postgres_not_ready "port=$postgres_port"
fi
printf 'postgres_via_host=127.0.0.1:%s\n' "$postgres_port" \
  | tee "$evidence_directory/compose-guest-fallback.txt"

# Registry host publish can drop independently of Postgres. Prefer loopback,
# then guest :5000 — never leave OCI pointing at a dead 127.0.0.1:50020.
registry_port=${A3S_CLOUD_REGISTRY_PORT:-50020}
if port_reachable 127.0.0.1 "$registry_port"; then
  export A3S_CLOUD_BX0_OCI_REGISTRY="127.0.0.1:${registry_port}"
else
  guest_hosts=${guest_hosts:-$("$box_bin" exec dev-postgres -- cat /etc/hosts 2>/dev/null || true)}
  registry_guest_ip=$(printf '%s\n' "$guest_hosts" | awk '$2 == "dev-registry" { print $1; exit }')
  if [[ -n $registry_guest_ip ]] && port_reachable "$registry_guest_ip" 5000; then
    export A3S_CLOUD_BX0_OCI_REGISTRY="${registry_guest_ip}:5000"
  else
    fail_blocked registry_not_ready \
      "host=127.0.0.1:${registry_port} guest=${registry_guest_ip:-unset}:5000"
  fi
fi
nats_port=${A3S_CLOUD_NATS_PORT:-42220}
if ! port_reachable 127.0.0.1 "$nats_port"; then
  guest_hosts=${guest_hosts:-$("$box_bin" exec dev-postgres -- cat /etc/hosts 2>/dev/null || true)}
  nats_guest_ip=$(printf '%s\n' "$guest_hosts" | awk '$2 == "dev-nats" { print $1; exit }')
  if [[ -n $nats_guest_ip ]]; then
    export A3S_CLOUD_NATS_URL="nats://${nats_guest_ip}:4222"
  fi
fi
printf 'oci_registry=%s nats=%s\n' \
  "${A3S_CLOUD_BX0_OCI_REGISTRY:-}" "${A3S_CLOUD_NATS_URL:-}" \
  | tee "$evidence_directory/compose-endpoints.txt"

migration_postgres_url="postgres://${pg_user}:${pg_password}@${postgres_host}:${postgres_connect_port}/${pg_db}"
# Control-plane serving URL uses the ACL serving_role (created by compose init).
serving_postgres_url="postgres://a3s_cloud_serving:${pg_password}@${postgres_host}:${postgres_connect_port}/${pg_db}"
postgres_url=$serving_postgres_url

if [[ -z ${A3S_CLOUD_BOOTSTRAP_TOKEN:-} ]]; then
  bootstrap_token=$(openssl rand -hex 32)
else
  bootstrap_token=$A3S_CLOUD_BOOTSTRAP_TOKEN
fi
if [[ -z ${A3S_CLOUD_GITHUB_WEBHOOK_SECRET:-} ]]; then
  webhook_secret=$(openssl rand -hex 32)
else
  webhook_secret=$A3S_CLOUD_GITHUB_WEBHOOK_SECRET
fi
if [[ -z ${A3S_GATEWAY_ADMIN_TOKEN:-} ]]; then
  gateway_admin_token=$(openssl rand -hex 32)
else
  gateway_admin_token=$A3S_GATEWAY_ADMIN_TOKEN
fi

export A3S_CLOUD_POSTGRES_URL=$postgres_url
export A3S_CLOUD_POSTGRES_MIGRATION_URL=$migration_postgres_url
export A3S_CLOUD_BOOTSTRAP_TOKEN=$bootstrap_token
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET=$webhook_secret
export A3S_GATEWAY_ADMIN_TOKEN=$gateway_admin_token

target_directory="${CARGO_TARGET_DIR:-$repository_root/target}"
if [[ $target_directory != /* ]]; then
  target_directory="$repository_root/$target_directory"
fi
skip_prepare=${A3S_CLOUD_DEV_TEST_SKIP_PREPARE:-false}
if [[ $skip_prepare != true ]]; then
  (cd "$repository_root" && cargo build --locked -p a3s-cloud-control-plane) \
    || fail_blocked control_plane_build_failed
fi
api_bin="${A3S_CLOUD_DEV_API_BIN:-$target_directory/debug/a3s-cloud-control-plane}"
migration_bin="${A3S_CLOUD_DEV_MIGRATION_BIN:-$target_directory/debug/a3s-cloud-migrate}"
[[ -x $api_bin ]] || fail_blocked control_plane_bin_missing "path=$api_bin"
[[ -x $migration_bin ]] || fail_blocked migrate_bin_missing "path=$migration_bin"

cd "$repository_root"
# Drvfs (/mnt/d) cannot enforce 0600 on local signing keys; use a Linux-native
# security state_dir for LIVE prep so LocalBuildEvidenceSigner can start.
security_state_dir="${A3S_CLOUD_BX0_LIVE_SECURITY_STATE_DIR:-/tmp/a3s-cloud-bx0-security}"
mkdir -p -- "$security_state_dir"
live_cloud_acl="$state_dir/cloud.live.acl"
# Keep local CA / signing material on a Linux-native filesystem (drvfs cannot
# enforce 0600, and node-control paths must share the same CA tree as state_dir).
sed \
  -e "s#state_dir = \".a3s/cloud/security\"#state_dir = \"$security_state_dir\"#" \
  -e "s#\".a3s/cloud/security/#\"$security_state_dir/#g" \
  "$repository_root/config/cloud.acl" >"$live_cloud_acl" \
  || fail_blocked live_cloud_acl_rewrite_failed
# Drop stale CA/key material so node-control client CA matches local node CA.
rm -rf -- "$security_state_dir"
mkdir -p -- "$security_state_dir"

A3S_CLOUD_POSTGRES_MIGRATION_URL="$migration_postgres_url" \
  A3S_CLOUD_POSTGRES_URL="$postgres_url" \
  "$migration_bin" "$live_cloud_acl" \
  >"$evidence_directory/migrate.out" 2>"$evidence_directory/migrate.err" \
  || {
    printf '%s\n' 'migrate failed once; ensuring serving role and retrying' \
      | tee "$evidence_directory/migrate-retry.txt"
    "$box_bin" exec dev-postgres -- \
      gosu bx0 psql -U "$pg_user" -d "$pg_db" \
      -c "DO \$\$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'a3s_cloud_serving') THEN CREATE ROLE a3s_cloud_serving LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD '$pg_password'; END IF; END \$\$;" \
      >>"$evidence_directory/migrate-retry.txt" 2>&1 || true
    sleep 3
    A3S_CLOUD_POSTGRES_MIGRATION_URL="$migration_postgres_url" \
      A3S_CLOUD_POSTGRES_URL="$postgres_url" \
      "$migration_bin" "$live_cloud_acl" \
      >"$evidence_directory/migrate.out" 2>"$evidence_directory/migrate.err" \
      || fail_blocked migrate_failed
  }

# Stop previous prep control-plane if we own the pid file
if [[ -f $pid_file ]]; then
  old_pid=$(<"$pid_file")
  if kill -0 "$old_pid" 2>/dev/null; then
    kill "$old_pid" 2>/dev/null || true
    sleep 1
  fi
fi

umask 077
cat >"$env_file" <<EOF
# Generated by run_bx0_software_exit_live_prep.sh — not LOOP/EXIT
export A3S_CLOUD_URL='http://127.0.0.1:${api_port}/api/v1'
export A3S_CLOUD_POSTGRES_URL='$postgres_url'
export A3S_CLOUD_POSTGRES_MIGRATION_URL='$migration_postgres_url'
export A3S_CLOUD_BOOTSTRAP_TOKEN='$bootstrap_token'
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET='$webhook_secret'
export A3S_GATEWAY_ADMIN_TOKEN='$gateway_admin_token'
export A3S_CLOUD_BOX_BIN='$box_bin'
export A3S_CLOUD_CONTROL_PLANE_BIN='$api_bin'
EOF
if [[ -n ${A3S_CLOUD_BX0_OCI_REGISTRY:-} ]]; then
  printf "export A3S_CLOUD_BX0_OCI_REGISTRY='%s'\n" "$A3S_CLOUD_BX0_OCI_REGISTRY" >>"$env_file"
fi
if [[ -n ${A3S_CLOUD_NATS_URL:-} ]]; then
  printf "export A3S_CLOUD_NATS_URL='%s'\n" "$A3S_CLOUD_NATS_URL" >>"$env_file"
fi

nohup env \
  A3S_CLOUD_POSTGRES_URL="$postgres_url" \
  A3S_CLOUD_BOOTSTRAP_TOKEN="$bootstrap_token" \
  A3S_CLOUD_GITHUB_WEBHOOK_SECRET="$webhook_secret" \
  A3S_GATEWAY_ADMIN_TOKEN="$gateway_admin_token" \
  "$api_bin" "$live_cloud_acl" \
  >>"$log_file" 2>&1 &
echo $! >"$pid_file"

deadline=$((SECONDS + 90))
api_up=0
while ((SECONDS < deadline)); do
  if command -v nc >/dev/null 2>&1; then
    if nc -z 127.0.0.1 "$api_port" >/dev/null 2>&1; then
      api_up=1
      break
    fi
  elif (echo >/dev/tcp/127.0.0.1/"$api_port") >/dev/null 2>&1; then
    api_up=1
    break
  fi
  if ! kill -0 "$(cat "$pid_file")" 2>/dev/null; then
    fail_blocked control_plane_exited "log=$log_file"
  fi
  sleep 0.5
done
if ((api_up != 1)); then
  fail_blocked api_not_ready "port=$api_port log=$log_file"
fi

cp -- "$env_file" "$evidence_directory/bx0-live-prep-env.sh"

cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-prep.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_PREP_OK
cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
api=http://127.0.0.1:${api_port}/api/v1
pid=$(cat "$pid_file")
env_file=$env_file
deps=a3s-box-compose
product_exit=not_claimed
loop_certified=not_claimed
honesty=Docker refused; middleware via a3s-box only. Still need org/project/env token, pre-published OCI, HEALTH/HTTPS, then run_bx0_software_exit_live.sh.
EOF

printf '%s\n' \
  'Live prep ready (not EXIT):' \
  "  source $env_file" \
  '  # create org/project/environment + API token via bootstrap/CLI' \
  '  # export node.acl, agent release, OCI digests, HEALTH_URL, HTTPS_URL' \
  '  bash tools/box-conformance/run_bx0_software_exit_live.sh'
exit 0
