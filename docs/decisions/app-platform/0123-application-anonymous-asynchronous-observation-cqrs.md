# 0123. Anonymous-credential asynchronous observation CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C50`

## Context

`APP0.2-C45`/`APP0.2-C46` already own Asynchronous observation for
project-member sessions. Anonymous delivery still needed one Applications CQRS
poll that authorizes by opaque delivery credential, loads the exact anonymous
session/invocation, and projects `ApplicationAsynchronousObservation` without
inventing a second wait table, streaming cursor, or delivery surface.

## Decision

Applications owns anonymous-credential
`ObserveAnonymousApplicationAsynchronousInvocation`:

- load the delivery credential by opaque lookup key
- authorize the session through the credential-derived anonymous end-user id
- load the exact invocation on that session
- page existing session messages and keep only frames for that invocation
- project `ApplicationAsynchronousObservation` at the caller's `observed_at`
- Blocking/Streaming modes and domain invariant failures remain Invalid
- missing credentials/sessions/invocations and foreign credentials fail closed
  as not found
- already-admitted sessions remain observable after credential disable so
  mid-wait polls do not break; foreign credentials still fail closed

No migration or new repository is added. Observation is a read over C6
session/invocation/message state plus C16/C17 credential bindings.

## Consequences

- Anonymous delivery adapters must poll this query rather than reconstructing
  wait status from raw messages.
- REST/OpenAPI/client/CLI/MCP, Gateway, SSE, and public availability stay later
  numbered gates.
