# 0137. Delivery process drain and readiness admission gate

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C8

## Context

`ProcessRole::Delivery` already serves anonymous `/anonymous-delivery` and
authenticated `/delivery` session, invocation, lifecycle, and observation HTTP
with process-status health. Operators need a process-local drain that refuses
**new** admission while keeping observation, close, and cancel available so
in-flight work can finish. Fleet `NodeDrain` and any rate-limit rails are a
different concern and must not be reused here.

## Decision

- Introduce `DeliveryProcessDrain` as an `Arc<AtomicBool>` latch with
  `begin()`, `is_draining()`, and `refuse_new_admission()` returning
  `BootError::ServiceUnavailable("delivery process is draining")`.
- Wire **one** shared latch into `build_application_delivery_http`:
  - Readiness indicator `delivery-drain` is DOWN while draining, UP otherwise.
  - Liveness `/health/live` stays process-only and unaffected.
  - Module `before_application_shutdown` calls `begin()` on the shared latch.
  - Session-open and invocation POSTs on both authenticated and anonymous
    surfaces call `refuse_new_admission()` before CQRS.
  - Observation GET, close, and cancel continue while draining.
- Management `ApplicationsModule` mounts the same controllers with a **default
  non-draining** latch so OpenAPI/routes stay available without product drain
  semantics.
- Do not widen the Delivery capability matrix; do not import management modules
  into Delivery; do not invent rate limiters.

## Exclusions

- Gateway / SSE / browser / embed
- Secrets / Issue Environment material minting
- Rate limits, rollback rails, Fleet `NodeDrain`
- MCP / client / CLI bumps in this slice

## Consequences

- Load balancers can stop routing new Delivery traffic via readiness while
  existing sessions drain through observation/close/cancel.
- Management composition remains non-draining by default.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib delivery_readiness_reports_down_while_draining`
- `cargo test -p a3s-cloud-control-plane --lib delivery_liveness_stays_up_while_draining`
- `cargo test -p a3s-cloud-control-plane --lib delivery_refuses_new_`
- `cargo test -p a3s-cloud-control-plane --lib delivery_allows_observation_close_and_cancel_while_draining`
- `cargo test -p a3s-cloud-control-plane --lib delivery_composition_registers_one_drain_readiness_indicator`
