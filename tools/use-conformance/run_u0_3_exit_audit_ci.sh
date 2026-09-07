#!/usr/bin/env bash
set -euo pipefail

# Cloud U0.3 exit-audit CI fail-closed harness (H56).
# Validates live-host certification self-checks and proves the product-exit
# audit fail-closes without live-host evidence. Does NOT claim product
# A3S_CLOUD_U0_3_EXIT_CERTIFIED (no fake live host unlock in CI).

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/use-conformance"
revision=$(<"$tools/use-revision")
[[ $revision =~ ^[0-9a-f]{40}$ ]]

evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-u0-3-exit-audit-ci.XXXXXX")
cleanup() {
  rm -rf -- "$evidence_directory"
}
trap cleanup EXIT

forbid_exit_certified() {
  local path=$1
  if grep -R --fixed-strings --quiet 'A3S_CLOUD_U0_3_EXIT_CERTIFIED' "$path" 2>/dev/null; then
    printf '%s\n' "CI harness must never emit A3S_CLOUD_U0_3_EXIT_CERTIFIED: $path" >&2
    exit 1
  fi
}

echo "===== validator: example must FAIL ====="
set +e
bash "$tools/validate_live_host_certification.sh" \
  "$tools/live-host-certification.example.txt" "$revision" \
  >"$evidence_directory/validator-example.out" 2>"$evidence_directory/validator-example.err"
example_status=$?
set -e
if ((example_status == 0)); then
  printf '%s\n' "expected example certification to FAIL, got exit 0" >&2
  exit 1
fi
forbid_exit_certified "$evidence_directory/validator-example.out"
forbid_exit_certified "$evidence_directory/validator-example.err"

echo "===== validator: good cert must PASS ====="
good_cert="$evidence_directory/good-live-host-certification.txt"
printf '%s\n' \
  "A3S_CLOUD_U0_3_LIVE_HOST_CERTIFIED revision=$revision host=ci-host-1 assignment=assign-1 package=pkg-1 plan_digest=digest-1" \
  >"$good_cert"
bash "$tools/validate_live_host_certification.sh" "$good_cert" "$revision" \
  >"$evidence_directory/validator-good.out" 2>"$evidence_directory/validator-good.err"
forbid_exit_certified "$evidence_directory/validator-good.out"
forbid_exit_certified "$evidence_directory/validator-good.err"

echo "===== exit-audit script sources validator + fail-closes ====="
exit_audit="$tools/run_u0_3_exit_audit.sh"
grep -Fq 'validate_live_host_certification.sh' "$exit_audit"
grep -Fq 'A3S_CLOUD_U0_3_EXIT_BLOCKED' "$exit_audit"
grep -Eq 'exit 2' "$exit_audit"

echo "===== exit-audit without live-host env must exit 2 (LIGHT) ====="
# Unset any operator certification so CI cannot accidentally unlock EXIT_CERTIFIED.
unset A3S_CLOUD_U0_3_LIVE_HOST_CERTIFICATION || true
audit_evidence="$evidence_directory/exit-audit"
mkdir -p -- "$audit_evidence"
set +e
A3S_CLOUD_U0_3_EXIT_AUDIT_LIGHT=1 \
  bash "$exit_audit" "$audit_evidence" \
  >"$evidence_directory/exit-audit.out" 2>"$evidence_directory/exit-audit.err"
audit_status=$?
set -e
if ((audit_status != 2)); then
  printf '%s\n' "expected exit audit to exit 2 without live host, got $audit_status" >&2
  cat "$evidence_directory/exit-audit.out" >&2 || true
  cat "$evidence_directory/exit-audit.err" >&2 || true
  exit 1
fi
if ! grep -Fq 'A3S_CLOUD_U0_3_EXIT_BLOCKED' "$audit_evidence/u0-3-exit-certification.txt"; then
  printf '%s\n' "expected A3S_CLOUD_U0_3_EXIT_BLOCKED in exit certification evidence" >&2
  exit 1
fi
forbid_exit_certified "$evidence_directory"
forbid_exit_certified "$evidence_directory/exit-audit.out"
forbid_exit_certified "$evidence_directory/exit-audit.err"

echo "===== collector: PLACEHOLDER args must FAIL ====="
set +e
bash "$tools/collect_live_host_evidence.sh" \
  --host PLACEHOLDER_HOST --assignment a1 --package p1 --plan-digest d1 \
  --evidence-dir "$evidence_directory/collector-placeholder" \
  >"$evidence_directory/collector-placeholder.out" 2>"$evidence_directory/collector-placeholder.err"
placeholder_status=$?
set -e
if ((placeholder_status == 0)); then
  printf '%s\n' "expected collector to refuse PLACEHOLDER_*, got exit 0" >&2
  exit 1
fi
forbid_exit_certified "$evidence_directory/collector-placeholder.out"
forbid_exit_certified "$evidence_directory/collector-placeholder.err"

echo "===== collector: good args must PASS validator ====="
collector_evidence="$evidence_directory/collector-good"
bash "$tools/collect_live_host_evidence.sh" \
  --host ci-host-1 --assignment assign-1 --package pkg-1 --plan-digest digest-1 \
  --operation-id op-1 --evidence-dir "$collector_evidence" \
  >"$evidence_directory/collector-good.out" 2>"$evidence_directory/collector-good.err"
bash "$tools/validate_live_host_certification.sh" \
  "$collector_evidence/live-host-certification.txt" "$revision"
[[ -f $collector_evidence/checklist.txt ]]
[[ -f $collector_evidence/operation-id ]]
forbid_exit_certified "$collector_evidence"
forbid_exit_certified "$evidence_directory/collector-good.out"
forbid_exit_certified "$evidence_directory/collector-good.err"
if grep -Fq 'A3S_CLOUD_U0_3_EXIT_CERTIFIED' "$evidence_directory/collector-good.out" \
  || grep -Fq 'A3S_CLOUD_U0_3_EXIT_CERTIFIED' "$evidence_directory/collector-good.err"; then
  printf '%s\n' "collector output must not mention EXIT_CERTIFIED" >&2
  exit 1
fi

echo "===== live-host prep must bootstrap serving role + migration URL ====="
prep="$tools/run_u0_3_live_host_prep.sh"
grep -Fq 'ensure_postgres_roles' "$prep"
grep -Fq 'a3s_cloud_serving' "$prep"
grep -Fq 'A3S_CLOUD_POSTGRES_MIGRATION_URL' "$prep"
grep -Fq 'nodes bootstrap' "$prep"
grep -Fq 'plugin-assignments set' "$prep"
grep -Fq 'A3S_CLOUD_U0_3_EXIT_CERTIFIED' "$prep"
# Prep documents EXIT_CERTIFIED only as something it never claims.
if grep -E 'claim[s]? A3S_CLOUD_U0_3_EXIT_CERTIFIED' "$prep" | grep -vq 'NEVER\|Does not\|does NOT\|never'; then
  printf '%s\n' "prep script must not claim product EXIT_CERTIFIED" >&2
  exit 1
fi

printf '%s\n' "A3S_CLOUD_U0_3_EXIT_AUDIT_CI_CERTIFIED revision=$revision light=1"
