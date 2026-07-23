#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mode="${1:-dev}"

usage() {
  printf '%s\n' \
    'Usage: tools/dev/run_cloud.sh [dev|gateway]' \
    '' \
    '  dev      Start the Cloud API and hot-reloading Rsbuild web server.' \
    '  gateway  Build the SPA, start its production server, and expose both' \
    '           the SPA and API through A3S Gateway.'
}

case "$mode" in
  dev | gateway) ;;
  --help | -h)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'required command is unavailable: %s\n' "$command_name" >&2
    exit 1
  fi
}

skip_prepare="${A3S_CLOUD_DEV_TEST_SKIP_PREPARE:-false}"
if [[ $skip_prepare != true ]]; then
  require_command bun
  require_command cargo
fi

if [[ -z ${A3S_CLOUD_POSTGRES_URL:-} ]]; then
  require_command docker
  if ! docker info >/dev/null 2>&1; then
    printf '%s\n' \
      'Docker is unavailable. Start Docker or provide A3S_CLOUD_POSTGRES_URL' \
      'for an existing PostgreSQL instance.' >&2
    exit 1
  fi
  docker compose \
    --env-file "$repository_root/deploy/dev/.env.example" \
    --file "$repository_root/deploy/dev/compose.yaml" \
    up --detach --wait
  export A3S_CLOUD_POSTGRES_URL='postgres://a3s_cloud:a3s_cloud@127.0.0.1:54320/a3s_cloud'
fi

generated_bootstrap_token=false
if [[ -z ${A3S_CLOUD_BOOTSTRAP_TOKEN:-} ]]; then
  require_command openssl
  export A3S_CLOUD_BOOTSTRAP_TOKEN="$(openssl rand -hex 32)"
  generated_bootstrap_token=true
fi
if [[ -z ${A3S_CLOUD_GITHUB_WEBHOOK_SECRET:-} ]]; then
  require_command openssl
  export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="$(openssl rand -hex 32)"
fi

target_directory="${CARGO_TARGET_DIR:-$repository_root/target}"
if [[ $target_directory != /* ]]; then
  target_directory="$repository_root/$target_directory"
fi
if [[ $skip_prepare != true ]]; then
  (cd "$repository_root/web" && bun install --frozen-lockfile)
  (cd "$repository_root" && cargo build --locked -p a3s-cloud-control-plane)
fi
api_bin="${A3S_CLOUD_DEV_API_BIN:-$target_directory/debug/a3s-cloud-control-plane}"
web_dev_bin="${A3S_CLOUD_DEV_WEB_BIN:-$repository_root/web/node_modules/.bin/rsbuild}"
web_dev_argument="${A3S_CLOUD_DEV_WEB_ARGUMENT:-dev}"

gateway_bin=''
if [[ $mode == gateway ]]; then
  if [[ $skip_prepare == true ]]; then
    printf '%s\n' 'gateway mode cannot skip preparation' >&2
    exit 2
  fi
  (cd "$repository_root/web" && bun run build)
  (cd "$repository_root" && cargo build --locked -p a3s-cloud-web-server)
  gateway_bin="${A3S_GATEWAY_BIN:-}"
  if [[ -z $gateway_bin ]]; then
    gateway_bin="$(command -v a3s-gateway || true)"
  fi
  if [[ -z $gateway_bin || ! -x $gateway_bin ]]; then
    printf '%s\n' \
      'A3S Gateway is unavailable. Install a3s-gateway 1.0.12+ or set' \
      'A3S_GATEWAY_BIN to an executable path.' >&2
    exit 1
  fi
fi

run_directory="$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-dev.XXXXXX")"
completion_pipe="$run_directory/completed"
mkfifo "$completion_pipe"
exec 3<>"$completion_pipe"
pids=()
names=()

cleanup() {
  local status=$?
  trap - EXIT HUP INT TERM
  local pid
  for pid in "${pids[@]}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
  for pid in "${pids[@]}"; do
    wait "$pid" >/dev/null 2>&1 || true
  done
  exec 3>&-
  rm -rf "$run_directory"
  exit "$status"
}

trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

start_service() {
  local name="$1"
  shift
  local index="${#pids[@]}"
  (
    set +e
    service_pid=''
    forward_signal() {
      local signal_status="$1"
      trap - HUP INT TERM
      if [[ -n $service_pid ]]; then
        kill "$service_pid" >/dev/null 2>&1 || true
        wait "$service_pid" >/dev/null 2>&1 || true
      fi
      exit "$signal_status"
    }
    trap 'forward_signal 129' HUP
    trap 'forward_signal 130' INT
    trap 'forward_signal 143' TERM
    "$@" &
    service_pid=$!
    wait "$service_pid"
    service_status=$?
    printf '%s\t%s\n' "$index" "$service_status" >&3
    exit "$service_status"
  ) &
  pids+=("$!")
  names+=("$name")
}

run_api() {
  cd "$repository_root"
  exec "$api_bin" config/cloud.acl
}

run_dev_web() {
  cd "$repository_root/web"
  exec "$web_dev_bin" "$web_dev_argument"
}

run_production_web() {
  cd "$repository_root"
  exec "$target_directory/debug/a3s-cloud-web-server" \
    --listen 127.0.0.1:3011 \
    --root web/dist
}

run_gateway() {
  cd "$repository_root"
  exec "$gateway_bin" --config deploy/web/gateway.acl
}

start_service 'Cloud API' run_api
if [[ $mode == dev ]]; then
  start_service 'Cloud Web' run_dev_web
  printf '%s\n' \
    'A3S Cloud development services are starting:' \
    '  Web: http://127.0.0.1:3010' \
    '  API: http://127.0.0.1:8080/api/v1'
else
  start_service 'Cloud SPA server' run_production_web
  start_service 'A3S Gateway' run_gateway
  printf '%s\n' \
    'A3S Cloud Gateway delivery services are starting:' \
    '  Public origin: http://127.0.0.1:8088' \
    '  Private API:   http://127.0.0.1:8080/api/v1' \
    '  Private SPA:   http://127.0.0.1:3011'
fi

if [[ $generated_bootstrap_token == true ]]; then
  printf '  Bootstrap token for this run: %s\n' "$A3S_CLOUD_BOOTSTRAP_TOKEN"
fi
printf '%s\n' 'Press Ctrl-C to stop the API and web processes.'

while ! IFS=$'\t' read -r completed_index completed_status <&3; do
  :
done
printf '%s exited with status %s; stopping the remaining services.\n' \
  "${names[$completed_index]}" "$completed_status" >&2
exit "$completed_status"
