# 0292. K0.2 Cloud-owned file/text sealed-contract exhaustion

- Status: Accepted
- Date: 2026-09-16
- Gate: production-release-honesty / PROD-R3

## Context

ADRs `0285`–`0291` froze Knowledge-owned file/text sealed contracts for
datasource entrance, built-in plain-text processor output, ingestion
provenance, source tombstone, document incremental update, ingestion
cancellation, and failure cleanup (`K0.2-C1` through `K0.2-C7`). Digests from
those contracts already feed `KnowledgePipelineRelease` and
`KnowledgeDocument` pins.

A HEAD audit after `K0.2-C7` shows no further Cloud-owned file/text contract
kinds that can close product gate `K0.2` without inventing foreign work:

| Remaining K0.2 requirement | Why Cloud alone cannot close it |
| --- | --- |
| Cancellation / cleanup / extract workers | Needs durable execution composition and Box/Runtime isolation for non-trivial processors |
| Online document / drive datasource | Sources connection authenticity + Use Datasource capability (`U0.4`) |
| Web-crawler datasource | Connectors egress + `AUT0.5` crawl intent ownership |
| Marketplace / custom datasource | Use package/capability lifecycle (`U0.4`) |
| Tool / OCR / multimodal processors | Use Tool capability and/or Executions/Runtime/Box |

Persisting C1–C7 intents as CQRS aggregates without those workers would add
writable surface that no owner consumes, overfit presentation preference into
false release progress, and still leave every `knowledge.*` capability
unavailable.

## Decision

1. Treat Cloud-owned file/text sealed-contract coverage for `K0.2` as
   **exhausted** at `K0.2-C1`–`K0.2-C7` for production-foundation purposes.
2. Refuse inventing CQRS/persistence, cancellation/cleanup/extract workers,
   REST/Nest surfaces, or capability availability for those intents until a
   real owning execution path exists.
3. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.
4. Keep foreign product gates `I0.6`, `S0`, `K0.3`, `K0.5`, `U0.4`, and
   `MCP0.5` `planned` until their owning implementation evidence exists
   (ADR `0251` / `0283` refusals unchanged).
5. Next claimable Knowledge work must prove domain/application/infrastructure
   evidence for a specific foreign-backed or worker-backed capability ID before
   any gate or availability change.

## Consequences

- Production release remains blocked on honest foreign owners and real
  ingestion workers, not on further opaque digest contracts for file/text.
- `K0.1` corpus foundation (ADR `0284`) and Nest exhaustion (ADR `0283`) stay
  the prior honesty baselines; this ADR is the post-C7 Knowledge sealed-contract
  audit that proves blockers-only for the Cloud-owned file/text slice.

## Evidence

- This ADR
- Checked-in fixtures under `contracts/k0.2/`
- ADRs `0285`–`0291`
- Focused
  `k02_cloud_file_text_sealed_contracts_c1_through_c7_parse`
- `contracts/app-platform/v1/parity-manifest.acl` (`K0.2` remains `planned`)
