#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
listen_address="${A3S_CLOUD_WEB_VERIFY_LISTEN:-127.0.0.1:3011}"
origin="http://$listen_address"
work_directory="$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-web-verify.XXXXXX")"
server_log="$work_directory/server.log"
server_pid=''

cleanup() {
  local status=$?
  trap - EXIT HUP INT TERM
  if [[ -n $server_pid ]]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
  if [[ $status -ne 0 && -f $server_log ]]; then
    printf '%s\n' 'A3S Cloud SPA server output:' >&2
    sed -n '1,240p' "$server_log" >&2
  fi
  rm -rf "$work_directory"
  exit "$status"
}

trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

test -f "$repository_root/web/dist/index.html"
test -f "$repository_root/web/dist/favicon.svg"
grep -Eq 'rel="icon"[^>]+favicon\.svg' "$repository_root/web/dist/index.html"

cd "$repository_root"
cargo build -p a3s-cloud-web-server --locked
target_directory="${CARGO_TARGET_DIR:-$repository_root/target}"
if [[ $target_directory != /* ]]; then
  target_directory="$repository_root/$target_directory"
fi
"$target_directory/debug/a3s-cloud-web-server" \
  --listen "$listen_address" \
  --root web/dist >"$server_log" 2>&1 &
server_pid=$!

ready=false
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
  if curl --fail --silent "$origin/healthz" >/dev/null; then
    ready=true
    break
  fi
  sleep 1
done
if [[ $ready != true ]]; then
  printf 'SPA server did not become ready at %s\n' "$origin" >&2
  exit 1
fi

index_headers="$work_directory/index.headers"
index_body="$work_directory/index.html"
fallback_body="$work_directory/fallback.html"
asset_headers="$work_directory/asset.headers"
favicon_headers="$work_directory/favicon.headers"

curl --fail --silent --show-error --dump-header "$index_headers" \
  "$origin/" >"$index_body"
curl --fail --silent --show-error \
  "$origin/organizations/local/projects/cloud" >"$fallback_body"
cmp "$index_body" "$fallback_body"

asset_file="$(find "$repository_root/web/dist/static" -type f | sed -n '1p')"
test -n "$asset_file"
asset_path="${asset_file#"$repository_root/web/dist"}"
curl --fail --silent --show-error --dump-header "$asset_headers" \
  "$origin$asset_path" >/dev/null
curl --fail --silent --show-error --dump-header "$favicon_headers" \
  "$origin/favicon.svg" >/dev/null

grep -Eiq '^cache-control: no-cache, no-store, must-revalidate' "$index_headers"
grep -Eiq '^content-security-policy:' "$index_headers"
grep -Eiq '^x-content-type-options: nosniff' "$index_headers"
grep -Eiq '^cache-control: public, max-age=31536000, immutable' "$asset_headers"
grep -Eiq '^content-type: image/svg\+xml' "$favicon_headers"

api_status="$(curl --silent --output /dev/null --write-out '%{http_code}' \
  "$origin/api/v1/health/live")"
test "$api_status" = 404

printf 'A3S_CLOUD_SPA_DELIVERY_PASS\n'
