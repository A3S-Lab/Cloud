# 0255. AUT0.4 production foundation for normalized event dispatch

- Status: Accepted
- Date: 2026-09-16
- Gate: AUT0.4-PF1

## Context

Parity gate `AUT0.4` is still `planned` with only plan-doc evidence, while
AUT0.4-C1 through AUT0.4-C4 already ship the component-only normalized event
boundary:

- `AutomationNormalizedEventV1` + event invocation factory / filter evaluation
- bounded deterministic fan-out over exact revisions
- durable-ack A3S Event consumer for already-normalized events
- dispatch composition over injected candidate selection and idempotent
  invocation admission

ADR `0251` correctly refuses inventing `node.integration-trigger` /
`plugin.trigger` productization (those still require `U0.4` and a real
plugin-trigger subscription owner). Closing the `AUT0.4` gate on C1-C4
evidence without advertising those capabilities is the same production-
foundation pattern used for A1.4 and I0.2c. Marking `AUT0.4` `verified` or
flipping trigger availability would overfit.

## Decision

1. Move parity gate `AUT0.4` from `planned` to `implemented`.
2. Replace plan-only evidence with the event application modules, focused
   dispatch tests, this ADR, and the plan row.
3. Keep `node.integration-trigger` and `plugin.trigger` `unavailable`.
4. Keep `U0.4` `planned`; do not invent plugin package productization.
5. Do not mark `AUT0.4` `verified` or raise `parity_claim`.
6. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.

## Consequences

- AUT0.4 is an honest production foundation for normalized event fan-out and
  admission composition.
- Integration Trigger product availability, U0.4 plugins, and public APP0.6
  remain separately gated.
- ADR `0251` remains the refusal to invent integration-trigger capability
  claims; this ADR claims only the existing C1-C4 foundation.
