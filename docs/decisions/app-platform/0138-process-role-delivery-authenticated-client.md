# 0138. Authenticated Delivery client and CLI

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C9

## Context

APP0.3-C7 documented Principal-bound authenticated `/delivery` HTTP in OpenAPI
`1.110.0`. APP0.3-C8 added process drain. Operators and SDKs still lacked a
maintained client and CLI that call `/delivery/...` with bearer Identity
(`application:invoke` + exact Application grant) and without anonymous
`lookupKey`. Management MCP already exposes project-member application session
tools under `application:write`; wiring MCP to `/delivery` would invent a second
admission path or duplicate those tools without mirroring the anonymous
lookupKey pattern cleanly.

## Decision

- Add `CloudApi` methods that POST/GET the existing `/delivery/...` routes only:
  session open, invocation request, session close, invocation cancel, and the
  three observation polls (blocking / streaming / asynchronous).
- Reuse member input validators and shapes (`OpenApplicationSessionInput`,
  `RequestApplicationInvocationInput`, `ApplicationExpectedVersionInput`); do
  not accept `lookupKey`.
- Add CLI commands `application-delivery-sessions`,
  `application-delivery-invocations`, and
  `application-delivery-*-observation` that require Idempotency-Key /
  expected-version the same way member commands do.
- Skip Management MCP for this gate.

## Exclusions

- Gateway / SSE / browser / embed
- Secrets / Issue Environment material minting
- Rate limits, Fleet NodeDrain
- New HTTP routes or a second CQRS admission authority
- Management MCP tools for `/delivery`

## Consequences

- Clients and CLI can exercise Principal-bound Delivery using the same OpenAPI
  surface operators already run against HTTP.
- Anonymous delivery remains lookupKey-based; member management paths remain
  under `/organizations/.../applications/...`.

## Evidence

- `bun test` in `packages/cloud-client` filtered to Principal-bound authenticated `/delivery`
- `bun test` in `cli` filtered to authenticated `/delivery without lookupKey`
