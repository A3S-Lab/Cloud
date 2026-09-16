# 0278. Nest-macro Agent list-conversations query

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Agents read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Agent list-conversations is environment-scoped without deferred
resource scope. Org-scoped conversation/execution/approval reads and SSE
streams require deferred project admission. Tenant admission remains on Nest
`#[use_guard(OrganizationTenantGuard)]`.

## Decision

1. Convert list-conversations GET to Nest macros
   (`#[controller]` / `#[use_guard]` / `#[get(..., raw)]`).
2. Keep org-scoped gets and SSE streams attached via
   `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for agent conversation list. Deferred reads/streams remain
explicit. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_agent_queries_*` lib tests
