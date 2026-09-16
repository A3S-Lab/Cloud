# 0220. Nest-macro resource grant controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Resource-grant list/get/create/revocation are thin
administrator-scoped identity CQRS routes without deferred resource scope.
Dual-guard Nest stacking is already proven (ADR `0198` / `0219`).

## Decision

Convert `resource_grant_controller` to Nest macros with
`OrganizationTenantGuard` + `OrganizationAdministratorGuard`,
`IDENTITY_WRITE` scopes metadata, and `raw` GET/POST handlers. Keep the factory
for module registration. Refuse inventing `S0` BYOK and refuse marking Nest
migrations as full public production release.

## Consequences

Adds Nest coverage for resource-grant CQRS. Production release remains blocked
on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
