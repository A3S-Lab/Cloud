# 0099. Project-authorized message-variant CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C27`

## Context

`APP0.2-C26` persisted immutable `ApplicationMessageVariant` records. Operators
and later delivery surfaces still need one Applications CQRS owner for create
and session-scoped reads. Regeneration invocation and REST/MCP remain later
slices.

## Decision

Applications owns project-authorized CQRS for session message variants:

- `CreateApplicationMessageVariant` loads the exact session and required
  Answer/FinalOutput source message, constructs the C25 aggregate, and persists
  through the C26 repository with create/replay semantics
- session-scoped get/list queries authorize the same project grant and fail
  closed for missing sessions or foreign records
- no generation CAS, no regeneration Workflow invocation, no public routes

## Consequences

- Presentation adapters must call these commands/queries rather than inventing
  a second More Like This write path.
- Closed sessions, missing sources, and unauthorized projects fail closed
  before persistence.
- Identity issuance, Gateway, SSE, and availability stay later slices.
