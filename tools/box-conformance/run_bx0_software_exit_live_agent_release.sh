#!/usr/bin/env bash
# Publish a locally built a3s-cloud-node-agent over HTTPS for nodes bootstrap.
#
# Builds the agent (unless A3S_CLOUD_NODE_AGENT_BIN is set), serves it with a
# generated local CA + TLS cert, and exports:
#   A3S_CLOUD_BX0_AGENT_RELEASE_URL
#   A3S_CLOUD_BX0_AGENT_RELEASE_SHA256
#   A3S_CLOUD_BX0_TLS_CA_FILE  (for CA-aware probes)
#
# Never claims LOOP/EXIT. Bootstrap only requires an https:// URL shape; CREATE
# still starts the local agent binary directly.
#
# Usage:
#   bash tools/box-conformance/run_bx0_software_exit_live_agent_release.sh [EVIDENCE_DIR]

set -euo pipefail

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
state_dir=${A3S_CLOUD_BX0_LIVE_PREP_STATE_DIR:-$repository_root/.a3s/cloud/bx0-live-prep}
tls_dir="$state_dir/tls"
serve_dir="$state_dir/agent-release"

evidence_directory=${1:-}
if [[ -z $evidence_directory ]]; then
  evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-live-agent.XXXXXX")
fi
mkdir -p -- "$evidence_directory" "$tls_dir" "$serve_dir"

fail_blocked() {
  local reason=$1
  shift || true
  cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-agent-release.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_AGENT_RELEASE_BLOCKED reason=$reason
$*
product_exit=not_claimed
loop_certified=not_claimed
EOF
  exit 2
}

# shellcheck disable=SC1090
source "$tools/bx0_refuse_docker_host.sh"
bx0_refuse_docker_host
for cmd in openssl python3; do
  command -v "$cmd" >/dev/null 2>&1 || fail_blocked "${cmd}_unavailable"
done

agent_bin=${A3S_CLOUD_NODE_AGENT_BIN:-}
if [[ -z $agent_bin || ! -x $agent_bin ]]; then
  command -v cargo >/dev/null 2>&1 || fail_blocked cargo_unavailable
  target_directory="${CARGO_TARGET_DIR:-$repository_root/target}"
  if [[ $target_directory != /* ]]; then
    target_directory="$repository_root/$target_directory"
  fi
  (cd "$repository_root" && cargo build --locked -p a3s-cloud-node-agent) \
    || fail_blocked node_agent_build_failed
  agent_bin="$target_directory/debug/a3s-cloud-node-agent"
fi
[[ -x $agent_bin ]] || fail_blocked node_agent_missing "path=$agent_bin"
export A3S_CLOUD_NODE_AGENT_BIN=$agent_bin

install -m 0755 "$agent_bin" "$serve_dir/a3s-cloud-node-agent"
release_sha=$(
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$serve_dir/a3s-cloud-node-agent" | awk '{print $1}'
  else
    shasum -a 256 "$serve_dir/a3s-cloud-node-agent" | awk '{print $1}'
  fi
)
[[ $release_sha =~ ^[0-9a-f]{64}$ ]] || fail_blocked release_sha_invalid

ca_key="$tls_dir/ca.key"
ca_cert="$tls_dir/ca.pem"
server_key="$tls_dir/server.key"
server_csr="$tls_dir/server.csr"
server_cert="$tls_dir/server.pem"
if [[ ! -f $ca_cert || ! -f $server_cert ]]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$ca_key" -out "$ca_cert" \
    -days 30 -subj '/CN=a3s-bx0-live-ca' \
    >"$evidence_directory/openssl-ca.out" 2>"$evidence_directory/openssl-ca.err" \
    || fail_blocked openssl_ca_failed
  openssl req -newkey rsa:2048 -nodes -keyout "$server_key" -out "$server_csr" \
    -subj '/CN=127.0.0.1' \
    >"$evidence_directory/openssl-csr.out" 2>"$evidence_directory/openssl-csr.err" \
    || fail_blocked openssl_csr_failed
  san_ext="$tls_dir/server-san.ext"
  printf 'subjectAltName=IP:127.0.0.1,DNS:localhost\n' >"$san_ext"
  openssl x509 -req -in "$server_csr" -CA "$ca_cert" -CAkey "$ca_key" -CAcreateserial \
    -out "$server_cert" -days 30 -extfile "$san_ext" \
    >"$evidence_directory/openssl-sign.out" 2>"$evidence_directory/openssl-sign.err" \
    || fail_blocked openssl_sign_failed
fi

https_port=${A3S_CLOUD_BX0_AGENT_RELEASE_PORT:-18443}
pid_file="$serve_dir/https.pid"
if [[ -f $pid_file ]]; then
  old=$(<"$pid_file")
  kill "$old" 2>/dev/null || true
fi

python3 - "$serve_dir" "$https_port" "$server_cert" "$server_key" <<'PY' \
  >"$evidence_directory/https-server.log" 2>&1 &
import http.server, ssl, sys
from pathlib import Path
root, port, cert, key = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]

class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=root, **kwargs)

httpd = http.server.HTTPServer(('127.0.0.1', port), Handler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(certfile=cert, keyfile=key)
httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
httpd.serve_forever()
PY
https_pid=$!
printf '%s\n' "$https_pid" >"$pid_file"
sleep 0.5
kill -0 "$https_pid" 2>/dev/null || fail_blocked https_server_failed "log=$evidence_directory/https-server.log"

release_url="https://127.0.0.1:${https_port}/a3s-cloud-node-agent"
# Verify fetch with CA
curl -fsS --cacert "$ca_cert" --max-time 15 -o "$evidence_directory/agent.downloaded" \
  "$release_url" || fail_blocked agent_https_fetch_failed
cmp -s "$serve_dir/a3s-cloud-node-agent" "$evidence_directory/agent.downloaded" \
  || fail_blocked agent_https_bytes_mismatch

export A3S_CLOUD_BX0_AGENT_RELEASE_URL=$release_url
export A3S_CLOUD_BX0_AGENT_RELEASE_SHA256=$release_sha
export A3S_CLOUD_BX0_TLS_CA_FILE=$ca_cert

env_out=${A3S_CLOUD_BX0_AGENT_RELEASE_OUT:-$evidence_directory/bx0-live-agent-release-env.sh}
{
  printf '# Generated by run_bx0_software_exit_live_agent_release.sh\n'
  printf 'export A3S_CLOUD_NODE_AGENT_BIN=%q\n' "$agent_bin"
  printf 'export A3S_CLOUD_BX0_AGENT_RELEASE_URL=%q\n' "$release_url"
  printf 'export A3S_CLOUD_BX0_AGENT_RELEASE_SHA256=%q\n' "$release_sha"
  printf 'export A3S_CLOUD_BX0_TLS_CA_FILE=%q\n' "$ca_cert"
} >"$env_out"

cat <<EOF | tee "$evidence_directory/bx0-software-exit-live-agent-release.txt"
A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE_AGENT_RELEASE_OK
agent_bin=$agent_bin
release_url=$release_url
release_sha256=$release_sha
ca_file=$ca_cert
https_pid=$https_pid
env_file=$env_out
product_exit=not_claimed
loop_certified=not_claimed
honesty=Served locally built agent over HTTPS with local CA; sha256 of served bytes.
EOF
exit 0
