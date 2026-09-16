# 0203. Nest-macro workload profile controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Workload profile accept/list/get revision handlers are thin
tenant-scoped surfaces.

## Decision

Convert command and query factories to Nest macros with
`OrganizationTenantGuard`, scopes metadata (`BUILD_WRITE` / `CLOUD_READ`), and
`raw` handlers. Keep factories for module registration. Refuse inventing `S0`
BYOK and refuse marking Nest migrations as full production release.

## Consequences

Adds Nest developer-workflow workload-profile surface. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
