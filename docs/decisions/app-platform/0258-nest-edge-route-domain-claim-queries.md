# 0258. Nest-macro Edge route and domain-claim query controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / APP0.6-adjacent Edge HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Edge route publish and domain-claim commands already use Nest macros.
Route list / domain-claim list / gateway-certificate list were still builder
controllers; org-scoped get-by-id still requires deferred project-scope
admission after load.

## Decision

1. Convert route list and domain-claim list + gateway-certificate list to Nest
   macros (`#[controller]` / `#[use_guard]` / `#[get(..., raw)]`).
2. Keep org-scoped `GET .../routes/{route_id}` and
   `GET .../domain-claims/{claim_id}` on
   `.route(with_deferred_resource_scope(..., DeferredResourceScope::Project))`
   so deferred visibility admission is unchanged.
3. Refuse inventing remaining planned gates (`I0.6` / `S0` / `K0.*` / `U0.4` /
   `MCP0.5`) and refuse treating Nest migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for APP0.6-adjacent Edge read surfaces. Deferred gets remain
explicit non-macro routes. Production release remains blocked on honest
remaining foreign/joint gates; `parity_claim` stays false.

## Evidence

- This ADR
- Focused `nest_macro_route_queries_*` and
  `nest_macro_domain_claim_queries_*` lib tests
