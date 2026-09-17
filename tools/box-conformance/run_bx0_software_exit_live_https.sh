#!/usr/bin/env bash
# Bind a local HTTPS URL in front of the Box health HTTP port for GA-0 probes.
#
# Uses the live-prep TLS CA (from agent-release) or generates one. Installs a
# CA-aware curl probe as A3S_CLOUD_HEALTH_PROBE_BIN so gate HTTPS execute can
# verify without curl -k theater.
#
# Also writes a node.acl with absolute paths under the live-prep state dir.
#
# Env in:
#   A3S_CLOUD_BX0_HEALTH_URL (http://127.0.0.1:PORT/ready) — or HEALTH_HOST_PORT
#   A3S_CLOUD_BX0_TLS_CA_FILE (optional; shared with agent-release)
#
# Env out:
#   A3S_CLOUD_BX0_HTTPS_URL
#   A3S_CLOUD_HEALTH_PROBE_BIN
#   A3S_CLOUD_BX0_NODE_CONFIG
#
# Usage:
#   bash tools/box-conformance/run_bx0_software_exit_live_https.sh [EVIDENCE_DIR]

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
state_dir=${A3S_CLOUD_BX0_LIVE_PREP_STATE_DIR:-$repository_root/.a3s/cloud/bx0-live-prep}
tls_dir="$state_dir/tls"
example_acl="$repository_root/config/node.example.acl"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-live-https.XXXXXX")
fi
mkdir -p -- "$evidence_directory" "$tls_dir" "$state_dir"

fail_blocked() {
  local reason=$1
  shift || true
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-https.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_HTTPS_BLOCKED reason=$reason
$*
product_exit=not_claimed
loop_certified=not_claimed
EOF
  exit 2
}

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
bx0_refuse_docker_host
for cmd in openssl python3 curl; do
  command -v "$cmd" >/dev/null 2>&1 || fail_blocked "${cmd}_unavailable"
done

ca_key="$tls_dir/ca.key"
ca_cert=${A3S_CLOUD_BX0_TLS_CA_FILE:-$tls_dir/ca.pem}
proxy_key="$tls_dir/https-proxy.key"
proxy_csr="$tls_dir/https-proxy.csr"
proxy_cert="$tls_dir/https-proxy.pem"

if [[ ! -f $ca_cert ]]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ca_key" -out "$ca_cert" \
    -days 30 -subj '/CN=a3s-bx0-live-ca' \
    >"$evidence_directory/openssl-ca.out" 2>"$evidence_directory/openssl-ca.err" \
    || fail_blocked openssl_ca_failed
fi
if [[ ! -f $ca_key ]]; then
  # CA cert provided externally without key — cannot mint proxy cert
  fail_blocked tls_ca_key_missing "path=$ca_key"
fi

openssl req -newkey rsa:2048 -nodes -keyout "$proxy_key" -out "$proxy_csr" \
  -subj '/CN=127.0.0.1' \
  >"$evidence_directory/openssl-proxy-csr.out" 2>"$evidence_directory/openssl-proxy-csr.err" \
  || fail_blocked openssl_proxy_csr_failed
san_ext="$tls_dir/https-proxy-san.ext"
printf 'subjectAltName=IP:127.0.0.1,DNS:localhost\n' >"$san_ext"
openssl x509 -req -in "$proxy_csr" -CA "$ca_cert" -CAkey "$ca_key" -CAcreateserial \
  -out "$proxy_cert" -days 30 -extfile "$san_ext" \
  >"$evidence_directory/openssl-proxy-sign.out" 2>"$evidence_directory/openssl-proxy-sign.err" \
  || fail_blocked openssl_proxy_sign_failed

health_url=${A3S_CLOUD_BX0_HEALTH_URL:-}
health_port=${A3S_CLOUD_BX0_HEALTH_HOST_PORT:-18080}
if [[ -z $health_url ]]; then
  health_url="http://127.0.0.1:${health_port}/ready"
fi
if [[ ! $health_url =~ ^http://127\.0\.0\.1:([0-9]+)/ ]]; then
  # Still allow explicit HEALTH_URL; derive backend port when loopback
  if [[ $health_url =~ ^http://127\.0\.0\.1:([0-9]+) ]]; then
    health_port=${BASH_REMATCH[1]}
  else
    fail_blocked health_url_not_loopback \
      "live https binder expects loopback HEALTH_URL for local TLS terminate; got=$health_url"
  fi
else
  health_port=${BASH_REMATCH[1]}
fi

https_port=${A3S_CLOUD_BX0_HTTPS_PORT:-18444}
pid_file="$tls_dir/https-proxy.pid"
if [[ -f $pid_file ]]; then
  kill "$(cat "$pid_file")" 2>/dev/null || true
fi

# Tiny TLS reverse proxy: terminate TLS and forward to health_port
python3 - "$https_port" "$health_port" "$proxy_cert" "$proxy_key" <<'PY' \
  >"$evidence_directory/https-proxy.log" 2>&1 &
import select, socket, ssl, sys, threading

listen_port, backend_port = int(sys.argv[1]), int(sys.argv[2])
cert, key = sys.argv[3], sys.argv[4]

def pump(a, b):
    try:
        while True:
            r, _, _ = select.select([a, b], [], [], 60)
            if not r:
                break
            for s in r:
                data = s.recv(65536)
                if not data:
                    return
                (b if s is a else a).sendall(data)
    except Exception:
        return
    finally:
        try: a.close()
        except Exception: pass
        try: b.close()
        except Exception: pass

ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(certfile=cert, keyfile=key)
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(('127.0.0.1', listen_port))
server.listen(32)
while True:
    client, _ = server.accept()
    try:
        tls = ctx.wrap_socket(client, server_side=True)
        backend = socket.create_connection(('127.0.0.1', backend_port), timeout=5)
    except Exception:
        try: client.close()
        except Exception: pass
        continue
    threading.Thread(target=pump, args=(tls, backend), daemon=True).start()
PY
proxy_pid=$!
printf '%s\n' "$proxy_pid" >"$pid_file"
sleep 0.3
kill -0 "$proxy_pid" 2>/dev/null || fail_blocked https_proxy_failed "log=$evidence_directory/https-proxy.log"

https_url="https://127.0.0.1:${https_port}/ready"
export A3S_CLOUD_BX0_HTTPS_URL=$https_url
export A3S_CLOUD_BX0_HEALTH_URL=$health_url
export A3S_CLOUD_BX0_TLS_CA_FILE=$ca_cert

# CA-aware probe used by gate health + https execute steps
probe_bin="$state_dir/bx0-ca-curl-probe.sh"
cat >"$probe_bin" <<EOF
#!/usr/bin/env bash
set -euo pipefail
url=\${1:-}
[[ -n \$url ]] || exit 2
ca=$(printf '%q' "$ca_cert")
if [[ \$url == https://* ]]; then
  exec curl -fsS --cacert "\$ca" --max-time 15 -o /dev/null "\$url"
fi
exec curl -fsS --max-time 15 -o /dev/null "\$url"
EOF
chmod +x "$probe_bin"
export A3S_CLOUD_HEALTH_PROBE_BIN=$probe_bin

# node.acl with absolute paths.
# Box Secret requires a canonical provider-owned 0700 directory on Linux tmpfs
# (statfs TMPFS_MAGIC). Drvfs and ordinary disk paths are refused.
[[ -f $example_acl ]] || fail_blocked node_example_missing "path=$example_acl"
node_acl="$state_dir/node.acl"
node_state="${A3S_CLOUD_BX0_LIVE_NODE_STATE_DIR:-/tmp/a3s-cloud-bx0-node-state}"
box_secret_root="${A3S_CLOUD_BX0_LIVE_SECRET_ROOT:-/dev/shm/a3s-cloud-bx0-box-secrets}"
rm -rf -- "$node_state"
# Recreate secret root on tmpfs; refuse silently falling back to disk.
if [[ -d $box_secret_root ]]; then
  find "$box_secret_root" -mindepth 1 -delete 2>/dev/null || rm -rf -- "${box_secret_root:?}"/*
fi
mkdir -p -- "$node_state" "$box_secret_root" "$node_state/box-home"
chmod 0700 -- "$node_state" "$box_secret_root" "$node_state/box-home"
box_secret_root=$(realpath -- "$box_secret_root")
node_state=$(realpath -- "$node_state")
# Fail closed before enroll if Secret root is not on tmpfs.
fstype=$(findmnt -no FSTYPE -T "$box_secret_root" 2>/dev/null || true)
if [[ $fstype != tmpfs ]]; then
  fail_blocked secret_root_not_tmpfs "path=$box_secret_root fstype=${fstype:-unknown}"
fi
mode=$(stat -c '%a' "$box_secret_root" 2>/dev/null || true)
if [[ $mode != 700 && $mode != 710 ]]; then
  fail_blocked secret_root_mode "path=$box_secret_root mode=${mode:-unknown}"
fi
api_base=${A3S_CLOUD_URL:-http://127.0.0.1:8080/api/v1}
enroll_url="${api_base%/}/node-control/enroll"
# Node-control mTLS trusts the control-plane Local Node CA, not the LIVE HTTPS
# health-proxy CA. SAN is DNS:localhost — use that hostname, not 127.0.0.1.
node_ca="${A3S_CLOUD_BX0_LIVE_NODE_CA:-${A3S_CLOUD_BX0_LIVE_SECURITY_STATE_DIR:-/tmp/a3s-cloud-bx0-security}/node-ca/ca.pem}"
[[ -f $node_ca ]] || fail_blocked node_ca_missing "path=$node_ca"
node_control_url=${A3S_CLOUD_BX0_NODE_CONTROL_URL:-https://localhost:8443}
node_name=${A3S_CLOUD_BX0_NODE_NAME:-worker-bx0-software}
# crude replace of example into absolute local paths
python3 - "$example_acl" "$node_acl" "$enroll_url" "$node_state" "$node_ca" "$box_secret_root" "$node_control_url" "$node_name" <<'PY'
import pathlib, re, sys
src, dst, enroll, state, ca, secret_root, node_control, node_name = sys.argv[1:9]
text = pathlib.Path(src).read_text(encoding='utf-8')
text = re.sub(r'enrollment_url\s*=\s*".*"', f'enrollment_url = "{enroll}"', text, count=1)
text = re.sub(r'node_control_url\s*=\s*".*"', f'node_control_url = "{node_control}"', text, count=1)
text = re.sub(r'server_ca_file\s*=\s*".*"', f'server_ca_file = "{ca}"', text, count=1)
text = re.sub(r'state_dir\s*=\s*".*"', f'state_dir = "{state}"', text, count=1)
text = re.sub(r'home_dir\s*=\s*".*"', f'home_dir = "{state}/box-home"', text, count=1)
text = re.sub(r'secret_root\s*=\s*".*"', f'secret_root = "{secret_root}"', text, count=1)
text = re.sub(r'(node\s*\{[^}]*?name\s*=\s*)"[^"]*"', rf'\1"{node_name}"', text, count=1, flags=re.S)
pathlib.Path(dst).write_text(text, encoding='utf-8')
PY
export A3S_CLOUD_BX0_NODE_CONFIG=$node_acl
export A3S_CLOUD_BX0_SECRET_ROOT=$box_secret_root
# Box requires process A3S_HOME == box.home_dir (same authority for Runtime/ImageStore).
export A3S_HOME="$node_state/box-home"

env_out=${A3S_CLOUD_BX0_HTTPS_OUT:-$evidence_directory/bx0-live-https-env.sh}
{
  printf '# Generated by run_bx0_software_exit_live_https.sh\n'
  printf 'export A3S_CLOUD_BX0_HEALTH_URL=%q\n' "$health_url"
  printf 'export A3S_CLOUD_BX0_HTTPS_URL=%q\n' "$https_url"
  printf 'export A3S_CLOUD_BX0_TLS_CA_FILE=%q\n' "$ca_cert"
  printf 'export A3S_CLOUD_HEALTH_PROBE_BIN=%q\n' "$probe_bin"
  printf 'export A3S_CLOUD_BX0_NODE_CONFIG=%q\n' "$node_acl"
  printf 'export A3S_CLOUD_BX0_SECRET_ROOT=%q\n' "$box_secret_root"
  printf 'export A3S_HOME=%q\n' "$A3S_HOME"
} >"$env_out"

cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-https.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_HTTPS_OK
health_url=$health_url
https_url=$https_url
probe_bin=$probe_bin
node_config=$node_acl
proxy_pid=$proxy_pid
env_file=$env_out
product_exit=not_claimed
loop_certified=not_claimed
honesty=Local CA-signed TLS terminate to loopback health; probe uses --cacert not -k.
EOF
exit 0
