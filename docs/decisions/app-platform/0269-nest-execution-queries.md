# 0269. Nest-macro Executions query controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Executions read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Template/list and environment list are project-scoped without deferred
resource scope; org-scoped execution get requires deferred project admission.

## Decision

1. Convert list-templates, get-template, and list-executions GETs to Nest macros
   (`#[controller("/organizations")]` / `#[use_guard(OrganizationTenantGuard)]`
   / `#[get(..., raw)]`).
2. Keep get-execution attached via `.route(with_deferred_resource_scope(...))`
   after `.controller()`.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for execution reads. Deferred org-scoped get remains
explicit. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_execution_queries_*` lib tests
