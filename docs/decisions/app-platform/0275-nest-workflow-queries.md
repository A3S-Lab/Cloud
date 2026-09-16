# 0275. Nest-macro Workflow list queries

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Workflow read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Workflow project-scoped catalog/list reads do not require deferred
resource scope. Org-scoped resource reads require deferred project admission.
Tenant admission remains on Nest `#[use_guard(OrganizationTenantGuard)]`.

## Decision

1. Convert workflow-node-catalog, list-definitions, list-goals, list-runs, and
   list-human-tasks GETs to Nest macros
   (`#[controller]` / `#[use_guard]` / `#[get(..., raw)]`).
2. Keep org-scoped get/list-revision/wait/output/variables/diagnostics/history
   attached via `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for five workflow read surfaces. Deferred reads remain
explicit. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_workflow_queries_*` lib tests
