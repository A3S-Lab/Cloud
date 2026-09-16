# 0184. Nest-macro gateway scope query controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

ADR `0179` / `0182` / `0183` established Nest-style attribute macros for Boot
controllers, including guarded GET query surfaces. `gateway_scope_queries_controller`
is a thin Edge list query with `OrganizationTenantGuard` and fits Nest
`#[use_guard]` / `#[get(..., raw)]` without inventing behavior.

## Decision

1. **Convert** `gateway_scope_queries_controller` to Nest macros with
   `#[use_guard(OrganizationTenantGuard)]` and a `raw` BootResponse handler.
2. **Keep** the factory `gateway_scope_queries_controller(bus)` for module
   registration compatibility.
3. **Refuse** inventing `S0` BYOK / residency / air-gap; refuse marking Nest
   migrations as full production release.

## Consequences

- Adds a Nest precedent on Edge gateway-scope list queries.
- Does not declare production release complete.

## Evidence

- This ADR; `gateway_scope_queries_controller.rs`
- Tests: `cargo test -p a3s-cloud-control-plane --lib nest_macro_gateway_scope_queries_controller`
