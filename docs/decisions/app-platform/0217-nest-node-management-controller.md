# 0217. Nest-macro fleet node management commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Enrollment-token issue and node ready/drain/revoke actions are thin
tenant-scoped fleet commands without deferred resource scope. Sibling node
query controller already uses Nest macros (ADR `0191`).

## Decision

Convert `node_management_controller` to Nest macros with
`OrganizationTenantGuard`, `NODE_WRITE` scopes metadata, and `raw` POST
handlers. Keep `heartbeat_timeout` as a controller field for availability
projection. Keep the factory for module registration. Refuse inventing `S0`
BYOK and refuse marking Nest migrations as full public production release.

## Consequences

Adds Nest coverage for fleet node management commands. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
