#!/usr/bin/env bash
# BX0.5 clean-host ordered step helpers (sourced by run_bx0_clean_host_gate.sh).
#
# Steps 1–3 (enroll / OCI / deploy) are preflight-only: require resolvable
# binaries (and OCI Runtime pin for step 2), record evidence, never perform
# enroll/OCI publish/deploy, and never claim product EXIT. Steps 4–9 remain
# OPEN / not-run until later automation lands.

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

bx0_resolve_oci_cli() {
  local candidate box_dir
  if [[ -n ${A3S_CLOUD_OCI_BIN:-} ]]; then
    if [[ -x $A3S_CLOUD_OCI_BIN ]]; then
      printf '%s\n' "$A3S_CLOUD_OCI_BIN"
      return 0
    fi
    return 1
  fi
  if command -v a3s-oci >/dev/null 2>&1; then
    command -v a3s-oci
    return 0
  fi
  for candidate in \
    "${A3S_CLOUD_BOX_BIN:-}" \
    "${BX0_BOX_BINARY:-}"; do
    if [[ -n $candidate && -x $candidate ]]; then
      box_dir="$(dirname "$candidate")"
      if [[ -x $box_dir/a3s-oci ]]; then
        printf '%s\n' "$box_dir/a3s-oci"
        return 0
      fi
    fi
  done
  return 1
}

# Writes 02-oci.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
bx0_step_oci_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/02-oci.txt"
  mkdir -p -- "$evidence_dir"

  local pin_file="${A3S_CLOUD_BX0_OCI_RUNTIME_REVISION_FILE:-${CLOUD_ROOT:-}/tools/box-conformance/oci-runtime-revision}"
  local expected=
  if [[ ! -f $pin_file ]]; then
    cat >"$evidence" <<EOF
step=2
name=oci
status=preflight_failed
reason=oci_runtime_pin_missing
oci=not_run
EOF
    return 1
  fi
  expected="$(<"$pin_file")"
  if [[ ! $expected =~ ^[0-9a-f]{40}$ ]]; then
    cat >"$evidence" <<EOF
step=2
name=oci
status=preflight_failed
reason=oci_runtime_pin_invalid
oci=not_run
EOF
    return 1
  fi

  local oci=
  if ! oci="$(bx0_resolve_oci_cli)"; then
    cat >"$evidence" <<'EOF'
step=2
name=oci
status=preflight_failed
reason=oci_unavailable
oci=not_run
EOF
    return 1
  fi

  local installed="${A3S_CLOUD_OCI_RUNTIME_REVISION:-}"
  local sidecar
  sidecar="$(dirname "$oci")/OCI-RUNTIME-REVISION"
  if [[ -z $installed && -f $sidecar ]]; then
    installed="$(<"$sidecar")"
  fi
  if [[ -z $installed ]]; then
    cat >"$evidence" <<EOF
step=2
name=oci
status=preflight_failed
reason=oci_runtime_revision_missing
a3s_oci=$oci
expected_pin=$expected
oci=not_run
EOF
    return 1
  fi
  if [[ $installed != "$expected" ]]; then
    cat >"$evidence" <<EOF
step=2
name=oci
status=preflight_failed
reason=oci_runtime_revision_mismatch
a3s_oci=$oci
expected_pin=$expected
installed=$installed
oci=not_run
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=2
name=oci
status=preflight_ok
a3s_oci=$oci
oci_runtime_revision=$expected
oci=not_run
EOF
  return 0
}

bx0_resolve_control_plane() {
  local candidate
  # Prefer explicit BX0/deploy override, then the existing cloud_up API bin env.
  for candidate in \
    "${A3S_CLOUD_CONTROL_PLANE_BIN:-}" \
    "${A3S_CLOUD_DEV_API_BIN:-}"; do
    if [[ -n $candidate ]]; then
      if [[ -x $candidate ]]; then
        printf '%s\n' "$candidate"
        return 0
      fi
      return 1
    fi
  done
  if command -v a3s-cloud-control-plane >/dev/null 2>&1; then
    command -v a3s-cloud-control-plane
    return 0
  fi
  for candidate in \
    "${CLOUD_ROOT:-}/target/debug/a3s-cloud-control-plane" \
    "${CLOUD_ROOT:-}/target/release/a3s-cloud-control-plane"; do
    if [[ -n $candidate && -x $candidate ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

# Writes 03-deploy.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
# Requires a resolvable control-plane binary; does not deploy a Service.
bx0_step_deploy_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/03-deploy.txt"
  mkdir -p -- "$evidence_dir"
  local control_plane=
  if ! control_plane="$(bx0_resolve_control_plane)"; then
    cat >"$evidence" <<'EOF'
step=3
name=deploy
status=preflight_failed
reason=control_plane_unavailable
deploy=not_run
EOF
    return 1
  fi
  cat >"$evidence" <<EOF
step=3
name=deploy
status=preflight_ok
control_plane=$control_plane
deploy=not_run
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

# Record steps 4–9 as OPEN / not-run (remainder after enroll+OCI+deploy preflight).
bx0_write_remaining_open_steps() {
  local evidence_dir=$1
  mkdir -p -- "$evidence_dir"
  bx0_write_open_step "$evidence_dir" 4 health
  bx0_write_open_step "$evidence_dir" 5 https
  bx0_write_open_step "$evidence_dir" 6 logs
  bx0_write_open_step "$evidence_dir" 7 update
  bx0_write_open_step "$evidence_dir" 8 rollback
  bx0_write_open_step "$evidence_dir" 9 stop_cleanup
}
