# 0126. Application anonymous delivery admission

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.2-C53

## Context

APP0.2-C18 and APP0.2-C19 own anonymous session open and invocation request
CQRS over opaque delivery credential_lookup_key. APP0.2-C52 exposes
credential register/lifecycle management so operators can obtain that lookup
key. Production still needs one public presentation surface that admits
anonymous runs over HTTP without inventing APP0.3 Gateway/SSE/browser,
Identity secret verification, Issue/Secrets Environment coupling, or a second
authorization authority based on application:write project membership.

Project-member delivery (APP0.2-C8/C12) already uses Principal-scoped
admission under /organizations/... with AUTH_SCOPES_METADATA. Anonymous
admission must stay on a separate public prefix and authorize only by lookup
key, mirroring Automation signed webhook transport (AUTH_PUBLIC_METADATA).

## Decision

Expose C18 open-session and C19 request-invocation through a public
Applications presentation adapter only:

- REST under /anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions
  and .../sessions/{session_id}/invocations
- AUTH_PUBLIC_METADATA = true; do **not** attach OrganizationTenantGuard or
  AUTH_SCOPES_METADATA
- JSON body carries opaque lookupKey (never treat APPLICATION_WRITE as
  anonymous auth)
- server timestamps (Utc::now()) for opened_at / requested_at
- OpenAPI contract 1.106.0
- maintained cloud client and CLI; optional application:write Management MCP
  tools may call the **same** C18/C19 CQRS commands with an operator-supplied
  lookup key for ops testing (no second authority)
- reuse existing session/invocation mutation response DTOs

## Exclusions

- Anonymous observation delivery (APP0.2-C54)
- IssueApplicationDeliveryCredential / Secrets material / Environment invent
- anonymous close/cancel CQRS (APP0.2-C55 / ADR 0128) and public delivery (APP0.2-C56 / ADR 0129)
- Gateway / SSE / browser / embed

## Consequences

- Presentation adapters call the existing C18/C19 handlers only.
- Missing/inactive credentials and foreign sessions fail closed through CQRS.
- Public Identity-issued credential minting and live Gateway ingress remain
  under APP0.3.

## Evidence

- OpenAPI contract version: 1.106.0
- Focused verification:
  cargo test -p a3s-cloud-control-plane --lib anonymous_delivery
  cargo test -p a3s-cloud-control-plane --lib anonymous_invocation
  cargo test -p a3s-cloud-control-plane --lib committed_openapi_snapshot
