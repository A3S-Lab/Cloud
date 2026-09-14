# 0109. Project-authorized message citation CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C37`

## Context

`APP0.2-C36` persisted immutable `ApplicationMessageCitation` records. Operators
and later delivery surfaces still need one Applications CQRS owner for create
and session-scoped reads. Knowledge admission ports and REST/MCP remain later
slices.

## Decision

Applications owns project-authorized CQRS for Answer/FinalOutput message
citations:

- `CreateApplicationMessageCitation` loads the exact session and required
  Answer/FinalOutput message, constructs the C35 aggregate, and persists
  through the C36 repository with create/replay semantics
- session-scoped get/list queries authorize the same project grant and fail
  closed for missing sessions or foreign records
- no generation CAS, no Knowledge admission port, no public routes

## Consequences

- Presentation adapters must call these commands/queries rather than inventing
  a second citation write path.
- Closed sessions, missing Answer/FinalOutput sources, and unauthorized
  projects fail closed before persistence.
- Delivery, Gateway, SSE, and availability stay later slices.
