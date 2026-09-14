# 0112. Project-authorized blocking observation CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C40`

## Context

`APP0.2-C39` froze `ApplicationBlockingObservation` as a pure projection over
one Blocking-mode invocation. Callers still needed one Applications CQRS owner
that authorizes the project, loads the exact session/invocation, and polls that
projection without inventing a second run history, wait table, or streaming
cursor.

## Decision

Applications owns project-authorized `ObserveApplicationBlockingInvocation`:

- authorize through the existing project-member session grant
- load the exact invocation on that session
- page existing session messages and keep only frames for that invocation
- project `ApplicationBlockingObservation` at the caller's `observed_at`
- Streaming/asynchronous modes and domain invariant failures remain Invalid
- missing sessions/invocations and unauthorized projects fail closed as not
  found

No migration or new repository is added. Observation is a read over C6
session/invocation/message state.

## Consequences

- Presentation adapters must poll this query rather than reconstructing wait
  status from raw messages.
- REST/OpenAPI/client/CLI/MCP, Gateway, SSE, streaming cursor advancement, and
  public availability stay later numbered gates.
