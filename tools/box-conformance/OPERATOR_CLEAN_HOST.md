# BX0.5 clean-host operator loop (does not claim product EXIT)

Product `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED` stays open until:

1. A validated LOOP certification exists for enroll→…→stop/cleanup, and
2. A Power pin file is bound (PW0; currently UNBOUND).

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

# Opt-in execute for steps 1–7 (still not product EXIT):
export A3S_CLOUD_BX0_EXECUTE=1
export A3S_CLOUD_BX0_NODE_CONFIG=/absolute/path/to/node.acl
export A3S_CLOUD_BX0_ENROLL_NODE_ID=<real-node-uuid>
export A3S_CLOUD_BX0_ARTIFACT_DIGEST=sha256:<64-hex>
export A3S_CLOUD_BX0_SERVICE_ID=<real-service-id>
export A3S_CLOUD_BX0_HEALTH_URL=http://127.0.0.1:<port>/ready
export A3S_CLOUD_BX0_HTTPS_URL=https://<managed-host>/
export A3S_CLOUD_BX0_LOGS_CURSOR=<ordered-log-cursor>
export A3S_CLOUD_BX0_UPDATE_DIGEST=sha256:<64-hex>
export A3S_CLOUD_ENROLLMENT_TOKEN=...
bash tools/box-conformance/run_bx0_clean_host_gate.sh
```

3. Run the remaining rollback → stop/cleanup
   loop against exact Cloud/Runtime/Box/Gateway pins. Retain Service/node/artifact
   identities.

## Collect LOOP evidence (never claims EXIT)

```bash
bash tools/box-conformance/collect_bx0_clean_host_evidence.sh \
  --host "$HOST" \
  --service-id "$SERVICE_ID" \
  --node-id "$NODE_ID" \
  --artifact-digest "$ARTIFACT_DIGEST" \
  --evidence-dir /tmp/bx0-evidence
```

Point env at the collector output:

```bash
export A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFICATION=/tmp/bx0-evidence/bx0-clean-host-certification.txt
```

Do **not** point at `bx0-clean-host-certification.example.txt` (PLACEHOLDER_* → fail closed).

## Exit audit

```bash
bash tools/box-conformance/run_bx0_clean_host_exit_audit.sh /tmp/bx0-exit-audit
```

- Without LOOP certification → `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED` exit 2
- With LOOP but no Power pin → `EXIT_BLOCKED reason=power_unbound` exit 2
- With LOOP + Power pin → may emit `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED`

CI fail-closed harness `run_bx0_clean_host_gate_ci.sh` proves refuse paths only
(`A3S_CLOUD_BX0_CLEAN_HOST_CI_CERTIFIED`).
