# 0180. Proven H0.5 HA / disaster-recovery claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: H0.5-C1

## Context

Gate `H0.5` remains `planned`. The APP-platform enterprise capability
`enterprise.ha-disaster-recovery` is still `unavailable`.

Control-plane already ships Data object-namespace recovery Flow v2 with exact
workflow identities:

- `cloud.object-namespace.seal@2`
- `cloud.object-namespace.restore@2`
- `cloud.object-namespace.delete@2`

No second HA control plane, ops failover console, autoscaler product UI, or
BYOK residency CMS exists as a first-class surface today. Inventing those
would overfit. Leaving the proven recovery rail `unavailable` blocks honest
H0.5 progress on the APP enterprise claim path.

## Decision

1. **Production claim path** for `enterprise.ha-disaster-recovery` is the
   existing object-namespace recovery Flow v2 seal / restore / delete
   workflow identities. Full Workloads autoscaling, multi-node HA install
   console, and ops DR runbook CMS remain deferred inside this capability
   until first-principles slices exist.

2. **Parity availability:** mark `enterprise.ha-disaster-recovery` `internal`
   under gate `H0.5` with ADR/test/implementation evidence.

3. **Gate `H0.5`:** move from `planned` to `in_progress`. Do not mark
   `implemented` or `verified`. Do not set public `parity_claim`.

4. **Still refused:** inventing BYOK / residency / air-gap (`S0`) from Secrets
   CRUD alone; inventing a second HA plane; marking H0.5 `verified` / public.

## Consequences

- Starts truthful H0.5 progress without inventing HA/ops/BYOK products.
- Does not declare H0.5 or public APP0.6 production-complete.
- Full production release still requires S0 honesty, H0.5 foundation close-out
  when remaining H0.5-owned inventory is claimable, verified gates, and
  public parity.

## Evidence

- This ADR; plan row `H0.5-C1`; architecture + parity updates
- Implementation: Data object-namespace recovery Flow / application workflow
  constants
- Tests: `cargo test -p a3s-cloud-control-plane --lib enterprise_ha_disaster_recovery_claim_path`
  plus `app_platform_parity_manifest`
