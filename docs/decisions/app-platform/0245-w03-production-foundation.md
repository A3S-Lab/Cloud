# 0245. W0.3 production foundation for claimable Workflow/Answer nodes

- Status: Accepted
- Date: 2026-09-16
- Gate: W0.3-C4

## Context

W0.3-C1 through W0.3-C3 marked every `W0.3`-gated capability `internal`:
Workflow-owned Iteration/Loop plus the prior W0.3 Workflow locals, and
Applications-owned Answer (with `APP0.2` dependency). Gate `W0.3` remains
`in_progress`.

No further capabilities are gated on `W0.3`. Later Workflow nodes
(`node.http-request` is already claimed under other inventory; Knowledge/
LLM/Agent nodes remain on `W0.4`/`W0.5`/`I0.2`/`A0.*`) stay on foreign gates.
Public catalog advertisement and `parity_claim` remain forbidden until
`APP0.6` public verification.

Leaving `W0.3` `in_progress` after every W0.3-gated capability is honestly
claimed blocks the production-foundation pattern used by APP0.6 / C0.3 / H0.5.

## Decision

1. Move parity gate `W0.3` from `in_progress` to `implemented`.
2. Keep every W0.3 capability `internal` (not `public`).
3. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.
4. Refuse inventing public streaming/chat availability or foreign-gate claims
   under this close-out.

## Consequences

- W0.3 is an honest production foundation for its gated node inventory.
- Catalog projection remains non-public.
- AUT0.5 / A0.5 / S0 / APP0.6 public verification remain separate release
  blockers.

