# 0145. Application publication route intent Gateway snapshot compile

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C16

## Context

APP0.3-C15 freezes `ApplicationPublicationRouteIntentEdgeProjection` as the
Applications-owned Edge desired-state shape. Gateway snapshot compile still
needs a Published Language ACL admission path that mirrors managed Inference
ACL: contract renderer, Applications projection port, one Edge ACA adapter, and
`GatewaySnapshotCompiler` append after the inference block. Full desired-state
planner loading can wait; compile-time injection of projections is enough to
freeze the ACL vocabulary without inventing PublishRoute or Delivery rate
middleware.

## Decision

- Add contracts `ApplicationPublicationRouteIntentAclProjection` and
  `render_application_publication_route_intent_acl_blocks` (declare-only rate
  profile id + digest; empty input renders nothing).
- Add Applications `IApplicationPublicationRouteIntentAclProjectionPort` with an
  empty production port until reconciler wiring loads C15 projections.
- Add Edge `IEdgeManagedApplicationPublicationRouteIntentAccess` and one ACA
  `ApplicationsEdgeManagedPublicationRouteIntentAccessAdapter`. Edge must not
  import Applications domain aggregates.
- Extend `GatewaySnapshotCompiler` with
  `compile_with_application_publication_route_intent_policy` and append the ACL
  block after inference policy. `PlannedGatewayNodeDesiredState` carries an
  optional intents vector defaulting empty from the planner.
- Production composition constructs the ACA once with the empty Applications
  port; snapshot call sites pass `&[]` until a later slice loads projections.

## Exclusions

- Edge `PublishRoute` admission / node install / apply
- Delivery process rate-limit middleware / token bucket
- REST/OpenAPI/client/CLI/MCP changes
- CQRS emit of projections
- SSE / browser UI
- Full reconciler projection load (deferred; empty `Vec` is honest until C17)

## Consequences

- Later APP0.3 slices can wire reconciler loading of C15→ACL projections into
  desired state without reopening ACL vocabulary.
- Rate shaping remains declare-only until Gateway enforces the versioned profile
  per `docs/distributed-api-consistency-architecture.md` section 8.

## Evidence

- `cargo test -p a3s-cloud-contracts --lib application_publication_route_intent_acl`
- `cargo test -p a3s-cloud-control-plane --lib -- publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib -- gateway_snapshot_compiler_contract_tests`
- `cargo test -p a3s-cloud-control-plane --lib -- edge_managed_publication_route_intent_acl`
