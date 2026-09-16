# 0210. Nest-macro MCP route policy commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. MCP route policy create/revise are thin tenant-scoped edge commands
without deferred resource scope.

## Decision

Convert `mcp_route_policy_commands_controller` to Nest macros with
`OrganizationTenantGuard`, `MCP_WRITE` scopes metadata, and `raw` POST handlers.
Keep the factory for module registration. Refuse inventing `S0` BYOK and refuse
marking Nest migrations as full production release.

## Consequences

Adds Nest coverage for MCP route policy commands. Production release remains
blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
