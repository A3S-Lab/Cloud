# 0118. Project-authorized asynchronous observation CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C46`

## Context

`APP0.2-C45` froze `ApplicationAsynchronousObservation` as a pure projection over
one Asynchronous-mode invocation. Callers still needed one Applications CQRS owner
that authorizes the project, loads the exact session/invocation, and polls that
projection without inventing a second run history, timer worker, or delivery surface.

## Decision

Applications owns project-authorized `ObserveApplicationAsynchronousInvocation`:

- authorize through the existing project-member session grant
- load the exact invocation on that session
- page existing session messages and keep only frames for that invocation
- project `ApplicationAsynchronousObservation` at the caller's `observed_at`
- Blocking/Streaming modes and domain invariant failures remain Invalid
- missing sessions/invocations and unauthorized projects fail closed as not
  found

No migration or new repository is added. Observation is a read over C6
session/invocation/message state.

## Consequences

- Presentation adapters must poll this query rather than reconstructing wait
  status from raw messages.
- REST/OpenAPI/client/CLI/MCP, Gateway, SSE, streaming cursor advancement, and
  public availability stay later numbered gates.

## Evidence

- Merge commit: 3787e1f2e077544ca1087894b0271bab0c7adc67
- Verified commit: 407cd96cd20ab567938d52d511efd61478a53985
- Focused test: cargo test -p a3s-cloud-control-plane --lib asynchronous_observation (9 passed)
