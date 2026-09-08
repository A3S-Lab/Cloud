#!/usr/bin/env bash
# BX0.5 clean-host ordered step helpers (sourced by run_bx0_clean_host_gate.sh).
#
# Step 1 enroll is preflight-only: require a resolvable node-agent binary and
# record evidence. It never performs enrollment and never claims product EXIT.
# Steps 2–9 remain OPEN / not-run until later automation lands.

bx0_resolve_node_agent() {
  local candidate
  if [[ -n ${A3S_CLOUD_NODE_AGENT_BIN:-} ]]; then
    if [[ -x $A3S_CLOUD_NODE_AGENT_BIN ]]; then
      printf '%s\n' "$A3S_CLOUD_NODE_AGENT_BIN"
      return 0
    fi
    return 1
  fi
  if command -v a3s-cloud-node-agent >/dev/null 2>&1; then
    command -v a3s-cloud-node-agent
    return 0
  fi
  for candidate in \
    "${CLOUD_ROOT:-}/target/debug/a3s-cloud-node-agent" \
    "${CLOUD_ROOT:-}/target/release/a3s-cloud-node-agent"; do
    if [[ -n $candidate && -x $candidate ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

# Writes 01-enroll.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
bx0_step_enroll_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/01-enroll.txt"
  mkdir -p -- "$evidence_dir"
  local agent=
  if ! agent="$(bx0_resolve_node_agent)"; then
    cat >"$evidence" <<'EOF'
step=1
name=enroll
status=preflight_failed
reason=node_agent_unavailable
enroll=not_run
EOF
    return 1
  fi
  cat >"$evidence" <<EOF
step=1
name=enroll
status=preflight_ok
node_agent=$agent
enroll=not_run
EOF
  return 0
}

bx0_write_open_step() {
  local evidence_dir=$1
  local index=$2
  local name=$3
  local file
  printf -v file '%s/%02d-%s.txt' "$evidence_dir" "$index" "$name"
  cat >"$file" <<EOF
step=$index
name=$name
status=OPEN
not_run=1
EOF
}

# Record steps 2–9 as OPEN / not-run (ordered checklist remainder).
bx0_write_remaining_open_steps() {
  local evidence_dir=$1
  mkdir -p -- "$evidence_dir"
  bx0_write_open_step "$evidence_dir" 2 oci
  bx0_write_open_step "$evidence_dir" 3 deploy
  bx0_write_open_step "$evidence_dir" 4 health
  bx0_write_open_step "$evidence_dir" 5 https
  bx0_write_open_step "$evidence_dir" 6 logs
  bx0_write_open_step "$evidence_dir" 7 update
  bx0_write_open_step "$evidence_dir" 8 rollback
  bx0_write_open_step "$evidence_dir" 9 stop_cleanup
}
