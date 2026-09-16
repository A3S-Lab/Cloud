# 0289. K0.2-C5 file/text document incremental-update contract

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.2-C5

## Context

Plan `K0.2` requires incremental update of admitted document sources.
ADR `0288` left that contract open after freezing exact source tombstones.
File/text paths already have entrance, processor, provenance, and tombstone
contracts; Knowledge still needs a sealed update that pins previous and next
provenance/content digests without inventing workers or foreign datasources.

## Decision

1. Freeze `cloud.knowledge-document-incremental-update.v1` for
   `replace_admitted_user_file` and `replace_immutable_object` only, with
   sealed ACL, digest, and fail-closed parse/restore.
2. Require both provenance and content digests to change (reject no-ops).
3. Add strong identity `KnowledgeDocumentIncrementalUpdateId` and checked-in
   fixture `contracts/k0.2/knowledge-document-incremental-update.acl`.
4. Refuse deferred kinds (`online_document`, `web_crawler`,
   `marketplace_datasource`) on parse.
5. Do not add persistence, CQRS, workers/sagas, REST/Nest surfaces, or
   capability availability.
6. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.

## Consequences

- Document source replacement can pin an exact Knowledge-owned update
  contract for file/text paths without inventing drive/crawl productization.
- Closing `K0.2` still requires remaining foreign-owner paths and
  cancellation/cleanup workers.
