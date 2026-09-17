# BX0.5 clean-host operator loop

GA-0 / `BX0.software` EXIT (`profile=software`) needs:

1. A validated LOOP certification for enroll→…→stop/cleanup, and
2. Matching gate execute receipts (steps 1–9 `*=executed`).

Stronger `profile=power` EXIT additionally needs a Power pin file (PW0).
Hardware TEE (`BX0.tee`) is a separate audit and must not block software EXIT.

## Prerequisites

- Supported **Linux** host (x86_64 for the pinned Box release fixture)
- No `DOCKER_HOST` and no Docker sock (default `/var/run/docker.sock`;
  harness override `A3S_CLOUD_BX0_DOCKER_SOCK_PATH` must be absolute) — a3s-box only
- Exact Cloud / Runtime / Box / Gateway pins from this checkout
- `a3s-box` from `install_box_release.sh` (or matching pin)

## Linux CI stubs via a3s-box (not Docker)

Darwin hosts can exercise the Linux-only armed stubs of
`run_bx0_clean_host_gate_ci.sh` inside an a3s-box MicroVM:

```bash
bash tools/box-conformance/run_bx0_clean_host_gate_ci_via_box.sh
```

This certifies `A3S_CLOUD_BX0_CLEAN_HOST_CI_VIA_BOX_CERTIFIED` only. It does
**not** replace the clean Linux x86_64 LOOP or product EXIT.

## Create enroll→deploy (optional)

On a clean Linux x86_64 host with control-plane up and a **pre-published**
OCI artifact (digest never invented by the tool):

```bash
export A3S_CLOUD_URL=...
export A3S_CLOUD_TOKEN=...
export A3S_CLOUD_ORGANIZATION_ID=...
export A3S_CLOUD_PROJECT_ID=...
export A3S_CLOUD_ENVIRONMENT_ID=...
export A3S_CLOUD_BX0_NODE_CONFIG=/absolute/path/to/node.acl
export A3S_CLOUD_BX0_AGENT_RELEASE_URL=https://...
export A3S_CLOUD_BX0_AGENT_RELEASE_SHA256=<64-hex>
export A3S_CLOUD_BX0_ARTIFACT_URI=oci://registry/...@sha256:<64-hex>
export A3S_CLOUD_BX0_ARTIFACT_DIGEST=sha256:<64-hex>
# For CREATE_FULL / EXIT:
export A3S_CLOUD_BX0_HEALTH_URL=http://127.0.0.1:<port>/ready
export A3S_CLOUD_BX0_HTTPS_URL=https://<managed-host>/
export A3S_CLOUD_BX0_UPDATE_ARTIFACT_URI=oci://...@sha256:<other>
export A3S_CLOUD_BX0_UPDATE_DIGEST=sha256:<other>
export A3S_CLOUD_BX0_CREATE_FULL=1
bash tools/box-conformance/run_bx0_software_loop_create.sh
# CREATE_FULL removes the Box via a3s-box rm and exports providerResourceId only.
# source the printed export_file, then:
export A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1
bash tools/box-conformance/run_bx0_software_exit_harness.sh
```

One-shot on a Docker-free host (same env as above):

```bash
# One-shot (prep→tenant→oci→agent HTTPS→TLS health→EXIT). Refuses Docker sock.
# Requires pin-matched a3s-gateway on PATH or A3S_CLOUD_GATEWAY_BIN.
export A3S_CLOUD_GATEWAY_BIN=/abs/a3s-gateway
bash tools/box-conformance/run_bx0_software_exit_live_chain.sh /tmp/bx0-live-chain

# Or step-by-step:
# 1) Box-only middleware + control-plane (refuses Docker sock):
bash tools/box-conformance/run_bx0_software_exit_live_prep.sh
source .a3s/cloud/bx0-live-prep/stack.env
# 2) Tenant bootstrap:
bash tools/box-conformance/run_bx0_software_exit_live_tenant.sh
# 3) Real OCI digests (crane|skopeo|oras):
bash tools/box-conformance/run_bx0_software_exit_live_oci.sh
# 4) Local CA agent release HTTPS:
bash tools/box-conformance/run_bx0_software_exit_live_agent_release.sh
# 5) TLS terminate + node.acl + CA-aware curl probe:
bash tools/box-conformance/run_bx0_software_exit_live_https.sh
# 6) CREATE_FULL + EXIT:
bash tools/box-conformance/run_bx0_software_exit_live.sh
```

Dirty outer host (docker.sock present) — run the chain in a Docker-free guest
(no sock override on the outer host):

```bash
export A3S_CLOUD_BOX_BIN=/abs/a3s-box
bash tools/box-conformance/run_bx0_software_exit_live_via_box.sh
```

Optional HTTPS bind during CREATE (when claim/scope already exist):

```bash
export A3S_CLOUD_BX0_GATEWAY_SCOPE_ID=<uuid>
export A3S_CLOUD_BX0_DOMAIN_CLAIM_ID=<uuid>
export A3S_CLOUD_BX0_ROUTE_HOSTNAME=app.example.test
# CREATE publishes routes and sets HTTPS_URL
```

## Prep

Print the enroll recipe (Darwin-safe; never claims LOOP/EXIT):

```bash
bash tools/box-conformance/run_bx0_clean_host_prep.sh
```

On a supported Linux host with no Docker/compatible daemon:

1. Install the pinned Box fixture:

```bash
bash tools/box-conformance/install_box_release.sh
```

2. Arm the fail-closed gate (preflight only until execute receipts are set):

```bash
export A3S_CLOUD_BX0_CLEAN_HOST=1
export A3S_CLOUD_BOX_BIN=/path/to/a3s-box
# plus node-agent, a3s-oci, control-plane, gateway, curl, jq, sha256sum/shasum, diff/cmp
bash tools/box-conformance/run_bx0_clean_host_gate.sh
# expected while unautomated: exit 3 A3S_CLOUD_BX0_CLEAN_HOST_OPEN

# Opt-in execute for steps 1–9 (still not product EXIT):
export A3S_CLOUD_BX0_EXECUTE=1
export A3S_CLOUD_BX0_NODE_CONFIG=/absolute/path/to/node.acl
export A3S_CLOUD_BX0_ENROLL_NODE_ID=<real-node-uuid>
export A3S_CLOUD_BX0_ARTIFACT_DIGEST=sha256:<64-hex>
export A3S_CLOUD_BX0_SERVICE_ID=<real-service-id>
export A3S_CLOUD_BX0_HEALTH_URL=http://127.0.0.1:<port>/ready
export A3S_CLOUD_BX0_HTTPS_URL=https://<managed-host>/
export A3S_CLOUD_BX0_LOGS_CURSOR=<ordered-log-cursor>
export A3S_CLOUD_BX0_UPDATE_DIGEST=sha256:<64-hex>
export A3S_CLOUD_BX0_ROLLBACK_DIGEST=sha256:<64-hex>
export A3S_CLOUD_BX0_CLEANUP_INSTANCE=<stopped-removed-instance-id>
export A3S_CLOUD_ENROLLMENT_TOKEN=...
bash tools/box-conformance/run_bx0_clean_host_gate.sh
# expected with all receipts: exit 3 OPEN execute_receipts_complete=1
#   next_loop=… next_exit=… next_exit_requires=LOOP+gate_evidence
#   next_exit_profile=software product_exit=not_claimed loop_exit=not_certified
```

3. After receipts, collect LOOP evidence against exact Cloud/Runtime/Box/Gateway
   pins using the gate `evidence_dir`. Retain Service/node/artifact identities.

## Collect LOOP evidence (never claims EXIT)

```bash
# GATE_EVIDENCE_DIR = absolute path from the armed EXECUTE gate (steps 1–9 *=executed).
# Without it the collector fail-closes (execute_receipts_dir_missing).
bash tools/box-conformance/collect_bx0_clean_host_evidence.sh \
  --host "$HOST" \
  --service-id "$SERVICE_ID" \
  --node-id "$NODE_ID" \
  --artifact-digest "$ARTIFACT_DIGEST" \
  --gate-evidence-dir "$GATE_EVIDENCE_DIR" \
  --evidence-dir /tmp/bx0-evidence
```

Point env at the collector output:

```bash
export A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION=/tmp/bx0-evidence/bx0-clean-host-certification.txt
```

Do **not** point at `bx0-clean-host-certification.example.txt` (PLACEHOLDER_* → fail closed).

## Exit audit

```bash
export A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION=/tmp/bx0-evidence/bx0-clean-host-certification.txt
export A3S_CLOUD_BX0_EVIDENCE_DIR="$GATE_EVIDENCE_DIR"
# GA-0 default:
export A3S_CLOUD_BX0_EXIT_PROFILE=software
bash tools/box-conformance/run_bx0_clean_host_exit_audit.sh /tmp/bx0-exit-audit
```

- Without LOOP certification → `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED` exit 2
- With LOOP but no gate execute receipts → `EXIT_BLOCKED reason=execute_receipts_incomplete` exit 2
- With LOOP + receipts and `profile=software` (default) → may emit
  `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED … profile=software power_revision=UNBOUND`
- With LOOP + receipts + `A3S_CLOUD_BX0_EXIT_PROFILE=power` but no Power pin →
  `EXIT_BLOCKED reason=power_unbound` exit 2
- With LOOP + receipts + invalid Power pin → `EXIT_BLOCKED reason=power_pin_invalid` exit 2
- With LOOP + receipts + valid Power pin → may emit
  `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED … profile=power`

CI fail-closed harness `run_bx0_clean_host_gate_ci.sh` proves refuse paths,
software EXIT without inventing `tools/power-conformance/power-revision`, and a
temp-pin power-profile unlock path. On non-Linux-x86_64 hosts it also proves
`install_box_release.sh` refuses the pinned fixture. Virt-capable hosts run
`run_bx0_host_box_smoke.sh` (lifecycle, ephemeral PID1→dead, plain `ps` hides
dead, published-port curl, `exec` requires `--`, host→guest `cp`,
exec-on-stopped fails, create+start `-v` mount, pause/unpause, snapshot
create→restore, guest→host `cp`, restart, kill, `wait` exit code). Via-box CI
must never claim product LOOP/EXIT. Emits
`A3S_CLOUD_BX0_CLEAN_HOST_CI_CERTIFIED` / `…_VIA_BOX_CERTIFIED` only.
