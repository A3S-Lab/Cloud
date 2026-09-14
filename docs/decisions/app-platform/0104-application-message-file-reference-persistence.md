# 0104. Persist Applications-owned message file references

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C32`

## Context

`APP0.2-C31` froze immutable `ApplicationMessageFileReference` aggregates in
memory. CQRS and delivery still require a durable Applications owner. Inventing
a second file-reference table outside Applications would split authority.
References must not rewrite ordered `ApplicationMessage` sequences or bump
session `aggregate_version`.

## Decision

Applications owns persistence for immutable Input message file references:

- migration `207` table `application_message_file_references`
- one A3S ORM repository plus an in-memory adapter
- create/replay by primary key (`IdempotentWrite`) without generation CAS
- immutability enforced with `reject_application_session_child_mutation`
- exact session/release/end-user/invocation/message foreign keys
- exact `user_files (organization_id, id)` foreign key
- message kind constrained to `input`

`APP0.2-C32` remains component-only. CQRS, Files admission ports, citations,
REST/MCP, Gateway, SSE, and availability stay later slices.

## Consequences

- Later CQRS and delivery surfaces must load these Applications records rather
  than inventing a parallel file-reference authority.
- Replay collisions and foreign-key misses fail closed at the repository.
- Public surfaces still must not appear until follow-on delivery slices.
