# 0159. Managed rollout rollback publications retain publication route intent ACL

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C30

## Context

APP0.3-C29 proved `compile_managed_retained_snapshot` emits publication route
intent ACL when desired state carries intents. Production rollback ownership
lives in `GatewayRolloutRollbackCompiler::compile_managed`, which already
passes each member's full `desired_state` into retained compile. Contract
coverage still stopped at the snapshot compiler helper.

## Decision

1. Keep rollback ownership unchanged: managed rollback continues to call
   `compile_managed_retained_snapshot` with member desired state (including
   publication intents).
2. Add a focused rollback-compiler contract test that injects
   `with_publication_route_intents` on each member desired state and asserts
   every rollback `GatewayPublication` ACL retains intent + bound rate-profile
   blocks.
3. Wire the test compiler with the same in-memory rate catalog admission used
   by ordinary managed compile (fail-closed without inventing a second owner).

## Explicit non-owners

- New rollback publication subsystem / PublishRoute-as-channel-owner
- Delivery-local rate-limit middleware
- Inventing path?channel or automatic identity `request_headers`
- REST catalog CRUD / Redis distributed limiter certification

## Consequences

- Rollout rollback publications keep exact-release publication ACL when
  desired state carries intents ? proven through the production rollback
  compiler path, not only the retained helper.
- Remaining residuals audited in APP0.3-C31 / ADR `0160`: optional non-invented
  upstream identity stamping remains Blocked; REST catalog CRUD deferred;
  epic production release unproven.

## Evidence

- `gateway_rollout_rollback_compiler_tests::managed_rollback_retains_application_publication_route_intent_acl`
- Focused `cargo test -p a3s-cloud-control-plane --lib
  managed_rollback_retains_application_publication_route_intent_acl`
- Cloud plan row `APP0.3-C30` (this ADR)
