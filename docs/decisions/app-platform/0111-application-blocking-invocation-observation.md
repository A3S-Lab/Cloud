# 0111. Application blocking invocation observation

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C39`

## Context

`APP0.2` still requires blocking/streaming protocol parity over the shared
cursor. Callers already admit `ApplicationResponseMode::Blocking`, append
ordered Input/Answer/FinalOutput messages, and observe invocation terminals,
but Applications had no fail-closed projection that turns that existing state
into one blocking-wait observation. Inventing a second run history, timer
worker, SSE stream, or Gateway route would reopen ownership.

## Decision

`APP0.2-C39` freezes `ApplicationBlockingObservation` as an Applications-owned
pure projection over one exact session and Blocking-mode invocation:

- Waiting while the invocation is non-terminal and FinalOutput is absent
- Succeeded only when the invocation is Succeeded with Input and FinalOutput
- Failed/Cancelled only for those terminals without FinalOutput
- Ordered Answer message identities by sequence; Input first; FinalOutput last
- Streaming/asynchronous modes, foreign messages, sequence drift, duplicate
  identities, Answer after FinalOutput, and Succeeded without FinalOutput fail
  closed

## Consequences

`APP0.2-C39` remains component-only. No migration, repository, CQRS poll loop,
streaming cursor advancement, REST/OpenAPI/client/CLI/MCP, Gateway, SSE, or
public availability is added. Later numbered gates may poll this observation
over existing session/invocation reads.
