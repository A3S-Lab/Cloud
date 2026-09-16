# 0224. Nest-macro GitHub webhooks controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. GitHub webhook acceptance is a public signed ingress adapter.

## Decision

Convert the GitHub webhooks factory to Nest macros with `auth.public` metadata.
Keep verifier injection on the Nest controller struct. Refuse OAuth/secrets
productization and refuse treating Nest migrations as full public release.

## Consequences

Adds Nest coverage for GitHub webhook transport.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
