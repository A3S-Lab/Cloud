# 0146. Application publication route intent desired-state load

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C17

## Context

APP0.3-C16 froze declare-only Gateway snapshot ACL admission for
`ApplicationPublicationRouteIntent`, but production composition still used the
empty Applications ACL projection port and managed compile sites often passed
`&[]`. Certificate / route / retained compile paths already accept
`PlannedGatewayNodeDesiredState`; the missing slice is loading real
projections through the existing Edge ACA and feeding them into those compile
paths.

## Decision

- Replace the empty Applications ACL projection port in root composition with
  repository-backed `ApplicationPublicationRouteIntentAclProjectionAdapter`
  (`list_intents_by_project` → C15 Edge projection → C16 ACL projection).
- Wire that port through the existing
  `ApplicationsEdgeManagedPublicationRouteIntentAccessAdapter` into
  `GatewayNodeDesiredStatePlanner::with_publication_route_intent_access`.
- Planner `plan()` builds org/project scopes from Gateway scopes, loads intents
  via Edge managed access, and attaches them on
  `PlannedGatewayNodeDesiredState`.
- Managed compile (`compile_mcp_reconciliation`, retained, certificate
  convergence, and managed route snapshot) reads
  `desired_state.publication_route_intents()` before `into_parts()` and passes
  them into snapshot ACL render. Still declare-only; no `PublishRoute`.

## Remaining empty call sites (honest)

Closed by APP0.3-C18 / ADR 0147:

- `mcp_gateway_desired_state_reconciler` direct `CompileMcpGatewaySnapshot`
  construction (no longer hard-codes `publication_route_intents: Vec::new()`)

These still compile with empty publication route intent ACL and are out of
this slice:

- Non-managed `GatewaySnapshotCompiler::compile*` helpers that pass `&[]`
- Test fixtures that intentionally omit intents

Production paths that already plan through `GatewayNodeDesiredStatePlanner`
(certificate reconciler, deployment route updater, rollout rollback) inherit
loaded intents when the planner is composed with the repository-backed ACA.

## Exclusions

- Edge `PublishRoute` admission / node install / apply
- Delivery process rate-limit middleware / token bucket
- REST/OpenAPI/client/CLI/MCP changes
- CQRS emit of projections
- SSE / browser UI
- Wiring MCP desired-state reconciler off the planner intent vector
  (completed in APP0.3-C18)

## Consequences

- Gateway snapshots for managed desired-state compile can carry Applications
  publication route intent ACL when intents exist for the Gateway scope
  org/project.
- Empty repositories still compile (no ACL block), preserving C16 empty-shell
  behavior.
- **Correction:** the next residual is **not** `PublishRoute` as channel owner.
  Applications owns channels on publication route intent; see ADR 0147.
  Prefer closing remaining empty compile sites or Gateway rate-profile apply
  binding—refuse Delivery middleware and SSE as the next default.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib -- publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib -- planner_loads_publication_route_intents`
- `cargo test -p a3s-cloud-control-plane --lib -- edge_managed_publication_route_intent_acl`
- `cargo test -p a3s-cloud-control-plane --lib -- gateway_snapshot_compiler_contract_tests`
