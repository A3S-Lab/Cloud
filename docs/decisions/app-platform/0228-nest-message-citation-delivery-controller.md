# 0228. Nest-macro application message citation delivery controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Application message citation delivery is thin session-scoped CQRS without
deferred resource scope.

## Decision

Convert the message citation command and query factories to Nest macros with tenant guard
and `APPLICATION_WRITE` scope metadata. Refuse inventing toolkit/channel
productization and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for application message citation delivery HTTP adapters.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
