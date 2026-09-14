# 0095. Project-authorized feedback and annotation CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C23`

## Context

`APP0.2-C22` persisted immutable `ApplicationFeedback` and `ApplicationAnnotation`
records. Operators and later delivery surfaces still need one Applications CQRS
owner for create and session-scoped reads. REST/MCP and Annotation Reply remain
later slices.

## Decision

Applications owns project-authorized CQRS for feedback and annotations:

- `CreateApplicationFeedback` / `CreateApplicationAnnotation` load the exact
  session (and optional source message), construct the C21 aggregate, and
  persist through the C22 repositories with create/replay semantics
- session-scoped get/list queries authorize the same project grant and fail
  closed for missing sessions or foreign records
- no generation CAS, no Annotation Reply matching, no public routes

## Consequences

- Presentation adapters must call these commands/queries rather than inventing
  a second feedback/annotation write path.
- Closed or foreign sessions fail closed before persistence.
- Identity issuance, Gateway, SSE, and availability stay later slices.
