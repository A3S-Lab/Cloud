# 0243. Advertise Loop as an internal Workflow node

- Status: Accepted
- Date: 2026-09-16
- Gate: W0.3-C2

## Context

`W0.3` already ships immutable composite-region admission, exact Iteration vs
Loop profile fencing (ADR `0060`), sequential Loop coordinator execution with
restart-safe bounds, and focused domain/coordinator/runtime tests. The
application-platform parity manifest still classified `node.loop` as
`unavailable`, which incorrectly described an implemented Workflow-local
composite capability as an absent implementation.

`internal` is deliberately distinct from public product availability. It means
an owning implementation slice exists and can be represented accurately by the
read-only node catalog; it does not verify Answer claim close-out, `W0.4`/
`W0.5`, public Loop, or `parity_claim`.

## Decision

Advertise Workflow-owned `node.loop` as `internal` under gate `W0.3`, keep the
gate `in_progress`, and refuse public Loop or a second orchestration authority.
Defer `node.answer` until its own claim slice lands.

## Consequences

- The checked-in Workflow node catalog can project Loop as `internal`.
- `parity_claim` remains `false` and `public_claim_gate` remains `APP0.6`.
- `W0.3` foundation close-out still requires Answer disposition before any
  gate??implemented` move.

