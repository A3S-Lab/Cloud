# 0236. Nest-macro organization-scoped application delivery controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Organization-scoped application delivery commands and queries under
`/organizations` are thin CQRS adapters with `application:write` scope.

## Decision

Convert organization-scoped delivery commands and queries to Nest macros with
`OrganizationTenantGuard` and `auth.scopes` metadata for `APPLICATION_WRITE`.
Refuse inventing toolkit/channel productization and refuse marking Nest
migrations as full public release.

## Consequences

Adds Nest coverage for organization-scoped application delivery HTTP surfaces.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests

