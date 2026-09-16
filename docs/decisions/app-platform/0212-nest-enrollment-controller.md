# 0212. Nest-macro node enrollment controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Node enrollment is a thin public fleet command.

## Decision

Convert `enrollment_controller` to Nest macros with `auth.public` metadata and a
`raw` POST handler. Keep the factory for module registration. Refuse inventing
`S0` BYOK and refuse marking Nest migrations as full production release.

## Consequences

Adds Nest coverage for node enrollment. Production release remains blocked on
honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
