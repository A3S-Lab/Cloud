# 0115. Project-authorized streaming observation CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C43`

## Context

`APP0.2-C42` froze `ApplicationStreamingObservation` as a pure projection over
one Streaming-mode invocation with cursor-based paging. Callers still needed one
Applications CQRS owner that authorizes the project, loads the exact
session/invocation, and polls that projection without inventing a second run
history, SSE stream, or Gateway route.

## Decision

Applications owns project-authorized `ObserveApplicationStreamingInvocation`:

- authorize through the existing project-member session grant
- load the exact invocation on that session
- page existing session messages and keep only frames for that invocation
- project `ApplicationStreamingObservation` at the caller's `after_sequence` and
  `observed_at`
- Blocking/asynchronous modes and domain invariant failures remain Invalid
- missing sessions/invocations and unauthorized projects fail closed as not
  found

No migration or new repository is added. Observation is a read over C6
session/invocation/message state.

## Consequences

- Presentation adapters must poll this query rather than reconstructing stream
  status from raw messages.
- REST/OpenAPI/client/CLI/MCP, Gateway, SSE, streaming delivery, and public
  availability stay later numbered gates.
