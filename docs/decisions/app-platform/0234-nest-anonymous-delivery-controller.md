# 0234. Nest-macro anonymous application delivery controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Anonymous delivery is a public admission surface without deferred
resource scope.

## Decision

Convert anonymous delivery commands to Nest macros with `auth.public` metadata.
Refuse inventing toolkit/channel productization and refuse marking Nest migrations
as full public release.

## Consequences

Adds Nest coverage for anonymous application delivery HTTP admission.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
