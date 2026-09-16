# 0150. Gateway runtime apply of publication rate-shaping ACL

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C21

## Context

APP0.3-C20 emits two top-level Gateway snapshot ACL blocks after compile
binding:

1. declare-only `application_publication_route_intents`
2. Gateway-bound `application_publication_rate_shaping_bound_profiles`
   (token-bucket or GCRA scalars)

Gateway historically rejected unknown top-level ACL blocks, so managed
snapshots carrying either block could not apply on the node. Distributed-api
§8 requires Gateway-owned rate shaping; Delivery must not grow rate-limit
middleware. Exact-release publication routing and HTTP admission attachment
remain separate residuals (no inventing `PublishRoute` as channel owner).

## Decision

In the Gateway crate (`a3s-gateway`):

1. Parse both Cloud-emitted top-level blocks with fail-closed unknown-field
   rejection (mirror inference ACL discipline).
2. Validate referential integrity: every intent rate-shaping
   `(profile_id, policy_revision_digest)` must match an admitted bound
   profile in the same snapshot. Orphan bound profiles are allowed.
3. Install a reloadable `ApplicationPublicationRateShapingCatalog` on the
   Gateway runtime from bound profiles (token-bucket + GCRA engines),
   preserving limiter state across same digest+algorithm reloads when
   practical.
4. Keep request-path HTTP enforcement and exact-release route attachment as
   explicit residuals until intents bind to an existing router or
   exact-release traffic owner exists.

Cloud work for this gate is documentation and plan tracking only; emission
already lives in APP0.3-C20.

## Explicit non-owners

- Delivery process rate-limit middleware
- REST/OpenAPI catalog CRUD in Cloud
- Inventing publication routers / `PublishRoute` channel ownership
- SSE / browser UI / exact-release traffic steering / rollback rails
- Re-opening C19 empty-intent compile bypasses

## Consequences

- Managed snapshots that include publication intent / bound-profile ACL can
  apply on Gateway instead of failing closed on unknown blocks.
- Bound profiles become live catalog state on the node; HTTP admit remains
  the next first-principles residual after a real route binding exists.
- Production still needs non-empty Cloud catalog registration before intents
  with rate refs compile (C20 empty catalog remains honest fail-closed).

## Evidence

- Gateway: `cargo test --lib -- application_publication` (and related ACL
  parse/validate tests) in `crates/gateway`
- Cloud plan row `APP0.3-C21` (this ADR)
