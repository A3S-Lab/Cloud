# 0233. Nest-macro automation management controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Automation webhook lifecycle commands and management queries are thin
CQRS without deferred resource scope.

## Decision

Convert the automation management factories to Nest macros and keep Presentation
helper wraps (`organization_tenant_automation_write_controller` /
`organization_tenant_cloud_read_controller`) so the module does not import
Identity presentation guards directly. Refuse inventing automation ops console
productization and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for automation management HTTP adapters.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
