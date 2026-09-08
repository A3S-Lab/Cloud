#!/usr/bin/env bash
set -euo pipefail

# Cloud BX0.5 clean-host gate CI fail-closed harness.
# Proves run_bx0_clean_host_gate.sh refuse-closes without claiming product
# A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED. Does NOT unlock product EXIT.

repository_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
tools="$repository_root/tools/box-conformance"
gate="$tools/run_bx0_clean_host_gate.sh"
revision=$(<"$tools/box-revision")
[[ $revision =~ ^[0-9a-f]{40}$ ]]
[[ -x $gate || -f $gate ]]

evidence_directory=$(mktemp -d "${TMPDIR:-/tmp}/a3s-cloud-bx0-clean-host-ci.XXXXXX")
cleanup() {
  rm -rf -- "$evidence_directory"
}
trap cleanup EXIT

forbid_exit_certified_claim() {
  local path=$1
  # Product EXIT marker may appear only as a documented required/refused marker.
  # Any line that claims certification (…EXIT_CERTIFIED without OPEN/required/
  # refuses/not emitted/Must bind scaffolding context) is forbidden in harness output.
  if grep -E --quiet \
    '^[[:space:]]*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED[[:space:]]*$' \
    "$path" 2>/dev/null; then
    printf '%s\n' \
      "CI harness must never emit product EXIT_CERTIFIED claim: $path" >&2
    exit 1
  fi
}

echo "===== static: gate sources + fail-closed contracts ====="
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$gate"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_OPEN' "$gate"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' "$gate"
grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED' "$gate"
grep -Fq 'refuses to fake EXIT_CERTIFIED' "$gate"
grep -Fq 'box-revision' "$gate"
grep -Fq 'BOX-REVISION' "$gate"
grep -Fq 'box_revision_missing' "$gate"
grep -Fq 'box_revision_mismatch' "$gate"
grep -Eq 'exit 1' "$gate"
grep -Eq 'exit 2' "$gate"
grep -Eq 'exit 3' "$gate"
# No success emission of EXIT_CERTIFIED (printf/echo/cat claiming it).
if grep -E '^([[:space:]]*)(printf|echo|cat).*A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED' "$gate" \
  | grep -Ev 'Required certification markers|not emitted|refuses to fake|Must bind'; then
  printf '%s\n' "gate must not printf/echo product EXIT_CERTIFIED" >&2
  exit 1
fi

echo "===== unarmed gate must fail-close (no product EXIT) ====="
unset A3S_CLOUD_BX0_CLEAN_HOST || true
unset A3S_CLOUD_BOX_BIN || true
os_name="$(uname -s)"
set +e
bash "$gate" \
  >"$evidence_directory/unarmed.out" 2>"$evidence_directory/unarmed.err"
unarmed_status=$?
set -e

case $os_name in
  Linux)
    if ((unarmed_status != 2)); then
      printf '%s\n' "expected Linux unarmed exit 2, got $unarmed_status" >&2
      cat "$evidence_directory/unarmed.out" >&2 || true
      cat "$evidence_directory/unarmed.err" >&2 || true
      exit 1
    fi
    if ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' "$evidence_directory/unarmed.err" \
      && ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_SKIP' "$evidence_directory/unarmed.out"; then
      printf '%s\n' "expected A3S_CLOUD_BX0_CLEAN_HOST_SKIP when unarmed" >&2
      exit 1
    fi
    if ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED' "$evidence_directory/unarmed.err" \
      && ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED' "$evidence_directory/unarmed.out"; then
      printf '%s\n' "expected A3S_CLOUD_BX0_CLEAN_HOST_BLOCKED when unarmed" >&2
      exit 1
    fi
    ;;
  *)
    if ((unarmed_status != 1)); then
      printf '%s\n' "expected non-Linux FAIL_CLOSED exit 1, got $unarmed_status" >&2
      cat "$evidence_directory/unarmed.out" >&2 || true
      cat "$evidence_directory/unarmed.err" >&2 || true
      exit 1
    fi
    if ! grep -Fq 'FAIL_CLOSED' "$evidence_directory/unarmed.err"; then
      printf '%s\n' "expected FAIL_CLOSED on non-Linux host" >&2
      exit 1
    fi
    ;;
esac
forbid_exit_certified_claim "$evidence_directory/unarmed.out"
forbid_exit_certified_claim "$evidence_directory/unarmed.err"

if [[ $os_name == Linux ]]; then
  echo "===== armed without a3s-box must exit 2 ====="
  set +e
  env -u A3S_CLOUD_BOX_BIN \
    PATH="/usr/bin:/bin" \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    bash "$gate" \
    >"$evidence_directory/armed-no-box.out" 2>"$evidence_directory/armed-no-box.err"
  armed_status=$?
  set -e
  if ((armed_status != 2)); then
    printf '%s\n' "expected armed-without-box exit 2, got $armed_status" >&2
    cat "$evidence_directory/armed-no-box.out" >&2 || true
    cat "$evidence_directory/armed-no-box.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'a3s-box_unavailable' "$evidence_directory/armed-no-box.err" \
    && ! grep -Fq 'a3s-box_unavailable' "$evidence_directory/armed-no-box.out"; then
    printf '%s\n' "expected a3s-box_unavailable when armed without box" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-no-box.out"
  forbid_exit_certified_claim "$evidence_directory/armed-no-box.err"

  echo "===== armed stub without BOX-REVISION must exit 1 ====="
  stub_root="$evidence_directory/stub-box-missing"
  mkdir -p -- "$stub_root"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_root/a3s-box"
  chmod +x "$stub_root/a3s-box"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_root/a3s-box" \
    bash "$gate" \
    >"$evidence_directory/armed-missing.out" 2>"$evidence_directory/armed-missing.err"
  missing_status=$?
  set -e
  if ((missing_status != 1)); then
    printf '%s\n' "expected missing BOX-REVISION exit 1, got $missing_status" >&2
    cat "$evidence_directory/armed-missing.out" >&2 || true
    cat "$evidence_directory/armed-missing.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'box_revision_missing' "$evidence_directory/armed-missing.err"; then
    printf '%s\n' "expected box_revision_missing" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-missing.out"
  forbid_exit_certified_claim "$evidence_directory/armed-missing.err"

  echo "===== armed stub with mismatched BOX-REVISION must exit 1 ====="
  stub_mismatch="$evidence_directory/stub-box-mismatch"
  mkdir -p -- "$stub_mismatch"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_mismatch/a3s-box"
  chmod +x "$stub_mismatch/a3s-box"
  printf '%s\n' '0000000000000000000000000000000000000000' \
    >"$stub_mismatch/BOX-REVISION"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_mismatch/a3s-box" \
    bash "$gate" \
    >"$evidence_directory/armed-mismatch.out" 2>"$evidence_directory/armed-mismatch.err"
  mismatch_status=$?
  set -e
  if ((mismatch_status != 1)); then
    printf '%s\n' "expected mismatch exit 1, got $mismatch_status" >&2
    cat "$evidence_directory/armed-mismatch.out" >&2 || true
    cat "$evidence_directory/armed-mismatch.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'box_revision_mismatch' "$evidence_directory/armed-mismatch.err"; then
    printf '%s\n' "expected box_revision_mismatch" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-mismatch.out"
  forbid_exit_certified_claim "$evidence_directory/armed-mismatch.err"

  echo "===== armed stub with matching pin must stay OPEN (exit 3) ====="
  stub_match="$evidence_directory/stub-box-match"
  mkdir -p -- "$stub_match"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$stub_match/a3s-box"
  chmod +x "$stub_match/a3s-box"
  printf '%s\n' "$revision" >"$stub_match/BOX-REVISION"
  set +e
  env -u A3S_CLOUD_BOX_REVISION \
    A3S_CLOUD_BX0_CLEAN_HOST=1 \
    A3S_CLOUD_BOX_BIN="$stub_match/a3s-box" \
    bash "$gate" \
    >"$evidence_directory/armed-match.out" 2>"$evidence_directory/armed-match.err"
  match_status=$?
  set -e
  if ((match_status != 3)); then
    printf '%s\n' "expected armed-match exit 3 OPEN, got $match_status" >&2
    cat "$evidence_directory/armed-match.out" >&2 || true
    cat "$evidence_directory/armed-match.err" >&2 || true
    exit 1
  fi
  if ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_OPEN' "$evidence_directory/armed-match.out" \
    && ! grep -Fq 'A3S_CLOUD_BX0_CLEAN_HOST_OPEN' "$evidence_directory/armed-match.err"; then
    printf '%s\n' "expected A3S_CLOUD_BX0_CLEAN_HOST_OPEN for pin-matched stub" >&2
    exit 1
  fi
  if ! grep -Fq "$revision" "$evidence_directory/armed-match.out" \
    && ! grep -Fq "$revision" "$evidence_directory/armed-match.err"; then
    printf '%s\n' "expected pinned box-revision $revision in armed output" >&2
    exit 1
  fi
  forbid_exit_certified_claim "$evidence_directory/armed-match.out"
  forbid_exit_certified_claim "$evidence_directory/armed-match.err"
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_CLEAN_HOST_CI_CERTIFIED revision=$revision host_os=$os_name fail_closed=1"
