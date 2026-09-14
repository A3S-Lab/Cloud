# 0089. Persist Applications-owned anonymous delivery credentials

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C17`

## Context

`APP0.2-C16` froze the Applications-owned anonymous delivery credential binding
in memory. Identity issuance and public delivery still require a durable owner
before `APP0.3` can issue material or admit routes. Inventing a second credential
table outside Applications would split authority.

## Decision

Applications owns persistence for anonymous delivery credential bindings:

- migration `204` table `application_delivery_credentials`
- one A3S ORM repository plus an in-memory adapter
- Application-scoped unique `lookup_key`
- generation-fenced CAS for disable/enable/revoke
- exact `SecretVersionReference` foreign key (no plaintext)

`APP0.2-C17` remains component-only. Identity issuance, REST/MCP, Gateway, SSE,
and availability stay later slices.

## Consequences

- Later Identity issuers must load this Applications binding rather than inventing
  a parallel credential authority.
- Lookup-key collisions and stale generations fail closed at the repository.
- Public surfaces still must not appear until `APP0.3` and follow-on delivery slices.
