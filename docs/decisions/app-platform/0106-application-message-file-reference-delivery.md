# 0106. Message file-reference management delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C34`

## Context

`APP0.2-C33` owns project-authorized CQRS for immutable Input message file
references. Management operators still need one REST/OpenAPI, client, CLI, and
Management MCP admission surface that reuses that CQRS owner without inventing a
second write path.

## Decision

Expose C33 create plus session-scoped get/list through the Applications
management boundary only:

- REST under session-scoped `/message-file-references`
- OpenAPI contract `1.100.0`
- maintained cloud client, CLI, and three `application:write` Management MCP tools
- no Files admission port, citations, Gateway, SSE, or public availability

## Consequences

- Presentation adapters call C33 commands/queries only.
- Invalid digests, unauthorized projects, closed sessions, and missing Input
  sources continue to fail closed before persistence.
- Citations and blocking/streaming parity remain later APP0.2 slices.
