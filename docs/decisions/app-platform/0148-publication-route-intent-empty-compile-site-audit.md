# 0148. Publication route intent empty compile-site audit

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C19

## Context

APP0.3-C17 and APP0.3-C18 wired repository-backed Edge ACA load into the two
production desired-state compile owners:

- `GatewayNodeDesiredStatePlanner` (planner-backed managed Gateway compile)
- `McpGatewayDesiredStateReconciler` (MCP desired-state reconciliation compile)

ADR 0147 deferred optional hygiene for non-managed `GatewaySnapshotCompiler::compile*`
helpers that still pass `&[]` for publication route intents. First principles
require verifying whether any **production** reconciler/composition path still
hard-codes empty intents before inventing further wiring.

## Decision

**Audit result: no remaining production empty sites after C18.**

Grep/evidence (control-plane, excluding `*_tests*` / fixtures):

- Production `mcp_gateway_desired_state_reconciler` loads via
  `list_for_scopes` / `load_publication_route_intents` and does **not** contain
  `publication_route_intents: Vec::new()`.
- Production managed compile reads `desired_state.publication_route_intents()`.
- Root composition attaches `with_publication_route_intent_access` to both
  planner and MCP reconciler.
- The only production `publication_route_intents: Vec::new()` in the planner is
  the `PlannedGatewayNodeDesiredState` default before
  `with_publication_route_intents` attaches the loaded vector—not a compile bypass.

**Intentionally left as low-level API (caller supplies intents):**

- `GatewaySnapshotCompiler::compile`
- `compile_with_inference_credentials`
- `compile_with_inference_policy`
- `compile_with_inference_policy_and_workers`
- Certificate-convergence / reuse helpers that omit publication intents

These helpers pass `&[]` for the publication-route-intent ACL slot by design.
Callers that need declare-only publication ACL must use
`compile_with_application_publication_route_intent_policy` (or managed desired-state
paths that already load through the Edge ACA).

**Test fixtures** may still construct `CompileMcpGatewaySnapshot` with
`publication_route_intents: Vec::new()` to assert empty-shell behavior.

No production code change is required for this gate. Architecture tests lock the
audit invariants; docs record the closed residual.

## Explicit non-owners

- Edge `PublishRoute` / `PublishRouteHandler` is **not** the channel owner.
- Delivery process rate-limit middleware / live token-bucket enforcement stays
  later (distributed-api §8).
- Gateway rate-shaping profile **apply/binding** from declared digests is the
  next substantive residual—not Delivery middleware.

## Exclusions

- Edge `PublishRoute` admission / node install / apply
- Delivery process rate-limit middleware / token bucket
- REST/OpenAPI/client/CLI/MCP changes
- SSE / browser UI / exact-release traffic steering / rollback rails
- Inventing wiring for low-level `compile*` helpers that correctly omit intents

## Consequences

- Declare-only ACL coverage for production desired-state compile is consistent
  across planner-backed and MCP-reconciler paths.
- Remaining `&[]` sites are documented as low-level compile API, not bypasses.
- APP0.3-C20 / ADR 0149 delivered Gateway rate-profile **compile binding**.
  Next residual: Gateway **node apply** of bound rate profiles (not Delivery
  middleware, PublishRoute-as-channel-owner, or SSE).

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib -- publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib -- edge_managed_publication_route_intent_acl`
- Architecture invariants under `edge_managed_publication_route_intent_acl`
