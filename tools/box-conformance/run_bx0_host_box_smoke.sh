#!/usr/bin/env bash
# First-principles host a3s-box smokes for virt-capable machines (Darwin
# Hypervisor or Linux+/dev/kvm). Never claims product LOOP/EXIT.
#
# Usage:
#   bash run_bx0_host_box_smoke.sh ABSOLUTE_EVIDENCE_DIR /path/to/a3s-box
#
# Skips (exit 0) when the host is not virt-capable or box binary is missing.
set -euo pipefail

evidence_directory=${1:-}
box_bin=${2:-}
if [[ -z $evidence_directory || $evidence_directory != /* || -z $box_bin ]]; then
  printf 'usage: %s ABSOLUTE_EVIDENCE_DIR /path/to/a3s-box\n' "$0" >&2
  exit 2
fi
mkdir -p -- "$evidence_directory"
if [[ ! -x $box_bin ]]; then
  printf '%s\n' "host a3s-box smoke skipped: box binary not executable: $box_bin"
  exit 0
fi
if [[ $(uname -s) != Darwin && ! -e /dev/kvm ]]; then
  printf '%s\n' \
    "host a3s-box smoke skipped: not Darwin and /dev/kvm absent ($(uname -s)/$(uname -m))"
  exit 0
fi

forbid_exit_certified_claim() {
  local path=$1
  if grep -E --quiet \
    '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]+[^[:space:]]' \
    "$path" 2>/dev/null; then
    printf '%s\n' \
      "host a3s-box smoke must never emit product EXIT_CERTIFIED claim: $path" >&2
    exit 1
  fi
}

echo "===== host a3s-box virt probe (Darwin Hypervisor or Linux+/dev/kvm) ====="
set +e
"$box_bin" info \
  >"$evidence_directory/host-box-info.out" 2>"$evidence_directory/host-box-info.err"
host_box_info_status=$?
set -e
if ((host_box_info_status != 0)); then
  printf '%s\n' "expected host a3s-box info to succeed, got $host_box_info_status" >&2
  cat "$evidence_directory/host-box-info.out" >&2 || true
  cat "$evidence_directory/host-box-info.err" >&2 || true
  exit 1
fi
cat "$evidence_directory/host-box-info.out" "$evidence_directory/host-box-info.err" \
  >"$evidence_directory/host-box-info.txt" 2>/dev/null || true
if grep -Fq 'Virtualization: not available' "$evidence_directory/host-box-info.txt"; then
  printf '%s\n' \
    "host a3s-box reports Virtualization: not available on virt-capable host" >&2
  exit 1
fi
if ! grep -Eiq 'Virtualization:.*(Hypervisor|available|KVM)' \
  "$evidence_directory/host-box-info.txt"; then
  printf '%s\n' "host a3s-box info missing positive Virtualization signal" >&2
  cat "$evidence_directory/host-box-info.txt" >&2 || true
  exit 1
fi
forbid_exit_certified_claim "$evidence_directory/host-box-info.txt"

echo "===== host a3s-box lifecycle smoke (run/exec/rm) ====="
smoke_name="bx0-ci-smoke-$$"
smoke_cleanup() {
  "$box_bin" rm -f "$smoke_name" >/dev/null 2>&1 || true
}
smoke_cleanup
set +e
"$box_bin" run -d --name "$smoke_name" alpine:3.20 -- sleep 180 \
  >"$evidence_directory/host-box-smoke-run.out" 2>"$evidence_directory/host-box-smoke-run.err"
smoke_run_status=$?
set -e
if ((smoke_run_status != 0)); then
  smoke_cleanup
  printf '%s\n' "expected host a3s-box run to succeed, got $smoke_run_status" >&2
  cat "$evidence_directory/host-box-smoke-run.out" >&2 || true
  cat "$evidence_directory/host-box-smoke-run.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" exec --timeout 60 "$smoke_name" -- uname -sm \
  >"$evidence_directory/host-box-smoke-exec.out" 2>"$evidence_directory/host-box-smoke-exec.err"
smoke_exec_status=$?
set -e
if ((smoke_exec_status != 0)); then
  smoke_cleanup
  printf '%s\n' "expected host a3s-box exec to succeed, got $smoke_exec_status" >&2
  cat "$evidence_directory/host-box-smoke-exec.out" >&2 || true
  cat "$evidence_directory/host-box-smoke-exec.err" >&2 || true
  exit 1
fi
grep -Eq 'Linux[[:space:]]+(aarch64|arm64|x86_64|amd64)' \
  "$evidence_directory/host-box-smoke-exec.out"
set +e
"$box_bin" stop "$smoke_name" \
  >"$evidence_directory/host-box-smoke-stop.out" 2>"$evidence_directory/host-box-smoke-stop.err"
smoke_stop_status=$?
set -e
if ((smoke_stop_status != 0)); then
  smoke_cleanup
  printf '%s\n' "expected host a3s-box stop to succeed, got $smoke_stop_status" >&2
  exit 1
fi
set +e
"$box_bin" rm "$smoke_name" \
  >"$evidence_directory/host-box-smoke-rm.out" 2>"$evidence_directory/host-box-smoke-rm.err"
smoke_rm_status=$?
set -e
if ((smoke_rm_status != 0)); then
  smoke_cleanup
  printf '%s\n' "expected host a3s-box rm to succeed, got $smoke_rm_status" >&2
  exit 1
fi
forbid_exit_certified_claim "$evidence_directory/host-box-smoke-run.out"
forbid_exit_certified_claim "$evidence_directory/host-box-smoke-exec.out"

echo "===== host a3s-box ephemeral PID1 must become dead (not running) ====="
dead_name="bx0-ci-dead-$$"
"$box_bin" rm -f "$dead_name" >/dev/null 2>&1 || true
set +e
"$box_bin" run -d --name "$dead_name" alpine:3.20 \
  >"$evidence_directory/host-box-dead-run.out" 2>"$evidence_directory/host-box-dead-run.err"
dead_run_status=$?
set -e
if ((dead_run_status != 0)); then
  "$box_bin" rm -f "$dead_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected ephemeral alpine run to accept create, got $dead_run_status" >&2
  cat "$evidence_directory/host-box-dead-run.out" >&2 || true
  cat "$evidence_directory/host-box-dead-run.err" >&2 || true
  exit 1
fi
dead_seen=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  set +e
  "$box_bin" ps -a --filter "name=$dead_name" \
    >"$evidence_directory/host-box-dead-ps.out" 2>"$evidence_directory/host-box-dead-ps.err"
  set -e
  if grep -Eiq 'dead \(Exit|[[:space:]]dead[[:space:]]' \
    "$evidence_directory/host-box-dead-ps.out"
  then
    dead_seen=1
    break
  fi
  if ! grep -Fq "$dead_name" "$evidence_directory/host-box-dead-ps.out"; then
    dead_seen=1
    break
  fi
  sleep 1
done
if grep -Eiq '[[:space:]]running[[:space:]]' "$evidence_directory/host-box-dead-ps.out" \
  && grep -Fq "$dead_name" "$evidence_directory/host-box-dead-ps.out"
then
  "$box_bin" rm -f "$dead_name" >/dev/null 2>&1 || true
  printf '%s\n' "ephemeral alpine box still running after wait" >&2
  cat "$evidence_directory/host-box-dead-ps.out" >&2 || true
  exit 1
fi

echo "===== host a3s-box plain ps must not list dead boxes ====="
# Anti-overfit: operators must use ps -a; empty plain ps is not a name typo.
set +e
"$box_bin" ps --filter "name=$dead_name" \
  >"$evidence_directory/host-box-dead-ps-plain.out" 2>"$evidence_directory/host-box-dead-ps-plain.err"
set -e
if grep -Fq "$dead_name" "$evidence_directory/host-box-dead-ps-plain.out"; then
  "$box_bin" rm -f "$dead_name" >/dev/null 2>&1 || true
  printf '%s\n' "plain a3s-box ps must not list dead box $dead_name" >&2
  cat "$evidence_directory/host-box-dead-ps-plain.out" >&2 || true
  exit 1
fi
"$box_bin" rm -f "$dead_name" >/dev/null 2>&1 || true
if ((dead_seen != 1)); then
  printf '%s\n' "expected ephemeral alpine box to become dead/gone" >&2
  cat "$evidence_directory/host-box-dead-ps.out" >&2 || true
  exit 1
fi

echo "===== host a3s-box published port must be reachable from host ====="
port_name="bx0-ci-port-$$"
host_port=$((19000 + ($$ % 1000)))
"$box_bin" rm -f "$port_name" >/dev/null 2>&1 || true
set +e
"$box_bin" run -d --name "$port_name" -p "${host_port}:8000" \
  alpine:3.20 -- \
  sh -c 'while true; do printf "HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nok" | nc -l -p 8000; done' \
  >"$evidence_directory/host-box-port-run.out" 2>"$evidence_directory/host-box-port-run.err"
port_run_status=$?
set -e
if ((port_run_status != 0)); then
  "$box_bin" rm -f "$port_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected published-port run to succeed, got $port_run_status" >&2
  cat "$evidence_directory/host-box-port-run.out" >&2 || true
  cat "$evidence_directory/host-box-port-run.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" port "$port_name" \
  >"$evidence_directory/host-box-port-map.out" 2>"$evidence_directory/host-box-port-map.err"
port_map_status=$?
set -e
if ((port_map_status != 0)); then
  "$box_bin" rm -f "$port_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box port to succeed, got $port_map_status" >&2
  cat "$evidence_directory/host-box-port-map.out" >&2 || true
  cat "$evidence_directory/host-box-port-map.err" >&2 || true
  exit 1
fi
grep -Fq ":${host_port}" "$evidence_directory/host-box-port-map.out"
curl_ok=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  set +e
  curl -fsS --max-time 2 "http://127.0.0.1:${host_port}/" \
    >"$evidence_directory/host-box-port-curl.out" 2>"$evidence_directory/host-box-port-curl.err"
  curl_status=$?
  set -e
  if ((curl_status == 0)); then
    curl_ok=1
    break
  fi
  sleep 1
done
"$box_bin" rm -f "$port_name" >/dev/null 2>&1 || true
if ((curl_ok != 1)); then
  printf '%s\n' "expected host curl to published box port $host_port to succeed" >&2
  cat "$evidence_directory/host-box-port-map.out" >&2 || true
  cat "$evidence_directory/host-box-port-curl.err" >&2 || true
  exit 1
fi
grep -Fq 'ok' "$evidence_directory/host-box-port-curl.out"
forbid_exit_certified_claim "$evidence_directory/host-box-port-curl.out"

echo "===== host a3s-box exec without -- must fail ====="
miss_name="bx0-ci-missdash-$$"
"$box_bin" rm -f "$miss_name" >/dev/null 2>&1 || true
set +e
"$box_bin" run -d --name "$miss_name" alpine:3.20 -- sleep 120 \
  >"$evidence_directory/host-box-miss-run.out" 2>"$evidence_directory/host-box-miss-run.err"
miss_run_status=$?
set -e
if ((miss_run_status != 0)); then
  "$box_bin" rm -f "$miss_name" >/dev/null 2>&1 || true
  printf '%s\n' "setup run for missing -- exec failed: $miss_run_status" >&2
  exit 1
fi
set +e
"$box_bin" exec --timeout 15 "$miss_name" uname \
  >"$evidence_directory/host-box-miss-exec.out" 2>"$evidence_directory/host-box-miss-exec.err"
miss_exec_status=$?
set -e
"$box_bin" rm -f "$miss_name" >/dev/null 2>&1 || true
if ((miss_exec_status == 0)); then
  printf '%s\n' "exec without -- must fail-closed" >&2
  cat "$evidence_directory/host-box-miss-exec.out" >&2 || true
  exit 1
fi
if ! grep -Eiq 'unexpected argument' \
  "$evidence_directory/host-box-miss-exec.err" \
  "$evidence_directory/host-box-miss-exec.out"
then
  printf '%s\n' "exec without -- must report unexpected argument" >&2
  cat "$evidence_directory/host-box-miss-exec.err" >&2 || true
  exit 1
fi

echo "===== host a3s-box cp host→guest then exec cat ====="
cp_name="bx0-ci-cp-$$"
"$box_bin" rm -f "$cp_name" >/dev/null 2>&1 || true
printf '%s\n' 'bx0-host-box-cp-marker' >"$evidence_directory/host-box-cp-payload.txt"
set +e
"$box_bin" run -d --name "$cp_name" alpine:3.20 -- sleep 120 \
  >"$evidence_directory/host-box-cp-run.out" 2>"$evidence_directory/host-box-cp-run.err"
cp_run_status=$?
set -e
if ((cp_run_status != 0)); then
  "$box_bin" rm -f "$cp_name" >/dev/null 2>&1 || true
  printf '%s\n' "setup run for cp smoke failed: $cp_run_status" >&2
  exit 1
fi
set +e
"$box_bin" cp "$evidence_directory/host-box-cp-payload.txt" "$cp_name:/tmp/bx0-cp-payload.txt" \
  >"$evidence_directory/host-box-cp.out" 2>"$evidence_directory/host-box-cp.err"
cp_status=$?
set -e
if ((cp_status != 0)); then
  "$box_bin" rm -f "$cp_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box cp host→guest to succeed, got $cp_status" >&2
  cat "$evidence_directory/host-box-cp.out" >&2 || true
  cat "$evidence_directory/host-box-cp.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" exec --timeout 30 "$cp_name" -- cat /tmp/bx0-cp-payload.txt \
  >"$evidence_directory/host-box-cp-cat.out" 2>"$evidence_directory/host-box-cp-cat.err"
cp_cat_status=$?
set -e
if ((cp_cat_status != 0)); then
  "$box_bin" rm -f "$cp_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected exec cat after cp to succeed, got $cp_cat_status" >&2
  exit 1
fi
grep -Fq 'bx0-host-box-cp-marker' "$evidence_directory/host-box-cp-cat.out"

echo "===== host a3s-box exec on stopped box must fail ====="
set +e
"$box_bin" stop "$cp_name" \
  >"$evidence_directory/host-box-stopped-stop.out" 2>"$evidence_directory/host-box-stopped-stop.err"
stopped_stop_status=$?
set -e
if ((stopped_stop_status != 0)); then
  "$box_bin" rm -f "$cp_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected stop before stopped-exec probe to succeed" >&2
  exit 1
fi
set +e
"$box_bin" exec --timeout 15 "$cp_name" -- true \
  >"$evidence_directory/host-box-stopped-exec.out" 2>"$evidence_directory/host-box-stopped-exec.err"
stopped_exec_status=$?
set -e
"$box_bin" rm -f "$cp_name" >/dev/null 2>&1 || true
if ((stopped_exec_status == 0)); then
  printf '%s\n' "exec on stopped box must fail-closed" >&2
  exit 1
fi
# Accept product language for non-running targets (stopped / not running /
# missing). Do not overfit to a single wording.
if ! grep -Eiq 'stopped|not running|No such box' \
  "$evidence_directory/host-box-stopped-exec.err" \
  "$evidence_directory/host-box-stopped-exec.out"
then
  printf '%s\n' "stopped exec must report stopped / not running / No such box" >&2
  cat "$evidence_directory/host-box-stopped-exec.err" >&2 || true
  exit 1
fi

echo "===== host a3s-box create+start with -v mount must see host file ====="
# First principles: via-box and agents bind-mount the monorepo; cp alone is
# not a substitute. create+start must work without run -d.
vol_name="bx0-ci-vol-$$"
vol_host="$evidence_directory/host-box-vol-host"
mkdir -p -- "$vol_host"
printf '%s\n' 'bx0-host-vol-marker' >"$vol_host/marker.txt"
"$box_bin" rm -f "$vol_name" >/dev/null 2>&1 || true
set +e
"$box_bin" create --name "$vol_name" -v "$vol_host:/host:ro" alpine:3.20 -- sleep 120 \
  >"$evidence_directory/host-box-vol-create.out" 2>"$evidence_directory/host-box-vol-create.err"
vol_create_status=$?
set -e
if ((vol_create_status != 0)); then
  "$box_bin" rm -f "$vol_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box create to succeed, got $vol_create_status" >&2
  cat "$evidence_directory/host-box-vol-create.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" start "$vol_name" \
  >"$evidence_directory/host-box-vol-start.out" 2>"$evidence_directory/host-box-vol-start.err"
vol_start_status=$?
set -e
if ((vol_start_status != 0)); then
  "$box_bin" rm -f "$vol_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box start to succeed, got $vol_start_status" >&2
  cat "$evidence_directory/host-box-vol-start.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" exec --timeout 30 "$vol_name" -- cat /host/marker.txt \
  >"$evidence_directory/host-box-vol-cat.out" 2>"$evidence_directory/host-box-vol-cat.err"
vol_cat_status=$?
set -e
if ((vol_cat_status != 0)); then
  "$box_bin" rm -f "$vol_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected mounted host file readable in guest, got $vol_cat_status" >&2
  cat "$evidence_directory/host-box-vol-cat.err" >&2 || true
  exit 1
fi
grep -Fq 'bx0-host-vol-marker' "$evidence_directory/host-box-vol-cat.out"

echo "===== host a3s-box pause then unpause must restore exec ====="
set +e
"$box_bin" pause "$vol_name" \
  >"$evidence_directory/host-box-pause.out" 2>"$evidence_directory/host-box-pause.err"
pause_status=$?
set -e
if ((pause_status != 0)); then
  "$box_bin" rm -f "$vol_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box pause to succeed, got $pause_status" >&2
  cat "$evidence_directory/host-box-pause.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" unpause "$vol_name" \
  >"$evidence_directory/host-box-unpause.out" 2>"$evidence_directory/host-box-unpause.err"
unpause_status=$?
set -e
if ((unpause_status != 0)); then
  "$box_bin" rm -f "$vol_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box unpause to succeed, got $unpause_status" >&2
  cat "$evidence_directory/host-box-unpause.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" exec --timeout 30 "$vol_name" -- true \
  >"$evidence_directory/host-box-unpause-exec.out" 2>"$evidence_directory/host-box-unpause-exec.err"
unpause_exec_status=$?
set -e
"$box_bin" rm -f "$vol_name" >/dev/null 2>&1 || true
if ((unpause_exec_status != 0)); then
  printf '%s\n' "expected exec after unpause to succeed, got $unpause_exec_status" >&2
  cat "$evidence_directory/host-box-unpause-exec.err" >&2 || true
  exit 1
fi

echo "===== host a3s-box snapshot create+restore must preserve guest file ====="
# First principles: snapshot is a MicroVM persistence contract, not a Docker
# commit alias. Prove restore into a new box still sees guest-written data.
snap_src="bx0-ci-snapsrc-$$"
snap_name="bx0-ci-snap-$$"
snap_rest="bx0-ci-snaprest-$$"
"$box_bin" snapshot rm "$snap_name" >/dev/null 2>&1 || true
"$box_bin" rm -f "$snap_src" "$snap_rest" >/dev/null 2>&1 || true
set +e
"$box_bin" run -d --name "$snap_src" alpine:3.20 -- sleep 180 \
  >"$evidence_directory/host-box-snap-run.out" 2>"$evidence_directory/host-box-snap-run.err"
snap_run_status=$?
set -e
if ((snap_run_status != 0)); then
  "$box_bin" rm -f "$snap_src" >/dev/null 2>&1 || true
  printf '%s\n' "setup run for snapshot failed: $snap_run_status" >&2
  exit 1
fi
set +e
"$box_bin" exec --timeout 30 "$snap_src" -- sh -c 'printf "%s\n" bx0-snap-marker > /tmp/bx0-snap-marker' \
  >"$evidence_directory/host-box-snap-write.out" 2>"$evidence_directory/host-box-snap-write.err"
snap_write_status=$?
set -e
if ((snap_write_status != 0)); then
  "$box_bin" rm -f "$snap_src" >/dev/null 2>&1 || true
  printf '%s\n' "expected guest write before snapshot to succeed" >&2
  exit 1
fi
set +e
"$box_bin" stop "$snap_src" \
  >"$evidence_directory/host-box-snap-stop.out" 2>"$evidence_directory/host-box-snap-stop.err"
snap_stop_status=$?
set -e
if ((snap_stop_status != 0)); then
  "$box_bin" rm -f "$snap_src" >/dev/null 2>&1 || true
  printf '%s\n' "expected stop before snapshot create to succeed" >&2
  exit 1
fi
set +e
"$box_bin" snapshot create --name "$snap_name" "$snap_src" \
  >"$evidence_directory/host-box-snap-create.out" 2>"$evidence_directory/host-box-snap-create.err"
snap_create_status=$?
set -e
if ((snap_create_status != 0)); then
  "$box_bin" rm -f "$snap_src" >/dev/null 2>&1 || true
  printf '%s\n' "expected snapshot create to succeed, got $snap_create_status" >&2
  cat "$evidence_directory/host-box-snap-create.err" >&2 || true
  exit 1
fi
"$box_bin" rm -f "$snap_src" >/dev/null 2>&1 || true
set +e
"$box_bin" snapshot restore --name "$snap_rest" "$snap_name" \
  >"$evidence_directory/host-box-snap-restore.out" 2>"$evidence_directory/host-box-snap-restore.err"
snap_restore_status=$?
set -e
if ((snap_restore_status != 0)); then
  "$box_bin" snapshot rm "$snap_name" >/dev/null 2>&1 || true
  "$box_bin" rm -f "$snap_rest" >/dev/null 2>&1 || true
  printf '%s\n' "expected snapshot restore to succeed, got $snap_restore_status" >&2
  cat "$evidence_directory/host-box-snap-restore.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" start "$snap_rest" \
  >"$evidence_directory/host-box-snap-rest-start.out" 2>"$evidence_directory/host-box-snap-rest-start.err"
snap_rest_start_status=$?
set -e
if ((snap_rest_start_status != 0)); then
  "$box_bin" snapshot rm "$snap_name" >/dev/null 2>&1 || true
  "$box_bin" rm -f "$snap_rest" >/dev/null 2>&1 || true
  printf '%s\n' "expected restored box start to succeed" >&2
  cat "$evidence_directory/host-box-snap-rest-start.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" exec --timeout 30 "$snap_rest" -- cat /tmp/bx0-snap-marker \
  >"$evidence_directory/host-box-snap-cat.out" 2>"$evidence_directory/host-box-snap-cat.err"
snap_cat_status=$?
set -e
"$box_bin" snapshot rm "$snap_name" >/dev/null 2>&1 || true
"$box_bin" rm -f "$snap_rest" >/dev/null 2>&1 || true
if ((snap_cat_status != 0)); then
  printf '%s\n' "expected cat after snapshot restore to succeed, got $snap_cat_status" >&2
  cat "$evidence_directory/host-box-snap-cat.err" >&2 || true
  exit 1
fi
grep -Fq 'bx0-snap-marker' "$evidence_directory/host-box-snap-cat.out"
forbid_exit_certified_claim "$evidence_directory/host-box-snap-cat.out"

echo "===== host a3s-box cp guest→host must round-trip ====="
g2h_name="bx0-ci-g2h-$$"
"$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
set +e
"$box_bin" run -d --name "$g2h_name" alpine:3.20 -- sleep 120 \
  >"$evidence_directory/host-box-g2h-run.out" 2>"$evidence_directory/host-box-g2h-run.err"
g2h_run_status=$?
set -e
if ((g2h_run_status != 0)); then
  "$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
  printf '%s\n' "setup run for guest→host cp failed: $g2h_run_status" >&2
  exit 1
fi
set +e
"$box_bin" exec --timeout 30 "$g2h_name" -- sh -c 'printf "%s\n" bx0-g2h-marker > /tmp/bx0-g2h.txt' \
  >"$evidence_directory/host-box-g2h-write.out" 2>"$evidence_directory/host-box-g2h-write.err"
g2h_write_status=$?
set -e
if ((g2h_write_status != 0)); then
  "$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected guest write before cp guest→host to succeed" >&2
  exit 1
fi
set +e
"$box_bin" cp "$g2h_name:/tmp/bx0-g2h.txt" "$evidence_directory/host-box-g2h-out.txt" \
  >"$evidence_directory/host-box-g2h-cp.out" 2>"$evidence_directory/host-box-g2h-cp.err"
g2h_cp_status=$?
set -e
if ((g2h_cp_status != 0)); then
  "$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box cp guest→host to succeed, got $g2h_cp_status" >&2
  cat "$evidence_directory/host-box-g2h-cp.err" >&2 || true
  exit 1
fi
grep -Fq 'bx0-g2h-marker' "$evidence_directory/host-box-g2h-out.txt"

echo "===== host a3s-box restart must restore exec ====="
set +e
"$box_bin" restart "$g2h_name" \
  >"$evidence_directory/host-box-restart.out" 2>"$evidence_directory/host-box-restart.err"
restart_status=$?
set -e
if ((restart_status != 0)); then
  "$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box restart to succeed, got $restart_status" >&2
  cat "$evidence_directory/host-box-restart.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" exec --timeout 30 "$g2h_name" -- true \
  >"$evidence_directory/host-box-restart-exec.out" 2>"$evidence_directory/host-box-restart-exec.err"
restart_exec_status=$?
set -e
if ((restart_exec_status != 0)); then
  "$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected exec after restart to succeed, got $restart_exec_status" >&2
  exit 1
fi

echo "===== host a3s-box kill must leave box non-execable ====="
set +e
"$box_bin" kill "$g2h_name" \
  >"$evidence_directory/host-box-kill.out" 2>"$evidence_directory/host-box-kill.err"
kill_status=$?
set -e
if ((kill_status != 0)); then
  "$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
  printf '%s\n' "expected a3s-box kill to succeed, got $kill_status" >&2
  cat "$evidence_directory/host-box-kill.err" >&2 || true
  exit 1
fi
set +e
"$box_bin" exec --timeout 15 "$g2h_name" -- true \
  >"$evidence_directory/host-box-kill-exec.out" 2>"$evidence_directory/host-box-kill-exec.err"
kill_exec_status=$?
set -e
"$box_bin" rm -f "$g2h_name" >/dev/null 2>&1 || true
if ((kill_exec_status == 0)); then
  printf '%s\n' "exec after kill must fail-closed" >&2
  exit 1
fi
if ! grep -Eiq 'stopped|not running|No such box' \
  "$evidence_directory/host-box-kill-exec.err" \
  "$evidence_directory/host-box-kill-exec.out"
then
  printf '%s\n' "kill exec must report stopped / not running / No such box" >&2
  cat "$evidence_directory/host-box-kill-exec.err" >&2 || true
  exit 1
fi

echo "===== host a3s-box wait must surface guest exit code ====="
# Anti-overfit: wait is not "box gone"; it must report the PID1 exit status.
wait_name="bx0-ci-wait-$$"
"$box_bin" rm -f "$wait_name" >/dev/null 2>&1 || true
set +e
"$box_bin" run -d --name "$wait_name" alpine:3.20 -- sh -c 'sleep 1; exit 7' \
  >"$evidence_directory/host-box-wait-run.out" 2>"$evidence_directory/host-box-wait-run.err"
wait_run_status=$?
set -e
if ((wait_run_status != 0)); then
  "$box_bin" rm -f "$wait_name" >/dev/null 2>&1 || true
  printf '%s\n' "setup run for wait smoke failed: $wait_run_status" >&2
  exit 1
fi
set +e
"$box_bin" wait --timeout 60 "$wait_name" \
  >"$evidence_directory/host-box-wait.out" 2>"$evidence_directory/host-box-wait.err"
wait_cmd_status=$?
set -e
"$box_bin" rm -f "$wait_name" >/dev/null 2>&1 || true
if ((wait_cmd_status != 0)); then
  printf '%s\n' "expected a3s-box wait to succeed, got $wait_cmd_status" >&2
  cat "$evidence_directory/host-box-wait.err" >&2 || true
  exit 1
fi
# Accept a line that is exactly 7 (Docker-compatible wait).
if ! grep -Eq '^7$' "$evidence_directory/host-box-wait.out"; then
  printf '%s\n' "expected wait stdout to be guest exit code 7" >&2
  cat "$evidence_directory/host-box-wait.out" >&2 || true
  exit 1
fi
forbid_exit_certified_claim "$evidence_directory/host-box-wait.out"

printf '%s\n' \
  "A3S_CLOUD_BX0_HOST_BOX_SMOKE_OK box=$box_bin host_os=$(uname -s) product_exit=not_claimed"
