# 0183. Nest-macro inference usage retention query controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

ADR `0179` / `0182` established Nest-style attribute macros for Boot
controllers, including scoped GET query surfaces. `usage_retention_controller`
is a thin authenticated GET with organization tenant/admin guards and fits the
Nest `#[use_guard]` / `#[get(..., raw)]` composition without inventing behavior.

## Decision

1. **Convert** `usage_retention_controller` to Nest macros with
   `#[use_guard(OrganizationTenantGuard)]`,
   `#[use_guard(OrganizationAdministratorGuard)]`, scopes metadata, and a `raw`
   BootResponse handler.
2. **Keep** the factory `usage_retention_controller(bus)` for module registration
   and claim-path tests.
3. **Refuse** mass-rewriting guarded command controllers in this slice.

## Consequences

- Adds a Nest precedent for controller-level guards on management query routes.
- Does not declare production release complete; `S0` BYOK remains refused.

## Evidence

- This ADR; `usage_retention_controller.rs`
- Tests: `cargo test -p a3s-cloud-control-plane --lib nest_macro_usage_retention_controller`
