# 0105. Project-authorized message file-reference CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C33`

## Context

`APP0.2-C32` persisted immutable `ApplicationMessageFileReference` records.
Operators and later delivery surfaces still need one Applications CQRS owner for
create and session-scoped reads. Files admission ports, citations, and REST/MCP
remain later slices.

## Decision

Applications owns project-authorized CQRS for Input message file references:

- `CreateApplicationMessageFileReference` loads the exact session and required
  Input message, constructs the C31 aggregate, and persists through the C32
  repository with create/replay semantics
- session-scoped get/list queries authorize the same project grant and fail
  closed for missing sessions or foreign records
- no generation CAS, no Files admission port, no citations, no public routes

## Consequences

- Presentation adapters must call these commands/queries rather than inventing
  a second file-reference write path.
- Closed sessions, missing Input sources, and unauthorized projects fail closed
  before persistence.
- Citations, Gateway, SSE, and availability stay later slices.
