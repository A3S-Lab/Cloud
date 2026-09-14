# 0093. Application feedback and annotation authority

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C21`

## Context

The platform model already names `ApplicationFeedback` and
`ApplicationAnnotation` beside sessions and messages, but Applications had no
domain authority for them. Remaining APP0.2 work still listed feedback and
annotations after C20. Persistence, CQRS surfaces, Annotation Reply matching,
and public delivery must not invent a second conversation history.

## Decision

`APP0.2-C21` freezes both aggregates as immutable Applications-owned records
bound to an exact organization/project/application/release digest, session, and
end user. Optional source messages must belong to that exact session. Feedback
carries a discrete positive/negative rating plus optional bounded comment.
Annotations carry bounded JSON content. Deterministic UUIDv5 identities derive
from the session, optional source message, and content digest so exact create
replays without rewriting ordered `ApplicationMessage` sequences. Inactive
sessions, foreign source messages, oversized payloads, and identity drift fail
closed. One in-memory conformance path proves the rules.

## Consequences

`APP0.2-C21` remains component-only. No migration, repository, CQRS command,
REST/MCP, Gateway, SSE, Annotation Reply Inference/Search matching, or public
availability is added. Persistence and write commands stay later numbered
gates.
