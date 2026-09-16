# 0170. APP0.5 production foundation with ops monitoring deferral

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.5-C3

## Context

APP0.5-C1 and APP0.5-C2 marked Applications-owned `monitoring.feedback-review`
and Inference-owned `monitoring.usage-cost` `internal` through non-invented
claim paths (session feedback/annotation management; environment-scoped
inference-usage showback). Gate `APP0.5` remains `in_progress` while five
operations_telemetry-owned monitoring capabilities stay `unavailable`
(`run-history`, `latency-failure`, `retention-redaction`, `telemetry-export`,
`alerts`).

A repository audit shows **no `operations_telemetry` control-plane module**
and no Applications/Inference domain owners for those five surfaces today.
Inventing run logs, alert projections, telemetry export, or reclaiming foreign
owners under Applications would overfit. Leaving the gate `in_progress` after
the only claimable monitoring surfaces are honestly claimed blocks the same
production-foundation pattern already accepted for APP0.2 (ADR `0164`),
APP0.3 (ADR `0162`), and APP0.4 (ADR `0167`).

`I0.2c` remains non-`verified` (provisioned-deployment recovery open); this
slice does not close Inference usage as verified.

## Decision

1. **APP0.5 production scope is the two claimable monitoring internals**, not
   full ops telemetry parity. Production foundation means
   `monitoring.feedback-review` and `monitoring.usage-cost` are `internal`
   with Proven C1/C2 claim-path evidence (ADR `0168` / `0169`).

2. **Explicit first-principles deferral:** keep every APP0.5-gated
   `operations_telemetry` monitoring capability `unavailable` until that owner
   ships first-principles domain slices. Do not invent run-history,
   latency/failure diagnostics, retention/redaction product surfaces, telemetry
   export, or operator alerts as the close-out of APP0.5.

3. **Parity gate `APP0.5`:** move state from `in_progress` to `implemented`
   and extend evidence with this ADR plus C1/C2 tests. Do not mark `verified`
   or set public `parity_claim`. `implemented` is the production-foundation
   gate state for claimable monitoring.

4. **Still refused:** inventing an operations_telemetry product module;
   Applications re-ownership of ops surfaces; marking APP0.5 `verified` /
   public; substituting APP0.6 enterprise work as APP0.5 close-out; flipping
   `I0.2c` to `verified`.

## Consequences

- Epic `APP0.5` may move to production-ready for claimable monitoring while
  deferred ops surfaces stay `unavailable`.
- Full monitoring parity and APP0.6 public claim remain separate.
- Goal "reach production release conditions" advances to APP0.6 as the next
  honest blocker, not invented ops monitoring.

## Evidence

- This ADR; plan row `APP0.5-C3`; architecture + parity gate refresh
- Prior: ADR `0168` / `0169`; two monitoring caps = `internal`
- Tests: `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
  (APP0.5 foundation assertions); existing
  `monitoring_feedback_review_claim_path` /
  `monitoring_usage_cost_claim_path` suites

