# GA-1 / Code-path Agent availability checklist

**Status as of 2026-09-17: In progress** (starts after Verified `BX0.software`).

Authority:
[architecture-optimization-roadmap.md](architecture-optimization-roadmap.md)
(GA-1) and [ROADMAP.md](../ROADMAP.md) (`A0` / `A1`).

Prerequisite: retained
[BX0.software EXIT](evidence/bx0-software-exit-2026-09-17/)
(`profile=software`).

## Exit definition

GA-1 is **Verified** only when one immutable Agent release path is **available**
end-to-end on the software Box path:

```text
One Code-path Agent release
  → Workload/Deployment (A0.4)
  → native Code Harness execution (A1.2)
  → durable semantic events (A1.0/A1.1)
  → reachable through Gateway (public traffic path)
  → retained process-death / control-plane recovery evidence
```

**Required:**

- Exact published Agent release (no invented digests).
- Deploy/update/stop through ordinary Workload authorities on Box.
- Start conversation/execution; observe durable sequenced events.
- Gateway-reachable Agent surface (not management-plane-only theater).
- Retained recovery evidence already owned by `A1.2` (or stronger), plus
  one LIVE software-path smoke that proves the public path still works on
  current `BX0.software` pins.

**Not required for GA-1:**

- Full heterogeneous provider matrix (`A1.3+` non-Code cells).
- `AR0` governed Agent Runtime experience.
- `A1.6` external HTTPS/S3/Box checkpoint production claims.
- `PW0.tee`, distributed `I0`, `CELL0`, `EV0`.

## Engineering backlog (ordered)

1. **Inventory** Verified `A0.4` / `A1.0` / `A1.2` evidence vs current
   Cloud/Runtime/Box/Gateway pins from `BX0.software` EXIT.
   - **Finding (`2026-09-17`):** `BX0.software` EXIT pins Box
     `8b2804c585d2f31bce06f213990407606a475634` (matches
     `tools/box-conformance/box-revision`). Prior retained `A0.4` Box gate
     evidence cited Box `65f3d3fc7c1e0e2cb1ba2d409a79f7357314f5ae` — **pin skew**.
   - **Re-bind (`2026-09-17`):** Retained
     [a0-4-real-box-2026-09-17](evidence/a0-4-real-box-2026-09-17/) with
     `A3S_CLOUD_A0_4_REAL_BOX_RELEASE_CERTIFIED` on Box `8b2804c…` and published
     Agent runtime `@sha256:34cfe15ffa81bcd2d75f5e9b72ed5f3dd8ac1a6e9dd83e0b62f57ab30e7ecda1`.
     A0.4 pin skew is closed; Gateway LIVE **public traffic** is retained under
     [ga1-gateway-public-2026-09-17](evidence/ga1-gateway-public-2026-09-17/)
     (`A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN`). Full CERTIFIED still needs
     management-plane conversation/execution + durable events (phase-2b).
2. **Define** one fail-closed LIVE smoke (refuse PLACEHOLDER / fake Gateway)
   that creates or binds one Agent release, deploys, starts one Code
   execution, reads events, hits Gateway, recovers once, cleans up.
   - **Defined (`2026-09-17`):** marker
     `A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED` with exact
     `gateway_revision` / `box_revision` / `artifact_digest`,
     `gateway_public=traffic`, conversation/execution counts, recovery, cleanup.
     Runner: `tools/box-conformance/run_ga1_gateway_live_code_path_local.sh`.
     Test: ignored
     `ga1_gateway_live_code_path_agent_release_through_real_gateway`.
     Intermediate marker: `A3S_CLOUD_GA1_GATEWAY_PUBLIC_TRAFFIC_PROVEN`.
     **Honesty:** BX0 EXIT Python TLS `:18444/ready` is **not** Gateway LIVE;
     management REST alone is **not** Gateway LIVE; refuse `PLACEHOLDER_*` and
     stub gateways. Public probe is pin-matched `a3s-gateway` → Agent Service
     `/health/ready`. Conversations/events stay on the management plane against
     the same published release.
3. **Run** that smoke on Docker-free software Box; retain evidence under
   `docs/evidence/ga1-gateway-live-code-path-YYYY-MM-DD/`.
4. **Mark** ROADMAP GA-1 / AaaS narrow availability Verified only with
   retained evidence; leave broader AaaS matrix In progress.
5. **Then** start GA-2 (one Workflow/Application vertical).

## Honesty rules

- Component-only `A1.x` slices are not GA-1 availability.
- Management MCP/REST without Gateway public traffic is not GA-1.
- Reusing stale Box pins from older A1.2 CI without re-checking against
  current `BX0.software` pins is overfit — re-bind or re-run.
- Do not invent a second Agent platform or AR0 to claim GA-1.
