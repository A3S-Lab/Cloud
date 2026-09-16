# 0226. Nest-macro preview management controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Pull-request preview policy and preview query adapters are thin
Developer Workflows routes that keep tenant scope through root Presentation
helpers.

## Decision

Convert preview-management command and query factories to Nest macros, then wrap
with `organization_tenant_build_write_controller` /
`organization_tenant_cloud_read_controller`. Refuse inventing HA/ops console
productization and refuse marking Nest migrations as full public release.

## Consequences

Completes Nest coverage for remaining Developer Workflows HTTP adapters that
are not deferred-scope.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
