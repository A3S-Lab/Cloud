# 0211. Nest-macro source revisions controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. External source revision resolve is a thin tenant-scoped sources
command without deferred resource scope.

## Decision

Convert `source_revisions_controller` to Nest macros with
`OrganizationTenantGuard`, `SOURCE_WRITE` scopes metadata, and a `raw` POST
handler. Keep the factory for module registration. Refuse inventing `S0` BYOK
and refuse marking Nest migrations as full production release.

## Consequences

Adds Nest coverage for source revision resolve. Production release remains
blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
