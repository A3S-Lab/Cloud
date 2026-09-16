# 0276. Nest-macro Agent create-conversation command

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Agents write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Agent create-conversation is environment-scoped without deferred
resource scope. Start/cancel/checkpoint/fork/approval mutations require
deferred project admission. Tenant admission and `EXECUTION_WRITE` scope remain
on Nest `#[use_guard]` / `#[metadata]`.

## Decision

1. Convert create-conversation POST to Nest macros
   (`#[controller]` / `#[use_guard]` / `#[metadata]` / `#[post(..., raw)]`).
2. Keep start/cancel/checkpoint/fork/approval attached via
   `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for agent conversation create. Deferred mutations remain
explicit. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_agent_commands_*` lib tests
