# 0287. K0.2-C3 file/text ingestion provenance contract

- Status: Accepted
- Date: 2026-09-16
- Gate: K0.2-C3

## Context

`KnowledgeDocument` already pins an opaque `provenance_digest`, but no
Knowledge-owned ingestion provenance contract existed to produce that
digest. ADR `0251` / `0283` refuse claiming product gate `K0.2` or any
`knowledge.*` availability from Nest or corpus foundation alone.
File-upload and inline-text paths composed with built-in plain-text
extract are Knowledge-owned after K0.2-C1/C2; drive/crawl/marketplace
and Tool/OCR provenance still require foreign owners.

## Decision

1. Freeze `cloud.knowledge-ingestion-provenance.v1` for
   `file_upload_builtin_text` and `inline_text_builtin_text` only, with
   sealed ACL, digest, and fail-closed parse/restore.
2. Bind exact entrance digest, processor output-contract digest, and
   admitted content digest (plus `UserFileId` for file-upload).
3. Add strong identity `KnowledgeIngestionProvenanceId` and checked-in
   fixture `contracts/k0.2/knowledge-ingestion-provenance.acl`.
4. Refuse deferred kinds (`online_document`, `web_crawler`,
   `marketplace_datasource`, `tool_processor`, `ocr_layout`) on parse.
5. Do not add persistence, CQRS, workers, REST/Nest surfaces, or
   capability availability.
6. Keep product gate `K0.2` `planned`, every `knowledge.*` capability
   `unavailable`, `parity_claim=false`, and public APP0.6 blocked.

## Consequences

- Documents can pin real provenance digests for the file/text +
  builtin-text path without inventing foreign datasource or processor
  productization.
- Closing `K0.2` still requires remaining foreign-owner paths,
  cancellation/cleanup workers, and exact source tombstones.
