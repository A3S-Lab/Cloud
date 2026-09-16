# 0239. Nest-macro application controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Application commands and queries are thin CQRS adapters without
deferred resource scope.

## Decision

Convert application presentation controllers to Nest macros with
`OrganizationTenantGuard` and scoped `auth.scopes` metadata
(`APPLICATION_WRITE` / `CLOUD_READ`). Refuse inventing toolkit/channel
productization and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for application HTTP surfaces.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests

