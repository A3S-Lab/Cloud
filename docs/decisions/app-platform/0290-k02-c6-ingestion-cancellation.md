# 0290. K0.2-C6 file/text ingestion-cancellation intent contract

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.2-C6

## Context

Plan `K0.2` requires cancellation of admitted document ingestion work.
ADR `0289` left cancellation/cleanup open after freezing incremental updates.
File/text paths already have entrance, processor, provenance, tombstone, and
incremental-update contracts; Knowledge still needs a sealed cancellation
intent that pins the in-flight work identity without inventing workers.

## Decision

1. Freeze `cloud.knowledge-ingestion-cancellation.v1` for
   `cancel_admitted_user_file_ingestion`,
   `cancel_immutable_object_ingestion`, and
   `cancel_document_incremental_update` only, with sealed ACL, digest, and
   fail-closed parse/restore.
2. For incremental-update cancellation, require previous and next provenance
   digests to differ (reject no-ops).
3. Add strong identity `KnowledgeIngestionCancellationId` and checked-in
   fixture `contracts/k0.2/knowledge-ingestion-cancellation.acl`.
4. Refuse deferred kinds (`online_document`, `web_crawler`,
   `marketplace_datasource`) on parse.
5. Do not add persistence, CQRS, cancellation workers/sagas, REST/Nest
   surfaces, or capability availability.
6. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.

## Consequences

- In-flight file/text ingestion and incremental updates can pin an exact
  Knowledge-owned cancellation intent without inventing drive/crawl
  productization or a second cancellation engine.
- Closing `K0.2` still requires failure-cleanup intent, cleanup/cancellation
  workers, and remaining foreign-owner paths.
