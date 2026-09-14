# 0113. Application blocking observation delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C41`

## Context

`APP0.2-C40` owns project-authorized CQRS polling for
`ApplicationBlockingObservation`. Management operators still need one
REST/OpenAPI, client, CLI, and Management MCP admission surface that reuses
that query without inventing a second wait store, streaming cursor, or public
delivery path.

## Decision

Expose C40 `ObserveApplicationBlockingInvocation` through the Applications
management boundary only:

- REST `GET` under session-scoped
  `/invocations/{invocation_id}/blocking-observation`
- OpenAPI contract `1.102.0`
- maintained cloud client, CLI, and one `application:write` Management MCP tool
- observation timestamp is server-side at poll time
- no streaming cursor, SSE, Gateway, or public availability

## Consequences

- Presentation adapters call the C40 query only.
- Unauthorized projects, missing sessions/invocations, and Streaming modes
  continue to fail closed through C40.
- Streaming observation parity remains a later APP0.2 slice.
