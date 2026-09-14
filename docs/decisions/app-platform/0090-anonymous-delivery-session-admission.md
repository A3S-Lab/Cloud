# 0090. Anonymous delivery session admission

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C18`

## Context

`APP0.2-C16` froze Applications-owned anonymous delivery credential bindings.
`APP0.2-C17` persisted those bindings. Project-member session open remains on
`OpenApplicationSession` and refuses anonymous releases. Anonymous and
explicitly linked ApplicationEndUser paths must stay separate delivery policies.

## Decision

`APP0.2-C18` adds one Applications-owned anonymous session-open command that:

1. Loads the credential by opaque Application-scoped lookup key.
2. Admits the stable Principal-free ApplicationEndUser through
   `ApplicationDeliveryCredential::admit_anonymous_end_user` against an exact
   anonymous ApplicationRelease.
3. Opens or idempotently replays ApplicationSession through the existing C6
   session repository without project-member Principal authorization.

Applications retains only the credential reference and generation. It does not
verify Identity-issued secret material. Missing or inactive credentials,
project-member releases, and foreign session identities fail closed.

## Consequences

`APP0.2-C18` remains component-only. Identity issuance, REST/MCP, Gateway, SSE,
and public availability stay deferred to later numbered gates. Anonymous
invocation admission continues in `APP0.2-C19`.
