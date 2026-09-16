# 0147. MCP Gateway publication route intent ACL load

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C18

## Context

APP0.3-C17 wired repository-backed publication route intent ACL into
`GatewayNodeDesiredStatePlanner` and planner-backed managed compile paths, but
documented a production split-brain: `mcp_gateway_desired_state_reconciler`
still constructed `CompileMcpGatewaySnapshot` with
`publication_route_intents: Vec::new()`. MCP desired-state reconciliation is a
primary staging path and must load the same Edge ACA projections.

ADR 0146 Consequences incorrectly suggested the next residual should
"admit `PublishRoute` from declared intents." That is wrong for channel
ownership: Applications owns channels on `ApplicationPublicationRouteIntent`;
`PublishRoute` is not the owner for channels and must not become the next
default slice from this lineage.

## Decision

- Inject
  `IEdgeManagedApplicationPublicationRouteIntentAccess` into
  `McpGatewayDesiredStateReconciler` via
  `with_publication_route_intent_access` (same Edge ACA as C17).
- Before `compile_mcp_reconciliation`, build org/project intent scopes from the
  node-wide Gateway reconciliation scope set, call `list_for_scopes`, and pass
  the loaded projections into `CompileMcpGatewaySnapshot`.
- Absent access or an empty repository still compiles (no publication ACL
  block), preserving C16 empty-shell behavior.
- Root composition attaches the existing
  `ApplicationsEdgeManagedPublicationRouteIntentAccessAdapter` to the MCP
  reconciler beside the planner.

## Explicit non-owners

- **Channels** remain Applications-owned declare-only facts on publication route
  intent projections. Closing this split-brain does **not** make Edge
  `PublishRoute` / `PublishRouteHandler` / durable-cell route publication the
  channel authority.
- Delivery process rate-limit middleware and live token-bucket enforcement stay
  later (distributed-api §8), after Gateway can bind rate-shaping profile refs.

## Remaining empty call sites (honest)

Still out of this slice:

- Non-managed `GatewaySnapshotCompiler::compile*` helpers that pass `&[]`
- Test fixtures that intentionally omit intents

## Exclusions

- Edge `PublishRoute` admission / node install / apply
- Delivery process rate-limit middleware / token bucket
- REST/OpenAPI/client/CLI/MCP changes
- SSE / browser UI / exact-release traffic steering / rollback rails
- Optional hygiene for non-managed `compile*` helpers (closed as audit-only in
  APP0.3-C19 / ADR 0148: intentionally low-level; no invented wiring)

## Consequences

- MCP Gateway desired-state reconciliation stages snapshots whose ACL can carry
  Applications publication route intent blocks when intents exist for Gateway
  scope org/project.
- Planner-backed and reconciler-backed production compile paths now share one
  ACA load contract.
- APP0.3-C19 / ADR 0148 audited remaining empty compile sites: no production
  hard-coded empties remain; non-managed `compile*` `&[]` sites stay intentional
  low-level API. Next substantive residual: Gateway rate-shaping profile
  **apply/binding** from declared intent digests—refuse Delivery middleware and
  SSE as the next owner.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib -- publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib -- mcp_desired_state_loads_publication_route_intents`
- `cargo test -p a3s-cloud-control-plane --lib -- mcp_desired_state_compiles_with_empty_publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib -- edge_managed_publication_route_intent_acl`
