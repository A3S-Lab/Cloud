# 0169. Proven APP0.5 usage/cost showback claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.5-C2

## Context

Gate `APP0.5` is `in_progress` after Applications claimed
`monitoring.feedback-review` (ADR `0168`). `monitoring.usage-cost` is owned by
`inference`, depends on `I0.2c`, and already ships environment-scoped showback
reads through Inference management routes (`daily-rollups`, request facts) plus
durable ledger/projection evidence documented under I0.2c.

Leaving that proven Inference surface `unavailable` while inventing a second
cost correlation path under Applications or operations_telemetry would overfit.
`I0.2c` remains non-`verified` (provisioned-deployment recovery still open); this
slice does not claim public parity or close I0.2c.

## Decision

1. **Production claim path** for `monitoring.usage-cost` is the existing
   Inference usage query controller under `/organizations`...`/inference-usage/`.
   No second ledger, no Gateway mirror store, no Applications re-ownership.

2. **Parity availability:** mark `monitoring.usage-cost` `internal` under gate
   `APP0.5` with ADR/test/implementation evidence. Keep operations_telemetry
   monitoring capabilities `unavailable`.

3. **Gate `APP0.5`:** remain `in_progress`. Do not mark `implemented` /
   `verified`. Do not set public `parity_claim`. Do not flip `I0.2c` to
   `verified`.

4. **Focused conformance:** add an Inference presentation lib test that freezes
   environment-scoped daily-rollups and request-fact claim paths.

## Consequences

- Continues truthful APP0.5 progress on the only remaining Applications/Inference
  owned monitoring surface with proven code.
- Does not declare APP0.5 production-complete.
- Remaining ops-owned monitoring and APP0.6 still block full production release.

## Evidence

- This ADR; plan row `APP0.5-C2`; architecture + parity updates
- Prior: ADR `0168`; `docs/inference-plan.md` I0.2c shipped notes
- Tests: `cargo test -p a3s-cloud-control-plane --lib monitoring_usage_cost_claim_path`
  plus `app_platform_parity_manifest`

