# 0110. Message citation management delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C38`

## Context

`APP0.2-C37` owns project-authorized CQRS for immutable Answer/FinalOutput
message citations. Management operators still need one REST/OpenAPI, client,
CLI, and Management MCP admission surface that reuses that CQRS owner without
inventing a second write path.

## Decision

Expose C37 create plus session-scoped get/list through the Applications
management boundary only:

- REST under session-scoped `/message-citations`
- OpenAPI contract `1.101.0`
- maintained cloud client, CLI, and three `application:write` Management MCP tools
- no Knowledge admission port, Gateway, SSE, or public availability

## Consequences

- Presentation adapters call C37 commands/queries only.
- Unauthorized projects, closed sessions, and missing Answer/FinalOutput sources
  continue to fail closed before persistence.
- Blocking/streaming parity and Knowledge admission remain later APP0.2 slices.
