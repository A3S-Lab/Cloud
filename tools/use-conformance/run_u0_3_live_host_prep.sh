#!/usr/bin/env bash
# U0.3 live-host operator prep (control-plane deps + printed next steps).
#
# Starts or checks Postgres, NATS JetStream, and the local OCI registry used by
# deploy/dev/compose.acl, builds the control-plane when needed, and prints the
# exact env/commands for tools/dev/run_cloud.sh plus Linux node-agent enrollment.
#
# This script NEVER claims A3S_CLOUD_U0_3_EXIT_CERTIFIED (or LIVE_HOST_CERTIFIED).
# Product exit still requires a real enrolled Linux host, Fleet long-poll, signed
# Use Registry enrollment, and collect_live_host_evidence.sh.
set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
compose_acl="$repository_root/deploy/dev/compose.acl"
tools="$repository_root/tools/use-conformance"

postgres_port=${A3S_CLOUD_POSTGRES_PORT:-54320}
nats_port=${A3S_CLOUD_NATS_PORT:-42220}
nats_monitor_port=${A3S_CLOUD_NATS_MONITOR_PORT:-8220}
registry_port=${A3S_CLOUD_REGISTRY_PORT:-50020}
pg_user=${A3S_CLOUD_POSTGRES_USER:-a3s_cloud}
pg_password=${A3S_CLOUD_POSTGRES_PASSWORD:-a3s_cloud}
pg_db=${A3S_CLOUD_POSTGRES_DB:-a3s_cloud}

# Known operator conformance container (docker mapping when a3s-box is absent).
legacy_pg_name=${A3S_CLOUD_U0_3_LEGACY_PG_NAME:-a3s-u03-pg}
legacy_pg_port=${A3S_CLOUD_U0_3_LEGACY_PG_PORT:-54329}
legacy_pg_user=${A3S_CLOUD_U0_3_LEGACY_PG_USER:-postgres}
legacy_pg_password=${A3S_CLOUD_U0_3_LEGACY_PG_PASSWORD:-a3s-cloud-conformance}
legacy_pg_db=${A3S_CLOUD_U0_3_LEGACY_PG_DB:-postgres}

skip_build=${A3S_CLOUD_DEV_TEST_SKIP_PREPARE:-false}
allow_partial=${A3S_CLOUD_U0_3_PREP_ALLOW_PARTIAL:-false}
partial_warnings=()
mode=unknown
postgres_url=${A3S_CLOUD_POSTGRES_URL:-}

usage() {
  printf '%s\n' \
    'Usage: bash tools/use-conformance/run_u0_3_live_host_prep.sh' \
    '' \
    'Prepare local U0.3 live-host dependencies and print next operator steps.' \
    'Does not claim EXIT_CERTIFIED.' \
    '' \
    'Env overrides:' \
    '  A3S_CLOUD_POSTGRES_URL   Prefer an existing Postgres URL (skips PG bring-up)' \
    '  A3S_CLOUD_POSTGRES_PORT / A3S_CLOUD_NATS_PORT / A3S_CLOUD_REGISTRY_PORT' \
    '  A3S_CLOUD_U0_3_LEGACY_PG_*  Mapping for existing docker container a3s-u03-pg' \
    '  A3S_CLOUD_DEV_TEST_SKIP_PREPARE=true  Skip cargo build'
    '  A3S_CLOUD_U0_3_PREP_ALLOW_PARTIAL=true  Continue if NATS/registry pull fails'
}

case "${1:-}" in
  '' ) ;;
  --help | -h )
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

port_open() {
  local host=$1
  local port=$2
  if command -v nc >/dev/null 2>&1; then
    nc -z "$host" "$port" >/dev/null 2>&1
  else
    (echo >/dev/tcp/"$host"/"$port") >/dev/null 2>&1
  fi
}

require_command() {
  local name=$1
  if ! command -v "$name" >/dev/null 2>&1; then
    printf 'required command is unavailable: %s\n' "$name" >&2
    exit 1
  fi
}

ensure_database_via_docker() {
  local container=$1
  local user=$2
  local db=$3
  if [[ $db == postgres ]]; then
    return 0
  fi
  if ! docker exec "$container" psql -U "$user" -tAc "SELECT 1 FROM pg_database WHERE datname='${db}'" | grep -q 1; then
    docker exec "$container" psql -U "$user" -c "CREATE DATABASE ${db}"
  fi
}

# Digests match deploy/dev/compose.acl (keep in sync).
NATS_IMAGE=${A3S_CLOUD_NATS_IMAGE:-nats:2.11-alpine@sha256:e4bf19f15fd3218814a4e3c9e0064e1334bd8aa20d5984b9f1a0afd084f8cc00}
REGISTRY_IMAGE=${A3S_CLOUD_REGISTRY_IMAGE:-registry:2.8.3@sha256:a3d8aaa63ed8681a604f1dea0aa03f100d5895b6a58ace528858a7b332415373}
POSTGRES_IMAGE=${A3S_CLOUD_POSTGRES_IMAGE:-postgres:17-alpine@sha256:742f40ea20b9ff2ff31db5458d127452988a2164df9e17441e191f3b72252193}


start_nats_registry_docker() {
  require_command docker

  if ! docker ps --format '{{.Names}}' | grep -qx 'a3s-cloud-dev-nats'; then
    if docker ps -a --format '{{.Names}}' | grep -qx 'a3s-cloud-dev-nats'; then
      docker start a3s-cloud-dev-nats >/dev/null
    elif docker image inspect "$NATS_IMAGE" >/dev/null 2>&1; then
      printf 'starting NATS (%s) on %s/%s
' "$NATS_IMAGE" "$nats_port" "$nats_monitor_port"
      docker run -d --name a3s-cloud-dev-nats \
        -p "${nats_port}:4222" -p "${nats_monitor_port}:8222" \
        "$NATS_IMAGE" \
        --jetstream --store_dir=/data --http_port=8222 >/dev/null
    else
      msg="image not present locally: $NATS_IMAGE (pull required; Docker Hub may be unreachable)"
      if [[ $allow_partial == true ]]; then
        partial_warnings+=("$msg")
        printf 'WARNING: %s
' "$msg" >&2
      else
        printf '%s
' "$msg" \
          'Pull manually, or set A3S_CLOUD_U0_3_PREP_ALLOW_PARTIAL=true to print Postgres-only next steps.' >&2
        exit 1
      fi
    fi
  fi

  if ! docker ps --format '{{.Names}}' | grep -qx 'a3s-cloud-dev-registry'; then
    if docker ps -a --format '{{.Names}}' | grep -qx 'a3s-cloud-dev-registry'; then
      docker start a3s-cloud-dev-registry >/dev/null
    elif docker image inspect "$REGISTRY_IMAGE" >/dev/null 2>&1; then
      printf 'starting OCI registry (%s) on %s
' "$REGISTRY_IMAGE" "$registry_port"
      docker run -d --name a3s-cloud-dev-registry \
        -p "${registry_port}:5000" \
        "$REGISTRY_IMAGE" >/dev/null
    else
      msg="image not present locally: $REGISTRY_IMAGE (pull required; Docker Hub may be unreachable)"
      if [[ $allow_partial == true ]]; then
        partial_warnings+=("$msg")
        printf 'WARNING: %s
' "$msg" >&2
      else
        printf '%s
' "$msg" \
          'Pull manually, or set A3S_CLOUD_U0_3_PREP_ALLOW_PARTIAL=true to print Postgres-only next steps.' >&2
        exit 1
      fi
    fi
  fi
}

bring_up_deps() {
  if [[ -n $postgres_url ]]; then
    mode=existing-postgres-url
    start_nats_registry_docker
    return 0
  fi

  if command -v a3s-box >/dev/null 2>&1 && a3s-box info >/dev/null 2>&1; then
    mode=a3s-box-compose
    a3s-box compose --file "$compose_acl" up --detach --timeout 120
    postgres_url="postgres://${pg_user}:${pg_password}@127.0.0.1:${postgres_port}/${pg_db}"
    return 0
  fi

  require_command docker
  mode=docker-fallback

  if docker ps --format '{{.Names}}' | grep -qx "$legacy_pg_name"; then
    postgres_url="postgres://${legacy_pg_user}:${legacy_pg_password}@127.0.0.1:${legacy_pg_port}/${legacy_pg_db}"
    printf 'reusing docker Postgres container %s on port %s\n' "$legacy_pg_name" "$legacy_pg_port"
  elif port_open 127.0.0.1 "$postgres_port"; then
    postgres_url="postgres://${pg_user}:${pg_password}@127.0.0.1:${postgres_port}/${pg_db}"
    printf 'reusing Postgres already listening on %s\n' "$postgres_port"
  else
    if docker ps -a --format '{{.Names}}' | grep -qx 'a3s-cloud-dev-postgres'; then
      docker start a3s-cloud-dev-postgres >/dev/null
    else
      if ! docker image inspect "$POSTGRES_IMAGE" >/dev/null 2>&1; then
        printf 'image not present locally: %s\n' "$POSTGRES_IMAGE" >&2
        exit 1
      fi
      docker run -d --name a3s-cloud-dev-postgres \
        -e POSTGRES_USER="$pg_user" \
        -e POSTGRES_PASSWORD="$pg_password" \
        -e POSTGRES_DB="$pg_db" \
        -p "${postgres_port}:5432" \
        "$POSTGRES_IMAGE" >/dev/null
    fi
    for _ in $(seq 1 60); do
      if docker exec a3s-cloud-dev-postgres pg_isready -U "$pg_user" -d "$pg_db" >/dev/null 2>&1; then
        break
      fi
      sleep 1
    done
    postgres_url="postgres://${pg_user}:${pg_password}@127.0.0.1:${postgres_port}/${pg_db}"
  fi

  start_nats_registry_docker
}

wait_for_ports() {
  local deadline=$((SECONDS + 90))
  local pg_host=127.0.0.1
  local pg_listen=$postgres_port
  if [[ $postgres_url == *":${legacy_pg_port}/"* ]]; then
    pg_listen=$legacy_pg_port
  elif [[ $postgres_url =~ @[^:]+:([0-9]+)/ ]]; then
    pg_listen=${BASH_REMATCH[1]}
  fi

  # Do not spin for 90s when optional containers were deliberately skipped.
  if [[ $allow_partial == true ]]; then
    if ! docker ps --format '{{.Names}}' 2>/dev/null | grep -qx 'a3s-cloud-dev-nats' \
      || ! docker ps --format '{{.Names}}' 2>/dev/null | grep -qx 'a3s-cloud-dev-registry'; then
      deadline=$((SECONDS + 5))
    fi
  fi

  while ((SECONDS < deadline)); do
    local ok=1
    port_open "$pg_host" "$pg_listen" || ok=0
    port_open 127.0.0.1 "$nats_port" || ok=0
    port_open 127.0.0.1 "$registry_port" || ok=0
    if ((ok == 1)); then
      return 0
    fi
    sleep 1
  done
  if [[ $allow_partial == true ]]; then
    partial_warnings+=("timed out waiting for postgres=$pg_listen nats=$nats_port registry=$registry_port")
    printf 'WARNING: dependency ports incomplete; continuing due to A3S_CLOUD_U0_3_PREP_ALLOW_PARTIAL=true\n' >&2
    return 0
  fi
  printf 'timed out waiting for postgres=%s nats=%s registry=%s\n' \
    "$pg_listen" "$nats_port" "$registry_port" >&2
  exit 1
}

build_control_plane() {
  if [[ $skip_build == true ]]; then
    printf 'skipping cargo build (A3S_CLOUD_DEV_TEST_SKIP_PREPARE=true)\n'
    return 0
  fi
  require_command cargo
  (
    cd "$repository_root"
    cargo build --locked -p a3s-cloud-control-plane -p a3s-cloud-migrate
  )
}

print_next_steps() {
  local target_directory=${CARGO_TARGET_DIR:-$repository_root/target}
  if [[ $target_directory != /* ]]; then
    target_directory="$repository_root/$target_directory"
  fi
  local api_bin=${A3S_CLOUD_DEV_API_BIN:-$target_directory/debug/a3s-cloud-control-plane}
  local agent_bin="$target_directory/debug/a3s-cloud-node-agent"
  local host_os
  host_os=$(uname -s)

  printf '\n'
  if ((${#partial_warnings[@]} > 0)); then
    printf 'partial_warnings=%s\n' "${#partial_warnings[@]}"
    for w in "${partial_warnings[@]}"; do
      printf '  - %s\n' "$w"
    done
  fi
  printf '%s\n' \
    '===== U0.3 live-host prep summary =====' \
    "mode=$mode" \
    "compose=$compose_acl" \
    "postgres_url=$postgres_url" \
    "nats=127.0.0.1:${nats_port} (monitor ${nats_monitor_port})" \
    "registry=127.0.0.1:${registry_port}" \
    "host_os=$host_os" \
    '' \
    'This prep does NOT claim A3S_CLOUD_U0_3_EXIT_CERTIFIED.' \
    'Signed Use Registry enrollment + an enrolled Linux node-agent are still required for a real assignment.' \
    '' \
    '1) Start control-plane (this host is enough for the API):' \
    "  export A3S_CLOUD_POSTGRES_URL='$postgres_url'" \
    '  # optional: export A3S_CLOUD_BOOTSTRAP_TOKEN=... A3S_CLOUD_GITHUB_WEBHOOK_SECRET=...' \
    '  bash tools/dev/run_cloud.sh' \
    '' \
    "  # binary path (after build): $api_bin" \
    '' \
    '2) Fleet long-poll node-agent (Linux ONLY; a3s-box is the sole Runtime provider):' \
    "  # binary: $agent_bin" \
    '  # usage: a3s-cloud-node-agent /absolute/path/to/node-config.acl' \
    '  # start from config/node.example.acl after replacing enrollment/node_control URLs,' \
    '  # CA path, and exporting A3S_CLOUD_ENROLLMENT_TOKEN from nodes bootstrap.' \
    '  # Darwin hosts cannot run the agent binary; use a Linux worker with a3s-box.' \
    '' \
    '3) Operator enrollment / assignment (high level):' \
    '  - Bootstrap a node (CLI nodes bootstrap ... --enrollment-token-stdin ...)' \
    '  - Enroll the Linux agent with the one-time token' \
    '  - Enroll a signed Use Registry (public HTTPS fixture/revision pin; never fake EXIT)' \
    '  - Create a plugin assignment and wait for Fleet plan/apply/observe digests' \
    '' \
    '4) After a real converge, collect evidence (still not EXIT by itself):' \
    "  bash $tools/collect_live_host_evidence.sh \\" \
    '    --host HOST --assignment ID --package ID --plan-digest DIGEST' \
    '  A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION=<cert> \' \
    "    bash $tools/run_u0_3_exit_audit.sh <evidence-dir>"
}

bring_up_deps
wait_for_ports
build_control_plane
print_next_steps
