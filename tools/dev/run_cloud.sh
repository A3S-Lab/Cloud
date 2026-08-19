#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

usage() {
  printf '%s\n' \
    'Usage: tools/dev/run_cloud.sh' \
    '' \
    'Start the A3S Cloud control-plane API.'
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

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'required command is unavailable: %s\n' "$command_name" >&2
    exit 1
  fi
}

skip_prepare="${A3S_CLOUD_DEV_TEST_SKIP_PREPARE:-false}"
if [[ $skip_prepare != true ]]; then
  require_command cargo
fi

if [[ -z ${A3S_CLOUD_POSTGRES_URL:-} ]]; then
  require_command a3s-box
  if ! a3s-box info >/dev/null 2>&1; then
    printf '%s\n' \
      'A3S Box is unavailable. Install A3S Box or provide A3S_CLOUD_POSTGRES_URL' \
      'for an existing PostgreSQL instance.' >&2
    exit 1
  fi
  a3s-box compose \
    --file "$repository_root/deploy/dev/compose.acl" \
    up --detach --timeout 120
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
  (cd "$repository_root" && cargo build --locked -p a3s-cloud-control-plane)
fi
api_bin="${A3S_CLOUD_DEV_API_BIN:-$target_directory/debug/a3s-cloud-control-plane}"
migration_bin="${A3S_CLOUD_DEV_MIGRATION_BIN:-$target_directory/debug/a3s-cloud-migrate}"
cd "$repository_root"
"$migration_bin" config/cloud.acl
printf '%s\n' \
  'A3S Cloud control-plane API is starting:' \
  '  API: http://127.0.0.1:8080/api/v1'

if [[ $generated_bootstrap_token == true ]]; then
  printf '  Bootstrap token for this run: %s\n' "$A3S_CLOUD_BOOTSTRAP_TOKEN"
fi
exec "$api_bin" config/cloud.acl
