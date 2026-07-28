#!/usr/bin/env bash
set -euo pipefail
umask 077

readonly POSTGRES_IMAGE="docker.io/library/postgres:17-alpine@sha256:742f40ea20b9ff2ff31db5458d127452988a2164df9e17441e191f3b72252193"

cloud_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
evidence_directory="${1:-}"
scenario="${2:-cross-surface}"

[[ $# -le 2 ]] || {
  printf 'usage: %s EVIDENCE_DIRECTORY [cross-surface|management-mcp]\n' "$0" >&2
  exit 2
}

case "$scenario" in
cross-surface)
  scenario_evidence_name=cross-surface.json
  scenario_log_name=cross-surface-test.log
  ;;
management-mcp)
  scenario_evidence_name=management-mcp.json
  scenario_log_name=management-mcp-test.log
  ;;
*)
  printf 'unsupported C0 conformance scenario: %s\n' "$scenario" >&2
  exit 2
  ;;
esac

die() {
  printf 'C0 %s conformance failed: %s\n' "$scenario" "$1" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command is unavailable: $1"
}

for command_name in bun cargo curl docker git openssl python3; do
  require_command "$command_name"
done

[[ -n $evidence_directory ]] || die "an absolute evidence directory is required"
[[ $evidence_directory == /* ]] || die "evidence directory must be absolute"
if [[ -d $evidence_directory ]] &&
  [[ -n $(find "$evidence_directory" -mindepth 1 -maxdepth 1 -print -quit) ]]; then
  die "evidence directory must be empty"
fi
mkdir -p "$evidence_directory"

cloud_revision="$(git -C "$cloud_root" rev-parse HEAD)"
[[ $cloud_revision =~ ^[0-9a-f]{40}$ ]] || die "Cloud revision is not exact"
source_state=clean
if [[ -n $(git -C "$cloud_root" status --porcelain=v1) ]]; then
  [[ ${A3S_CLOUD_C0_ALLOW_DIRTY:-false} == true ]] || die "Cloud source tree is not clean"
  source_state=dirty-rehearsal
fi

runtime_revision="$(<"$cloud_root/tools/runtime-conformance/runtime-revision")"
[[ $runtime_revision =~ ^[0-9a-f]{40}$ ]] || die "Runtime revision is not exact"
runtime_checkout="$(cd "$cloud_root/../../crates/runtime" && pwd)"
[[ $(git -C "$runtime_checkout" rev-parse HEAD) == "$runtime_revision" ]] ||
  die "Runtime checkout does not match the pinned revision"

python3 - 8080 8443 <<'PY'
import socket
import sys

listeners = []
try:
    for raw_port in sys.argv[1:]:
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", int(raw_port)))
        listeners.append(listener)
finally:
    for listener in listeners:
        listener.close()
PY

run_directory="$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-c0.XXXXXX")"
[[ $run_directory == "${TMPDIR:-/tmp}"/a3s-cloud-c0.* ]] || die "temporary directory is invalid"
postgres_container="a3s-cloud-c0-${cloud_revision:0:8}-$$"
api_pid=''

cleanup() {
  local status=$?
  trap - EXIT HUP INT TERM
  if [[ -n $api_pid ]]; then
    kill "$api_pid" >/dev/null 2>&1 || true
    wait "$api_pid" >/dev/null 2>&1 || true
  fi
  if docker container inspect "$postgres_container" >/dev/null 2>&1; then
    docker logs "$postgres_container" >"$evidence_directory/postgres.log" 2>&1 || true
    docker rm --force "$postgres_container" >/dev/null 2>&1 || true
  fi
  if [[ -d $run_directory && $run_directory == "${TMPDIR:-/tmp}"/a3s-cloud-c0.* ]]; then
    rm -rf "$run_directory"
  fi
  exit "$status"
}

trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

target_directory="${CARGO_TARGET_DIR:-$cloud_root/target}"
if [[ $target_directory != /* ]]; then
  target_directory="$cloud_root/$target_directory"
fi

(
  cd "$cloud_root/packages/cloud-client"
  bun install --frozen-lockfile
)
if [[ $scenario == cross-surface ]]; then
  (
    cd "$cloud_root/cli"
    bun install --frozen-lockfile
    bun run build
  )
fi
(
  cd "$cloud_root"
  cargo build --locked -p a3s-cloud-control-plane
)

api_binary="$target_directory/debug/a3s-cloud-control-plane"
cli_binary="$cloud_root/cli/dist/a3s-cloud"
[[ -x $api_binary ]] || die "Cloud API binary was not built"
if [[ $scenario == cross-surface ]]; then
  [[ -x $cli_binary ]] || die "compiled Cloud CLI was not built"
fi

postgres_password="c0_$(openssl rand -hex 16)"
docker pull "$POSTGRES_IMAGE" >"$evidence_directory/postgres-pull.log"
docker run --detach --name "$postgres_container" --pull=never \
  --publish 127.0.0.1::5432 \
  --tmpfs /var/lib/postgresql/data:rw,nosuid,nodev,noexec,size=1073741824 \
  --env POSTGRES_DB=a3s_cloud \
  --env "POSTGRES_PASSWORD=$postgres_password" \
  --env POSTGRES_USER=a3s_cloud \
  "$POSTGRES_IMAGE" >"$evidence_directory/postgres.id"

postgres_port="$(docker inspect --format '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}' "$postgres_container")"
[[ $postgres_port =~ ^[0-9]+$ ]] || die "PostgreSQL host port is invalid"
# The image exposes a transient bootstrap postmaster before restarting as PID 1.
# Bind readiness to one stable server identity so migrations never race that restart.
postgres_ready=false
postgres_ready_identity=''
postgres_ready_streak=0
for _ in $(seq 1 60); do
  postgres_identity="$(docker exec "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command='select pg_postmaster_start_time()' 2>/dev/null || true)"
  if [[ -n $postgres_identity ]]; then
    if [[ $postgres_identity == "$postgres_ready_identity" ]]; then
      postgres_ready_streak=$((postgres_ready_streak + 1))
    else
      postgres_ready_identity="$postgres_identity"
      postgres_ready_streak=1
    fi
    if ((postgres_ready_streak >= 3)); then
      postgres_ready=true
      break
    fi
  else
    postgres_ready_identity=''
    postgres_ready_streak=0
  fi
  sleep 1
done
[[ $postgres_ready == true ]] || die "PostgreSQL did not become stably ready"
[[ -z $(docker inspect --format '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' "$postgres_container") ]] ||
  die "PostgreSQL unexpectedly owns an anonymous volume"
postgres_version="$(docker exec "$postgres_container" postgres --version)"
[[ $postgres_version == *"PostgreSQL) 17."* ]] || die "PostgreSQL major version is not 17"
printf '%s\n' "$postgres_version" >"$evidence_directory/postgres-version.txt"

bootstrap_token="$(openssl rand -hex 32)"
admin_token="a3s_$(openssl rand -hex 32)"
restricted_token="a3s_$(openssl rand -hex 32)"
github_webhook_secret="$(openssl rand -hex 32)"
postgres_url="postgres://a3s_cloud:$postgres_password@127.0.0.1:$postgres_port/a3s_cloud"

(
  cd "$run_directory"
  exec env \
    A3S_CLOUD_BOOTSTRAP_TOKEN="$bootstrap_token" \
    A3S_CLOUD_GITHUB_WEBHOOK_SECRET="$github_webhook_secret" \
    A3S_CLOUD_POSTGRES_URL="$postgres_url" \
    RUST_LOG=info \
    "$api_binary" "$cloud_root/config/cloud.acl"
) >"$evidence_directory/cloud-api.log" 2>&1 &
api_pid=$!

api_ready=false
for _ in $(seq 1 120); do
  if ! kill -0 "$api_pid" >/dev/null 2>&1; then
    die "Cloud API exited before readiness"
  fi
  if curl --fail --silent --show-error --max-time 2 \
    http://127.0.0.1:8080/api/v1/health/ready >/dev/null 2>&1; then
    api_ready=true
    break
  fi
  sleep 1
done
[[ $api_ready == true ]] || die "Cloud API did not become ready"

scenario_evidence="$evidence_directory/$scenario_evidence_name"
scenario_log="$evidence_directory/$scenario_log_name"
case "$scenario" in
cross-surface)
  (
    cd "$cloud_root/packages/cloud-client"
    A3S_CLOUD_C0_ADMIN_TOKEN="$admin_token" \
      A3S_CLOUD_C0_BASE_URL=http://127.0.0.1:8080/api/v1 \
      A3S_CLOUD_C0_BOOTSTRAP_TOKEN="$bootstrap_token" \
      A3S_CLOUD_C0_CLI_BIN="$cli_binary" \
      A3S_CLOUD_C0_CLOUD_REVISION="$cloud_revision" \
      A3S_CLOUD_C0_CONFORMANCE=1 \
      A3S_CLOUD_C0_EVIDENCE_FILE="$scenario_evidence" \
      A3S_CLOUD_C0_RESTRICTED_TOKEN="$restricted_token" \
      bun test src/cross-surface.test.ts --timeout 60000
  ) 2>&1 | tee "$scenario_log"
  ;;
management-mcp)
  (
    cd "$cloud_root/packages/cloud-client"
    A3S_CLOUD_C0_MCP_ADMIN_TOKEN="$admin_token" \
      A3S_CLOUD_C0_MCP_BASE_URL=http://127.0.0.1:8080/api/v1 \
      A3S_CLOUD_C0_MCP_BOOTSTRAP_TOKEN="$bootstrap_token" \
      A3S_CLOUD_C0_MCP_CLOUD_REVISION="$cloud_revision" \
      A3S_CLOUD_C0_MCP_CONFORMANCE=1 \
      A3S_CLOUD_C0_MCP_EVIDENCE_FILE="$scenario_evidence" \
      A3S_CLOUD_C0_MCP_READ_ONLY_TOKEN="$restricted_token" \
      bun test src/management-mcp-conformance.test.ts --timeout 60000
  ) 2>&1 | tee "$scenario_log"
  ;;
esac

[[ -s $scenario_evidence ]] || die "$scenario scenario did not write evidence"

admin_digest="sha256:$(printf '%s' "$admin_token" | openssl dgst -sha256 | awk '{print $NF}')"
restricted_digest="sha256:$(printf '%s' "$restricted_token" | openssl dgst -sha256 | awk '{print $NF}')"
stored_token_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
  psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
  --command="select count(*) from api_tokens where token_hash in ('$admin_digest', '$restricted_digest')")"
[[ $stored_token_count == 2 ]] || die "PostgreSQL did not retain exactly the two expected token digests"
revoked_token_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
  psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
  --command="select count(*) from api_tokens where token_hash = '$restricted_digest' and revoked_at is not null")"
[[ $revoked_token_count == 1 ]] || die "restricted token revocation was not durable"

if [[ $scenario == management-mcp ]]; then
  mcp_project_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="select count(*) from projects where name in ('MCP Conformance Project', 'MCP Foreign Project')")"
  [[ $mcp_project_count == 2 ]] || die "PostgreSQL did not retain the two expected MCP projects"
  mcp_environment_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="select count(*) from environments e join projects p on p.organization_id = e.organization_id and p.id = e.project_id where p.name = 'MCP Conformance Project' and e.name = 'MCP Operational Environment'")"
  [[ $mcp_environment_count == 1 ]] || die "PostgreSQL did not retain the expected MCP operational environment"
  hidden_project_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="select count(*) from projects where name = 'Hidden Mutation Must Not Exist'")"
  [[ $hidden_project_count == 0 ]] || die "hidden MCP mutation changed PostgreSQL state"
  mcp_idempotency_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="select count(*) from idempotency_records where idempotency_key = 'c0:mcp:rest-project'")"
  [[ $mcp_idempotency_count == 1 ]] || die "REST-to-MCP replay did not preserve one idempotency record"
  mcp_workload_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="select count(*) from workloads where name = 'mcp-stop' and desired_state = 'stopped'")"
  [[ $mcp_workload_count == 1 ]] || die "MCP Workload stop did not persist the expected desired state"
  mcp_workload_stop_idempotency_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="select count(*) from idempotency_records where idempotency_key = 'c0:mcp:workload-stop'")"
  [[ $mcp_workload_stop_idempotency_count == 1 ]] || die "MCP Workload stop replay did not preserve one idempotency record"
  read_only_scope_count="$(docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
    psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="select count(*) from api_tokens where token_hash = '$restricted_digest' and scopes = '[\"cloud:read\"]'::jsonb and revoked_at is not null")"
  [[ $read_only_scope_count == 1 ]] || die "read-only MCP scope or revocation was not durable"
fi

database_dump="$run_directory/postgres.sql"
docker exec --env "PGPASSWORD=$postgres_password" "$postgres_container" \
  pg_dump --dbname=a3s_cloud --username=a3s_cloud --data-only --no-owner --no-privileges \
  >"$database_dump" 2>"$evidence_directory/postgres-dump.log"
docker logs "$postgres_container" >"$evidence_directory/postgres.log" 2>&1

for credential in \
  "$bootstrap_token" "$admin_token" "$restricted_token" "$github_webhook_secret" "$postgres_password"; do
  for candidate in \
    "$database_dump" \
    "$evidence_directory/cloud-api.log" \
    "$scenario_log" \
    "$evidence_directory/postgres.log" \
    "$scenario_evidence"; do
    if grep --fixed-strings --quiet -- "$credential" "$candidate"; then
      die "credential material appeared in $(basename "$candidate")"
    fi
  done
done

{
  printf 'stored_api_token_digests=2\nrevoked_api_token_digests=1\nplaintext_credentials=0\n'
  if [[ $scenario == management-mcp ]]; then
    printf 'mcp_project_rows=2\nmcp_environment_rows=1\nmcp_stopped_workload_rows=1\nhidden_mutation_project_rows=0\nmcp_idempotency_rows=1\nmcp_workload_stop_idempotency_rows=1\nread_only_scope_rows=1\n'
  fi
} >"$evidence_directory/persistence-check.txt"

if [[ $scenario == cross-surface ]]; then
  printf 'A3S_CLOUD_C0_1_CROSS_SURFACE_PASS cloud=%s runtime=%s source=%s contract=1.0.0\n' \
    "$cloud_revision" "$runtime_revision" "$source_state" \
    | tee "$evidence_directory/result.txt"
else
  printf 'A3S_CLOUD_C0_2_MANAGEMENT_MCP_PASS cloud=%s runtime=%s source=%s protocol=2025-06-18 contract=1.0.0\n' \
    "$cloud_revision" "$runtime_revision" "$source_state" \
    | tee "$evidence_directory/result.txt"
fi
