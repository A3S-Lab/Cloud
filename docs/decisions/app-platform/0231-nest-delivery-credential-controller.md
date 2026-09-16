# 0231. Nest-macro application delivery credential controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Application delivery credential register/lifecycle/query is thin CQRS
without deferred resource scope.

## Decision

Convert the delivery credential command and query factories to Nest macros with
tenant guard and `APPLICATION_WRITE` scope metadata. Refuse inventing secrets CMS
productization and refuse marking Nest migrations as full public release.

## Consequences

Adds Nest coverage for application delivery credential HTTP adapters.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
