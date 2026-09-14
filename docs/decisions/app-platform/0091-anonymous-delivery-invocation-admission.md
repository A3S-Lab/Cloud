# 0091. Anonymous delivery invocation admission

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C19`

## Context

`APP0.2-C18` admits anonymous ApplicationSession open over persisted delivery
credential bindings. Project-member `RequestApplicationInvocation` still refuses
anonymous releases and requires a linked Principal. Anonymous callers need a
separate invocation path that reuses C6 session persistence and C6 WorkflowRun
composition without Identity secret verification or public routes.

## Decision

`APP0.2-C19` adds one Applications-owned anonymous invocation command that:

1. Loads the credential by opaque Application-scoped lookup key.
2. Authorizes the Principal-free session end user through
   `anonymous_credential_session`.
3. Persists the exact invocation plus Workflow authority using the credential
   issuer Principal as `requested_by`.
4. Composes through the existing C6 WorkflowRun port.

New requests require an Active credential. Exact invocation replay does not
re-require Active so idempotent retries survive a later disable. Missing
credentials, inactive new requests, foreign sessions, and project-member
handlers against anonymous releases fail closed.

## Consequences

`APP0.2-C19` remains component-only. Identity issuance, REST/MCP, Gateway, SSE,
and public availability stay deferred to later numbered gates.
