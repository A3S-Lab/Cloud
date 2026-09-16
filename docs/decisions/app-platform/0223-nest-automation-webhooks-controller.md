# 0223. Nest-macro automation webhooks controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Automation webhook transport is a public signed ingress adapter with
no deferred resource scope.

## Decision

Convert the automation webhooks factory to Nest macros with `auth.public`
metadata. Keep Gateway as live public ingress authority. Refuse inventing HA/ops
console surfaces and refuse treating Nest migrations as full public release.

## Consequences

Adds Nest coverage for Automation webhook transport.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
