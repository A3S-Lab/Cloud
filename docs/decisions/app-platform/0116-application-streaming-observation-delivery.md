# 0116. Application streaming observation delivery

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C44`

## Context

`APP0.2-C43` owns project-authorized CQRS polling for
`ApplicationStreamingObservation` with an optional `afterSequence` cursor.
Management operators still need one REST/OpenAPI, client, CLI, and Management
MCP admission surface that reuses that query without inventing a second stream
store, SSE route, or public delivery path.

## Decision

Expose C43 `ObserveApplicationStreamingInvocation` through the Applications
management boundary only:

- REST `GET` under session-scoped
  `/invocations/{invocation_id}/streaming-observation`
- optional query `afterSequence` defaulting to `0`
- OpenAPI contract `1.103.0`
- maintained cloud client, CLI, and one `application:write` Management MCP tool
- observation timestamp is server-side at poll time
- no SSE, Gateway, or public availability

## Consequences

- Presentation adapters call the C43 query only.
- Unauthorized projects, missing sessions/invocations, and Blocking modes
  continue to fail closed through C43.
- Public streaming delivery and SSE remain later APP0.2 slices.
## Evidence

- Merge commit: `ea2399a8c66c522aedef1b354720a133e9a88d5b`
- Verified commit: `b862db84519dd0cbd220c531f849af63e63f22a2`
- OpenAPI contract version: `1.103.0`
