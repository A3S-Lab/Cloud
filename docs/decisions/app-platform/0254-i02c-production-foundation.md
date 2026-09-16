# 0254. I0.2c production foundation for usage/cost showback

- Status: Accepted
- Date: 2026-09-16
- Gate: I0.2c-PF1

## Context

Parity gate `I0.2c` is still `planned` with only `doc:docs/inference-plan.md`
evidence, while the repository already ships the APP0.5 usage/cost showback
claim path:

- ADR `0169` freezes the bounded showback claim and refuses public LLM productization
- Nest-style presentation macros on
  `usage_queries_controller.rs` (scoped GET routes + OrganizationTenantGuard)
- Focused claim-path tests in `monitoring_usage_cost_claim_path_tests.rs`

Capability `monitoring.usage-cost` is already advertised `internal` under
`APP0.5` with `dependencies = ["I0.2c"]`. Broader Inference gates (`I0.2`,
`I0.6`), LLM nodes, BYOK/air-gap (`S0`), and `parity_claim` remain separate.
Marking `I0.2c` `verified` or flipping availability would overfit.

## Decision

1. Move parity gate `I0.2c` from `planned` to `implemented`.
2. Replace inference-plan-only evidence with ADR `0169`, the Nest usage
   controller, focused monitoring claim-path tests, this ADR, and the plan row.
3. Keep `monitoring.usage-cost` `internal` under `APP0.5`; do not advertise
   public usage/cost or invent LLM node availability.
4. Keep `I0.2` and `I0.6` `planned` until their own owning evidence lands.
5. Do not mark `I0.2c` `verified` or raise `parity_claim`.
6. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.

## Consequences

- I0.2c is an honest production foundation for internal usage/cost showback.
- Inference model routing (`I0.2`), provider productization (`I0.6`), S0, and
  public APP0.6 remain separately gated.
