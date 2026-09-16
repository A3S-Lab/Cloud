# 0215. Nest-macro project and environment command controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Project create/attribution and environment create are thin
tenant-scoped commands without deferred resource scope. Sibling project query
controllers already use Nest macros (ADR `0195`).

## Decision

Convert `projects_controller` and `environments_controller` to Nest macros with
`OrganizationTenantGuard`, `PROJECT_WRITE` / `ENVIRONMENT_WRITE` scopes
metadata, and `raw` POST handlers. Keep factories for module registration.
Refuse inventing `S0` BYOK and refuse marking Nest migrations as full public
production release.

## Consequences

Adds Nest coverage for project/environment commands. Production release remains
blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
