# 0235. Nest-macro authenticated application delivery controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Authenticated delivery is a scoped admission surface under `/delivery`
with `application:invoke` and exact Application ResourceGrant checks.

## Decision

Convert authenticated delivery commands to Nest macros with
`OrganizationTenantGuard` and `auth.scopes` metadata for `APPLICATION_INVOKE`.
Refuse inventing toolkit/channel productization and refuse marking Nest
migrations as full public release.

## Consequences

Adds Nest coverage for authenticated application delivery HTTP admission.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests

