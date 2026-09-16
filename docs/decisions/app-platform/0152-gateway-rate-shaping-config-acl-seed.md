# 0152. Cloud Edge Gateway rate-shaping catalog config ACL seed

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C23

## Context

APP0.3-C22 wired one shared process-local in-memory Gateway rate-shaping catalog
into both production compile sites and added CQRS `RegisterGatewayRateShapingProfile`.
Processes still booted empty, so api-worker and management could diverge unless
operators registered the same profiles in each process. Durable Postgres catalog
ownership is a larger cut; operators already express Edge runtime facts in Cloud
ACL. Delivery must not grow rate-limit middleware. REST catalog CRUD remains
deferred.

## Decision

1. Admit optional nested `edge.rate_shaping_profile "<id>"` blocks in Cloud ACL
   with closed fields: `policy_revision_digest` plus exactly one of
   `token_bucket { capacity, refill_tokens_per_second }` or
   `gcra { emission_interval_nanos, burst_tolerance }`.
2. Parse seeds into validated `GatewayRateShapingProfile` values on
   `EdgeConfig.rate_shaping_profiles` (unique profile ids, fail closed).
3. At boot, seed the shared `InMemoryGatewayRateShapingProfileCatalog` from
   config in both production compile compositions (api-worker deployment
   compiler and management PublishRoute compiler) before admission adapters
   attach. Empty seed list remains honest fail-closed.
4. Keep CQRS register as an in-process overlay/replace; do not invent REST CRUD
   or durable store in this gate.

## Explicit non-owners

- Durable / Postgres-backed catalog authority
- REST/OpenAPI/client/CLI catalog management
- Delivery process rate-limit middleware
- Gateway HTTP request-path admit
- Exact-release publication routing / rollback / inventing `PublishRoute` as
  channel owner
- Shipping default profiles in production ACL (operators opt in)

## Consequences

- Api-worker and management boot from the same Edge ACL seed list and compile
  against the same process-local catalog contents without cross-process CQRS.
- Absence of seed blocks preserves empty fail-closed behavior.
- Next first-principles residuals stay: Gateway HTTP admit after a real route
  binding; exact-release routing/rollback; optional durable catalog later.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib -- edge_rate_shaping_profile_acl`
- `cargo test -p a3s-cloud-control-plane --lib -- rate_shaping`
- `cargo test -p a3s-cloud-control-plane --lib -- register_gateway_rate_shaping`
