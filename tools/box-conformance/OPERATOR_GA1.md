# Operator: GA-1 Gateway LIVE Code-path

Fail-closed smoke for narrow AaaS availability after Verified `BX0.software`.
Does **not** invent Verified; only a retained
`A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED` line plus checklist update may
mark GA-1 Verified.

## Host requirements

- Linux **x86_64** (pinned Box fixture is `linux-x86_64`; arm64 Mac MicroVM
  guests cannot install that fixture and must not invent CERTIFIED)
- `/dev/kvm` present and usable (Box warm-pool / Agent VMs). OrbStack guests on
  Apple Silicon do **not** expose nested KVM — middleware prep there is fine,
  CERTIFIED is not.
- No `/var/run/docker.sock` (and no `.raw`)
- Postgres listening on `:54320` (or set `A3S_CLOUD_TEST_POSTGRES_URL`)
- OCI registry on `http://127.0.0.1:50020/v2/`
- Pin-matched Box install with `BOX-REVISION` == `tools/box-conformance/box-revision`
- Exact A0.4 Agent image env (digest-pinned), typically from retained
  `docs/evidence/a0-4-real-box-2026-09-17/a0-4-image.env`

## Required environment

```bash
export A3S_CLOUD_ROOT=/abs/path/to/apps/cloud
export A3S_CLOUD_BOX_INSTALL_DIR=/abs/path/to/box-install   # contains a3s-box + BOX-REVISION
# or: export A3S_CLOUD_BOX_BIN=/abs/path/to/a3s-box
export A3S_CLOUD_A0_4_IMAGE_ENV=$A3S_CLOUD_ROOT/docs/evidence/a0-4-real-box-2026-09-17/a0-4-image.env
# optional durable cache for gateway pin install:
export A3S_CLOUD_A0_4_DURABLE=$HOME/a0-4-durable
export A3S_CLOUD_GA1_RETAIN_TO=$A3S_CLOUD_ROOT/docs/evidence/ga1-gateway-live-code-path-$(date +%F)
```

Gateway binary: leave unset to build via `install_gateway_pin.sh`, or set
`A3S_CLOUD_GATEWAY_BIN` to a pin-matched binary with sibling `GATEWAY-REVISION`.

## Run

```bash
cd "$A3S_CLOUD_ROOT"
bash tools/box-conformance/run_ga1_gateway_live_code_path_local.sh
```

Success requires the **same** log to contain all of:

1. `A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN`
2. `A3S_CLOUD_GA1_MANAGEMENT_PLANE_PROVEN`
3. `A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED`

Management-plane proof uses Agents-owned
`CreateAgentConversation` → `StartAgentExecution` → `GetAgentExecutionEvents`
(no second scheduler). Public traffic uses pin-matched `a3s-gateway` → Agent
`/health/ready`.

## Honesty

- BX0 Python TLS `:18444` is not Gateway LIVE.
- Management REST/MCP alone is not GA-1.
- Public traffic alone is not CERTIFIED (phase-2b required).
- Do not mark ROADMAP GA-1 Verified without retained evidence under
  `docs/evidence/ga1-gateway-live-code-path-YYYY-MM-DD/`.
- Missing `/dev/kvm` (including OrbStack Apple Silicon guests) must emit
  `A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_BLOCKED reason=box_vm_requires_kvm`
  and must not be marked Verified. See
  `docs/evidence/ga1-gateway-live-code-path-2026-09-18/`.
