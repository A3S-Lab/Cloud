# 0222. Nest-macro security investigation controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Security investigation timeline is a thin query adapter that must keep
administrator authorization composed through root Presentation helpers rather
than importing Identity presentation guards.

## Decision

Convert the security investigation timeline factory to Nest macros for route
definition, then wrap with `organization_administrator_read_controller` for
tenant/admin guards and `CLOUD_READ` scope metadata. Refuse inventing SIEM/PII
CMS productization and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage while preserving the Security/Identity presentation boundary.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
