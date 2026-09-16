# 0132. Delivery process public anonymous delivery HTTP

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C3

## Context

APP0.3-C2 admitted `ProcessRole::Delivery` with a closed capability matrix but
booted only process-status routes. Published anonymous `/anonymous-delivery`
CQRS already exists on management (`ApplicationPublicDeliveryModule`). Delivery
must register that surface without importing management modules, Gateway, SSE,
browser/embed, or Principal-bound credential minting.

## Decision

- Early-return `build_delivery_application` for `ProcessRole::Delivery`,
  parallel to Relay: postgres only, no Flow/event ownership, no workers.
- Compose `DeliveryPostgresAdapters` for Applications sessions/credentials plus
  Workflow run ports required by anonymous invocation request/cancel.
- Register exactly the anonymous admission, observation, and close/cancel CQRS
  handlers plus `ApplicationPublicDeliveryModule`, health, and platform.
- Management All/Api continue composing the same public controllers through
  `ApplicationsModule`; Delivery does not import `ApplicationsModule`.

## Exclusions

- Authenticated Principal-bound delivery
- Gateway / SSE / browser / embed
- Identity Issue/Secrets credential minting
- Rate limits, drain, rollback, routing recovery
- OpenAPI/MCP/client/CLI surface changes (routes already shipped under APP0.2)

## Consequences

- Operators can run a delivery-only process that exposes anonymous published
  application protocol without management CRUD.
- APP0.3-C4 admits Identity `application:invoke` minting (ADR `0133`); APP0.3-C5 registers authenticated Delivery HTTP (ADR `0134`) without widening anonymous composition.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib delivery_role_exposes_anonymous`
- `cargo test -p a3s-cloud-control-plane --lib delivery_composition_has_one_closed`
- `cargo test -p a3s-cloud-control-plane --lib public_delivery_module`
- `cargo test -p a3s-cloud-control-plane --lib process_roles_have_one_closed_capability_matrix`
