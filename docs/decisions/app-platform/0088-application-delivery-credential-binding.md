# 0088. Applications-owned anonymous delivery credential binding

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C16`

## Context

`APP0.2` still requires application-scoped credential and anonymous end-user
admission before `APP0.3` can own Identity-issued credentials plus public
browser/API/embed routes. Project-member delivery (`APP0.2-C8`/`C12`) already
exists. Inventing public routes or Identity issuers without a frozen Applications
binding would mix owners.

## Decision

Applications owns one anonymous delivery credential *binding*:

- opaque Application-scoped `lookup_key`
- exact `SecretVersionReference` (Secrets owns plaintext materialization)
- monotonic generation for disable/enable/revoke
- stable Principal-free `ApplicationEndUser` identity derived from the credential

`APP0.2-C16` is component-only. Persistence is `APP0.2-C17`. Anonymous session admission is `APP0.2-C18`. Identity issuance,
REST/MCP, Gateway, SSE, and availability remain later slices (`APP0.3` and follow-ons).

## Consequences

- Anonymous end users cannot carry a workspace Principal link.
- Inactive or revoked credentials cannot admit sessions.
- Later Identity issuers must target this binding shape rather than inventing a
  second Applications credential authority.
