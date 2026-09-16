# 0291. K0.2-C7 file/text failure-cleanup intent contract

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.2-C7

## Context

Plan `K0.2` requires failure cleanup after admitted document ingestion work
fails. ADR `0290` left cleanup open after freezing cancellation intents.
File/text paths already have entrance, processor, provenance, tombstone,
incremental-update, and cancellation contracts; Knowledge still needs a sealed
cleanup intent that pins the failed work and failure digests without inventing
workers.

## Decision

1. Freeze `cloud.knowledge-failure-cleanup.v1` for
   `failed_admitted_user_file_ingestion`,
   `failed_immutable_object_ingestion`, and
   `failed_document_incremental_update` only, with sealed ACL, digest, and
   fail-closed parse/restore.
2. Require an opaque `failure_digest` on every kind. For incremental-update
   cleanup, require previous and next provenance digests to differ.
3. Add strong identity `KnowledgeFailureCleanupId` and checked-in fixture
   `contracts/k0.2/knowledge-failure-cleanup.acl`.
4. Refuse deferred kinds (`online_document`, `web_crawler`,
   `marketplace_datasource`) on parse.
5. Do not add persistence, CQRS, cleanup workers/sagas, REST/Nest surfaces, or
   capability availability.
6. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.

## Consequences

- Failed file/text ingestion and incremental updates can pin an exact
  Knowledge-owned cleanup intent without inventing drive/crawl productization
  or a cleanup executor.
- Closing `K0.2` still requires cancellation/cleanup workers and remaining
  foreign-owner paths (Sources/Use/AUT0.5/U0.4/Box).
