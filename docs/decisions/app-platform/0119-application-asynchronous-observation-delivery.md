# 0119. Application asynchronous observation delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C47`

## Context

`APP0.2-C46` owns project-authorized CQRS polling for
`ApplicationAsynchronousObservation`. Management operators still need one
REST/OpenAPI, client, CLI, and Management MCP admission surface that reuses
that query without inventing a second wait store, streaming cursor, or public
delivery path.

## Decision

Expose C46 `ObserveApplicationAsynchronousInvocation` through the Applications
management boundary only:

- REST `GET` under session-scoped
  `/invocations/{invocation_id}/asynchronous-observation`
- OpenAPI contract `1.104.0`
- maintained cloud client, CLI, and one `application:write` Management MCP tool
- observation timestamp is server-side at poll time
- no streaming cursor, SSE, Gateway, or public availability

## Consequences

- Presentation adapters call the C46 query only.
- Unauthorized projects, missing sessions/invocations, and Blocking/Streaming modes
  continue to fail closed through C46.
- Public asynchronous delivery and SSE remain later APP0.2 slices.

## Evidence

- Merge commit: `72eb9fe8bc5101e256583fe5dfc2a82119f47ec3`
- Verified commit: `ee707d49f334abe438175a4c1ca1a28878d72515`
- OpenAPI contract version: `1.104.0`
