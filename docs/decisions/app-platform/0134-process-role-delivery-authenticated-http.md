# 0134. Process role Delivery authenticated published-application HTTP

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C5

## Context

APP0.3-C3 serves anonymous Delivery HTTP without global auth.
APP0.3-C4 admits Identity `application:invoke` minting plus Rule 8 grant
evidence. Authenticated published-application callers still need an HTTP
admission surface on `ProcessRole::Delivery` that consumes invoke scope and an
exact Application ResourceGrant without opening management composition.

## Decision

- Wire closed Identity **read** verification on Delivery:
  `AuthModule` + `ApiTokenVerifier(api_tokens, resource_grants)` +
  `.use_global_auth()`.
- Register `ApplicationAuthenticatedDeliveryModule` under `/delivery` with
  `AUTH_SCOPES_METADATA = [application:invoke]`, `OrganizationTenantGuard`, and
  controller fail-closed unless `exact_application_is_authorized`.
- Reuse APP0.2 admit CQRS (`AdmitApplicationSession`,
  `AdmitApplicationInvocation`, close/cancel) with
  `published_application` ACL so exact Application grants can open Principal-
  bound sessions without requiring Project grant.
- Fix tenant `resource_scope` so `application_id` maps to
  `ResourceGrantScope::Application`.
- Keep anonymous `/anonymous-delivery` public metadata routes unchanged.

## Exclusions

- Gateway / SSE / browser / embed
- Secrets / Issue Environment material minting
- Full `ApplicationsModule` / `IdentityModule::new` on Delivery
- OpenAPI / MCP / client / CLI bumps
- Switching management C8 routes off `application:write`

## Consequences

- Delivery can authenticate Principal-bound invoke tokens for exact Application
  grants while remaining a closed process role.
- Project-only grants fail closed on `/delivery` even though CQRS
  `published_application` accepts Project authority for management reuse.
- APP0.3-C6 adds authenticated observation polls on the same `/delivery`
  surface (ADR `0135`) without Gateway/SSE.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib delivery_role_exposes`
- `cargo test -p a3s-cloud-control-plane --lib delivery_composition_has_one_closed`
- `cargo test -p a3s-cloud-control-plane --lib authenticated_delivery`
- `cargo test -p a3s-cloud-control-plane --lib published_application`
