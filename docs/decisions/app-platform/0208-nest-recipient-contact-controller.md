# 0208. Nest-macro recipient contact controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Recipient-contact list/get and begin/complete/revoke are thin
tenant-scoped identity handlers.

## Decision

Convert query and command factories to Nest macros with
`OrganizationTenantGuard`, scopes metadata (`CLOUD_READ` / `IDENTITY_WRITE`),
and `raw` handlers. Keep factories for module registration. Refuse inventing
`S0` BYOK and refuse marking Nest migrations as full production release.

## Consequences

Adds Nest recipient-contact surface. Production release remains blocked on
honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
