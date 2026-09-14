# 0114. Application streaming invocation observation

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C42`

## Context

`APP0.2` still requires blocking/streaming protocol parity over the shared
cursor. Callers already admit `ApplicationResponseMode::Streaming`, append
ordered Input/Answer/FinalOutput messages, and observe invocation terminals,
but Applications had no fail-closed projection that turns that existing state
into one cursor-paged streaming observation. Inventing a second run history,
timer worker, SSE stream, or Gateway route would reopen ownership.

## Decision

`APP0.2-C42` freezes `ApplicationStreamingObservation` as an Applications-owned
pure projection over one exact session and Streaming-mode invocation:

- Open while the invocation is non-terminal and FinalOutput is absent
- Succeeded only when the invocation is Succeeded with Input and FinalOutput
- Failed/Cancelled only for those terminals without FinalOutput
- Cursor pages expose frames with `sequence > after_sequence`, sorted by sequence
- `next_sequence` tracks the page tail; `has_more` when later frames remain
- Blocking/asynchronous modes, foreign messages, sequence drift, duplicate
  identities, cursor beyond session head, Answer after FinalOutput, and
  Succeeded without FinalOutput fail closed

## Consequences

`APP0.2-C42` remains component-only. No migration, repository, CQRS poll loop,
REST/OpenAPI/client/CLI/MCP, Gateway, SSE, or public availability is added.
Later numbered gates may poll this observation over existing session/invocation
reads.
