#!/usr/bin/env bash
# GA-0 / BX0.software LOOP create orchestrator.
#
# Creates real enroll → OCI identity binding → workload deploy receipts and
# exports A3S_CLOUD_BX0_* identities for the software EXIT harness.
#
# Honesty:
#   - Never invents digests, node IDs, EXIT, LOOP, or Power pins
#   - Never claims A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED
#   - Fail-closes when the live stack or pre-published OCI artifact is missing
#
# Required env:
#   A3S_CLOUD_URL A3S_CLOUD_TOKEN
#   A3S_CLOUD_ORGANIZATION_ID A3S_CLOUD_PROJECT_ID A3S_CLOUD_ENVIRONMENT_ID
#   A3S_CLOUD_BX0_NODE_CONFIG          absolute *.acl
#   A3S_CLOUD_BX0_AGENT_RELEASE_URL    https://… (bootstrap recipe)
#   A3S_CLOUD_BX0_AGENT_RELEASE_SHA256 64 lowercase hex
#   A3S_CLOUD_BX0_ARTIFACT_URI         oci://…@sha256:… (pre-published)
#   A3S_CLOUD_BX0_ARTIFACT_DIGEST      sha256:<64-hex> (must match URI)
#
# Optional:
#   A3S_CLOUD_BX0_NODE_NAME            default: worker-bx0-software-<hex>
#   A3S_CLOUD_BX0_WORKLOAD_NAME        default: bx0-software-loop-<hex>
#   A3S_CLOUD_BX0_HEALTH_URL           http(s)://… Ready probe
#   A3S_CLOUD_BX0_HTTPS_URL            https://… managed edge
#   A3S_CLOUD_BX0_GATEWAY_SCOPE_ID + DOMAIN_CLAIM_ID + ROUTE_HOSTNAME
#                                      — optional routes publish to set HTTPS_URL
#   A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI / A3S_CLOUD_BX0_UPDATE_DIGEST
#   A3S_CLOUD_BX0_CREATE_FULL=1        also drive logs→update→rollback→stop
#   A3S_CLOUD_BX0_CREATE_OUT           write export env file (default under evidence)
#
# Usage:
#   bash tools/box-conformance/run_bx0_software_loop_create.sh [EVIDENCE_DIR]

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
# shellcheck source=bx0_clean_host_steps.sh
# shellcheck disable=SC1090
source "$tools/bx0_clean_host_steps.sh"
# shellcheck source=bx0_ensure_middleware_ports.sh
# shellcheck disable=SC1090
source "$tools/bx0_ensure_middleware_ports.sh"
CLOUD_ROOT="$repository_root"
export CLOUD_ROOT

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-loop-create.XXXXXX")
fi
mkdir -p -- "$evidence_directory"
report="$evidence_directory/software-loop-create-report.txt"
: >"$report"

record() {
  printf '%s\t%s\n' "$1" "$2" | tee -a "$report"
}

fail_blocked() {
  local reason=$1
  shift || true
  record FAIL "$reason $*"
  cat <<EOF | tee "$evidence_directory/bx0-software-loop-create.txt"
A3S_CLOUD_BX0_SOFTWARE_LOOP_CREATE_BLOCKED reason=$reason
$*
product_exit=not_claimed
loop_certified=not_claimed
EOF
  exit 2
}

os_name=$(uname -s)
arch_name=$(uname -m)
if [[ $os_name != Linux || $arch_name != x86_64 ]]; then
  fail_blocked host_unsupported "got=${os_name}-${arch_name}"
fi

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
bx0_refuse_docker_host

if [[ -f $repository_root/tools/power-conformance/power-revision ]]; then
  fail_blocked invented_power_pin
fi

required_base=(
  A3S_CLOUD_URL
  A3S_CLOUD_TOKEN
  A3S_CLOUD_ORGANIZATION_ID
  A3S_CLOUD_PROJECT_ID
  A3S_CLOUD_ENVIRONMENT_ID
  A3S_CLOUD_BX0_NODE_CONFIG
  A3S_CLOUD_BX0_AGENT_RELEASE_URL
  A3S_CLOUD_BX0_AGENT_RELEASE_SHA256
  A3S_CLOUD_BX0_ARTIFACT_URI
  A3S_CLOUD_BX0_ARTIFACT_DIGEST
)
missing=()
for key in "${required_base[@]}"; do
  if [[ -z ${!key:-} || ${!key} == PLACEHOLDER_* ]]; then
    missing+=("$key")
  fi
done
if ((${#missing[@]} > 0)); then
  fail_blocked create_prereqs_incomplete "missing=${missing[*]}"
fi

node_config=$A3S_CLOUD_BX0_NODE_CONFIG
if [[ $node_config != /* || $node_config != *.acl || ! -f $node_config ]]; then
  fail_blocked node_config_invalid "path=$node_config"
fi

digest=$A3S_CLOUD_BX0_ARTIFACT_DIGEST
if [[ $digest =~ ^[0-9a-f]{64}$ ]]; then
  digest="sha256:$digest"
fi
if [[ ! $digest =~ ^sha256:[0-9a-f]{64}$ ]]; then
  fail_blocked artifact_digest_invalid "digest=$digest"
fi
uri=$A3S_CLOUD_BX0_ARTIFACT_URI
if [[ $uri != oci://* || $uri == *PLACEHOLDER* ]]; then
  fail_blocked artifact_uri_invalid "uri=$uri"
fi
uri_digest=${uri##*@}
if [[ $uri == *@sha256:* && $uri_digest != "$digest" ]]; then
  fail_blocked artifact_uri_digest_mismatch "uri_digest=$uri_digest digest=$digest"
fi
export A3S_CLOUD_BX0_ARTIFACT_DIGEST=$digest

release_sha=$A3S_CLOUD_BX0_AGENT_RELEASE_SHA256
if [[ ! $release_sha =~ ^[0-9a-f]{64}$ ]]; then
  fail_blocked agent_release_sha_invalid
fi
release_url=$A3S_CLOUD_BX0_AGENT_RELEASE_URL
if [[ $release_url != https://* ]]; then
  fail_blocked agent_release_url_invalid
fi

agent_bin=
if ! agent_bin="$(bx0_resolve_node_agent)"; then
  fail_blocked node_agent_unavailable
fi
box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
if [[ -z $box_bin || ! -x $box_bin ]]; then
  fail_blocked a3s_box_unavailable
fi
export A3S_CLOUD_BOX_BIN=$box_bin

if ! command -v bun >/dev/null 2>&1; then
  fail_blocked bun_unavailable
fi
if ! command -v python3 >/dev/null 2>&1; then
  fail_blocked python3_unavailable
fi

cloud_cli() {
  (
    cd -- "$repository_root/cli"
    bun_bin=${A3S_CLOUD_BUN_BIN:-}
    if [[ -z $bun_bin && -x ${HOME:-}/.bun/bin/bun ]]; then
      bun_bin="${HOME}/.bun/bin/bun"
    fi
    bun_bin=${bun_bin:-$(command -v bun)}
    "$bun_bin" src/main.ts --output=json "$@"
  )
}

json_get() {
  python3 - "$1" "$2" <<'PY'
import json, sys
data = json.loads(sys.argv[1])
path = sys.argv[2].split(".")
cur = data
for part in path:
    if part == "":
        continue
    if isinstance(cur, list):
        cur = cur[int(part)]
    else:
        cur = cur[part]
if cur is None:
    raise SystemExit(1)
if isinstance(cur, (dict, list)):
    print(json.dumps(cur))
else:
    print(cur)
PY
}

rfc3339_plus_hours() {
  python3 -c "import datetime; print((datetime.datetime.now(datetime.timezone.utc)+datetime.timedelta(hours=int('$1'))).strftime('%Y-%m-%dT%H:%M:%SZ'))"
}

random_hex() {
  python3 -c "import secrets; print(secrets.token_hex(int('$1')))"
}

node_name=${A3S_CLOUD_BX0_NODE_NAME:-worker-bx0-software-$(random_hex 4)}
workload_name=${A3S_CLOUD_BX0_WORKLOAD_NAME:-bx0-software-loop-$(random_hex 4)}
create_full=${A3S_CLOUD_BX0_CREATE_FULL:-0}

# Start a background keepalive that rebinds compose host ports when they drop.
# WSL + a3s-box: sibling port_maps disappear while guests stay "healthy".
bx0_start_middleware_keepalive() {
  local box_bin=$1
  local compose_acl=$2
  local evidence_directory=$3
  local pid_file=$4
  local helper=$tools/bx0_ensure_middleware_ports.sh
  if [[ -z $box_bin || ! -x $box_bin || ! -f $compose_acl || ! -f $helper ]]; then
    return 1
  fi
  if [[ -f $pid_file ]]; then
    kill "$(cat "$pid_file")" 2>/dev/null || true
  fi
  nohup bash -c '
    set +e
    box_bin=$1
    compose_acl=$2
    evidence_directory=$3
    helper=$4
    # shellcheck disable=SC1090
    source "$helper"
    bx0_ensure_passt_on_path
    printf "%s keepalive start box=%s\\n" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$box_bin"
    while true; do
      if bx0_middleware_ports_ok; then
        printf "%s ports_ok\\n" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
      else
        printf "%s ports_missing; rebinding\\n" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
        bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory"
        printf "%s rebind_rc=%s\\n" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$?"
      fi
      # Slow loop: avoid TIME_WAIT storms from thrashing compose restart.
      sleep 10
    done
  ' bash "$box_bin" "$compose_acl" "$evidence_directory" "$helper" \
    >"$evidence_directory/middleware-keepalive.log" 2>&1 &
  printf '%s\n' "$!" >"$pid_file"
  sleep 0.2
  kill -0 "$(cat "$pid_file")" 2>/dev/null
}

bx0_stop_middleware_keepalive() {
  local pid_file=$1
  if [[ -f $pid_file ]]; then
    kill "$(cat "$pid_file")" 2>/dev/null || true
    rm -f -- "$pid_file"
  fi
}

# --- 1. Enroll: issue credential + start agent + wait for node id ---
box_bin=${A3S_CLOUD_BOX_BIN:-$(command -v a3s-box || true)}
compose_acl=${A3S_CLOUD_BX0_COMPOSE_ACL:-$repository_root/deploy/dev/compose.bx0-live.acl}
keepalive_pid_file="$evidence_directory/middleware-keepalive.pid"
trap 'bx0_stop_middleware_keepalive "$keepalive_pid_file"' EXIT
if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
  if ! bx0_middleware_ports_ok; then
    record PASS "rebinding full compose port_map before nodes bootstrap"
    bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" \
      || fail_blocked middleware_ports_before_bootstrap
  fi
  bx0_start_middleware_keepalive "$box_bin" "$compose_acl" "$evidence_directory" \
    "$keepalive_pid_file"
  record PASS "middleware keepalive pid=$(cat "$keepalive_pid_file")"
fi

enrollment_token="a3sn_$(random_hex 32)"
export A3S_CLOUD_ENROLLMENT_TOKEN=$enrollment_token
expires_at=$(rfc3339_plus_hours 2)
idem_bootstrap="bx0-create-bootstrap-$(random_hex 8)"

record PASS "issuing enrollment credential via nodes bootstrap"
set +e
bootstrap_json=$(
  printf '%s' "$enrollment_token" | cloud_cli nodes bootstrap "$node_name" \
    --enrollment-token-stdin \
    --expires-at="$expires_at" \
    --agent-release-url="$release_url" \
    --agent-release-sha256="$release_sha" \
    --node-config="$node_config" \
    --idempotency-key="$idem_bootstrap" \
    2>"$evidence_directory/bootstrap.cli.err"
)
bootstrap_rc=$?
set -e
if ((bootstrap_rc != 0)); then
  printf '%s\n' "$bootstrap_json" >"$evidence_directory/bootstrap.err"
  fail_blocked nodes_bootstrap_failed \
    "exit=$bootstrap_rc body=$(printf '%s' "$bootstrap_json" | head -c 400) stderr=$(head -c 400 "$evidence_directory/bootstrap.cli.err" 2>/dev/null || true)"
fi
printf '%s\n' "$bootstrap_json" >"$evidence_directory/bootstrap.json"
record PASS "nodes bootstrap ok"

# Bootstrap name must match node.acl name or enroll returns 409 CONFLICT.
# Also drop durable identity so a prior enrollment fingerprint cannot collide.
runtime_node_acl="$evidence_directory/node.runtime.acl"
python3 - "$node_config" "$runtime_node_acl" "$node_name" <<'PY'
import pathlib, re, sys
src, dst, name = sys.argv[1:4]
text = pathlib.Path(src).read_text(encoding="utf-8")
patched, n = re.subn(
    r'(node\s*\{[^}]*?name\s*=\s*)"[^"]*"',
    rf'\1"{name}"',
    text,
    count=1,
    flags=re.S,
)
if n != 1:
    raise SystemExit(f"node.name rewrite failed count={n}")
pathlib.Path(dst).write_text(patched, encoding="utf-8")
print(dst)
PY
node_config=$runtime_node_acl
export A3S_CLOUD_BX0_NODE_CONFIG=$node_config
record PASS "node.acl name aligned to bootstrap name=$node_name"

acl_state=$(
  python3 - "$node_config" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
m = re.search(r'state_dir\s*=\s*"([^"]+)"', text)
print(m.group(1) if m else "")
PY
)
if [[ -n $acl_state ]]; then
  mkdir -p -- "$acl_state"
  rm -f -- "$acl_state/identity.json" "$acl_state/identity.lock" \
    "$acl_state/node-agent.lock"
  record PASS "cleared stale node identity under $acl_state"
fi

agent_log="$evidence_directory/node-agent.log"
agent_pid_file="$evidence_directory/node-agent.pid"
if [[ -f $agent_pid_file ]]; then
  old_pid=$(<"$agent_pid_file")
  kill "$old_pid" 2>/dev/null || true
fi
: >"$agent_log"
record PASS "starting node-agent $agent_bin"
[[ -n ${A3S_GATEWAY_ADMIN_TOKEN:-} ]] \
  || fail_blocked gateway_admin_token_missing \
    'hint=export A3S_GATEWAY_ADMIN_TOKEN from live_prep stack.env'
# Prefer home_dir from node.acl so A3S_HOME matches Box's single-authority rule.
acl_home=$(
  python3 - "$node_config" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
m = re.search(r'home_dir\s*=\s*"([^"]+)"', text)
print(m.group(1) if m else "")
PY
)
if [[ -n $acl_home ]]; then
  export A3S_HOME=$acl_home
fi
[[ -n ${A3S_HOME:-} ]] || fail_blocked a3s_home_missing
mkdir -p -- "$A3S_HOME"
# Rebind ALL middleware host ports before agent start — partial restart drops siblings.
if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
  if ! bx0_middleware_ports_ok; then
    record PASS "rebinding full compose port_map before agent start"
    bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" \
      || fail_blocked middleware_ports_before_enroll
  fi
fi
set +e
# Unprivileged hosts cannot overlay-mount MicroVM rootfs without CAP_SYS_ADMIN
# inside a user namespace. Prefer rootlesskit (full subuid/subgid map) over plain
# `unshare -Urm` (single-id map): Alpine layers chown gid=42 and fail otherwise.
agent_prefix=()
if [[ $(id -u) -ne 0 ]]; then
  if command -v rootlesskit >/dev/null 2>&1; then
    agent_prefix=(rootlesskit)
    record PASS "launching node-agent under rootlesskit for overlay + layer ownership"
  elif command -v unshare >/dev/null 2>&1 && unshare -Urm true 2>/dev/null; then
    agent_prefix=(unshare -Urm)
    record PASS "launching node-agent under unshare -Urm (no rootlesskit; single-id map)"
  fi
fi
nohup "${agent_prefix[@]}" env \
  A3S_CLOUD_ENROLLMENT_TOKEN="$enrollment_token" \
  A3S_GATEWAY_ADMIN_TOKEN="$A3S_GATEWAY_ADMIN_TOKEN" \
  A3S_CLOUD_BOX_BIN="${A3S_CLOUD_BOX_BIN:-}" \
  A3S_HOME="$A3S_HOME" \
  A3S_REGISTRY_PROTOCOL="${A3S_REGISTRY_PROTOCOL:-http}" \
  LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-}" \
  PATH="$PATH" \
  "$agent_bin" "$node_config" >"$agent_log" 2>&1 &
agent_pid=$!
set -e
printf '%s\n' "$agent_pid" >"$agent_pid_file"
if ! kill -0 "$agent_pid" 2>/dev/null; then
  fail_blocked node_agent_start_failed "log=$agent_log"
fi
record PASS "node-agent env A3S_REGISTRY_PROTOCOL=${A3S_REGISTRY_PROTOCOL:-http} (loopback HTTP registry)"

enroll_node_id=
deadline=$((SECONDS + ${A3S_CLOUD_BX0_ENROLL_TIMEOUT_SEC:-180}))
pg_port=${A3S_CLOUD_POSTGRES_PORT:-54320}
while ((SECONDS < deadline)); do
  # Keep ALL middleware host ports published — partial restart drops registry.
  if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
    if ! bx0_port_reachable 127.0.0.1 "$pg_port"; then
      bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" || true
    fi
  fi
  set +e
  nodes_json=$(cloud_cli nodes list 2>"$evidence_directory/nodes-list.err")
  nodes_rc=$?
  set -e
  if ((nodes_rc == 0)); then
    printf '%s\n' "$nodes_json" >"$evidence_directory/nodes-list.json"
    enroll_node_id=$(
      python3 - "$node_name" "$nodes_json" <<'PY'
import json, sys
name = sys.argv[1]
rows = json.loads(sys.argv[2])
if not isinstance(rows, list):
    raise SystemExit(1)
for row in rows:
    if row.get("name") == name and row.get("id"):
        print(row["id"])
        raise SystemExit(0)
raise SystemExit(2)
PY
    ) || true
    if [[ -n $enroll_node_id && $enroll_node_id != PLACEHOLDER_* ]]; then
      break
    fi
  fi
  if [[ -f $agent_log ]] && grep -Eq 'ControlPlane\(Rejected|CONFLICT|node name or identity already exists' "$agent_log"; then
    fail_blocked enroll_rejected \
      "agent_pid=$agent_pid log=$agent_log snippet=$(tail -c 400 "$agent_log" | tr '\n' ' ')"
  fi
  if ! kill -0 "$agent_pid" 2>/dev/null; then
    fail_blocked enroll_agent_exited \
      "agent_pid=$agent_pid log=$agent_log snippet=$(tail -c 400 "$agent_log" | tr '\n' ' ')"
  fi
  sleep 2
done
if [[ -z $enroll_node_id ]]; then
  fail_blocked enroll_timeout "agent_pid=$agent_pid log=$agent_log"
fi
export A3S_CLOUD_BX0_ENROLL_NODE_ID=$enroll_node_id
record PASS "enrolled node_id=$enroll_node_id"

# Rebind + restore OCI before deploy — enroll remounts used to wipe guest /tmp
# registry storage; compose now keeps blobs under /var/lib/registry.
if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
  if ! bx0_middleware_ports_ok; then
    record PASS "rebinding full compose port_map before OCI/deploy"
    bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" \
      || fail_blocked middleware_ports_before_deploy
  fi
fi
if ! bx0_oci_manifest_reachable "$uri" "$digest"; then
  record PASS "OCI digest missing after remount; restoring operator tags"
  registry_host=${A3S_CLOUD_BX0_OCI_REGISTRY:-127.0.0.1:${A3S_CLOUD_REGISTRY_PORT:-50020}}
  repo=${A3S_CLOUD_BX0_OCI_REPOSITORY:-a3s/bx0-software}
  tag_a=${A3S_CLOUD_BX0_OCI_TAG:-live-a}
  tag_b=${A3S_CLOUD_BX0_OCI_UPDATE_TAG:-live-b}
  update_digest_restore=${A3S_CLOUD_BX0_UPDATE_DIGEST:-}
  set +e
  bx0_oci_restore_tags "$registry_host" "$repo" "$tag_a" "$tag_b" \
    "$digest" "$update_digest_restore"
  restore_rc=$?
  set -e
  if ((restore_rc != 0)); then
    record PASS "cache restore rc=$restore_rc; republishing OCI digests"
    oci_republish_dir="$evidence_directory/oci-republish"
    mkdir -p -- "$oci_republish_dir"
    set +e
    bash "$tools/run_bx0_software_exit_live_oci.sh" "$oci_republish_dir"
    oci_rc=$?
    set -e
    if ((oci_rc != 0)); then
      fail_blocked oci_digest_restore_failed \
        "restore_rc=$restore_rc oci_rc=$oci_rc digest=$digest registry=$registry_host"
    fi
    # shellcheck disable=SC1090
    source "$oci_republish_dir/bx0-live-oci-env.sh"
    digest=$A3S_CLOUD_BX0_ARTIFACT_DIGEST
    uri=$A3S_CLOUD_BX0_ARTIFACT_URI
    update_digest_restore=${A3S_CLOUD_BX0_UPDATE_DIGEST:-}
  fi
fi
if ! bx0_oci_manifest_reachable "$uri" "$digest"; then
  fail_blocked oci_manifest_unreachable "uri=$uri digest=$digest"
fi
# Keep ARTIFACT_* aligned after possible republish.
export A3S_CLOUD_BX0_ARTIFACT_DIGEST=$digest
export A3S_CLOUD_BX0_ARTIFACT_URI=$uri
if [[ -n ${A3S_CLOUD_BX0_UPDATE_DIGEST:-} ]]; then
  export A3S_CLOUD_BX0_UPDATE_DIGEST
fi
if [[ -n ${A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI:-} ]]; then
  export A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI
fi

# --- 2. OCI: bind pre-published digest (never invent) ---
oci_bin=
if ! oci_bin="$(bx0_resolve_oci_cli)"; then
  fail_blocked oci_unavailable \
    'hint=export A3S_CLOUD_OCI_BIN to an executable a3s-oci (often next to a3s-box)'
fi
record PASS "OCI runtime CLI present ($oci_bin); using operator-published digest=$digest"
printf 'artifact_uri=%s\nartifact_digest=%s\nsource=operator_prepublished\n' \
  "$uri" "$digest" >"$evidence_directory/02-oci-create.txt"

# --- 3. Deploy workload from ACL ---
workload_acl="$evidence_directory/workload.create.acl"
cat >"$workload_acl" <<EOF
version = 1

workload "$workload_name" {
  artifact {
    uri = "$uri"
    expected_digest = "$digest"
  }

  process {
    command = ["/bin/sh"]
    # Brief settle before listen: MicroVM NIC can lag process start; nc one-shot
    # loops also leave short unbound gaps between accepts.
    args = ["-c", "mkdir -p /tmp/www && printf ready >/tmp/www/ready && sleep 2 && while true; do printf 'HTTP/1.0 200 OK\\r\\nContent-Length: 5\\r\\nConnection: close\\r\\n\\r\\nready' | /bin/busybox nc -l -p 8080 || true; done"]
  }

  resources {
    # MicroVM a3s-box does not advertise EphemeralStorage (writable-layer
    # quota is Sandbox-only). Requesting it makes Schedule stay Pending forever.
    # Keep requests small enough that a rolling update surge (2 replicas) can
    # schedule on a single BX0.software node.
    cpu_millis = 100
    memory_bytes = 67108864
    pids = 64
  }

  port "http" {
    container_port = 8080
  }

  health {
    port_name = "http"
    path = "/ready"
    interval_ms = 3000
    timeout_ms = 2000
    healthy_threshold = 1
    unhealthy_threshold = 15
    stabilization_window_ms = 10000
  }
}
EOF

idem_create="bx0-create-workload-$(random_hex 8)"
set +e
create_json=$(
  cloud_cli workloads create \
    --file="$workload_acl" \
    --idempotency-key="$idem_create" \
    2>"$evidence_directory/workload-create.err"
)
create_rc=$?
set -e
printf '%s\n' "$create_json" >"$evidence_directory/workload-create.json"
if ((create_rc != 0)); then
  fail_blocked workloads_create_failed \
    "exit=$create_rc body=$(printf '%s' "$create_json" | head -c 400) stderr=$(head -c 400 "$evidence_directory/workload-create.err" 2>/dev/null || true)"
fi

workload_id=$(json_get "$create_json" workloadId) || fail_blocked workloads_create_parse "workloadId"
revision_id=$(json_get "$create_json" revisionId) || fail_blocked workloads_create_parse "revisionId"
service_id=$workload_id
export A3S_CLOUD_BX0_WORKLOAD_ID=$workload_id
export A3S_CLOUD_BX0_REVISION_ID=$revision_id
export A3S_CLOUD_BX0_SERVICE_ID=$service_id
record PASS "deployed workload_id=$workload_id revision_id=$revision_id"

# Box instance id for cleanup must be providerResourceId (a3s-box inspect target),
# never workload/member UUIDs — those make inspect-absent vacuously true.
box_instance_id=
member_id=
deploy_status=
deadline=$((SECONDS + ${A3S_CLOUD_BX0_DEPLOY_TIMEOUT_SEC:-300}))
while ((SECONDS < deadline)); do
  # Keep middleware ports alive — PG/registry port_map drops stall resolve/schedule.
  if [[ -n $box_bin && -x $box_bin && -f $compose_acl ]]; then
    if ! bx0_middleware_ports_ok; then
      bx0_ensure_middleware_ports "$box_bin" "$compose_acl" "$evidence_directory" || true
      # Full compose remount wipes ephemeral registry; restore operator digests.
      if ! bx0_oci_manifest_reachable "$uri" "$digest"; then
        registry_host=${A3S_CLOUD_BX0_OCI_REGISTRY:-127.0.0.1:${A3S_CLOUD_REGISTRY_PORT:-50020}}
        repo=${A3S_CLOUD_BX0_OCI_REPOSITORY:-a3s/bx0-software}
        tag_a=${A3S_CLOUD_BX0_OCI_TAG:-live-a}
        tag_b=${A3S_CLOUD_BX0_OCI_UPDATE_TAG:-live-b}
        update_digest_restore=${A3S_CLOUD_BX0_UPDATE_DIGEST:-}
        bx0_oci_restore_tags "$registry_host" "$repo" "$tag_a" "$tag_b" \
          "$digest" "$update_digest_restore" || true
      fi
    fi
  fi
  set +e
  get_json=$(cloud_cli workloads get "$workload_id" 2>"$evidence_directory/workload-get.err")
  get_rc=$?
  set -e
  if ((get_rc == 0)); then
    printf '%s\n' "$get_json" >"$evidence_directory/workload-get.json"
    parsed=$(
      python3 - "$get_json" <<'PY'
import json, sys
w = json.loads(sys.argv[1])
member = ""
for replica in w.get("replicas") or []:
    for m in replica.get("members") or []:
        if m.get("id"):
            member = m["id"]
            break
    if member:
        break
provider = ""
status = ""
for dep in w.get("deployments") or []:
    if not status:
        status = str(dep.get("status") or "")
    obs = dep.get("observedRuntime") or {}
    pid = obs.get("providerResourceId")
    if pid:
        provider = pid
        status = str(dep.get("status") or status)
        break
if not member and not provider:
    raise SystemExit(2)
print(member)
print(provider)
print(status)
PY
    ) || true
    if [[ -n $parsed ]]; then
      member_id=$(printf '%s\n' "$parsed" | sed -n '1p')
      box_instance_id=$(printf '%s\n' "$parsed" | sed -n '2p')
      deploy_status=$(printf '%s\n' "$parsed" | sed -n '3p')
      if [[ -n $box_instance_id ]]; then
        break
      fi
    fi
  fi
  sleep 2
done
if [[ -n $box_instance_id ]]; then
  record PASS "box providerResourceId=$box_instance_id member=${member_id:-none} status=${deploy_status:-unknown}"
elif [[ -n $member_id ]]; then
  record PASS "replica member=$member_id status=${deploy_status:-unknown} (providerResourceId not yet observed)"
else
  record PASS "replica/box instance not yet observed status=${deploy_status:-unknown} (continuing)"
fi

if [[ ${A3S_CLOUD_BX0_CREATE_FULL:-0} == 1 && -z $box_instance_id ]]; then
  fail_blocked box_instance_unobserved \
    "CREATE_FULL requires deployments[].observedRuntime.providerResourceId; status=${deploy_status:-unknown}"
fi

health_url=${A3S_CLOUD_BX0_HEALTH_URL:-}
https_url=${A3S_CLOUD_BX0_HTTPS_URL:-}

# Always bind HEALTH_URL via a3s-box port-forward when a Box instance is known.
# live_https may pre-export HEALTH_URL (TLS proxy backend) before anything listens;
# do not skip port-forward merely because the URL env is already set.
if [[ -n $box_instance_id ]]; then
  host_port=${A3S_CLOUD_BX0_HEALTH_HOST_PORT:-18080}
  if [[ -n $health_url && $health_url =~ ^http://127\.0\.0\.1:([0-9]+) ]]; then
    host_port=${BASH_REMATCH[1]}
  fi
  health_url="http://127.0.0.1:${host_port}/ready"
  # Wait until the Box is actually running (providerResourceId can appear while
  # still applying / already failed). Fail closed on terminal failed/exited.
  box_ready=0
  box_wait_deadline=$((SECONDS + ${A3S_CLOUD_BX0_BOX_READY_TIMEOUT_SEC:-180}))
  while ((SECONDS < box_wait_deadline)); do
    set +e
    box_status=$(
      A3S_HOME="${A3S_HOME:-}" "$box_bin" inspect --format '{{.State.Status}}' \
        "$box_instance_id" 2>/dev/null \
        || A3S_HOME="${A3S_HOME:-}" "$box_bin" inspect "$box_instance_id" 2>/dev/null \
        | python3 -c 'import json,sys; d=json.load(sys.stdin); print((d[0] if isinstance(d,list) else d).get("state") or (d[0] if isinstance(d,list) else d).get("status") or "")' \
          2>/dev/null
    )
    set -e
    box_status=$(printf '%s' "$box_status" | tr '[:upper:]' '[:lower:]' | head -n1 | tr -d '\r')
    if [[ $box_status == running || $box_status == ready ]]; then
      box_ready=1
      record PASS "box status=$box_status before port-forward"
      break
    fi
    if [[ $box_status == failed || $box_status == exited || $box_status == dead || $box_status == stopped ]]; then
      fail_blocked box_not_running \
        "providerResourceId=$box_instance_id status=$box_status pf_hint=guest process must stay up for HEALTH"
    fi
    sleep 2
  done
  if ((box_ready != 1)); then
    fail_blocked box_ready_timeout \
      "providerResourceId=$box_instance_id last_status=${box_status:-unknown}"
  fi
  pf_log="$evidence_directory/port-forward.log"
  pf_pid_file="$evidence_directory/port-forward.pid"
  if [[ -f $pf_pid_file ]]; then
    kill "$(cat "$pf_pid_file")" 2>/dev/null || true
  fi
  # Drop stale listeners (prior microvm proxy / failed port-forward) on HEALTH port.
  fuser -k "${host_port}/tcp" >/dev/null 2>&1 || true
  pkill -f "bx0_microvm_health_proxy.py .* ${host_port} " >/dev/null 2>&1 || true
  sleep 1
  if bx0_port_reachable 127.0.0.1 "$host_port"; then
    fail_blocked health_port_busy "host_port=$host_port still listening after cleanup"
  fi
  # Prefer native Sandbox netns port-forward; MicroVM Service reachability is
  # vsock-backed and CLI port-forward refuses it — fall back to exec proxy.
  set +e
  nohup env A3S_HOME="${A3S_HOME:-}" "$box_bin" port-forward \
    --host-port "$host_port" --guest-port 8080 "$box_instance_id" \
    >"$pf_log" 2>&1 &
  pf_pid=$!
  set -e
  printf '%s\n' "$pf_pid" >"$pf_pid_file"
  sleep 1
  pf_ok=0
  if kill -0 "$pf_pid" 2>/dev/null && bx0_port_reachable 127.0.0.1 "$host_port"; then
    pf_ok=1
    record PASS "HEALTH_URL=$health_url via sandbox port-forward pid=$pf_pid"
  else
    kill "$pf_pid" 2>/dev/null || true
    set +e
    nohup env A3S_HOME="${A3S_HOME:-}" "$box_bin" port-forward \
      "$box_instance_id" "${host_port}:8080" \
      >"$pf_log" 2>&1 &
    pf_pid=$!
    set -e
    printf '%s\n' "$pf_pid" >"$pf_pid_file"
    sleep 1
    if kill -0 "$pf_pid" 2>/dev/null && bx0_port_reachable 127.0.0.1 "$host_port"; then
      pf_ok=1
      record PASS "HEALTH_URL=$health_url via legacy port-forward pid=$pf_pid"
    else
      kill "$pf_pid" 2>/dev/null || true
    fi
  fi
  if ((pf_ok != 1)); then
    proxy_py=$tools/bx0_microvm_health_proxy.py
    [[ -f $proxy_py ]] || fail_blocked microvm_health_proxy_missing "path=$proxy_py"
    : >"$pf_log"
    set +e
    nohup python3 "$proxy_py" "$box_bin" "$box_instance_id" "$host_port" 8080 \
      "${A3S_HOME:-}" >>"$pf_log" 2>&1 &
    pf_pid=$!
    set -e
    printf '%s\n' "$pf_pid" >"$pf_pid_file"
    sleep 1
    if ! kill -0 "$pf_pid" 2>/dev/null || ! bx0_port_reachable 127.0.0.1 "$host_port"; then
      fail_blocked health_port_forward_failed \
        "log=$pf_log snippet=$(tail -c 400 "$pf_log" | tr '\n' ' ')"
    fi
    record PASS "HEALTH_URL=$health_url via microvm exec proxy pid=$pf_pid"
  fi
  export A3S_CLOUD_BX0_HEALTH_URL=$health_url
elif [[ -n $health_url ]]; then
  # Without a Box instance, a pre-set HEALTH_URL is aspirational — do not probe yet.
  record PASS "deferring health probe until providerResourceId (preset HEALTH_URL=$health_url)"
  health_url=
fi

# Optional: publish managed route when edge identities already exist (never invents claims).
gateway_scope_id=${A3S_CLOUD_BX0_GATEWAY_SCOPE_ID:-}
domain_claim_id=${A3S_CLOUD_BX0_DOMAIN_CLAIM_ID:-}
route_hostname=${A3S_CLOUD_BX0_ROUTE_HOSTNAME:-}
route_path=${A3S_CLOUD_BX0_ROUTE_PATH_PREFIX:-/}
route_port_name=${A3S_CLOUD_BX0_ROUTE_PORT_NAME:-http}
if [[ -z $https_url && -n $gateway_scope_id && -n $domain_claim_id && -n $route_hostname ]]; then
  if [[ $gateway_scope_id == PLACEHOLDER_* || $domain_claim_id == PLACEHOLDER_* || $route_hostname == PLACEHOLDER_* ]]; then
    fail_blocked route_publish_placeholder
  fi
  idem_route="bx0-create-route-$(random_hex 8)"
  set +e
  route_json=$(
    cloud_cli routes publish \
      "$gateway_scope_id" \
      "$revision_id" \
      "$domain_claim_id" \
      "$route_hostname" \
      "$route_path" \
      "$route_port_name" \
      --idempotency-key="$idem_route"
  )
  route_rc=$?
  set -e
  printf '%s\n' "$route_json" >"$evidence_directory/route-publish.json"
  if ((route_rc != 0)); then
    fail_blocked routes_publish_failed "exit=$route_rc"
  fi
  https_url="https://${route_hostname}${route_path}"
  # Normalize double slash after host when path is /
  if [[ $route_path == / ]]; then
    https_url="https://${route_hostname}/"
  fi
  export A3S_CLOUD_BX0_HTTPS_URL=$https_url
  record PASS "routes publish bound HTTPS_URL=$https_url"
fi
if [[ -n $health_url ]]; then
  if [[ $health_url == PLACEHOLDER_* || ! $health_url =~ ^https?:// ]]; then
    fail_blocked health_url_invalid
  fi
  deadline=$((SECONDS + ${A3S_CLOUD_BX0_HEALTH_TIMEOUT_SEC:-120}))
  health_ok=0
  while ((SECONDS < deadline)); do
    if curl -fsS -o /dev/null --max-time 5 "$health_url"; then
      health_ok=1
      break
    fi
    sleep 2
  done
  if ((health_ok != 1)); then
    fail_blocked health_probe_failed "url=$health_url"
  fi
  export A3S_CLOUD_BX0_HEALTH_URL=$health_url
  record PASS "health ready at $health_url"
  mkdir -p -- "$evidence_directory/receipts"
  cat >"$evidence_directory/receipts/04-health.txt" <<EOF
step=4
name=health
status=execute_ok
health_url=$health_url
probe_ran=1
health=executed
source=loop_create
EOF
fi
if [[ -n $https_url ]]; then
  if [[ $https_url != https://* || $https_url == PLACEHOLDER_* ]]; then
    fail_blocked https_url_invalid
  fi
  # Probe managed TLS before cleanup — gate cannot re-hit this URL after rm.
  https_probe=${A3S_CLOUD_HEALTH_PROBE_BIN:-}
  if [[ -z $https_probe || ! -x $https_probe ]]; then
    if [[ -x ${A3S_CLOUD_BX0_LIVE_PREP_STATE_DIR:-$repository_root/.a3s/cloud/bx0-live-prep}/bx0-ca-curl-probe.sh ]]; then
      https_probe=${A3S_CLOUD_BX0_LIVE_PREP_STATE_DIR:-$repository_root/.a3s/cloud/bx0-live-prep}/bx0-ca-curl-probe.sh
    elif command -v curl >/dev/null 2>&1; then
      https_probe=$(command -v curl)
    fi
  fi
  https_ok=0
  if [[ -n $https_probe && -x $https_probe ]]; then
    deadline=$((SECONDS + ${A3S_CLOUD_BX0_HTTPS_TIMEOUT_SEC:-60}))
    while ((SECONDS < deadline)); do
      if [[ $(basename -- "$https_probe") == curl ]]; then
        if "$https_probe" -fsS -o /dev/null --max-time 5 "$https_url"; then
          https_ok=1
          break
        fi
      else
        if "$https_probe" "$https_url"; then
          https_ok=1
          break
        fi
      fi
      sleep 2
    done
  fi
  if ((https_ok != 1)); then
    fail_blocked https_probe_failed "url=$https_url probe=${https_probe:-missing}"
  fi
  export A3S_CLOUD_BX0_HTTPS_URL=$https_url
  record PASS "https url ready $https_url"
  mkdir -p -- "$evidence_directory/receipts"
  cat >"$evidence_directory/receipts/05-https.txt" <<EOF
step=5
name=https
status=execute_ok
https_url=$https_url
probe_ran=1
https=executed
source=loop_create
EOF
fi

update_digest=${A3S_CLOUD_BX0_UPDATE_DIGEST:-}

if [[ $create_full == 1 ]]; then
  if [[ -z $health_url || -z $https_url ]]; then
    fail_blocked create_full_requires_health_https
  fi
  if [[ -z $box_instance_id || $box_instance_id == PLACEHOLDER_* ]]; then
    fail_blocked box_instance_unobserved \
      "CREATE_FULL requires deployments[].observedRuntime.providerResourceId before cleanup"
  fi
  # Prove the id is a real Box instance while it still exists.
  set +e
  "$box_bin" inspect "$box_instance_id" >"$evidence_directory/box-inspect-before.txt" 2>&1
  inspect_before_rc=$?
  set -e
  if ((inspect_before_rc != 0)); then
    fail_blocked box_inspect_before_failed \
      "providerResourceId=$box_instance_id must be inspectable before stop/rm"
  fi
  record PASS "a3s-box inspect ok before cleanup for $box_instance_id"

  set +e
  logs_json=
  logs_rc=1
  logs_cursor=
  logs_deadline=$((SECONDS + ${A3S_CLOUD_BX0_LOGS_TIMEOUT_SEC:-120}))
  while ((SECONDS < logs_deadline)); do
    # Nudge guest stdout so json-file log shipping has something to upload.
    env A3S_HOME="${A3S_HOME:-}" "$box_bin" exec "$box_instance_id" -- \
      /bin/sh -c 'echo bx0-software-log-heartbeat' >/dev/null 2>&1 || true
    logs_json=$(cloud_cli workloads logs "$workload_id" "$revision_id" 2>"$evidence_directory/logs.err")
    logs_rc=$?
    if ((logs_rc == 0)); then
      printf '%s\n' "$logs_json" >"$evidence_directory/logs.json"
      if logs_cursor=$(
        python3 - "$logs_json" <<'PY'
import json, sys
page = json.loads(sys.argv[1])
cursor = page.get("nextCursor")
if cursor:
    print(cursor)
    raise SystemExit(0)
rows = page.get("items") or page.get("records") or []
if rows:
    seq = rows[-1].get("sequence")
    if seq is not None:
        print(str(seq))
        raise SystemExit(0)
raise SystemExit(2)
PY
      ); then
        break
      fi
      logs_cursor=
    fi
    sleep 3
  done
  set -e
  if ((logs_rc != 0)); then
    fail_blocked workloads_logs_failed "exit=$logs_rc stderr=$(head -c 400 "$evidence_directory/logs.err" 2>/dev/null || true)"
  fi
  printf '%s\n' "$logs_json" >"$evidence_directory/logs.json"
  if [[ -z ${logs_cursor:-} ]]; then
    fail_blocked logs_cursor_missing \
      "body=$(printf '%s' "$logs_json" | head -c 400)"
  fi
  export A3S_CLOUD_BX0_LOGS_CURSOR=$logs_cursor
  record PASS "logs cursor=$logs_cursor"
  mkdir -p -- "$evidence_directory/receipts"
  cat >"$evidence_directory/receipts/06-logs.txt" <<EOF
step=6
name=logs
status=execute_ok
logs_cursor=$logs_cursor
probe_ran=1
logs=executed
source=loop_create
EOF

  update_uri=${A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI:-}
  if [[ -z $update_digest || -z $update_uri || $update_digest == PLACEHOLDER_* ]]; then
    fail_blocked create_full_requires_update_artifact
  fi
  if [[ $update_digest =~ ^[0-9a-f]{64}$ ]]; then
    update_digest="sha256:$update_digest"
  fi
  if [[ ! $update_digest =~ ^sha256:[0-9a-f]{64}$ ]]; then
    fail_blocked update_digest_invalid
  fi
  update_acl="$evidence_directory/workload.update.acl"
  cat >"$update_acl" <<EOF
version = 1

workload "$workload_name" {
  artifact {
    uri = "$update_uri"
    expected_digest = "$update_digest"
  }

  process {
    command = ["/bin/sh"]
    args = ["-c", "mkdir -p /tmp/www && printf ready >/tmp/www/ready && sleep 2 && while true; do printf 'HTTP/1.0 200 OK\\r\\nContent-Length: 5\\r\\nConnection: close\\r\\n\\r\\nready' | /bin/busybox nc -l -p 8080 || true; done"]
  }

  resources {
    # MicroVM a3s-box does not advertise EphemeralStorage (writable-layer
    # quota is Sandbox-only). Requesting it makes Schedule stay Pending forever.
    cpu_millis = 100
    memory_bytes = 67108864
    pids = 64
  }

  port "http" {
    container_port = 8080
  }

  health {
    port_name = "http"
    path = "/ready"
    interval_ms = 3000
    timeout_ms = 2000
    healthy_threshold = 1
    unhealthy_threshold = 15
    stabilization_window_ms = 10000
  }
}
EOF
  idem_update="bx0-create-update-$(random_hex 8)"
  set +e
  update_json=$(
    cloud_cli workloads update "$workload_id" \
      --file="$update_acl" \
      --idempotency-key="$idem_update" \
      2>"$evidence_directory/workload-update.err"
  )
  update_rc=$?
  set -e
  printf '%s\n' "$update_json" >"$evidence_directory/workload-update.json"
  if ((update_rc != 0)); then
    fail_blocked workloads_update_failed \
      "exit=$update_rc body=$(printf '%s' "$update_json" | head -c 400) stderr=$(head -c 400 "$evidence_directory/workload-update.err" 2>/dev/null || true)"
  fi
  update_revision_id=$(json_get "$update_json" revisionId) \
    || fail_blocked workloads_update_parse "revisionId"
  export A3S_CLOUD_BX0_UPDATE_DIGEST=$update_digest
  export A3S_CLOUD_BX0_UPDATE_REVISION_ID=$update_revision_id
  record PASS "updated to digest=$update_digest revision_id=$update_revision_id"

  # Rollback targets the prior revision; wait until the update revision is
  # *active* (not merely desired). Accepting desired-only left active on the
  # original revision and made rollback CONFLICT with "already using".
  update_ready=0
  update_deadline=$((SECONDS + ${A3S_CLOUD_BX0_UPDATE_TIMEOUT_SEC:-180}))
  while ((SECONDS < update_deadline)); do
    set +e
    get_json=$(cloud_cli workloads get "$workload_id" 2>"$evidence_directory/workload-get-after-update.err")
    get_rc=$?
    set -e
    if ((get_rc == 0)); then
      printf '%s\n' "$get_json" >"$evidence_directory/workload-get-after-update.json"
      if python3 - "$get_json" "$update_revision_id" <<'PY'
import json, sys
w = json.loads(sys.argv[1])
want = sys.argv[2]
active = str((w.get("activeRevision") or {}).get("id") or w.get("activeRevisionId") or "")
raise SystemExit(0 if want and want == active else 2)
PY
      then
        update_ready=1
        break
      fi
    fi
    sleep 2
  done
  if ((update_ready != 1)); then
    fail_blocked workloads_update_not_active \
      "update_revision_id=$update_revision_id body=$(head -c 400 "$evidence_directory/workload-get-after-update.json" 2>/dev/null || true)"
  fi
  record PASS "update revision active=$update_revision_id"

  rollback_rev=$revision_id
  # Rollback creates a new deployment; API requires no nonterminal siblings
  # (retiring/cleanup_pending/queued/…). Wait for prior deployment to finish.
  settle_ready=0
  settle_deadline=$((SECONDS + ${A3S_CLOUD_BX0_ROLLBACK_SETTLE_TIMEOUT_SEC:-240}))
  while ((SECONDS < settle_deadline)); do
    set +e
    settle_json=$(cloud_cli workloads get "$workload_id" 2>"$evidence_directory/workload-get-before-rollback.err")
    settle_rc=$?
    set -e
    if ((settle_rc == 0)); then
      printf '%s\n' "$settle_json" >"$evidence_directory/workload-get-before-rollback.json"
      if python3 - "$settle_json" "$update_revision_id" <<'PY'
import json, sys
w = json.loads(sys.argv[1])
want = sys.argv[2]
active = str((w.get("activeRevision") or {}).get("id") or w.get("activeRevisionId") or "")
if active != want:
    raise SystemExit(2)
terminal = {"active", "failed", "orphaned", "cancelled"}
for dep in w.get("deployments") or []:
    status = str(dep.get("status") or "").lower()
    if status and status not in terminal:
        raise SystemExit(3)
raise SystemExit(0)
PY
      then
        settle_ready=1
        break
      fi
    fi
    sleep 2
  done
  if ((settle_ready != 1)); then
    fail_blocked workloads_update_not_settled \
      "update active but nonterminal deployments remain; body=$(head -c 600 "$evidence_directory/workload-get-before-rollback.json" 2>/dev/null || true)"
  fi
  record PASS "deployments settled before rollback"

  # If somehow still on the prior revision, fail closed with evidence rather than
  # calling rollback (API returns CONFLICT "already using").
  set +e
  pre_rb=$(cloud_cli workloads get "$workload_id" 2>/dev/null)
  set -e
  if python3 - "$pre_rb" "$rollback_rev" "$update_revision_id" <<'PY'
import json, sys
w = json.loads(sys.argv[1] or "{}")
prior = sys.argv[2]
update = sys.argv[3]
active = str((w.get("activeRevision") or {}).get("id") or w.get("activeRevisionId") or "")
if active == prior:
    raise SystemExit(3)
if active != update:
    raise SystemExit(4)
raise SystemExit(0)
PY
  then
    :
  else
    pre_rc=$?
    if ((pre_rc == 3)); then
      fail_blocked workloads_update_not_active \
        "active still prior revision; cannot rollback prior onto itself active=$rollback_rev"
    fi
    fail_blocked workloads_update_not_active \
      "active is neither update nor expected prior before rollback"
  fi

  idem_rb="bx0-create-rollback-$(random_hex 8)"
  set +e
  rb_json=$(
    cloud_cli workloads rollback "$workload_id" "$rollback_rev" \
      --idempotency-key="$idem_rb" \
      2>"$evidence_directory/workload-rollback.err"
  )
  rb_rc=$?
  set -e
  printf '%s\n' "$rb_json" >"$evidence_directory/workload-rollback.json"
  if ((rb_rc != 0)); then
    # Idempotent success: already on prior after a racing reconcile.
    if grep -Fq 'already using the requested rollback revision' \
      "$evidence_directory/workload-rollback.err" 2>/dev/null \
      || printf '%s' "$rb_json" | grep -Fq 'already using the requested rollback revision'; then
      set +e
      cur=$(cloud_cli workloads get "$workload_id" 2>/dev/null)
      set -e
      if python3 - "$cur" "$rollback_rev" <<'PY'
import json, sys
w = json.loads(sys.argv[1] or "{}")
want = sys.argv[2]
active = str((w.get("activeRevision") or {}).get("id") or w.get("activeRevisionId") or "")
raise SystemExit(0 if active == want else 2)
PY
      then
        export A3S_CLOUD_BX0_ROLLBACK_DIGEST=$digest
        record PASS "rolled back (already active) to digest=$digest revision_id=$rollback_rev"
      else
        fail_blocked workloads_rollback_failed \
          "exit=$rb_rc already-using but active!=prior body=$(printf '%s' "$rb_json" | head -c 400)"
      fi
    else
      fail_blocked workloads_rollback_failed \
        "exit=$rb_rc body=$(printf '%s' "$rb_json" | head -c 400) stderr=$(head -c 400 "$evidence_directory/workload-rollback.err" 2>/dev/null || true)"
    fi
  else
    export A3S_CLOUD_BX0_ROLLBACK_DIGEST=$digest
    record PASS "rolled back to digest=$digest revision_id=$rollback_rev"
  fi

  idem_stop="bx0-create-stop-$(random_hex 8)"
  set +e
  stop_json=$(
    cloud_cli workloads stop "$workload_id" \
      --idempotency-key="$idem_stop"
  )
  stop_rc=$?
  set -e
  printf '%s\n' "$stop_json" >"$evidence_directory/workload-stop.json"
  if ((stop_rc != 0)); then
    fail_blocked workloads_stop_failed "exit=$stop_rc"
  fi
  record PASS "workloads stop ok"

  # Gate step 9 only verifies inspect-absent; create must actually remove the Box.
  set +e
  "$box_bin" rm --force "$box_instance_id" \
    >"$evidence_directory/box-rm.out" 2>"$evidence_directory/box-rm.err"
  rm_rc=$?
  if ((rm_rc != 0)); then
    "$box_bin" rm -f "$box_instance_id" \
      >"$evidence_directory/box-rm.out" 2>"$evidence_directory/box-rm.err"
    rm_rc=$?
  fi
  set -e
  if ((rm_rc != 0)); then
    # Accept already-absent after stop, but only if inspect now fails.
    set +e
    "$box_bin" inspect "$box_instance_id" >/dev/null 2>&1
    still_present=$?
    set -e
    if ((still_present == 0)); then
      fail_blocked box_rm_failed "providerResourceId=$box_instance_id still inspectable"
    fi
    record PASS "box already absent after stop (rm exit=$rm_rc)"
  else
    record PASS "a3s-box rm --force $box_instance_id"
  fi

  set +e
  "$box_bin" inspect "$box_instance_id" >"$evidence_directory/box-inspect-after.txt" 2>&1
  inspect_after_rc=$?
  set -e
  if ((inspect_after_rc == 0)); then
    fail_blocked box_still_present "providerResourceId=$box_instance_id"
  fi
  export A3S_CLOUD_BX0_CLEANUP_INSTANCE=$box_instance_id
  record PASS "cleanup verified inspect-absent; CLEANUP_INSTANCE=$box_instance_id"
fi

export_file=${A3S_CLOUD_BX0_CREATE_OUT:-$evidence_directory/bx0-software-loop-env.sh}
{
  printf '# Generated by run_bx0_software_loop_create.sh — source to arm LIVE harness\n'
  printf 'export A3S_CLOUD_ENROLLMENT_TOKEN=%q\n' "$A3S_CLOUD_ENROLLMENT_TOKEN"
  printf 'export A3S_CLOUD_BX0_NODE_CONFIG=%q\n' "$node_config"
  printf 'export A3S_CLOUD_BX0_ENROLL_NODE_ID=%q\n' "$A3S_CLOUD_BX0_ENROLL_NODE_ID"
  printf 'export A3S_CLOUD_BX0_ARTIFACT_DIGEST=%q\n' "$A3S_CLOUD_BX0_ARTIFACT_DIGEST"
  printf 'export A3S_CLOUD_BX0_SERVICE_ID=%q\n' "$A3S_CLOUD_BX0_SERVICE_ID"
  printf 'export A3S_CLOUD_BX0_WORKLOAD_ID=%q\n' "$A3S_CLOUD_BX0_WORKLOAD_ID"
  printf 'export A3S_CLOUD_BX0_REVISION_ID=%q\n' "$A3S_CLOUD_BX0_REVISION_ID"
  if [[ -n ${A3S_CLOUD_BX0_HEALTH_URL:-} ]]; then
    printf 'export A3S_CLOUD_BX0_HEALTH_URL=%q\n' "$A3S_CLOUD_BX0_HEALTH_URL"
  fi
  if [[ -n ${A3S_CLOUD_BX0_HTTPS_URL:-} ]]; then
    printf 'export A3S_CLOUD_BX0_HTTPS_URL=%q\n' "$A3S_CLOUD_BX0_HTTPS_URL"
  fi
  if [[ -n ${A3S_CLOUD_BX0_LOGS_CURSOR:-} ]]; then
    printf 'export A3S_CLOUD_BX0_LOGS_CURSOR=%q\n' "$A3S_CLOUD_BX0_LOGS_CURSOR"
  fi
  if [[ -n ${A3S_CLOUD_BX0_UPDATE_DIGEST:-} ]]; then
    printf 'export A3S_CLOUD_BX0_UPDATE_DIGEST=%q\n' "$A3S_CLOUD_BX0_UPDATE_DIGEST"
  fi
  if [[ -n ${A3S_CLOUD_BX0_ROLLBACK_DIGEST:-} ]]; then
    printf 'export A3S_CLOUD_BX0_ROLLBACK_DIGEST=%q\n' "$A3S_CLOUD_BX0_ROLLBACK_DIGEST"
  fi
  if [[ -n ${A3S_CLOUD_BX0_CLEANUP_INSTANCE:-} ]]; then
    printf 'export A3S_CLOUD_BX0_CLEANUP_INSTANCE=%q\n' "$A3S_CLOUD_BX0_CLEANUP_INSTANCE"
  fi
  if [[ -d $evidence_directory/receipts ]]; then
    printf 'export A3S_CLOUD_BX0_PRIOR_RECEIPTS_DIR=%q\n' "$evidence_directory/receipts"
  fi
  printf 'export A3S_CLOUD_BOX_BIN=%q\n' "$A3S_CLOUD_BOX_BIN"
} >"$export_file"

created_phase=enroll_oci_deploy
if [[ $create_full == 1 ]]; then
  created_phase=enroll_through_cleanup
fi

cat <<EOF | tee "$evidence_directory/bx0-software-loop-create.txt"
A3S_CLOUD_BX0_SOFTWARE_LOOP_CREATE_OK phase=$created_phase
cloud_revision=$(git -C "$repository_root" rev-parse HEAD)
node_id=$A3S_CLOUD_BX0_ENROLL_NODE_ID
artifact_digest=$A3S_CLOUD_BX0_ARTIFACT_DIGEST
service_id=$A3S_CLOUD_BX0_SERVICE_ID
workload_id=$A3S_CLOUD_BX0_WORKLOAD_ID
revision_id=$A3S_CLOUD_BX0_REVISION_ID
export_file=$export_file
agent_pid=$(cat "$agent_pid_file")
product_exit=not_claimed
loop_certified=not_claimed
honesty=Pre-published OCI digest required; create never invents digests/EXIT/Power.
EOF

record PASS "create phase=$created_phase export=$export_file"
exit 0
