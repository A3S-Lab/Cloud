# 0221. Nest-macro membership invitation controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Membership invitation administration, self-list, and acceptance are
thin identity CQRS routes without deferred resource scope.

## Decision

Convert the three membership-invitation factories to Nest macros:
administrator dual-guard org-scoped routes, principal self-list under
`CLOUD_READ`, and acceptance under `IDENTITY_WRITE`. Keep factories for module
registration. Refuse inventing `S0` BYOK and refuse marking Nest migrations as
full public production release.

## Consequences

Adds Nest coverage for membership invitations. Production release remains
blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
