# 0158. Retained/rollback Gateway snapshots retain publication route intent ACL

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C29

## Context

APP0.3-C17/C18 load Applications-owned publication route intents into managed
Gateway desired state and emit declare-only ACL (plus C20 bound rate profiles)
on the ordinary managed compile path. Rollout rollback and retained-snapshot
compile already call `compile_managed_retained_snapshot`, which copies
`desired_state.publication_route_intents()` into MCP reconciliation and
snapshot render ? but contract coverage only asserted the non-retained
`compile_with_application_publication_route_intent_policy` path.

APP0.3-C28 closed durable rate-catalog authority and listed publication
rollback verification as a residual. Inventing new rollback rails, Delivery
rate middleware, or path?channel identity coupling remains refused.

## Decision

Keep ownership on the existing retained compile path:

1. `compile_managed_retained_snapshot` continues to project
   `desired_state.publication_route_intents()` into the compiled ACL (intents
   + bound rate profiles via the same rate-shaping binding admission as
   ordinary managed compile).
2. Add a focused contract test that builds
   `PlannedGatewayNodeDesiredState` with `with_publication_route_intents`,
   compiles through `CompileManagedGatewayRetainedSnapshot`, and asserts
   `application_publication_route_intents` and
   `application_publication_rate_shaping_bound_profiles` are present.
3. Do not invent separate rollback publication machinery; rollback already
   passes full `desired_state` into retained compile
   (`gateway_rollout_rollback_compiler`).

## Explicit non-owners

- New rollback aggregate / PublishRoute-as-channel-owner
- Delivery-local rate-limit middleware
- Inventing path?channel or automatic identity `request_headers` without a
  non-invented intent?route binding
- REST catalog CRUD / Redis distributed limiter certification

## Consequences

- Retained and rollback snapshot publications keep exact-release publication
  ACL when desired state carries intents ? proven by contract, not by a new
  subsystem.
- Remaining residuals: managed rollback compiler path closed by APP0.3-C30 / ADR 0159;
  optional non-invented upstream identity stamping; REST catalog CRUD deferred.

## Evidence

- `gateway_snapshot_compiler.rs` retained path loads
  `publication_route_intents` from desired state
- Contract test
  `retained_snapshot_retains_application_publication_route_intent_acl`
- Focused `cargo test -p a3s-cloud-control-plane --lib
  retained_snapshot_retains_application_publication_route_intent_acl`
- Cloud plan row `APP0.3-C29` (this ADR)
