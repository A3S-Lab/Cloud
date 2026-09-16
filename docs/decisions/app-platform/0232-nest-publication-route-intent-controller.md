# 0232. Nest-macro application publication route intent controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Publication route intent create/get/list-by-release is thin CQRS without
deferred resource scope.

## Decision

Convert the publication route intent command and query factories to Nest macros
with tenant guard and write/read scope metadata. Refuse inventing Edge rate
middleware productization here and refuse marking Nest migrations as full public
release.

## Consequences

Adds Nest coverage for application publication route intent HTTP adapters.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
