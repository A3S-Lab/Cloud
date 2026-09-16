# 0209. Nest-macro routes publish controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Route publish is a thin tenant-scoped edge command without deferred
resource scope.

## Decision

Convert `routes_controller` to Nest macros with `OrganizationTenantGuard`,
`ROUTE_WRITE` scopes metadata, and a `raw` POST handler. Keep the factory for
module registration. Refuse inventing `S0` BYOK and refuse marking Nest
migrations as full production release.

## Consequences

Adds Nest coverage for route publish. Production release remains blocked on
honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
