#!/usr/bin/env bash
# Stop the detached a3s-cloud control-plane and local development dependencies.
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
state_dir="${A3S_CLOUD_DEV_STATE_DIR:-$repository_root/.a3s/cloud/dev}"
pid_file="$state_dir/control-plane.pid"
meta_file="$state_dir/stack.meta"
compose_acl="$repository_root/deploy/dev/compose.acl"

usage() {
  printf '%s\n' \
    'Usage: tools/dev/cloud_down.sh' \
    '' \
    'Stop the detached control-plane API and tear down local Cloud dependencies.' \
    'Does not remove Postgres data volumes. Does not stop a3s-u03-pg (live-host prep).'
}

case "${1:-}" in
  '') ;;
  --help | -h)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

stop_control_plane() {
  if [[ ! -f $pid_file ]]; then
    printf 'no control-plane pid file (%s)\n' "$pid_file"
    return 0
  fi
  local pid
  pid=$(cat "$pid_file" 2>/dev/null || true)
  if [[ -z $pid ]]; then
    rm -f "$pid_file"
    return 0
  fi
  if kill -0 "$pid" 2>/dev/null; then
    printf 'stopping control-plane pid %s\n' "$pid"
    kill "$pid" 2>/dev/null || true
    local deadline=$((SECONDS + 15))
    while kill -0 "$pid" 2>/dev/null && ((SECONDS < deadline)); do
      sleep 0.2
    done
    if kill -0 "$pid" 2>/dev/null; then
      printf 'control-plane did not exit; sending SIGKILL\n' >&2
      kill -9 "$pid" 2>/dev/null || true
    fi
  else
    printf 'control-plane pid %s is not running\n' "$pid"
  fi
  rm -f "$pid_file"
}

read_meta_mode() {
  local mode=
  owned_containers=
  deps_mode=
  if [[ -f $meta_file ]]; then
    # shellcheck disable=SC1090
    source "$meta_file"
    deps_mode=${mode:-}
  fi
}

stop_docker_named() {
  local name=$1
  if ! command -v docker >/dev/null 2>&1; then
    return 0
  fi
  if docker ps -a --format '{{.Names}}' | grep -qx "$name"; then
    printf 'stopping docker container %s\n' "$name"
    docker stop "$name" >/dev/null 2>&1 || true
  fi
}

stop_deps() {
  read_meta_mode

  if [[ ${deps_mode:-} == a3s-box-compose ]] \
    || { [[ -z ${deps_mode:-} ]] && command -v a3s-box >/dev/null 2>&1 \
      && a3s-box info >/dev/null 2>&1; }; then
    if command -v a3s-box >/dev/null 2>&1 && a3s-box info >/dev/null 2>&1; then
      printf 'tearing down a3s-box compose dependencies\n'
      a3s-box compose --file "$compose_acl" down || true
      return 0
    fi
  fi

  if [[ ${deps_mode:-} == existing-url ]]; then
    printf 'leaving operator-supplied Postgres running\n'
    return 0
  fi

  # Docker fallback / unknown: stop standard compose.acl container names.
  # Never stop a3s-u03-pg (shared live-host prep).
  local names=(a3s-cloud-dev-nats a3s-cloud-dev-registry a3s-cloud-dev-postgres)
  if [[ -n ${owned_containers:-} ]]; then
    IFS=',' read -r -a owned <<<"$owned_containers"
    # Prefer owned list when present; still stop known compose names for restarts.
    names=("${owned[@]}" a3s-cloud-dev-nats a3s-cloud-dev-registry a3s-cloud-dev-postgres)
  fi

  # Deduplicate while preserving order
  local seen=
  local name
  for name in "${names[@]}"; do
    [[ -n $name ]] || continue
    [[ $name == a3s-u03-pg ]] && continue
    case " $seen " in
      *" $name "*) continue ;;
    esac
    seen+=" $name"
    stop_docker_named "$name"
  done
}

stop_control_plane
stop_deps
rm -f "$meta_file"
printf 'a3s-cloud is down\n'
