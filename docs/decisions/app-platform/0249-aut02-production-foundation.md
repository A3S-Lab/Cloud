# 0249. AUT0.2 production foundation for Webhook Trigger

- Status: Accepted
- Date: 2026-09-16
- Gate: AUT0.2-C1

## Context

Automations already own a Nest-macro webhook controller (ADR `0223`),
definition/revision/admission contracts, and control-plane webhook lifecycle
transport. Parity still listed `node.webhook-trigger` as `unavailable` under
gate `AUT0.2` `planned`, which understates the claimable invocation-only
trigger slice.

`AUT0.3` schedule and `AUT0.4` integration triggers remain separate owners.
Public Workflow trigger advertisement and `parity_claim` stay closed.

## Decision

1. Advertise `node.webhook-trigger` as `internal` (owner `automations`,
   profile `automation.webhook`).
2. Move parity gate `AUT0.2` from `planned` to `implemented`.
3. Bind evidence to Nest webhook controllers, webhook admission/lifecycle,
   automation contracts, this ADR, and focused parity tests.
4. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.
5. Refuse inventing schedule/integration trigger claims under this close-out.

## Consequences

- Catalog can honestly expose Webhook Trigger as an internal automation node.
- Schedule and integration triggers stay `unavailable` until their own
  claimable evidence lands.
- Public product advertisement remains gated by APP0.6 verification rules.

