# 0262. Nest-macro Sources GitHub connections controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Sources GitHub connection HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Sources GitHub connection begin/get/discovery routes already use
`OrganizationTenantGuard` and `SOURCE_WRITE` scopes on the builder path, with
an OAuth no-store error filter.

## Decision

1. Convert GitHub connection POST/GET and discovery GET routes to Nest macros
   (`#[controller("/organizations")]` / `#[use_guard(OrganizationTenantGuard)]`
   / `#[metadata("auth.scopes", …)]` / `#[post|get(..., raw)]`).
2. Keep `OAuthNoStoreErrorFilter` attached after `.controller()` because filter
   attachment remains wiring-owned.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for Sources GitHub connection HTTP. OAuth no-store remains
explicit at wiring. Production release remains blocked on honest remaining
gates.

## Evidence

- This ADR
- Focused `nest_macro_github_connections_*` lib tests
