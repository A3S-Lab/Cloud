# 0107. Application message citation authority

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C35`

## Context

APP0.2 still requires Applications-owned citations beside Answer and FinalOutput
messages. Knowledge already owns corpus, chunk, and retrieval policy. Inventing
an application-local RAG index, or rewriting ordered `ApplicationMessage`
content to embed citations, would reopen the single-authority boundary.

## Decision

`APP0.2-C35` freezes `ApplicationMessageCitation` as an immutable
Applications-owned record bound to an exact organization/project/application/
release digest, session, and end user. The required source message must belong
to that exact session and must be `Answer` or `FinalOutput`. The record stores
the source message's `invocation_id`, exact Knowledge base/revision/document/
chunk lineage identities, and an optional excerpt with a canonical digest.
Deterministic UUIDv5 identity derives from the session, source message,
Knowledge base revision, chunk, and excerpt digest so exact create replays
without rewriting ordered `ApplicationMessage` sequences. Inactive sessions,
Input sources, foreign messages, nil lineage identities, oversized excerpts,
and identity drift fail closed. Applications does not own Knowledge bytes,
indexes, retrieval policy, or a second search client.

## Consequences

`APP0.2-C35` remains component-only. No migration, repository, Knowledge
admission port check, CQRS, blocking/streaming wait, REST/MCP, Gateway, SSE,
or public availability is added. Persistence and write commands stay later
numbered gates.
