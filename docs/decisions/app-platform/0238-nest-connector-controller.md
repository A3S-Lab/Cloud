# 0238. Nest-macro connector controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Connector profile commands and queries are thin CQRS adapters without
deferred resource scope.

## Decision

Convert connector presentation controllers to Nest macros with
`OrganizationTenantGuard` and scoped `auth.scopes` metadata
(`CONNECTOR_WRITE` / `CLOUD_READ`). Refuse inventing toolkit/channel
productization and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for connector HTTP surfaces.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests

