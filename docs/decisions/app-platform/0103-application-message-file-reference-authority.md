# 0103. Application message file-reference authority

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C31`

## Context

APP0.2 still requires Applications-owned file references beside sessions and
messages. Files already owns UserFile admission and bytes. Inventing an
application-local blob table, or attaching files by rewriting ordered
`ApplicationMessage` content, would reopen the single-authority boundary.

## Decision

`APP0.2-C31` freezes `ApplicationMessageFileReference` as an immutable
Applications-owned record bound to an exact organization/project/application/
release digest, session, and end user. The required source message must belong
to that exact session and must be `Input`. The record stores the source
message's `invocation_id`, an exact `UserFileId`, and the admitted content
digest. Deterministic UUIDv5 identity derives from the session, Input message,
UserFile, and content digest so exact create replays without rewriting ordered
`ApplicationMessage` sequences. Inactive sessions, Answer/FinalOutput sources,
foreign messages, nil identities, non-canonical digests, and identity drift fail
closed. Applications does not own file bytes, scan state, or a second object
client.

## Consequences

`APP0.2-C31` remains component-only. No migration, repository, Files admission
port check, CQRS, citations, blocking/streaming wait, REST/MCP, Gateway, SSE,
or public availability is added. Persistence and write commands stay later
numbered gates.
