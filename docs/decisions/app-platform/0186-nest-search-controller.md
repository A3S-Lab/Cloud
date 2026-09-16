# 0186. Nest-macro organization search query controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

`search_controller` is a thin organization-scoped GET with query parameters and
tenant guard. It fits Nest macros without inventing behavior.

## Decision

Convert `search_controller` to Nest macros with `#[use_guard(OrganizationTenantGuard)]`
and a `raw` BootResponse handler. Keep the factory. Refuse `S0` invention and
false public release claims.

## Consequences

Adds Search Nest query precedent. Production release remains blocked on honest
remaining gates (notably unavailable `S0` BYOK).

## Evidence

- This ADR; `search_controller.rs`
- Tests: `cargo test -p a3s-cloud-control-plane --lib nest_macro_search_controller`
