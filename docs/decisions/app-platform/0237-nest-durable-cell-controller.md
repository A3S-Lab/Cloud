# 0237. Nest-macro durable cell controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Durable cell commands, route publish, and queries are thin CQRS
adapters without deferred resource scope.

## Decision

Convert durable cell presentation controllers to Nest macros with
`OrganizationTenantGuard` and scoped `auth.scopes` metadata
(`WORKLOAD_WRITE` / `ROUTE_WRITE` / `CLOUD_READ`). Refuse inventing HA/ops
consoles and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for durable cell HTTP surfaces.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests

