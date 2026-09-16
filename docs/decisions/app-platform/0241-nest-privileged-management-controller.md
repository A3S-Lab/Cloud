# 0241. Nest-macro privileged management controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Platform RBAC, workload trust, and tenant-support controllers are
CQRS adapters under `/platform` without deferred resource scope.

## Decision

Convert privileged management presentation controllers to Nest macros with
scoped `auth.scopes` metadata (`PLATFORM_WRITE` / `CLOUD_READ`). Refuse
inventing SAML/SCIM/SIEM productization and refuse marking Nest migrations as
full public release.

## Consequences

Adds Nest coverage for privileged management HTTP surfaces.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests

