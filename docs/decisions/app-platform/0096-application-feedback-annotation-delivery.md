# 0096. Management feedback and annotation delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C24`

## Context

`APP0.2-C23` owns project-authorized CQRS for immutable feedback and annotations.
Operators still need one management REST/OpenAPI/client/CLI/MCP admission surface
over those commands without inventing a second write path or public Gateway route.

## Decision

Applications owns a thin management delivery adapter over C23:

- create feedback/annotation and session-scoped get/list through the existing
  delivery controller family under `application:write`
- exact create identity replays through C22 repositories
- unauthorized projects and closed/missing sessions fail closed via C23
- no Annotation Reply matching, anonymous delivery, Gateway, SSE, or availability

## Consequences

- Presentation adapters must call C23 commands/queries only.
- OpenAPI contract version advances with the new routes.
- Identity-issued public delivery stays under `APP0.3`.
