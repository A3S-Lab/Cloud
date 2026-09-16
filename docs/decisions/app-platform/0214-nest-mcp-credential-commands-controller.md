# 0214. Nest-macro MCP credential commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. MCP credential create/rotate/revoke are thin tenant-scoped edge
commands without deferred resource scope.

## Decision

Convert `mcp_credential_commands_controller` to Nest macros with
`OrganizationTenantGuard`, `MCP_WRITE` scopes metadata, and `raw` POST handlers.
Keep the factory for module registration. Refuse inventing `S0` BYOK and refuse
marking Nest migrations as full public production release.

## Consequences

Adds Nest coverage for MCP credential commands. Production release remains
blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
