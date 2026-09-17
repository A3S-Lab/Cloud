# A3S Box Runtime Conformance

Cloud consumes the shared A3S Box Runtime driver directly. The provider
contract and its real-host conformance suite therefore have one implementation
and one source of truth in A3S Box rather than a duplicate Cloud provider.

The `box-revision` and `oci-runtime-revision` files pin the exact provider and
shared-kernel runtime pair used by the Cloud gate. The Box revision must match
the `a3s-box-runtime` Git revision in the workspace manifest.

The GitHub workflow builds the pinned Box shim and guest init, installs the
pinned A3S OCI Runtime into a dedicated temporary Box home, and runs every
Runtime profile advertised by the Box driver. The gate runs on a host where no
Docker-compatible daemon is required or contacted. It fails if the provider
leaves a managed unit, shim, runtime owner, mount, socket, or Runtime state
record behind.

The Cloud consumer phase reuses the same exact provider and the existing Flow,
Fleet command lease, Node Agent journal, Runtime receipt, and resource Claim
state machines. Its cleanup interruption probe sends `SIGKILL` after Box has
durably removed a Service but before the Agent records command completion. A
reconstructed Agent and Flow must adopt the byte-identical removal receipt,
keep the prepared Claim capacity held until that evidence is acknowledged,
release it exactly once, reach terminal cancellation, and leave empty Box
state. The workflow retains the consumer logs and one machine-checkable
certification marker with the provider evidence.

### A0.4 real Agent release gate

The A0.4 consumer test creates a published Agent release and deploys it
through the ordinary Workload, Deployment, Operation, Flow, Fleet, and
Runtime path. It builds the exact Code CLI image through the pinned Box
provider, checks the final OCI manifest/config/archive digests, and binds two
Secrets (provider environment plus a mode-0400 signing file). PostgreSQL
records the durable command, acknowledgement, and observation facts. The
probe kills the Box process, reconstructs the control-plane state, verifies
health/readiness/liveness and restart-time Secret rematerialization, then
stops, removes, and cleans every runtime-owned record. A successful run emits
one `A3S_CLOUD_A0_4_REAL_BOX_RELEASE_CERTIFIED` marker containing the pinned
Box/Code revisions and the exact artifact identity. Local re-bind evidence
(`2026-09-17`) is retained under
[`docs/evidence/a0-4-real-box-2026-09-17`](../../docs/evidence/a0-4-real-box-2026-09-17/)
on Box `8b2804c585d2f31bce06f213990407606a475634` (current `box-revision`).
An older CI citation against Box `65f3d3fc7c1e0e2cb1ba2d409a79f7357314f5ae`
must not be used to claim GA-1 on current pins. Hosted MCP remains owned by
`MCP0`; this gate does not claim `G0`, hosted MCP, or Gateway public Agent
traffic (see GA-1 Gateway LIVE runner below).

### GA-1 Gateway LIVE Code-path smoke

Fail-closed runner:
`tools/box-conformance/run_ga1_gateway_live_code_path_local.sh`.

Requires Docker-free host, pin-matched `a3s-box` + `a3s-gateway`
(`install_gateway_pin.sh`), Postgres `:54320`, registry `:50020`, and the
exact A0.4 image env. Emits
`A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED` only when public traffic
traverses pin-matched Gateway to the published Agent Service. Refuses
`PLACEHOLDER_*`, stub gateways, and BX0 Python TLS theater.

The allocation consumer probe requires Box to advertise CPU, memory, PID, and
execution-timeout controls after the provider phase has passed every profile
derived from those capabilities. When the host qualification advertises
`EphemeralStorage`, the probe also carries an exact byte quota into the
Sandbox writable layer and proves that writes beyond the quota fail closed;
hosts that cannot enforce the bounded layer must not advertise that control.
The probe then carries one inventory-bound Claim through prepare, exact Runtime
binding, reconstructed inspection, pre-fence release rejection, durable stop,
release, removal, and cleanup. The uploaded artifact contains both the
advertised-profile result and the allocation certification marker.

The storage consumer probe binds Box's one Artifact port to the existing node
Artifact manager. It exercises a read-only Artifact mount, a persistent Volume
across driver reconstruction, isolated tmpfs, deterministic Task-output capture
and authenticated publication, exact journal replay, and final removal. The
cleanup gate requires empty Box records, execution directories, VolumeStore
metadata and paths, Secret tmpfs, and node Artifact inventories; retained image
cache and audit evidence are not treated as live workload state.

The build consumer probe uses the same pinned Box revision and the production
Node Agent `BoxBuildStart`/`BoxBuildInspect`/`BoxBuildRemove` adapter. A private
test subprocess completes a real bounded Linux build and uploads its OCI layout
and native cache, then is killed before it can return. A reconstructed executor
must replay the exact output without another logical upload. The probe removes
that operation, clears the sole native cache under the explicitly armed
dedicated home, downloads the immediate-parent cache Artifact, proves native
cache hydration, rebuilds to the exact original OCI manifest descriptor, and
removes again. Revision-bound JSON records every check; build receipts,
operation-owned ImageStore references, and node Artifact files must return to
their pre-test baseline. Shared content-addressed image and layer caches remain
provider-owned reusable state rather than live operations.

The companion Fleet/Flow probe injects loss before persisting nine step
completion events: start dispatch, start acknowledgement, output receipt, and
each cancel/inspect/remove dispatch and acknowledgement. It reconstructs the
Flow engine at every boundary and requires the complete Fleet command object to
remain identical. Validation, publication, attestation, and cleanup counters
must each record one logical effect. Its retained JSON evidence is bound to the
same exact Cloud and Box revisions as the native build probe.

This local build-consumer gate does not replace operator-owned private source,
HTTPS Registry, Vault Transit, OS-process interruption over persistent
Fleet/Flow stores, or published-Workload evidence required to close `G0`.

Real MicroVM and TEE profiles remain hardware-qualified in A3S Box. Cloud does
not reimplement those provider tests.

`run_bx0_tee_isolation_audit.sh` fail-closes BX0.3 hardware TEE binding until an
operator supplies `A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION` with
`A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED ... simulate=false` bound to the exact
Cloud tip and `box-revision`, citing a green Box
`Integration (hardware SEV-SNP)` Actions run URL. Missing evidence prints
`A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED` and exits 2. Simulated SEV-SNP and skipped
Box SEV jobs are rejected. This audit never invents Verified. `bx0-tee-isolation-certification.example.txt` is documentation-only (PLACEHOLDER_* fails closed).

See `OPERATOR_TEE.md` for the org arming + Cloud bind sequence. `preflight_bx0_sev_org_arming.sh` fail-closes until an online `sev-snp` runner exists and never sets `SEV_SNP_CI`. Prefer `collect_bx0_tee_isolation_evidence.sh` over hand-edited cert files: it
calls GitHub and refuses to write `TEE_ISOLATION_CERTIFIED` unless Box job
`Integration (hardware SEV-SNP)` is `success` on the pinned `box-revision`.
Skipped SEV jobs and SHA-mismatched runs fail closed.
`run_bx0_tee_isolation_audit_ci.sh` is the repository-policy CI harness: it proves BLOCKED-without-evidence and rejects `simulate=true` / placeholders without claiming Verified.

`install_box_release.sh` installs checksum-pinned Linux x86_64 Box host
libraries and companion artifacts, then builds the Box CLI, A3S OCI CLI, and
A3S OCI Agent from the exact Cloud-pinned revisions. Disposable PostgreSQL,
NATS, Registry, and object-storage fixtures therefore use the same Box and OCI
capability surface as the Cloud provider gate. The script verifies the host
artifact archive and does not require a VM-capable host or another workload
daemon.

### BX0.5 clean-host release gate

`run_bx0_clean_host_gate.sh` is the fail-closed entrypoint for the BX0.5/E0
Box re-certification clean-host loop (enroll → OCI → deploy → health → HTTPS →
logs → update → rollback → stop/cleanup). Non-Linux hosts fail closed. On Linux
without `A3S_CLOUD_BX0_CLEAN_HOST=1` and a usable `a3s-box`, the script prints
`SKIP`/`BLOCKED` and exits 2. When armed, the gate requires the install-tree pin
(`A3S_CLOUD_BOX_REVISION` or sibling `BOX-REVISION` from `install_box_release.sh`)
to match `tools/box-conformance/box-revision`; missing or mismatched pins
fail-closed with exit 1. A pin-matched armed run also binds Cloud (`git rev-parse
HEAD`), Runtime (`tools/runtime-conformance/runtime-revision`), and Gateway
(`tools/gateway-conformance/gateway-revision`), runs step-1 enroll **preflight**
only (`bx0_clean_host_steps.sh`: require `A3S_CLOUD_NODE_AGENT_BIN` / cargo
target / PATH agent; write `01-enroll.txt` with `enroll=not_run`), then step-2
OCI **preflight** (require `a3s-oci` + matching `OCI-RUNTIME-REVISION` against
`oci-runtime-revision`; write `02-oci.txt` with `oci=not_run`), then step-3
deploy **preflight** (require `A3S_CLOUD_CONTROL_PLANE_BIN` /
`A3S_CLOUD_DEV_API_BIN` / PATH / cargo `a3s-cloud-control-plane`; write
`03-deploy.txt` with `deploy=not_run`), then step-4 health **preflight**
(require `A3S_CLOUD_HEALTH_PROBE_BIN` or `curl`; write `04-health.txt` with
`health=not_run`), then step-5 HTTPS **preflight** (require
`A3S_CLOUD_GATEWAY_BIN` / `A3S_CLOUD_TEST_GATEWAY_BIN` / PATH / cargo
`a3s-gateway` plus matching `GATEWAY-REVISION` against
`tools/gateway-conformance/gateway-revision`; write `05-https.txt` with
`https=not_run`), then step-6 logs **preflight** (require
`A3S_CLOUD_LOGS_PROBE_BIN` or `jq`; write `06-logs.txt` with `logs=not_run`),
then step-7 update **preflight** (require `A3S_CLOUD_DIGEST_PROBE_BIN` or
`sha256sum`/`shasum`; write `07-update.txt` with `update=not_run`), then step-8 rollback **preflight** (require
`A3S_CLOUD_ROLLBACK_PROBE_BIN` or `diff`/`cmp`; write `08-rollback.txt` with
`rollback=not_run`), then step-9 stop/cleanup **preflight** (require resolvable
`a3s-box`; write `09-stop_cleanup.txt` with `stop_cleanup=not_run`), prints
`power_revision=UNBOUND reason=pw0_no_pin_file`, then exits 3 with
`A3S_CLOUD_BX0_CLEAN_HOST_OPEN`, `execute_receipts_complete=0` (preflight-only),
and `loop_exit=not_certified`. Missing node-agent, OCI, control-plane, health
probe, gateway, logs probe, digest probe, rollback probe, or cleanup box
fail-closes with exit 1. It never emits
`A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED`. Use `install_box_release.sh` to
install the pinned Linux Box fixture before arming the gate.

With `A3S_CLOUD_BX0_EXECUTE=1`, steps 1–9 may advance to `*=executed` only
when absolute node `.acl`, enrollment token, real
`A3S_CLOUD_BX0_ENROLL_NODE_ID`, `A3S_CLOUD_BX0_ARTIFACT_DIGEST` (`sha256:` + 64
hex), `A3S_CLOUD_BX0_SERVICE_ID`, `A3S_CLOUD_BX0_HEALTH_URL` (`http`/`https`),
`A3S_CLOUD_BX0_HTTPS_URL` (`https://` only), `A3S_CLOUD_BX0_LOGS_CURSOR`,
`A3S_CLOUD_BX0_UPDATE_DIGEST` (`sha256:` + 64 hex),
`A3S_CLOUD_BX0_ROLLBACK_DIGEST` (`sha256:` + 64 hex), and
`A3S_CLOUD_BX0_CLEANUP_INSTANCE` are supplied (rejects `PLACEHOLDER_*`);
health and HTTPS execute invoke the HTTP probe against those URLs
(`probe_ran=1`, fail-closed on `*_probe_failed`); logs/update/rollback execute
invoke override probe bins with cursor/digest (host jq/sha256sum/diff are
preflight-only); stop/cleanup execute verifies the instance is absent via
`a3s-box inspect`; missing execute inputs
fail-closed. When all nine execute receipts land, OPEN
prints `execute_receipts_complete=1` only after re-verifying on-disk
`*=executed` files, plus `next_loop` / `next_exit` /
`product_exit=not_claimed`, and still `loop_exit=not_certified` — not
product EXIT. The gate does not start the long-poll agent or
publish/deploy/route/read logs/update/rollback/stop. `run_bx0_clean_host_prep.sh`
prints the operator enroll recipe without claiming LOOP or EXIT. See
`OPERATOR_CLEAN_HOST.md`.

`run_bx0_clean_host_gate_ci.sh` is the CI fail-closed harness (mirror of
U0.3 `run_u0_3_exit_audit_ci.sh`). It certifies static refuse-to-fake contracts,
Darwin-safe enroll…stop/cleanup preflight library cases, unarmed /
armed-without-box paths, Linux stub pin missing/mismatch/match, Runtime
pin-missing, and missing-dependency fail-closed behavior, then emits
`A3S_CLOUD_BX0_CLEAN_HOST_CI_CERTIFIED` only. Product EXIT stays open.
Virt-capable hosts also run `run_bx0_host_box_smoke.sh` (extracted first-
principles MicroVM smokes: lifecycle, ephemeral→dead, ports, `cp` both ways,
mounts, pause/unpause, snapshot restore, restart/kill, `wait` exit code).
That harness never claims product LOOP/EXIT.

On Darwin (or any host with `a3s-box`), run the same harness inside a Linux
MicroVM — never Docker (default guest image `alpine:3.20`):

```bash
export A3S_CLOUD_BOX_BIN=$HOME/.local/a3s-box/a3s-box   # if needed
bash tools/box-conformance/run_bx0_clean_host_gate_ci_via_box.sh
# emits A3S_CLOUD_BX0_CLEAN_HOST_CI_VIA_BOX_CERTIFIED; still not product EXIT
```

Product clean-host LOOP still requires a supported Linux x86_64 host and the
pinned Box fixture from `install_box_release.sh` (see `OPERATOR_CLEAN_HOST.md`).
Armed clean-host refuses `DOCKER_HOST` and `/var/run/docker.sock` (zero-Docker;
a3s-box only). GA-0 software EXIT does **not** require Power. Power remains
UNBOUND until PW0 lands `tools/power-conformance/power-revision` for the
stronger `profile=power` EXIT (see that directory's README; do not invent the
pin). TEE isolation remains a separate `BX0.tee` audit.

### BX0.5 operator LOOP certification (exit audit)

`validate_bx0_clean_host_certification.sh` validates an operator
`A3S_CLOUD_BX0_CLEAN_HOST_LOOP_CERTIFIED` line (exact Cloud/Runtime/Box/Gateway
pins plus host/service_id/node_id/artifact_digest; rejects `PLACEHOLDER_*`).
`collect_bx0_clean_host_evidence.sh` writes that line only when
`--gate-evidence-dir` (or `A3S_CLOUD_BX0_EVIDENCE_DIR`) contains steps 1–9
`*=executed` receipts matching node_id / artifact_digest / service_id; IDs alone
are insufficient. It never claims product EXIT.
`run_bx0_software_loop_create.sh` creates enroll (bootstrap + node-agent),
binds a **pre-published** OCI digest (never invents digests), deploys a
workload via CLI, optionally publishes a managed route when gateway scope +
domain claim + hostname are supplied, and with `A3S_CLOUD_BX0_CREATE_FULL=1`
drives logs→update→rollback→`a3s-box rm` (providerResourceId only). Emits
`SOFTWARE_LOOP_CREATE_OK` only — never product EXIT.

`run_bx0_software_exit_live_prep.sh` brings up Box-compose middleware +
control-plane on a Docker-free host (refuses docker.sock; no Docker fallback).
`run_bx0_software_exit_live_oci.sh` publishes real digests (crane|skopeo|oras).
`run_bx0_software_exit_live_agent_release.sh` serves a built node-agent over
local-CA HTTPS and exports URL+sha256. `run_bx0_software_exit_live_https.sh`
terminates TLS to the health port with a CA-aware curl probe (not `curl -k`)
and writes absolute `node.acl`. `run_bx0_software_exit_live_chain.sh` runs
prep→tenant→oci→agent→https→EXIT. `run_bx0_software_exit_live_via_box.sh`
runs that chain inside a Docker-free a3s-box guest when the outer host has
docker.sock. `install_gateway_pin.sh` installs pin-matched `a3s-gateway` +
`GATEWAY-REVISION` sidecar (HTTPS preflight). `run_bx0_software_exit_live.sh`
is the CREATE_FULL+EXIT binder alone. Canonical `/var/run/docker.sock` is
always refused (no sock-path override theater).

`run_bx0_software_exit_harness.sh` orchestrates GA-0 software EXIT: default mode
is refuse-to-fake CI (`A3S_CLOUD_BX0_SOFTWARE_EXIT_HARNESS_CI_CERTIFIED`, never
product EXIT). With `A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1` and operator-owned
real enroll→…→cleanup identities it arms EXECUTE (using
`probe_bx0_{logs,digest,rollback}.sh`), collects LOOP, and runs
`A3S_CLOUD_BX0_EXIT_PROFILE=software` exit audit. It refuses Docker socks,
PLACEHOLDER identities, and invented Power pins. CI binder:
`run_bx0_software_exit_harness_ci.sh`. See
[ga0-bx0-software-checklist.md](../../docs/ga0-bx0-software-checklist.md).

`run_bx0_clean_host_exit_audit.sh` fail-closes with
`A3S_CLOUD_BX0_CLEAN_HOST_EXIT_BLOCKED` when LOOP evidence is missing or when
gate execute receipts are incomplete (`A3S_CLOUD_BX0_EVIDENCE_DIR`). Default
`A3S_CLOUD_BX0_EXIT_PROFILE=software` (GA-0 / `BX0.software`) emits
`A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED … profile=software` with
`power_revision=UNBOUND` when LOOP + receipts pass. `profile=power` still
requires a bound Power pin; a present-but-invalid pin always fail-closes.
Hardware TEE remains a separate `BX0.tee` audit. See `OPERATOR_CLEAN_HOST.md`
and [ga0-bx0-software-checklist.md](../../docs/ga0-bx0-software-checklist.md).
