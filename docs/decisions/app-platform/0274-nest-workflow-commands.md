# 0274. Nest-macro Workflow create commands

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Workflow write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Workflow create-definition, create-goal, and start-run are
project-scoped without deferred resource scope. Revise, cancel, and human-task
mutations require deferred resource admission. Tenant admission and
`WORKFLOW_WRITE` scope remain on Nest `#[use_guard]` / `#[metadata]`.

## Decision

1. Convert create-definition, create-goal, and start-run POSTs to Nest macros
   (`#[controller]` / `#[use_guard]` / `#[metadata]` / `#[post(..., raw)]`).
2. Keep revise/cancel/claim/release/submission attached via
   `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for three workflow write surfaces. Deferred mutations remain
explicit. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_workflow_commands_*` lib tests
