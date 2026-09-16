# 0136. Authenticated `/delivery` OpenAPI contract

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C7

## Context

APP0.3-C5/C6 expose Principal-bound `/delivery` session, invocation, lifecycle,
and observation routes on `ProcessRole::Delivery`. The committed management
OpenAPI snapshot still documented only `/anonymous-delivery` for public
admission. Delivery intentionally 404s `/api/v1/openapi.json`, so callers and
clients need the routes in the management-owned contract without widening the
Delivery process capability matrix.

## Decision

- Mount `ApplicationAuthenticatedDeliveryModule` controllers into
  `ApplicationsModule` the same way `ApplicationPublicDeliveryModule` already
  shares anonymous routes for OpenAPI generation and management composition.
- Document `/delivery` summaries distinctly from anonymous and management
  surfaces; reuse existing session/invocation/observation request schemas
  (no `lookupKey`).
- Bump `OPENAPI_CONTRACT_VERSION` to `1.110.0` and regenerate `openapi/v1.json`.
- Keep Delivery process openapi 404 and closed Delivery composition unchanged.

## Exclusions

- Gateway / SSE / browser / embed
- Secrets / Issue Environment material minting
- Rate limits, drain, rollback rails
- MCP / client / CLI bumps in this slice (follow existing OpenAPI-first pattern;
  clients may lag one gate)

## Consequences

- Authenticated Delivery HTTP is discoverable in the committed OpenAPI
  contract without registering OpenAPI on the Delivery process itself.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib committed_openapi_snapshot_matches_the_resolved_route_contract`
- `cargo test -p a3s-cloud-control-plane --lib authenticated_delivery`
- `cargo test -p a3s-cloud-control-plane --lib delivery_role_exposes`
