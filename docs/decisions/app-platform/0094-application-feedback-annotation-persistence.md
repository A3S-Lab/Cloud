# 0094. Persist Applications-owned feedback and annotations

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C22`

## Context

`APP0.2-C21` froze immutable `ApplicationFeedback` and `ApplicationAnnotation`
aggregates in memory. Delivery, CQRS, and Annotation Reply still require a durable
Applications owner. Inventing a second feedback/annotation table outside
Applications would split authority. Feedback and annotations must not rewrite
ordered `ApplicationMessage` sequences or bump session `aggregate_version`.

## Decision

Applications owns persistence for immutable session feedback and annotations:

- migration `205` tables `application_feedbacks` and `application_annotations`
- one A3S ORM repository plus an in-memory adapter for each aggregate
- create/replay by primary key (`IdempotentWrite`) without generation CAS
- immutability enforced with `reject_application_session_child_mutation`
- exact session/release/end-user/source-message foreign keys

`APP0.2-C22` remains component-only. CQRS, Annotation Reply matching, REST/MCP,
Gateway, SSE, and availability stay later slices.

## Consequences

- Later CQRS and delivery surfaces must load these Applications records rather
  than inventing parallel feedback/annotation authorities.
- Replay collisions and foreign-key misses fail closed at the repository.
- Public surfaces still must not appear until follow-on delivery slices.
