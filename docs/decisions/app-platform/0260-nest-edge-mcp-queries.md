# 0260. Nest-macro Edge MCP credential and route-policy query controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Edge MCP HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. MCP credential and route-policy commands already use Nest macros.
List/get queries were still builder controllers; org-scoped get-by-id still
requires deferred project-scope admission after load.

## Decision

1. Convert MCP credential list and MCP route-policy list to Nest macros
   (`#[controller]` / `#[use_guard]` / `#[get(..., raw)]`). Preserve
   `CLOUD_READ` metadata on route-policy queries.
2. Keep org-scoped `GET .../mcp-credentials/{credential_id}` and
   `GET .../mcp-route-policies/{route_id}` on
   `.route(with_deferred_resource_scope(..., DeferredResourceScope::Project))`.
3. Refuse inventing `MCP0.5` joint closure, remaining planned foreign gates,
   and refuse treating Nest migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for Edge MCP read surfaces. Deferred gets remain explicit
non-macro routes. `MCP0.5` stays planned.

## Evidence

- This ADR
- Focused `nest_macro_mcp_credential_queries_*` and
  `nest_macro_mcp_route_policy_queries_*` lib tests
