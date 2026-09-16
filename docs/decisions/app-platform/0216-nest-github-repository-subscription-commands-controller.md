# 0216. Nest-macro GitHub repository subscription commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. GitHub repository subscription create/deactivate are thin
tenant-scoped source commands without deferred resource scope. Sibling query
controller already uses Nest macros (ADR `0187`).

## Decision

Convert `github_repository_subscriptions_controller` to Nest macros with
`OrganizationTenantGuard`, `SOURCE_WRITE` scopes metadata, and `raw` POST
handlers. Keep the factory for module registration. Refuse inventing `S0` BYOK
and refuse marking Nest migrations as full public production release.

## Consequences

Adds Nest coverage for GitHub subscription commands. Production release remains
blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
