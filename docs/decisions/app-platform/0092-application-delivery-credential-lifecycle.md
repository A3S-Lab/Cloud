# 0092. Application delivery credential lifecycle CQRS

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C20`

## Context

`APP0.2-C16` froze the Applications-owned anonymous delivery credential binding.
`APP0.2-C17` persisted it. `APP0.2-C18`/`C19` admit session and invocation over
Active bindings loaded by opaque lookup key. Tests still constructed bindings by
calling domain `issue` plus the repository directly. Project-authorized CQRS
write commands were missing, so production callers had no Applications-owned
register/disable/enable/revoke path that stayed within Rule 8 (reference and
generation only; no second API-key kind; no Identity secret material).

## Decision

`APP0.2-C20` adds Applications CQRS commands that:

1. Register one binding against an exact anonymous release (opaque lookup key +
   exact Secrets version reference + issuer Principal).
2. Generation-CAS disable, enable, and revoke that binding.
3. Authorize through the existing project `ApplicationAccess` projection.
4. Replay exact register identity without rewriting Active generation `1`.

Admission consumers remain unchanged: new anonymous session/invocation work
still requires Active; Identity secret minting/verification, REST/MCP, Gateway,
SSE, and public availability stay deferred.

## Consequences

`APP0.2-C20` remains component-only. Identity-issued Principal-bound application
credentials and public delivery routes continue under `APP0.3`. Preset
authoring-profile publication and remaining message/toolkit surfaces stay on
later numbered APP0.2 rows.
