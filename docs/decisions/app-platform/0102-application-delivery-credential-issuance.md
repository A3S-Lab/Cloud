# 0102. Application delivery credential Identity/Secrets issuance

- Status: Accepted
- Date: 2026-09-14
- Gate: `APP0.2-C30`

## Context

`APP0.2-C16` through `APP0.2-C20` froze, persisted, and lifecycle-managed
anonymous delivery credential bindings that reference an exact Secrets version
without storing plaintext. Callers still had to invent a `SecretVersionReference`
out of band. Inventing a second Applications key store, public routes, or
Principal-bound end-user credentials would reopen Rule 8 and APP0.3 ownership.

## Decision

`APP0.2-C30` adds one project-authorized Applications command that:

1. Authorizes the Project before any mint or write.
2. Replays an existing credential identity without reminting or returning
   plaintext.
3. Otherwise mints opaque lookup-key plus Secrets material through
   `IApplicationDeliveryCredentialMaterialPort` (Identity/Secrets behind the
   port).
4. Registers the binding through the existing C20 register handler.
5. Returns one-time plaintext only on first successful mint.

Anonymous session admission continues to use the opaque lookup key from C18.
Secret verification at admission, Principal-bound application credentials,
REST/OpenAPI/client/CLI/MCP, Gateway, and SSE stay deferred to APP0.3.

## Consequences

- C16-C20 bindings become operable without callers forging Secret references.
- Applications remains free of plaintext persistence and Identity RBAC copies.
- Management delivery of issuance and public Identity-issued end-user credentials
  remain later numbered slices.
