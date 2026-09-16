# 0265. Nest-macro Workflow ontology create command

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Workflow ontology write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Ontology create is org-tenant scoped without deferred resource scope;
revise requires deferred project scope on the builder route path.

## Decision

1. Convert ontology create POST to Nest macros
   (`#[controller("/organizations")]` / `#[use_guard(OrganizationTenantGuard)]`
   / `#[metadata("auth.scopes", …)]` / `#[post(..., raw)]`).
2. Keep revise attached via `.route(with_deferred_resource_scope(...))` after
   `.controller()` because deferred resource scope is not a Nest attribute.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for ontology create. Deferred revise remains explicit.
Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_ontology_commands_*` lib tests
