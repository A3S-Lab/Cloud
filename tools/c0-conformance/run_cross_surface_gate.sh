#!/usr/bin/env bash
set -euo pipefail
umask 077

readonly POSTGRES_IMAGE="docker.io/library/postgres:17-alpine@sha256:742f40ea20b9ff2ff31db5458d127452988a2164df9e17441e191f3b72252193"
readonly POSTGRES_PORT=54320

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

for command_name in bun cargo curl git openssl python3; do
  require_command "$command_name"
done
box_binary="${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}"
[[ -n $box_binary && -x $box_binary ]] || die "A3S Box is unavailable"
expected_box_revision="$(<"$cloud_root/tools/box-conformance/box-revision")"
[[ $expected_box_revision =~ ^[0-9a-f]{40}$ ]] || die "Box revision is not exact"
box_revision="${A3S_CLOUD_BOX_REVISION:-}"
box_revision_file="$(dirname "$box_binary")/BOX-REVISION"
if [[ -z $box_revision && -f $box_revision_file ]]; then
  box_revision="$(<"$box_revision_file")"
fi
[[ $box_revision == "$expected_box_revision" ]] ||
  die "Box fixture does not match the pinned revision"
if [[ ${A3S_CLOUD_BOX_RUN_AS_ROOT:-false} == true ]]; then
  require_command sudo
fi

run_box() {
  if [[ ${A3S_CLOUD_BOX_RUN_AS_ROOT:-false} == true ]]; then
    sudo env \
      A3S_HOME="$A3S_HOME" \
      A3S_BOX_OCI_AGENT_PATH="${A3S_BOX_OCI_AGENT_PATH:-}" \
      A3S_BOX_OCI_RUNTIME_PATH="${A3S_BOX_OCI_RUNTIME_PATH:-}" \
      LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}" \
      "$box_binary" "$@"
  else
    "$box_binary" "$@"
  fi
}

run_box_forwarder() {
  if [[ ${A3S_CLOUD_BOX_RUN_AS_ROOT:-false} == true ]]; then
    exec sudo env \
      A3S_HOME="$A3S_HOME" \
      A3S_BOX_OCI_AGENT_PATH="${A3S_BOX_OCI_AGENT_PATH:-}" \
      A3S_BOX_OCI_RUNTIME_PATH="${A3S_BOX_OCI_RUNTIME_PATH:-}" \
      LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}" \
      "$box_binary" "$@"
  else
    exec "$box_binary" "$@"
  fi
}

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

python3 - 8080 8443 "$POSTGRES_PORT" <<'PY'
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
chmod 0711 "$run_directory"
export A3S_HOME="$run_directory/box"
postgres_box="a3s-cloud-c0-${cloud_revision:0:8}-$$"
api_pid=''
port_forward_pid=''

cleanup() {
  local status=$?
  trap - EXIT HUP INT TERM
  if [[ -n $api_pid ]]; then
    kill "$api_pid" >/dev/null 2>&1 || true
    wait "$api_pid" >/dev/null 2>&1 || true
  fi
  if [[ -n $port_forward_pid ]]; then
    kill "$port_forward_pid" >/dev/null 2>&1 || true
    wait "$port_forward_pid" >/dev/null 2>&1 || true
  fi
  run_box logs "$postgres_box" >"$evidence_directory/postgres.log" 2>&1 || true
  run_box rm --force "$postgres_box" >/dev/null 2>&1 || true
  if [[ -d $run_directory && $run_directory == "${TMPDIR:-/tmp}"/a3s-cloud-c0.* ]]; then
    if [[ ${A3S_CLOUD_BOX_RUN_AS_ROOT:-false} == true ]]; then
      sudo rm -rf "$run_directory"
    else
      rm -rf "$run_directory"
    fi
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
run_box pull "$POSTGRES_IMAGE" >"$evidence_directory/postgres-pull.log"
run_box run "$POSTGRES_IMAGE" \
  --isolation sandbox \
  --detach \
  --name "$postgres_box" \
  --env POSTGRES_DB=a3s_cloud \
  --env "POSTGRES_PASSWORD=$postgres_password" \
  --env POSTGRES_USER=a3s_cloud \
  >"$evidence_directory/postgres.id"

postgres_query() {
  run_box exec "$postgres_box" \
    --env "PGPASSWORD=$postgres_password" \
    -- psql --dbname=a3s_cloud --username=a3s_cloud --tuples-only --no-align \
    --command="$1"
}

# The image exposes a transient bootstrap postmaster before restarting as PID 1.
# Bind readiness to one stable server identity so migrations never race that restart.
postgres_ready=false
postgres_ready_identity=''
postgres_ready_streak=0
for _ in $(seq 1 60); do
  postgres_identity="$(postgres_query 'select pg_postmaster_start_time()' 2>/dev/null || true)"
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
postgres_version="$(run_box exec "$postgres_box" -- postgres --version)"
[[ $postgres_version == *"PostgreSQL) 17."* ]] || die "PostgreSQL major version is not 17"
printf '%s\n' "$postgres_version" >"$evidence_directory/postgres-version.txt"

run_box_forwarder port-forward "$postgres_box" \
  --host-port "$POSTGRES_PORT" \
  --guest-port 5432 \
  --max-connections 64 \
  --connect-timeout-secs 5 \
  >"$evidence_directory/postgres-port-forward.log" 2>&1 &
port_forward_pid=$!

port_forward_ready=false
for _ in $(seq 1 30); do
  kill -0 "$port_forward_pid" >/dev/null 2>&1 ||
    die "A3S Box PostgreSQL port-forward exited before readiness"
  if python3 - "$POSTGRES_PORT" <<'PY'
import socket
import sys

with socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=1):
    pass
PY
  then
    port_forward_ready=true
    break
  fi
  sleep 1
done
[[ $port_forward_ready == true ]] || die "A3S Box PostgreSQL port-forward did not become ready"

bootstrap_token="$(openssl rand -hex 32)"
admin_token="a3s_$(openssl rand -hex 32)"
restricted_token="a3s_$(openssl rand -hex 32)"
github_webhook_secret="$(openssl rand -hex 32)"
postgres_url="postgres://a3s_cloud:$postgres_password@127.0.0.1:$POSTGRES_PORT/a3s_cloud"

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
stored_token_count="$(postgres_query \
  "select count(*) from api_tokens where token_hash in ('$admin_digest', '$restricted_digest')")"
[[ $stored_token_count == 2 ]] || die "PostgreSQL did not retain exactly the two expected token digests"
revoked_token_count="$(postgres_query \
  "select count(*) from api_tokens where token_hash = '$restricted_digest' and revoked_at is not null")"
[[ $revoked_token_count == 1 ]] || die "restricted token revocation was not durable"

if [[ $scenario == management-mcp ]]; then
  mcp_project_count="$(postgres_query \
    "select count(*) from projects where name in ('MCP Conformance Project', 'MCP Foreign Project')")"
  [[ $mcp_project_count == 2 ]] || die "PostgreSQL did not retain the two expected MCP projects"
  mcp_environment_count="$(postgres_query \
    "select count(*) from environments e join projects p on p.organization_id = e.organization_id and p.id = e.project_id where p.name = 'MCP Conformance Project' and e.name = 'MCP Operational Environment'")"
  [[ $mcp_environment_count == 1 ]] || die "PostgreSQL did not retain the expected MCP operational environment"
  mcp_ontology_count="$(postgres_query \
    "select count(*) from ontologies o join projects p on p.organization_id = o.organization_id and p.id = o.project_id where p.name = 'MCP Conformance Project' and o.name = 'Support' and o.aggregate_version = 3")"
  [[ $mcp_ontology_count == 1 ]] || die "PostgreSQL did not retain the expected versioned Ontology"
  mcp_ontology_revision_count="$(postgres_query \
    "select count(*) from ontology_revisions r join ontologies o on o.organization_id = r.organization_id and o.id = r.ontology_id join projects p on p.organization_id = o.organization_id and p.id = o.project_id where p.name = 'MCP Conformance Project' and o.name = 'Support'")"
  [[ $mcp_ontology_revision_count == 3 ]] || die "PostgreSQL did not retain the three expected Ontology revisions"
  mcp_ontology_idempotency_count="$(postgres_query \
    "select count(*) from idempotency_records where idempotency_key in ('c0:mcp:rest-ontology', 'c0:mcp:ontology-compatible', 'c0:mcp:ontology-breaking')")"
  [[ $mcp_ontology_idempotency_count == 3 ]] || die "Ontology replay did not preserve one record per accepted idempotency identity"
  mcp_form_draft_count="$(postgres_query \
    "select count(*) from form_drafts f join projects p on p.organization_id = f.organization_id and p.id = f.project_id where p.name = 'MCP Conformance Project' and f.name = 'MCP Approval request' and f.aggregate_version = 3 and f.latest_release_id is not null")"
  [[ $mcp_form_draft_count == 1 ]] || die "PostgreSQL did not retain the expected published Form draft"
  mcp_form_release_count="$(postgres_query \
    "select count(*) from form_releases r join form_drafts f on f.organization_id = r.organization_id and f.id = r.form_id where f.name = 'MCP Approval request' and r.revision = 1 and r.source_draft_version = 2 and r.compiler_revision = 'a3s-form-core@0.1.0'")"
  [[ $mcp_form_release_count == 1 ]] || die "PostgreSQL did not retain the expected immutable Form release"
  mcp_form_idempotency_count="$(postgres_query \
    "select count(*) from idempotency_records where idempotency_key in ('c0:mcp:rest-form', 'c0:mcp:form-revise', 'c0:mcp:form-publish')")"
  [[ $mcp_form_idempotency_count == 3 ]] || die "Form replay did not preserve one record per accepted idempotency identity"
  hidden_project_count="$(postgres_query \
    "select count(*) from projects where name = 'Hidden Mutation Must Not Exist'")"
  [[ $hidden_project_count == 0 ]] || die "hidden MCP mutation changed PostgreSQL state"
  mcp_idempotency_count="$(postgres_query \
    "select count(*) from idempotency_records where idempotency_key = 'c0:mcp:rest-project'")"
  [[ $mcp_idempotency_count == 1 ]] || die "REST-to-MCP replay did not preserve one idempotency record"
  mcp_workload_count="$(postgres_query \
    "select count(*) from workloads where name = 'mcp-stop' and desired_state = 'stopped'")"
  [[ $mcp_workload_count == 1 ]] || die "MCP Workload stop did not persist the expected desired state"
  mcp_workload_stop_idempotency_count="$(postgres_query \
    "select count(*) from idempotency_records where idempotency_key = 'c0:mcp:workload-stop'")"
  [[ $mcp_workload_stop_idempotency_count == 1 ]] || die "MCP Workload stop replay did not preserve one idempotency record"
  read_only_scope_count="$(postgres_query \
    "select count(*) from api_tokens where token_hash = '$restricted_digest' and scopes = '[\"cloud:read\"]'::jsonb and revoked_at is not null")"
  [[ $read_only_scope_count == 1 ]] || die "read-only MCP scope or revocation was not durable"
fi

database_dump="$run_directory/postgres.sql"
run_box exec "$postgres_box" --env "PGPASSWORD=$postgres_password" -- \
  pg_dump --dbname=a3s_cloud --username=a3s_cloud --data-only --no-owner --no-privileges \
  >"$database_dump" 2>"$evidence_directory/postgres-dump.log"
run_box logs "$postgres_box" >"$evidence_directory/postgres.log" 2>&1

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
    printf 'mcp_project_rows=2\nmcp_environment_rows=1\nmcp_form_draft_rows=1\nmcp_form_release_rows=1\nmcp_form_idempotency_rows=3\nmcp_stopped_workload_rows=1\nhidden_mutation_project_rows=0\nmcp_idempotency_rows=1\nmcp_workload_stop_idempotency_rows=1\nread_only_scope_rows=1\n'
  fi
} >"$evidence_directory/persistence-check.txt"

if [[ $scenario == cross-surface ]]; then
  printf 'A3S_CLOUD_C0_1_CROSS_SURFACE_PASS cloud=%s runtime=%s box=%s source=%s contract=1.15.0\n' \
    "$cloud_revision" "$runtime_revision" "$box_revision" "$source_state" \
    | tee "$evidence_directory/result.txt"
else
  printf 'A3S_CLOUD_C0_2M_MANAGEMENT_MCP_PASS cloud=%s runtime=%s box=%s source=%s protocol=2026-07-28 contract=1.15.0\n' \
    "$cloud_revision" "$runtime_revision" "$box_revision" "$source_state" \
    | tee "$evidence_directory/result.txt"
fi
