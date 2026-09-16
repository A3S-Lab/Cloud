# 0286. K0.2-C2 built-in plain-text processor output contract

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.2-C2

## Context

`KnowledgePipelineRelease` already pins an opaque `output_contract_digest`,
but no Knowledge-owned processor output contract existed to produce that
digest. ADR `0251` / `0283` refuse claiming product gate `K0.2` or any
`knowledge.*` availability from Nest or corpus foundation alone. Built-in
plain-text extract admission is Knowledge-owned; tool processors require
`U0.4`, and OCR/layout/multimodal require Executions/Runtime/Box owners.

## Decision

1. Freeze `cloud.knowledge-processor-output-contract.v1` for
   `builtin_plain_text_extract` only, with sealed ACL, digest, and
   fail-closed parse/restore.
2. Add strong identity `KnowledgeProcessorOutputContractId` and
   checked-in fixture
   `contracts/k0.2/knowledge-processor-output-contract.acl`.
3. Require `text/*` output media type and bounded `max_input_bytes`.
4. Refuse deferred kinds (`tool_processor`, `ocr_layout`,
   `multimodal_attachment`) on parse.
5. Do not add persistence, CQRS, workers, Box/OCR execution, REST/Nest
   surfaces, or capability availability.
6. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.

## Consequences

- Pipeline releases can pin real processor output digests for built-in
  plain-text extract without inventing Tool/OCR productization.
- Closing `K0.2` still requires foreign-owner processors, provenance
  workers, cancellation/cleanup, and remaining datasource paths.
