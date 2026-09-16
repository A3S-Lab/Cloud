# 0164. APP0.2 production foundation with opener/follow-up deferral

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.2-C58

## Context

After APP0.2-C57, proven toolkit surfaces are `internal`. Anonymous delivery
C48-C56 already ship CQRS + `/anonymous-delivery` HTTP with focused lib tests
passing in this worktree, but plan rows remained `Implemented` rather than
Verified. Gate `APP0.2` stayed `in_progress` while:

- `toolkit.opener` and `toolkit.follow-up` have **no domain code** (inventing
  them would overfit),
- `application.chatbot` / `chatflow` / `text-generator` / `workflow` remain
  gated `APP0.2` even though epic `APP0.4` owns completing those modes and
  `application.classic-agent` / `new-agent` are already gated `APP0.4`.

Closing APP0.2 by inventing opener/follow-up or claiming application-mode
product completion would violate first principles. ADR `0162` already shows
the accepted pattern: scoped production foundation + explicit deferral + gate
`implemented` (not `verified` / public claim).

## Decision

1. **Verify anonymous delivery C48-C56** against the focused lib suites that
   already pass (`anonymous_*`, `delivery_credential`) in this worktree. Plan
   status becomes Verified with the worktree tip digest; do not invent new
   anonymous rails.

2. **Explicit deferral:** keep `toolkit.opener` and `toolkit.follow-up`
   `unavailable` under gate `APP0.2`. Do not invent personalized-opener or
   suggested-follow-up aggregates in this slice.

3. **Gate ownership correction:** move `application.chatbot`,
   `application.chatflow`, `application.text-generator`, and
   `application.workflow` from gate `APP0.2` to gate `APP0.4`, matching classic
   / New Agent ownership. Keep them `unavailable` until APP0.4 slices prove
   mode behavior.

4. **Parity gate `APP0.2`:** move state from `in_progress` to `implemented`
   with evidence covering verified anonymous delivery, C57 toolkit claim-path,
   and this ADR. Do not mark `verified` and do not set `parity_claim` / public
   advertising. APP0.6 remains the public claim gate.

5. **Still refused:** inventing opener/follow-up domains, flipping application
   modes to `internal` without APP0.4 behavior, SSE/browser overfitting, and
   treating this as APP0.6 public readiness.

## Consequences

- Epic `APP0.2` may read as production-foundation complete for delivery +
  proven toolkit surfaces, while opener/follow-up stay deferred and application
  modes move to APP0.4.
- APP0.4 becomes the honest owner for chatbot/chatflow/text-generator/workflow
  availability work.
- Full production release across APP0 still requires APP0.4+ gates.

## Evidence

- This ADR; plan rows `APP0.2-C48`..`C56` Verified + `APP0.2-C58`; architecture
  + parity gate refresh
- Prior: ADR `0163`; anonymous delivery ADRs `0121`-`0129`
- Tests: `cargo test -p a3s-cloud-control-plane --lib anonymous_`;
  `cargo test -p a3s-cloud-control-plane --lib delivery_credential`;
  `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`

