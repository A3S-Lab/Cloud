# 0247. W0.4 production foundation with foreign-owner node deferral

- Status: Accepted
- Date: 2026-09-16
- Gate: W0.4-C1

## Context

`node.http-request` is already advertised `internal` under gate `W0.4` through
the Connectors claim path (ADR `0054`) with an `AUT0.5` dependency that is now
`implemented` (ADR `0246`). Gate `W0.4` remains `planned` while nine other
W0.4-gated nodes stay `unavailable`:

- `node.agent`, `node.tool` ??Agent / toolkit owners (`A0.5` / APP0.4)
- `node.code` ??Executions
- `node.document-extractor`, `node.knowledge-retrieval` ??Knowledge
- `node.llm`, `node.parameter-extractor`, `node.question-classifier` ??Inference
- `node.variable-assigner` ??Applications variable path beyond W0.3 locals

Inventing those owners under W0.4 would overfit. Leaving `W0.4` `planned` after
the only claimable W0.4 node is already internal blocks the production-foundation
pattern used by APP0.5 / AUT0.5 / W0.3.

## Decision

1. Move parity gate `W0.4` from `planned` to `implemented`.
2. Keep `node.http-request` `internal`; keep the nine foreign-owner nodes
   `unavailable` without inventing implementations.
3. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.
4. Refuse public Workflow service-node advertisement under this close-out.

## Consequences

- W0.4 is an honest production foundation for its claimable Connector HTTP
  Request slice.
- Agent / Knowledge / Inference / Executions nodes remain deferred on their
  owning gates.
- Catalog projection still never infers public availability.

