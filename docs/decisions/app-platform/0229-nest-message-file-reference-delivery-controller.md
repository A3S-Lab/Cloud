# 0229. Nest-macro application message file-reference delivery controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Application message file-reference delivery is thin session-scoped CQRS without
deferred resource scope.

## Decision

Convert the message file-reference command and query factories to Nest macros with tenant guard
and `APPLICATION_WRITE` scope metadata. Refuse inventing toolkit/channel
productization and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for application message file-reference delivery HTTP adapters.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
