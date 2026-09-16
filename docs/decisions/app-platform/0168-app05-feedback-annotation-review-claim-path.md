# 0168. Proven APP0.5 feedback/annotation review claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.5-C1

## Context

Gate `APP0.5` is still `planned` while seven `monitoring.*` capabilities remain
`unavailable`. Only `monitoring.feedback-review` is owned by Applications and
depends on APP0.2. Feedback and annotation create/get/list already ship as
Verified APP0.2 toolkit surfaces (ADR `0093`-`0096`, claim path ADR `0163`)
through Applications management `/organizations/.../sessions/{session_id}/`
routes.

The other APP0.5 capabilities are owned by `operations_telemetry` or
`inference` and have no Applications-owned domain evidence
(`monitoring.run-history`, `latency-failure`, `retention-redaction`,
`telemetry-export`, `alerts`, `usage-cost`). Inventing those projections, a
second run log, or reclaiming foreign owners under Applications would overfit.
Leaving the proven feedback/annotation review surface `unavailable` blocks
honest APP0.5 progress after APP0.4 production foundation (ADR `0167`).

## Decision

1. **Production claim path** for `monitoring.feedback-review` is the existing
   Applications management traffic owner: session-scoped feedback and
   annotation command/query routes under `/organizations`. No separate review
   microservice, mirror store, or second run log.

2. **Parity availability:** mark `monitoring.feedback-review` `internal` under
   gate `APP0.5` with ADR/test/implementation evidence. Keep the other six
   `monitoring.*` capabilities `unavailable` until their declared owners ship
   first-principles slices.

3. **Gate `APP0.5`:** move from `planned` to `in_progress`. Do not mark
   `implemented` or `verified`. Do not set public `parity_claim`.

4. **Focused conformance:** add a Cloud presentation lib test that freezes
   session-scoped feedback/annotation list and get claim paths on
   `/organizations`.

## Consequences

- Starts truthful APP0.5 progress without inventing operations/inference
  monitoring surfaces.
- Does not declare APP0.5 production-complete.
- Full production release still requires remaining APP0.5 monitoring owners
  and APP0.6.

## Evidence

- This ADR; plan row `APP0.5-C1`; architecture + parity updates
- Prior: ADR `0093`-`0096`, `0163`, `0167`
- Tests: `cargo test -p a3s-cloud-control-plane --lib monitoring_feedback_review_claim_path`
  plus `app_platform_parity_manifest`

