# 0098. Persist Applications-owned message variants

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C26`

## Context

`APP0.2-C25` froze immutable `ApplicationMessageVariant` ("More Like This")
aggregates in memory. CQRS and delivery still require a durable Applications
owner. Inventing a second variant table outside Applications would split
authority. Variants must not rewrite ordered `ApplicationMessage` sequences or
bump session `aggregate_version`.

## Decision

Applications owns persistence for immutable session message variants:

- migration `206` table `application_message_variants`
- one A3S ORM repository plus an in-memory adapter
- create/replay by primary key (`IdempotentWrite`) without generation CAS
- immutability enforced with `reject_application_session_child_mutation`
- exact session/release/end-user/invocation/source-message foreign keys
- source kind constrained to `answer` and `final_output`

`APP0.2-C26` remains component-only. CQRS, regeneration invocation, REST/MCP,
Gateway, SSE, and availability stay later slices.

## Consequences

- Later CQRS and delivery surfaces must load these Applications records rather
  than inventing a parallel More Like This authority.
- Replay collisions and foreign-key misses fail closed at the repository.
- Public surfaces still must not appear until follow-on delivery slices.
