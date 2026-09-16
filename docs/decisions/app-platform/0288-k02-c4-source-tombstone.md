# 0288. K0.2-C4 exact source tombstone contract

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.2-C4

## Context

Plan `K0.2` requires exact source tombstones when a document is deleted.
ADR `0287` left that contract open after freezing file/text provenance.
UserFile tombstones remain Files-owned; Knowledge still needs its own
sealed tombstone that pins the exact document source and provenance
digest without inventing cleanup workers or foreign datasource paths.

## Decision

1. Freeze `cloud.knowledge-source-tombstone.v1` for
   `admitted_user_file` and `immutable_object` only, with sealed ACL,
   digest, and fail-closed parse/restore.
2. Bind exact `document_id`, `provenance_digest`, and source content
   identity (`user_file_id` + `content_digest`, or `content_digest`
   alone for immutable objects).
3. Add strong identity `KnowledgeSourceTombstoneId` and checked-in
   fixture `contracts/k0.2/knowledge-source-tombstone.acl`.
4. Refuse deferred kinds (`online_document`, `web_crawler`,
   `marketplace_datasource`) on parse.
5. Do not add persistence, CQRS, cleanup workers/sagas, REST/Nest
   surfaces, or capability availability.
6. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.

## Consequences

- Document deletion can pin a real Knowledge-owned source tombstone
  for file/text paths without inventing drive/crawl productization or
  a second Files tombstone authority.
- Closing `K0.2` still requires remaining foreign-owner paths,
  cancellation/cleanup workers, and incremental-update contracts.
