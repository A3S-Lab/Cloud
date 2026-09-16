# 0122. Anonymous-credential streaming observation CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C49`

## Context

`APP0.2-C42`/`APP0.2-C43` already own Streaming observation for project-member
sessions. Anonymous delivery still needed one Applications CQRS poll that
authorizes by opaque delivery credential, loads the exact anonymous
session/invocation, and projects `ApplicationStreamingObservation` with cursor
paging without inventing SSE, Gateway, or a second stream history.

## Decision

Applications owns anonymous-credential
`ObserveAnonymousApplicationStreamingInvocation`:

- load the delivery credential by opaque lookup key
- authorize the session through the credential-derived anonymous end-user id
- load the exact invocation on that session
- page existing session messages and keep only frames for that invocation
- project `ApplicationStreamingObservation` at the caller's `after_sequence` and
  `observed_at`
- Blocking/asynchronous modes and domain invariant failures remain Invalid
- missing credentials/sessions/invocations and foreign credentials fail closed
  as not found
- already-admitted sessions remain observable after credential disable so
  mid-stream polls do not break; foreign credentials still fail closed

No migration or new repository is added. Observation is a read over C6
session/invocation/message state plus C16/C17 credential bindings.

## Consequences

- Anonymous delivery adapters must poll this query rather than reconstructing
  stream frames from raw messages.
- REST/OpenAPI/client/CLI/MCP, Gateway, SSE, streaming delivery, and public
  availability stay later numbered gates.
