# 0181. H0.5 production foundation for claimable HA / disaster-recovery

- Status: Accepted
- Date: 2026-09-16
- Gate: H0.5-C2

## Context

H0.5-C1 marked platform-owned `enterprise.ha-disaster-recovery` `internal`
through the proven Data object-namespace recovery Flow v2 claim path
(`cloud.object-namespace.{seal,restore,delete}@2`). Gate `H0.5` remains
`in_progress`.

No further APP-platform enterprise capabilities are gated on `H0.5`. Workloads
autoscaling consoles, multi-node HA install UI, ops DR runbook CMS, and BYOK /
residency / air-gap (`S0`) remain deferred **inside** the claimed capability or
on foreign gates, matching the C0.5 / APP0.6 production-foundation pattern
(ADR `0176` / `0173`).

Leaving `H0.5` `in_progress` after every H0.5-gated enterprise capability is
honestly claimed blocks that same foundation pattern.

## Decision

1. **H0.5 production scope (APP enterprise inventory) is the one claimable
   H0.5 enterprise internal** (`enterprise.ha-disaster-recovery`), not inventing
   deferred autoscaling / HA-install / ops-CMS product surfaces.

2. **Explicit first-principles deferral:** keep Workloads autoscaling console,
   multi-node HA install UI, and ops DR runbook CMS deferred inside the claimed
   capability until first-principles slices exist. Do not invent them as H0.5
   close-out. Do not invent `S0` BYOK / residency / air-gap from Secrets CRUD.

3. **Parity gate `H0.5`:** move state from `in_progress` to `implemented` and
   extend evidence with this ADR plus the C1 claim-path test. Do not mark
   `verified` or set public `parity_claim`. `public_claim_gate` remains
   `APP0.6`.

4. **Still refused:** inventing BYOK/`S0`; inventing a second HA control plane;
   marking H0.5 `verified` / public; treating Nest-only migrations as release
   completion.

## Consequences

- Epic `H0.5` may move to production-ready for the claimable APP HA/DR surface
  while deferred product depths stay unfinished inside the capability.
- Full production release still requires honest `S0`, verified dependency gates,
  and public APP0.6 parity.

## Evidence

- This ADR; plan row `H0.5-C2`; architecture + parity gate refresh
- Prior: ADR `0180`; `enterprise.ha-disaster-recovery` = `internal`
- Tests: `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
  (H0.5 foundation assertions); existing HA-DR claim-path suite
