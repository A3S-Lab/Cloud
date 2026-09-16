# 0225. Nest-macro build plan controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Build-plan accept/detect/list/get are thin Developer Workflows adapters
that keep tenant scope composition through root Presentation helpers.

## Decision

Convert build-plan command and query factories to Nest macros for route
definition, then wrap with `organization_tenant_build_write_controller` /
`organization_tenant_cloud_read_controller`. Refuse inventing deferred toolkit
surfaces and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage while preserving Presentation helper composition.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
