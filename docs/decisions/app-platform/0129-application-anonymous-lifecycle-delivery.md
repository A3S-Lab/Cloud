# 0129. Application anonymous session close and invocation cancel delivery

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.2-C56

## Context

APP0.2-C55 owns anonymous-credential CQRS for session close and invocation
cancel. APP0.2-C53/C54 already expose admission and observation on
`/anonymous-delivery` with `AUTH_PUBLIC_METADATA`. Production still needs the
matching public presentation adapters for lifecycle stop without inventing
Gateway/SSE/browser, Issue/Secrets Environment minting, or a second
`application:write` authority.

## Decision

Expose C55 through a public Applications presentation adapter only:

- REST POST
  `/anonymous-delivery/organizations/{organization_id}/projects/{project_id}/applications/{application_id}/sessions/{session_id}/close`
- REST POST
  `.../sessions/{session_id}/invocations/{invocation_id}/cancel`
- `AUTH_PUBLIC_METADATA = true`; no OrganizationTenantGuard / AUTH_SCOPES_METADATA
- request body requires `expectedVersion` and opaque `lookupKey`
- server timestamps (`closed_at` / `requested_at`)
- OpenAPI contract 1.108.0
- maintained cloud client and CLI; optional `application:write` Management MCP
  tools may call the same C55 CQRS with an operator-supplied lookup key
- reuse existing session mutation and invocation cancellation response DTOs

## Exclusions

- IssueApplicationDeliveryCredential / Secrets material / Environment invent
- Gateway / SSE / browser / embed
- Member close/cancel path behavior changes
- Observation delivery changes

## Consequences

- Anonymous lifecycle stop is reachable on the same public prefix as admission.
- MCP/CLI remain thin adapters over C55; opaque lookup keys stay the only
  anonymous authority.
- Identity-issued minting and live Gateway ingress remain under APP0.3.

## Evidence

- Focused `cargo test -p a3s-cloud-control-plane --lib anonymous_delivery`
- Focused `cargo test -p a3s-cloud-control-plane --lib anonymous_lifecycle`
- `A3S_CLOUD_UPDATE_OPENAPI=1 cargo test -p a3s-cloud-control-plane --lib committed_openapi_snapshot`
