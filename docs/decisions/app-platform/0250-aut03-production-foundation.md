# 0250. AUT0.3 production foundation for Schedule Trigger

- Status: Accepted
- Date: 2026-09-16
- Gate: AUT0.3-C1

## Context

Automations already own schedule domain state, candidate selection, and
bounded schedule-dispatch application services with focused tests. Parity
still listed `node.schedule-trigger` as `unavailable` under gate `AUT0.3`
`planned`, which understates the claimable invocation-only calendar trigger
slice.

`AUT0.4` integration triggers remain foreign until a real plugin-trigger
owner lands. Public Workflow trigger advertisement and `parity_claim` stay
closed.

## Decision

1. Advertise `node.schedule-trigger` as `internal` (owner `automations`,
   profile `automation.schedule`).
2. Move parity gate `AUT0.3` from `planned` to `implemented`.
3. Bind evidence to schedule domain/dispatch implementations, focused
   dispatch tests, this ADR, and parity tests.
4. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.
5. Refuse inventing integration-trigger / plugin-trigger claims under this
   close-out.

## Consequences

- Catalog can honestly expose Schedule Trigger as an internal automation
  node.
- Integration triggers stay `unavailable`.
- Public product advertisement remains gated by APP0.6 verification rules.

