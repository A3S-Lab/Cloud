# 0240. Nest-macro knowledge controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Knowledge commands and queries are CQRS adapters without deferred
resource scope; tenant admission and scopes map to
`OrganizationTenantGuard` + `auth.scopes`.

## Decision

Convert knowledge presentation controllers to Nest macros with
`OrganizationTenantGuard` and scoped `auth.scopes` metadata
(`KNOWLEDGE_WRITE` / `CLOUD_READ`). Refuse inventing retrieval productization
and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for knowledge HTTP surfaces.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests

