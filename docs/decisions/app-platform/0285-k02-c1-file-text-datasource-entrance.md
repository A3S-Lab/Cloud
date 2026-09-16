# 0285. K0.2-C1 file/text datasource entrance contract

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.2-C1

## Context

`KnowledgePipelineRelease` already pins opaque
`datasource_entrance_digests`, but no Knowledge-owned entrance contract
existed to produce those digests. ADR `0251` / `0283` correctly refuse
claiming product gate `K0.2` or any `knowledge.*` availability from Nest
HTTP or corpus foundation alone. File-upload and inline-text entrances
are Knowledge-owned and do not require Sources/`U0.4`/`AUT0.5` foreign
owners; drive, web-crawl, and marketplace kinds still do.

## Decision

1. Freeze `cloud.knowledge-datasource-entrance.v1` for
   `file_upload` and `inline_text` kinds only, with sealed ACL,
   digest, and fail-closed parse/restore.
2. Add strong identity `KnowledgeDatasourceEntranceId` and checked-in
   fixture `contracts/k0.2/knowledge-datasource-entrance.acl`.
3. Refuse deferred kinds (`web_crawler`, drive, marketplace, and any
   unknown kind name) on parse; require `text/*` for inline text.
4. Do not add persistence, CQRS, workers, OCR/processors, REST/Nest
   surfaces, or capability availability.
5. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.

## Consequences

- Pipeline releases can pin real entrance digests for file/text sources
  without inventing ingestion productization.
- Drive/crawl/marketplace entrances remain deferred to owning gates.
- Closing `K0.2` still requires processors, provenance workers,
  cancellation/cleanup, and the foreign-owner datasource paths.
