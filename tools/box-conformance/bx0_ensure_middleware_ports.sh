#!/usr/bin/env bash
# Ensure BX0 LIVE middleware host port_maps are bound together.
#
# a3s-box compose publishes host ports via passt. On this host:
# - passt.avx2 re-execs the non-AVX2 sibling; both must be on PATH
# - TIME_WAIT / stale relays on 54320/50020 make passt exit ("Address already in use")
# - compose restart while ports are merely flapping destroys healthy guests
#
# Usage (source, then call):
#   source tools/box-conformance/bx0_ensure_middleware_ports.sh
#   bx0_ensure_middleware_ports "$box_bin" "$compose_acl" [evidence_dir]

bx0_port_reachable() {
  local host=$1
  local port=$2
  if command -v nc >/dev/null 2>&1; then
    nc -z -w2 "$host" "$port" >/dev/null 2>&1
  else
    (echo >"/dev/tcp/${host}/${port}") >/dev/null 2>&1
  fi
}

bx0_wait_port() {
  local host=$1
  local port=$2
  local seconds=${3:-60}
  local deadline=$((SECONDS + seconds))
  while ((SECONDS < deadline)); do
    bx0_port_reachable "$host" "$port" && return 0
    sleep 1
  done
  return 1
}

bx0_middleware_ports_ok() {
  local pg=${A3S_CLOUD_POSTGRES_PORT:-54320}
  local reg=${A3S_CLOUD_REGISTRY_PORT:-50020}
  bx0_port_reachable 127.0.0.1 "$pg" || return 1
  bx0_port_reachable 127.0.0.1 "$reg" || return 1
  return 0
}

# Ensure passt + passt.avx2 siblings are first on PATH (AVX2 build re-execs sibling).
bx0_ensure_passt_on_path() {
  local src=${A3S_CLOUD_BX0_PASST_DIR:-/home/roylin/code/a3s-box-install}
  local bin=/tmp/bx0-passt-bin
  mkdir -p -- "$bin"
  if [[ -x $src/passt && -x $src/passt.avx2 ]]; then
    cp -f -- "$src/passt" "$bin/passt"
    cp -f -- "$src/passt.avx2" "$bin/passt.avx2"
    export PATH="$bin:$PATH"
  fi
}

# Wait until a host TCP port can be bound (clears TIME_WAIT / self-connect debris).
bx0_wait_port_bindable() {
  local port=$1
  local seconds=${2:-90}
  local deadline=$((SECONDS + seconds))
  while ((SECONDS < deadline)); do
    if python3 - "$port" <<'PY' 2>/dev/null
import socket, sys
port = int(sys.argv[1])
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
try:
    s.bind(("0.0.0.0", port))
except OSError:
    sys.exit(1)
finally:
    s.close()
sys.exit(0)
PY
    then
      return 0
    fi
    sleep 1
  done
  return 1
}

bx0_free_middleware_host_ports() {
  local pg=${A3S_CLOUD_POSTGRES_PORT:-54320}
  local reg=${A3S_CLOUD_REGISTRY_PORT:-50020}
  local nats=${A3S_CLOUD_NATS_PORT:-42220}
  local port pid
  for port in "$pg" "$reg" "$nats" 8220; do
    while read -r pid; do
      [[ -n $pid ]] || continue
      kill "$pid" 2>/dev/null || true
    done < <(
      ss -ltnp 2>/dev/null \
        | awk -v p=":$port" '$4 ~ p {print}' \
        | sed -n 's/.*pid=\([0-9]*\).*/\1/p' \
        | sort -u
    )
    # Unprivileged ss often omits pid=; fuser still finds listeners.
    if command -v fuser >/dev/null 2>&1; then
      fuser -k "${port}/tcp" >/dev/null 2>&1 || true
    fi
  done
  # Kill compose middleware passt by recorded pid files (ss may hide owners).
  python3 - <<'PY' 2>/dev/null || true
import json, os, signal
boxes_path = os.path.expanduser("~/.a3s/boxes.json")
if not os.path.exists(boxes_path):
    raise SystemExit(0)
try:
    boxes = json.load(open(boxes_path))
except Exception:
    raise SystemExit(0)
for b in boxes:
    name = str(b.get("name", ""))
    if not name.startswith("dev-"):
        continue
    if name not in ("dev-postgres", "dev-registry", "dev-nats"):
        # Also free any other compose project middleware boxes.
        if not any(x in name for x in ("postgres", "registry", "nats")):
            continue
    pidf = f"/tmp/a3s-box-sockets/{b.get('id', '')}/passt.pid"
    if not os.path.exists(pidf):
        continue
    try:
        pid = int(open(pidf).read().strip())
    except Exception:
        continue
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    except PermissionError:
        pass
    try:
        os.remove(pidf)
    except OSError:
        pass
PY
  for name in pg reg nats; do
    if [[ -f /tmp/bx0-relay-${name}.pid ]]; then
      kill "$(cat /tmp/bx0-relay-${name}.pid)" 2>/dev/null || true
      rm -f "/tmp/bx0-relay-${name}.pid"
    fi
  done
}

# Recover a single middleware service whose guest is "running" but host passt died.
# Prefer this over full free+restart — killing healthy postgres invalidates tenants.
bx0_recover_middleware_service() {
  local box_bin=$1
  local compose_acl=$2
  local service=$3
  local host_port=$4
  local evidence_directory=${5:-}
  local log_prefix=

  [[ -n $box_bin && -x $box_bin && -f $compose_acl && -n $service && -n $host_port ]] || return 2
  bx0_ensure_passt_on_path

  if [[ -n $evidence_directory ]]; then
    mkdir -p -- "$evidence_directory"
    log_prefix="$evidence_directory/"
    printf '%s\n' "recovering middleware service=${service} host_port=${host_port}" \
      | tee -a "${log_prefix}middleware-port-rebind.txt"
  fi

  if command -v fuser >/dev/null 2>&1; then
    fuser -k "${host_port}/tcp" >/dev/null 2>&1 || true
  fi
  # Stale passt often survives fuser; kill listeners by inode/cmdline.
  if [[ $service == registry ]]; then
    python3 - <<'PY' || true
import os, signal
port = int(os.environ.get("A3S_CLOUD_REGISTRY_PORT", "50020"))
port_hex = f"{port:04X}"
for pid in os.listdir("/proc"):
    if not pid.isdigit():
        continue
    try:
        cmd = open(f"/proc/{pid}/cmdline", "rb").read().replace(b"\0", b" ").decode(errors="replace")
    except OSError:
        continue
    if "passt" in cmd and str(port) in cmd:
        try:
            os.kill(int(pid), signal.SIGKILL)
        except OSError:
            pass
inodes = set()
try:
    with open("/proc/net/tcp") as fh:
        next(fh)
        for line in fh:
            parts = line.split()
            if parts[1].upper().endswith(f":{port_hex}"):
                inodes.add(parts[9])
except OSError:
    pass
for pid in os.listdir("/proc"):
    if not pid.isdigit():
        continue
    fd_dir = f"/proc/{pid}/fd"
    try:
        fds = os.listdir(fd_dir)
    except OSError:
        continue
    for fd in fds:
        try:
            target = os.readlink(f"{fd_dir}/{fd}")
        except OSError:
            continue
        if any(ino in target for ino in inodes):
            try:
                os.kill(int(pid), signal.SIGKILL)
            except OSError:
                pass
PY
  fi
  bx0_wait_port_bindable "$host_port" 30 || return 7

  # stop+up is more reliable than restart when passt.pid is stale.
  if [[ -n $log_prefix ]]; then
    "$box_bin" compose --file "$compose_acl" stop "$service" \
      >>"${log_prefix}middleware-compose-restart.out" 2>>"${log_prefix}middleware-compose-restart.err" \
      || true
    "$box_bin" compose --file "$compose_acl" up --detach --timeout 180 "$service" \
      >>"${log_prefix}middleware-compose-restart.out" 2>>"${log_prefix}middleware-compose-restart.err" \
      || return 3
  else
    "$box_bin" compose --file "$compose_acl" stop "$service" >/dev/null 2>&1 || true
    "$box_bin" compose --file "$compose_acl" up --detach --timeout 180 "$service" >/dev/null 2>&1 \
      || return 3
  fi

  bx0_wait_port 127.0.0.1 "$host_port" 60
}

bx0_ensure_middleware_ports() {
  local box_bin=$1
  local compose_acl=$2
  local evidence_directory=${3:-}
  local pg=${A3S_CLOUD_POSTGRES_PORT:-54320}
  local reg=${A3S_CLOUD_REGISTRY_PORT:-50020}
  local nats=${A3S_CLOUD_NATS_PORT:-42220}
  local log_prefix=
  local pg_ok=0
  local reg_ok=0

  [[ -n $box_bin && -x $box_bin && -f $compose_acl ]] || return 2

  bx0_ensure_passt_on_path

  if [[ -n $evidence_directory ]]; then
    mkdir -p -- "$evidence_directory"
    log_prefix="$evidence_directory/"
  fi

  if bx0_middleware_ports_ok; then
    return 0
  fi

  bx0_port_reachable 127.0.0.1 "$pg" && pg_ok=1
  bx0_port_reachable 127.0.0.1 "$reg" && reg_ok=1

  # Surgical path: postgres still mapped, only registry passt died (common).
  if ((pg_ok == 1 && reg_ok == 0)); then
    if bx0_recover_middleware_service "$box_bin" "$compose_acl" registry "$reg" \
      "$evidence_directory"; then
      bx0_middleware_ports_ok && return 0
    fi
  fi

  # Surgical path: registry mapped, postgres passt died.
  if ((pg_ok == 0 && reg_ok == 1)); then
    if bx0_recover_middleware_service "$box_bin" "$compose_acl" postgres "$pg" \
      "$evidence_directory"; then
      bx0_middleware_ports_ok && return 0
    fi
  fi

  if [[ -n $evidence_directory ]]; then
    printf '%s\n' "middleware host ports missing; preparing bindable ports + full compose restart" \
      | tee "${log_prefix}middleware-port-rebind.txt"
  fi

  bx0_free_middleware_host_ports
  sleep 1
  bx0_wait_port_bindable "$pg" 90 || return 6
  bx0_wait_port_bindable "$reg" 30 || return 7

  # Full project restart — partial restart drops sibling port_maps.
  if [[ -n $log_prefix ]]; then
    "$box_bin" compose --file "$compose_acl" restart \
      >"${log_prefix}middleware-compose-restart.out" 2>"${log_prefix}middleware-compose-restart.err" \
      || true
  else
    "$box_bin" compose --file "$compose_acl" restart >/dev/null 2>&1 || true
  fi

  if bx0_wait_port 127.0.0.1 "$pg" 45 && bx0_wait_port 127.0.0.1 "$reg" 45; then
    bx0_wait_port 127.0.0.1 "$nats" 15 || true
    return 0
  fi

  if [[ -n $evidence_directory ]]; then
    printf '%s\n' "ports still missing after restart; compose down/up" \
      | tee -a "${log_prefix}middleware-port-rebind.txt"
  fi

  bx0_free_middleware_host_ports
  sleep 1
  bx0_wait_port_bindable "$pg" 90 || return 6
  bx0_wait_port_bindable "$reg" 30 || return 7

  if [[ -n $log_prefix ]]; then
    "$box_bin" compose --file "$compose_acl" down \
      >>"${log_prefix}middleware-compose-restart.out" 2>>"${log_prefix}middleware-compose-restart.err" \
      || true
    "$box_bin" compose --file "$compose_acl" up --detach --timeout 180 \
      >>"${log_prefix}middleware-compose-restart.out" 2>>"${log_prefix}middleware-compose-restart.err" \
      || return 3
  else
    "$box_bin" compose --file "$compose_acl" down >/dev/null 2>&1 || true
    "$box_bin" compose --file "$compose_acl" up --detach --timeout 180 >/dev/null 2>&1 \
      || return 3
  fi

  bx0_wait_port 127.0.0.1 "$pg" 90 || return 4
  bx0_wait_port 127.0.0.1 "$reg" 90 || return 5
  bx0_wait_port 127.0.0.1 "$nats" 30 || true
  return 0
}

# Verify a digest-pinned OCI manifest is reachable over HTTP (insecure local registry).
# Args: oci_uri digest  (digest may be sha256:… or bare 64-hex)
bx0_oci_manifest_reachable() {
  local uri=$1
  local digest=$2
  if [[ $digest =~ ^[0-9a-f]{64}$ ]]; then
    digest="sha256:$digest"
  fi
  [[ $digest =~ ^sha256:[0-9a-f]{64}$ ]] || return 2
  [[ $uri == oci://* ]] || return 2

  local rest=${uri#oci://}
  local registry=${rest%%/*}
  local path_and_digest=${rest#*/}
  local repo=${path_and_digest%%@*}
  local dig_hex=${digest#sha256:}

  if command -v crane >/dev/null 2>&1; then
    crane manifest --insecure "${registry}/${repo}@${digest}" >/dev/null 2>&1 && return 0
  fi
  curl -fsS --max-time 10 \
    -H 'Accept: application/vnd.oci.image.manifest.v1+json,application/vnd.docker.distribution.manifest.v2+json' \
    "http://${registry}/v2/${repo}/manifests/sha256:${dig_hex}" \
    -o /dev/null
}

# Rewrite a registry tag to OCI image manifest media type (Runtime admit path).
bx0_oci_normalize_tag() {
  local ref=$1
  if ! command -v skopeo >/dev/null 2>&1; then
    if [[ -x /tmp/bx0-tools/skopeo ]]; then
      export PATH="/tmp/bx0-tools:${PATH}"
    fi
  fi
  command -v skopeo >/dev/null 2>&1 || return 20
  mkdir -p /tmp/bx0-containers "${HOME:-/tmp}/.config/containers"
  cat >/tmp/bx0-containers/policy.json <<'POLICY'
{
  "default": [{"type": "insecureAcceptAnything"}]
}
POLICY
  cp /tmp/bx0-containers/policy.json "${HOME:-/tmp}/.config/containers/policy.json"
  export CONTAINERS_POLICY=/tmp/bx0-containers/policy.json
  export REGISTRY_AUTH_FILE=${REGISTRY_AUTH_FILE:-/tmp/bx0-containers/auth.json}
  printf '%s\n' '{}' >"${REGISTRY_AUTH_FILE}"
  local tmp_ref="${ref%:*}:oci-norm-restore-$$"
  skopeo copy --policy "$CONTAINERS_POLICY" --format=oci --src-tls-verify=false --dest-tls-verify=false \
    "docker://${ref}" "docker://${tmp_ref}" >/dev/null 2>&1 || return 21
  skopeo copy --policy "$CONTAINERS_POLICY" --src-tls-verify=false --dest-tls-verify=false \
    "docker://${tmp_ref}" "docker://${ref}" >/dev/null 2>&1 || return 22
  return 0
}

# Restore operator tags from durable crane tarballs (no hub re-pull).
# Env:
#   A3S_CLOUD_BX0_OCI_CACHE_DIR  (default: .a3s/cloud/bx0-live-prep/oci-cache)
# Args: registry_host repo tag_a tag_b expected_digest_a [expected_digest_b]
bx0_oci_restore_tags() {
  local registry_host=$1
  local repo=$2
  local tag_a=$3
  local tag_b=$4
  local expected_a=$5
  local expected_b=${6:-}
  local cache_dir=${A3S_CLOUD_BX0_OCI_CACHE_DIR:-}

  if [[ $expected_a =~ ^[0-9a-f]{64}$ ]]; then
    expected_a="sha256:$expected_a"
  fi
  if [[ -n $expected_b && $expected_b =~ ^[0-9a-f]{64}$ ]]; then
    expected_b="sha256:$expected_b"
  fi

  if ! command -v crane >/dev/null 2>&1; then
    return 10
  fi
  if [[ -z $cache_dir || ! -d $cache_dir ]]; then
    return 17
  fi

  local tar_a="$cache_dir/${tag_a}.tar"
  local tar_b="$cache_dir/${tag_b}.tar"
  [[ -f $tar_a ]] || return 18

  crane push --insecure "$tar_a" "${registry_host}/${repo}:${tag_a}" || return 11
  local got_a
  got_a=$(crane digest --insecure "${registry_host}/${repo}:${tag_a}") || return 12
  if [[ $got_a != "$expected_a" ]]; then
    # Cached tarball may still be Docker Schema 2 while EXIT expects OCI.
    bx0_oci_normalize_tag "${registry_host}/${repo}:${tag_a}" || return 13
    got_a=$(crane digest --insecure "${registry_host}/${repo}:${tag_a}") || return 12
    [[ $got_a == "$expected_a" ]] || return 13
  fi

  if [[ -n $expected_b ]]; then
    [[ -f $tar_b ]] || return 19
    crane push --insecure "$tar_b" "${registry_host}/${repo}:${tag_b}" || return 14
    local got_b
    got_b=$(crane digest --insecure "${registry_host}/${repo}:${tag_b}") || return 15
    if [[ $got_b != "$expected_b" ]]; then
      bx0_oci_normalize_tag "${registry_host}/${repo}:${tag_b}" || return 16
      got_b=$(crane digest --insecure "${registry_host}/${repo}:${tag_b}") || return 15
      [[ $got_b == "$expected_b" ]] || return 16
    fi
  fi
  return 0
}
