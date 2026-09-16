# 0284. K0.1 production foundation for Knowledge corpus control plane

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.1-PF1

## Context

ROADMAP tracks `K0.1` as an in-progress corpus foundation with component slices
`C1` through `C16` already landed: Files admission, KnowledgeBase/Pipeline
catalogs, document/chunk lifecycle, index/retrieval/binding catalogs, REST/
OpenAPI, client/CLI/Management MCP, Nest knowledge controllers (ADR `0240`),
and Nest user-file surfaces (ADRs `0280`/`0281`).

The parity manifest previously listed only product gates `K0.2` / `K0.3` /
`K0.5` as `planned` with plan-doc evidence and kept every `knowledge.*`
capability `unavailable`. That hid the real owning foundation behind Nest and
plan rows. ADR `0251` correctly refuses inventing datasource/transform/pipeline
product availability from Nest HTTP alone; it does not forbid registering the
already-shipped corpus control-plane foundation as `K0.1` `implemented`.

## Decision

1. Add parity gate `K0.1` as `implemented` with migrations, knowledge/user-file
   implementation, focused tests, this ADR, and plan evidence.
2. Add sorted dependency `K0.1` on every `knowledge.*` capability so the
   foundation gate is referenced without changing owning product gates.
3. Keep `K0.2`, `K0.3`, and `K0.5` `planned`.
4. Keep every `knowledge.*` capability `unavailable`.
5. Do not mark `K0.1` `verified`, do not raise `parity_claim`, and do not claim
   public APP0.6 readiness from this foundation alone.
6. Keep Nest claimable-surface exhaustion (ADR `0283`) unchanged.

## Consequences

- `K0.1` is an honest production foundation for corpus identities, catalogs,
  authorized writes, and thin Nest/HTTP/MCP management surfaces.
- Datasource ingestion, chunk-profile productization, retrieval ports, and
  pipeline publish/run product claims remain separately gated.
- ADR `0251` remains the refusal to invent `K0.2`/`K0.3`/`K0.5` availability.
