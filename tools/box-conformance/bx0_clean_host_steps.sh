#!/usr/bin/env bash
# BX0.5 clean-host ordered step helpers (sourced by run_bx0_clean_host_gate.sh).
#
# Steps 1–9 preflight require resolvable binaries (OCI Runtime pin for step 2;
# Gateway pin for step 5), record evidence, and never claim product EXIT.
# When A3S_CLOUD_BX0_EXECUTE=1, steps 1–3 may advance to *=executed only with
# validated operator receipts (node_id, artifact digest, service_id) from a real
# enroll/OCI publish/deploy (never PLACEHOLDER_*). Full daemon work is out of
# band; this gate refuses to fake those steps.
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

# Validate node ACL + token + operator node_id. Does not start a3s-cloud-node-agent.
# Returns 0 on enroll=executed, 1 on execute_failed. No-op (return 0) when
# A3S_CLOUD_BX0_EXECUTE is unset/not 1 (caller should skip).
bx0_step_enroll_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/01-enroll.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local agent=
  if ! agent="$(bx0_resolve_node_agent)"; then
    cat >"$evidence" <<'EOF'
step=1
name=enroll
status=execute_failed
reason=node_agent_unavailable
enroll=not_run
EOF
    return 1
  fi

  local config=${A3S_CLOUD_BX0_NODE_CONFIG:-}
  if [[ -z $config || $config != /* || $config != *.acl || ! -f $config ]]; then
    cat >"$evidence" <<'EOF'
step=1
name=enroll
status=execute_failed
reason=node_config_unavailable
enroll=not_run
EOF
    return 1
  fi

  local missing_keys=()
  for key in enrollment_url enrollment_token_env node_control_url; do
    if ! grep -Eq "^[[:space:]]*${key}[[:space:]]*=" "$config"; then
      missing_keys+=("$key")
    fi
  done
  if ((${#missing_keys[@]} > 0)); then
    cat >"$evidence" <<EOF
step=1
name=enroll
status=execute_failed
reason=node_config_incomplete
missing_keys=${missing_keys[*]}
node_config=$config
enroll=not_run
EOF
    return 1
  fi

  local token_env
  token_env="$(
    awk -F= '/^[[:space:]]*enrollment_token_env[[:space:]]*=/ {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2)
      gsub(/^"|"$/, "", $2)
      print $2
      exit
    }' "$config"
  )"
  [[ -n $token_env ]] || token_env=A3S_CLOUD_ENROLLMENT_TOKEN
  if [[ -z ${!token_env:-} ]]; then
    cat >"$evidence" <<EOF
step=1
name=enroll
status=execute_failed
reason=enrollment_token_unavailable
token_env=$token_env
node_config=$config
enroll=not_run
EOF
    return 1
  fi
  if [[ ${!token_env} == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=1
name=enroll
status=execute_failed
reason=enrollment_token_placeholder
token_env=$token_env
enroll=not_run
EOF
    return 1
  fi

  local node_id=${A3S_CLOUD_BX0_ENROLL_NODE_ID:-}
  if [[ -z $node_id || $node_id == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=1
name=enroll
status=execute_failed
reason=enroll_node_id_missing
node_agent=$agent
node_config=$config
enroll=not_run
hint=Run real nodes bootstrap + Linux a3s-cloud-node-agent enroll, then set A3S_CLOUD_BX0_ENROLL_NODE_ID
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=1
name=enroll
status=execute_ok
node_agent=$agent
node_config=$config
node_id=$node_id
enroll=executed
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

# Operator artifact digest from a real OCI publish. Does not build/publish.
# Returns 0 on oci=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_oci_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/02-oci.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local oci=
  if ! oci="$(bx0_resolve_oci_cli)"; then
    cat >"$evidence" <<'EOF'
step=2
name=oci
status=execute_failed
reason=oci_unavailable
oci=not_run
EOF
    return 1
  fi

  local digest=${A3S_CLOUD_BX0_ARTIFACT_DIGEST:-}
  if [[ -z $digest || $digest == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=2
name=oci
status=execute_failed
reason=artifact_digest_missing
a3s_oci=$oci
oci=not_run
hint=Publish a digest-pinned OCI Artifact, then set A3S_CLOUD_BX0_ARTIFACT_DIGEST
EOF
    return 1
  fi
  if [[ ! $digest =~ ^(sha256:)?[0-9a-f]{64}$ ]]; then
    cat >"$evidence" <<EOF
step=2
name=oci
status=execute_failed
reason=artifact_digest_invalid
a3s_oci=$oci
artifact_digest=$digest
oci=not_run
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=2
name=oci
status=execute_ok
a3s_oci=$oci
artifact_digest=$digest
oci=executed
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

# Operator service_id from a real Box-hosted deploy. Does not deploy.
# Returns 0 on deploy=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_deploy_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/03-deploy.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local control_plane=
  if ! control_plane="$(bx0_resolve_control_plane)"; then
    cat >"$evidence" <<'EOF'
step=3
name=deploy
status=execute_failed
reason=control_plane_unavailable
deploy=not_run
EOF
    return 1
  fi

  local service_id=${A3S_CLOUD_BX0_SERVICE_ID:-}
  if [[ -z $service_id || $service_id == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=3
name=deploy
status=execute_failed
reason=service_id_missing
control_plane=$control_plane
deploy=not_run
hint=Deploy one Box-hosted Service via the ordinary Runtime path, then set A3S_CLOUD_BX0_SERVICE_ID
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=3
name=deploy
status=execute_ok
control_plane=$control_plane
service_id=$service_id
deploy=executed
EOF
  return 0
}

bx0_resolve_health_probe() {
  local candidate
  if [[ -n ${A3S_CLOUD_HEALTH_PROBE_BIN:-} ]]; then
    if [[ -x $A3S_CLOUD_HEALTH_PROBE_BIN ]]; then
      printf '%s\n' "$A3S_CLOUD_HEALTH_PROBE_BIN"
      return 0
    fi
    return 1
  fi
  if command -v curl >/dev/null 2>&1; then
    command -v curl
    return 0
  fi
  return 1
}

# Writes 04-health.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
# Requires a resolvable HTTP probe (curl or override); does not hit Ready.
bx0_step_health_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/04-health.txt"
  mkdir -p -- "$evidence_dir"
  local probe=
  if ! probe="$(bx0_resolve_health_probe)"; then
    cat >"$evidence" <<'EOF'
step=4
name=health
status=preflight_failed
reason=health_probe_unavailable
health=not_run
EOF
    return 1
  fi
  cat >"$evidence" <<EOF
step=4
name=health
status=preflight_ok
health_probe=$probe
health=not_run
EOF
  return 0
}

# Operator Ready URL from a real health probe. Does not curl the Service.
# Returns 0 on health=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_health_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/04-health.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local probe=
  if ! probe="$(bx0_resolve_health_probe)"; then
    cat >"$evidence" <<'EOF'
step=4
name=health
status=execute_failed
reason=health_probe_unavailable
health=not_run
EOF
    return 1
  fi

  local health_url=${A3S_CLOUD_BX0_HEALTH_URL:-}
  if [[ -z $health_url || $health_url == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=4
name=health
status=execute_failed
reason=health_url_missing
health_probe=$probe
health=not_run
hint=Probe the Box-hosted Service Ready endpoint, then set A3S_CLOUD_BX0_HEALTH_URL
EOF
    return 1
  fi
  if [[ ! $health_url =~ ^https?://[^[:space:]]+$ ]]; then
    cat >"$evidence" <<EOF
step=4
name=health
status=execute_failed
reason=health_url_invalid
health_probe=$probe
health_url=$health_url
health=not_run
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=4
name=health
status=execute_ok
health_probe=$probe
health_url=$health_url
health=executed
EOF
  return 0
}

bx0_resolve_gateway() {
  local candidate
  for candidate in \
    "${A3S_CLOUD_GATEWAY_BIN:-}" \
    "${A3S_CLOUD_TEST_GATEWAY_BIN:-}"; do
    if [[ -n $candidate ]]; then
      if [[ -x $candidate ]]; then
        printf '%s\n' "$candidate"
        return 0
      fi
      return 1
    fi
  done
  if command -v a3s-gateway >/dev/null 2>&1; then
    command -v a3s-gateway
    return 0
  fi
  for candidate in \
    "${CLOUD_ROOT:-}/crates/gateway/target/debug/a3s-gateway" \
    "${CLOUD_ROOT:-}/crates/gateway/target/release/a3s-gateway" \
    "${CLOUD_ROOT:-}/target/debug/a3s-gateway" \
    "${CLOUD_ROOT:-}/target/release/a3s-gateway"; do
    if [[ -n $candidate && -x $candidate ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

# Writes 05-https.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
# Requires resolvable a3s-gateway + matching Gateway pin; does not route TLS.
bx0_step_https_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/05-https.txt"
  mkdir -p -- "$evidence_dir"

  local pin_file="${A3S_CLOUD_BX0_GATEWAY_REVISION_FILE:-${CLOUD_ROOT:-}/tools/gateway-conformance/gateway-revision}"
  local expected=
  if [[ ! -f $pin_file ]]; then
    cat >"$evidence" <<EOF
step=5
name=https
status=preflight_failed
reason=gateway_pin_missing
https=not_run
EOF
    return 1
  fi
  expected="$(<"$pin_file")"
  if [[ ! $expected =~ ^[0-9a-f]{40}$ ]]; then
    cat >"$evidence" <<EOF
step=5
name=https
status=preflight_failed
reason=gateway_pin_invalid
https=not_run
EOF
    return 1
  fi

  local gateway=
  if ! gateway="$(bx0_resolve_gateway)"; then
    cat >"$evidence" <<'EOF'
step=5
name=https
status=preflight_failed
reason=gateway_unavailable
https=not_run
EOF
    return 1
  fi

  local installed="${A3S_CLOUD_GATEWAY_REVISION:-${A3S_CLOUD_TEST_GATEWAY_REVISION:-}}"
  local sidecar
  sidecar="$(dirname "$gateway")/GATEWAY-REVISION"
  if [[ -z $installed && -f $sidecar ]]; then
    installed="$(<"$sidecar")"
  fi
  if [[ -z $installed ]]; then
    cat >"$evidence" <<EOF
step=5
name=https
status=preflight_failed
reason=gateway_revision_missing
a3s_gateway=$gateway
expected_pin=$expected
https=not_run
EOF
    return 1
  fi
  if [[ $installed != "$expected" ]]; then
    cat >"$evidence" <<EOF
step=5
name=https
status=preflight_failed
reason=gateway_revision_mismatch
a3s_gateway=$gateway
expected_pin=$expected
installed=$installed
https=not_run
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=5
name=https
status=preflight_ok
a3s_gateway=$gateway
gateway_revision=$expected
https=not_run
EOF
  return 0
}

# Operator managed-TLS URL from a real Gateway route. Does not terminate TLS.
# Returns 0 on https=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_https_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/05-https.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local gateway=
  if ! gateway="$(bx0_resolve_gateway)"; then
    cat >"$evidence" <<'EOF'
step=5
name=https
status=execute_failed
reason=gateway_unavailable
https=not_run
EOF
    return 1
  fi

  local https_url=${A3S_CLOUD_BX0_HTTPS_URL:-}
  if [[ -z $https_url || $https_url == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=5
name=https
status=execute_failed
reason=https_url_missing
a3s_gateway=$gateway
https=not_run
hint=Reach the Service through managed Gateway TLS, then set A3S_CLOUD_BX0_HTTPS_URL
EOF
    return 1
  fi
  if [[ ! $https_url =~ ^https://[^[:space:]]+$ ]]; then
    cat >"$evidence" <<EOF
step=5
name=https
status=execute_failed
reason=https_url_invalid
a3s_gateway=$gateway
https_url=$https_url
https=not_run
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=5
name=https
status=execute_ok
a3s_gateway=$gateway
https_url=$https_url
https=executed
EOF
  return 0
}

bx0_resolve_logs_probe() {
  local candidate
  if [[ -n ${A3S_CLOUD_LOGS_PROBE_BIN:-} ]]; then
    if [[ -x $A3S_CLOUD_LOGS_PROBE_BIN ]]; then
      printf '%s\n' "$A3S_CLOUD_LOGS_PROBE_BIN"
      return 0
    fi
    return 1
  fi
  if command -v jq >/dev/null 2>&1; then
    command -v jq
    return 0
  fi
  return 1
}

# Writes 06-logs.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
# Requires jq (or override) for structured ordered-log evidence; does not read logs.
bx0_step_logs_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/06-logs.txt"
  mkdir -p -- "$evidence_dir"
  local probe=
  if ! probe="$(bx0_resolve_logs_probe)"; then
    cat >"$evidence" <<'EOF'
step=6
name=logs
status=preflight_failed
reason=logs_probe_unavailable
logs=not_run
EOF
    return 1
  fi
  cat >"$evidence" <<EOF
step=6
name=logs
status=preflight_ok
logs_probe=$probe
logs=not_run
EOF
  return 0
}

# Operator ordered-log cursor from a real log read. Does not fetch logs.
# Returns 0 on logs=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_logs_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/06-logs.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local probe=
  if ! probe="$(bx0_resolve_logs_probe)"; then
    cat >"$evidence" <<'EOF'
step=6
name=logs
status=execute_failed
reason=logs_probe_unavailable
logs=not_run
EOF
    return 1
  fi

  local cursor=${A3S_CLOUD_BX0_LOGS_CURSOR:-}
  if [[ -z $cursor || $cursor == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=6
name=logs
status=execute_failed
reason=logs_cursor_missing
logs_probe=$probe
logs=not_run
hint=Read durable ordered Service logs, then set A3S_CLOUD_BX0_LOGS_CURSOR
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=6
name=logs
status=execute_ok
logs_probe=$probe
logs_cursor=$cursor
logs=executed
EOF
  return 0
}

bx0_resolve_digest_probe() {
  local candidate
  if [[ -n ${A3S_CLOUD_DIGEST_PROBE_BIN:-} ]]; then
    if [[ -x $A3S_CLOUD_DIGEST_PROBE_BIN ]]; then
      printf '%s\n' "$A3S_CLOUD_DIGEST_PROBE_BIN"
      return 0
    fi
    return 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    command -v sha256sum
    return 0
  fi
  if command -v shasum >/dev/null 2>&1; then
    command -v shasum
    return 0
  fi
  return 1
}

# Writes 07-update.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
# Requires sha256sum/shasum (or override) for digest-pinned update evidence;
# does not perform an immutable update.
bx0_step_update_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/07-update.txt"
  mkdir -p -- "$evidence_dir"
  local probe=
  if ! probe="$(bx0_resolve_digest_probe)"; then
    cat >"$evidence" <<'EOF'
step=7
name=update
status=preflight_failed
reason=digest_probe_unavailable
update=not_run
EOF
    return 1
  fi
  cat >"$evidence" <<EOF
step=7
name=update
status=preflight_ok
digest_probe=$probe
update=not_run
EOF
  return 0
}

# Operator post-update digest from a real immutable update. Does not update.
# Returns 0 on update=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_update_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/07-update.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local probe=
  if ! probe="$(bx0_resolve_digest_probe)"; then
    cat >"$evidence" <<'EOF'
step=7
name=update
status=execute_failed
reason=digest_probe_unavailable
update=not_run
EOF
    return 1
  fi

  local digest=${A3S_CLOUD_BX0_UPDATE_DIGEST:-}
  if [[ -z $digest || $digest == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=7
name=update
status=execute_failed
reason=update_digest_missing
digest_probe=$probe
update=not_run
hint=Apply one digest-pinned immutable update, then set A3S_CLOUD_BX0_UPDATE_DIGEST
EOF
    return 1
  fi
  if [[ ! $digest =~ ^(sha256:)?[0-9a-f]{64}$ ]]; then
    cat >"$evidence" <<EOF
step=7
name=update
status=execute_failed
reason=update_digest_invalid
digest_probe=$probe
update_digest=$digest
update=not_run
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=7
name=update
status=execute_ok
digest_probe=$probe
update_digest=$digest
update=executed
EOF
  return 0
}

bx0_resolve_rollback_probe() {
  local candidate
  if [[ -n ${A3S_CLOUD_ROLLBACK_PROBE_BIN:-} ]]; then
    if [[ -x $A3S_CLOUD_ROLLBACK_PROBE_BIN ]]; then
      printf '%s\n' "$A3S_CLOUD_ROLLBACK_PROBE_BIN"
      return 0
    fi
    return 1
  fi
  if command -v diff >/dev/null 2>&1; then
    command -v diff
    return 0
  fi
  if command -v cmp >/dev/null 2>&1; then
    command -v cmp
    return 0
  fi
  return 1
}

# Writes 08-rollback.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
# Requires diff/cmp (or override) for cloned-revision evidence; does not roll back.
bx0_step_rollback_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/08-rollback.txt"
  mkdir -p -- "$evidence_dir"
  local probe=
  if ! probe="$(bx0_resolve_rollback_probe)"; then
    cat >"$evidence" <<'EOF'
step=8
name=rollback
status=preflight_failed
reason=rollback_probe_unavailable
rollback=not_run
EOF
    return 1
  fi
  cat >"$evidence" <<EOF
step=8
name=rollback
status=preflight_ok
rollback_probe=$probe
rollback=not_run
EOF
  return 0
}

# Operator post-rollback digest from a real cloned rollback. Does not roll back.
# Returns 0 on rollback=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_rollback_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/08-rollback.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local probe=
  if ! probe="$(bx0_resolve_rollback_probe)"; then
    cat >"$evidence" <<'EOF'
step=8
name=rollback
status=execute_failed
reason=rollback_probe_unavailable
rollback=not_run
EOF
    return 1
  fi

  local digest=${A3S_CLOUD_BX0_ROLLBACK_DIGEST:-}
  if [[ -z $digest || $digest == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=8
name=rollback
status=execute_failed
reason=rollback_digest_missing
rollback_probe=$probe
rollback=not_run
hint=Roll back to a cloned prior revision, then set A3S_CLOUD_BX0_ROLLBACK_DIGEST
EOF
    return 1
  fi
  if [[ ! $digest =~ ^(sha256:)?[0-9a-f]{64}$ ]]; then
    cat >"$evidence" <<EOF
step=8
name=rollback
status=execute_failed
reason=rollback_digest_invalid
rollback_probe=$probe
rollback_digest=$digest
rollback=not_run
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=8
name=rollback
status=execute_ok
rollback_probe=$probe
rollback_digest=$digest
rollback=executed
EOF
  return 0
}

bx0_resolve_cleanup_box() {
  local candidate
  for candidate in \
    "${A3S_CLOUD_BOX_BIN:-}" \
    "${BX0_BOX_BINARY:-}"; do
    if [[ -n $candidate ]]; then
      if [[ -x $candidate ]]; then
        printf '%s\n' "$candidate"
        return 0
      fi
      return 1
    fi
  done
  if command -v a3s-box >/dev/null 2>&1; then
    command -v a3s-box
    return 0
  fi
  return 1
}

# Writes 09-stop_cleanup.txt. Returns 0 on preflight_ok, 1 on preflight_failed.
# Requires resolvable a3s-box for stop/remove; does not stop or clean up.
bx0_step_stop_cleanup_preflight() {
  local evidence_dir=$1
  local evidence="$evidence_dir/09-stop_cleanup.txt"
  mkdir -p -- "$evidence_dir"
  local box=
  if ! box="$(bx0_resolve_cleanup_box)"; then
    cat >"$evidence" <<'EOF'
step=9
name=stop_cleanup
status=preflight_failed
reason=cleanup_box_unavailable
stop_cleanup=not_run
EOF
    return 1
  fi
  cat >"$evidence" <<EOF
step=9
name=stop_cleanup
status=preflight_ok
a3s_box=$box
stop_cleanup=not_run
EOF
  return 0
}

# Operator stop/remove receipt from a real Box cleanup. Does not stop or remove.
# Returns 0 on stop_cleanup=executed, 1 on execute_failed. No-op when EXECUTE unset.
bx0_step_stop_cleanup_execute() {
  local evidence_dir=$1
  local evidence="$evidence_dir/09-stop_cleanup.txt"
  mkdir -p -- "$evidence_dir"

  if [[ ${A3S_CLOUD_BX0_EXECUTE:-} != 1 ]]; then
    return 0
  fi

  local box=
  if ! box="$(bx0_resolve_cleanup_box)"; then
    cat >"$evidence" <<'EOF'
step=9
name=stop_cleanup
status=execute_failed
reason=cleanup_box_unavailable
stop_cleanup=not_run
EOF
    return 1
  fi

  local instance=${A3S_CLOUD_BX0_CLEANUP_INSTANCE:-}
  if [[ -z $instance || $instance == PLACEHOLDER_* ]]; then
    cat >"$evidence" <<EOF
step=9
name=stop_cleanup
status=execute_failed
reason=cleanup_instance_missing
a3s_box=$box
stop_cleanup=not_run
hint=Stop and remove the Box-hosted instance, then set A3S_CLOUD_BX0_CLEANUP_INSTANCE
EOF
    return 1
  fi

  cat >"$evidence" <<EOF
step=9
name=stop_cleanup
status=execute_ok
a3s_box=$box
cleanup_instance=$instance
stop_cleanup=executed
EOF
  return 0
}

# Require gate evidence dir with steps 1–9 *=executed. Optionally check
# node_id / artifact_digest / service_id match collector fields.
# Returns 0 when complete; prints reason and returns 1 otherwise.
bx0_require_execute_receipts_dir() {
  local evidence_dir=$1
  local expected_node_id=${2:-}
  local expected_artifact_digest=${3:-}
  local expected_service_id=${4:-}
  local file key expected actual
  local -a required=(
    '01-enroll.txt:enroll=executed'
    '02-oci.txt:oci=executed'
    '03-deploy.txt:deploy=executed'
    '04-health.txt:health=executed'
    '05-https.txt:https=executed'
    '06-logs.txt:logs=executed'
    '07-update.txt:update=executed'
    '08-rollback.txt:rollback=executed'
    '09-stop_cleanup.txt:stop_cleanup=executed'
  )

  if [[ -z $evidence_dir || $evidence_dir != /* || ! -d $evidence_dir ]]; then
    printf '%s\n' 'reason=execute_receipts_dir_missing' >&2
    return 1
  fi

  for entry in "${required[@]}"; do
    file=${entry%%:*}
    key=${entry#*:}
    if [[ ! -f $evidence_dir/$file ]]; then
      printf '%s\n' "reason=execute_receipt_missing file=$file" >&2
      return 1
    fi
    if ! grep -Fq "$key" "$evidence_dir/$file"; then
      printf '%s\n' "reason=execute_receipt_incomplete file=$file need=$key" >&2
      return 1
    fi
    if grep -Fq 'status=execute_failed' "$evidence_dir/$file"; then
      printf '%s\n' "reason=execute_receipt_failed file=$file" >&2
      return 1
    fi
  done

  if [[ -n $expected_node_id ]]; then
    actual=$(awk -F= '/^node_id=/{print $2; exit}' "$evidence_dir/01-enroll.txt" 2>/dev/null || true)
    if [[ $actual != "$expected_node_id" ]]; then
      printf '%s\n' \
        "reason=execute_receipt_node_id_mismatch got=$actual expected=$expected_node_id" >&2
      return 1
    fi
  fi
  if [[ -n $expected_artifact_digest ]]; then
    actual=$(awk -F= '/^artifact_digest=/{print $2; exit}' "$evidence_dir/02-oci.txt" 2>/dev/null || true)
    if [[ $actual != "$expected_artifact_digest" ]]; then
      printf '%s\n' \
        "reason=execute_receipt_artifact_digest_mismatch got=$actual expected=$expected_artifact_digest" >&2
      return 1
    fi
  fi
  if [[ -n $expected_service_id ]]; then
    actual=$(awk -F= '/^service_id=/{print $2; exit}' "$evidence_dir/03-deploy.txt" 2>/dev/null || true)
    if [[ $actual != "$expected_service_id" ]]; then
      printf '%s\n' \
        "reason=execute_receipt_service_id_mismatch got=$actual expected=$expected_service_id" >&2
      return 1
    fi
  fi

  return 0
}
