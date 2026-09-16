# 0135. Process role Delivery authenticated observation polls

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C6

## Context

APP0.3-C5 admits Principal-bound session open, invocation, close, and cancel on
`/delivery` with `application:invoke` and exact Application ResourceGrant
fail-closed. Callers still cannot poll Applications-owned observation
projections on the Delivery process without management `application:write`
routes or anonymous lookup keys. Inventing SSE/Gateway would overfit APP0.3
before poll parity exists.

## Decision

- Register Principal-bound `ObserveApplicationBlockingInvocation`,
  `ObserveApplicationStreamingInvocation`, and
  `ObserveApplicationAsynchronousInvocation` handlers on Delivery CQRS.
- Expose three GET polls under `/delivery` via
  `ApplicationAuthenticatedDeliveryModule`, reusing the C5
  `exact_application_access` fail-closed gate and `application:invoke` scope.
- Keep optional `afterSequence` on streaming polls only.
- Leave anonymous `/anonymous-delivery` observation routes unchanged.

## Exclusions

- Gateway / SSE / browser / embed
- OpenAPI / MCP / client / CLI bumps
- Secrets / Issue Environment material minting
- Full `ApplicationsModule` / `IdentityModule::new` on Delivery
- Rate limits, drain, rollback rails

## Consequences

- Authenticated Delivery callers can poll Blocking/Streaming/Asynchronous
  observation the same way management and anonymous surfaces already do,
  without widening the Delivery capability matrix.
- OpenAPI / MCP / client / CLI for authenticated `/delivery` remain excluded
  here; discoverability lands in APP0.3-C7 / ADR `0136`.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib authenticated_delivery`
- `cargo test -p a3s-cloud-control-plane --lib authenticated_observation`
- `cargo test -p a3s-cloud-control-plane --lib delivery_role_exposes`
- `cargo test -p a3s-cloud-control-plane --lib delivery_composition_has_one_closed`
