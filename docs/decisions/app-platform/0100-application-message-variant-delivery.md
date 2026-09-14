# 0100. Management message-variant delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C28`

## Context

`APP0.2-C27` owns project-authorized CQRS for immutable session message variants
("More Like This"). Operators still need one management REST/OpenAPI/client/CLI/MCP
admission surface over those commands without inventing a second write path,
regeneration invocation, or public Gateway route.

## Decision

Applications owns a thin management delivery adapter over C27:

- create message-variant and session-scoped get/list through the existing
  delivery controller family under `application:write`
- exact create identity replays through C26 repositories
- unauthorized projects and closed/missing sessions or source messages fail closed via C27
- no regeneration invocation, anonymous delivery, Gateway, SSE, or availability

## Consequences

- Presentation adapters must call C27 commands/queries only.
- OpenAPI contract version advances with the new routes.
- Identity-issued public delivery stays under `APP0.3`.
