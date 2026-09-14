# 0108. Persist Applications-owned message citations

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C36`

## Context

`APP0.2-C35` froze immutable `ApplicationMessageCitation` aggregates in memory.
CQRS and delivery still require a durable Applications owner. Inventing a second
citation table outside Applications would split authority. Citations must not
rewrite ordered `ApplicationMessage` sequences or bump session
`aggregate_version`.

## Decision

Applications owns persistence for immutable Answer/FinalOutput message
citations:

- migration `208` table `application_message_citations`
- one A3S ORM repository plus an in-memory adapter
- create/replay by primary key (`IdempotentWrite`) without generation CAS
- immutability enforced with `reject_application_session_child_mutation`
- exact session/release/end-user/invocation/message foreign keys
- exact Knowledge base/revision/document/chunk foreign keys
- message kind constrained to `answer` or `final_output`
- optional excerpt plus required excerpt digest

`APP0.2-C36` remains component-only. CQRS, Knowledge admission ports,
REST/MCP, Gateway, SSE, and availability stay later slices.

## Consequences

- Later CQRS and delivery surfaces must load these Applications records rather
  than inventing a parallel citation authority.
- Replay collisions and foreign-key misses fail closed at the repository.
- Public surfaces still must not appear until follow-on delivery slices.

