# 0268. Nest-macro Workflow ontology list query

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Workflow ontology read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Ontology list is project-scoped without deferred resource scope;
org-scoped ontology reads require deferred project admission.

## Decision

1. Convert ontology list GET to Nest macros
   (`#[controller("/organizations")]` / `#[use_guard(OrganizationTenantGuard)]`
   / `#[get(..., raw)]`).
2. Keep get/revisions/diff attached via
   `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for ontology list. Deferred org-scoped reads remain
explicit. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_ontology_queries_*` lib tests
