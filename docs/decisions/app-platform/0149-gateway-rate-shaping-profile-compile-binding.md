# 0149. Gateway rate-shaping profile compile binding

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C20

## Context

APP0.3-C10–C19 established declare-only `ApplicationPublicationRouteIntent`
material: Applications owns channels, optional embed origins, and opaque
`(rate_shaping_profile_id, policy_revision_digest)` references. Production
desired-state compile (planner + MCP reconciler) emits those refs into Gateway
snapshot ACL without numeric rate policy.

Distributed-api §8 requires Gateway-owned rate shaping. Binding must happen at
snapshot **compile** against a Gateway catalog so miss/mismatch fails closed
before node apply. Applications must not grow `requests_per_minute` / token-bucket
fields in the intent ACL.

## Decision

At Gateway snapshot compile:

1. Edge/Gateway owns a minimal rate-shaping profile catalog
   (`profile_id` + `policy_revision_digest` + bounded token-bucket or GCRA
   scalars).
2. Production may start with an empty in-memory catalog and **fail closed** until
   profiles are registered (honest).
3. `IApplicationPublicationRateShapingBindingAdmissionPort` mirrors Inference
   Edge binding admission: consumer-facing port, Edge adapter against Gateway
   authority. Applications does not own numeric policy.
4. `GatewaySnapshotCompiler` admits each unique intent rate ref after loading
   intents; on miss/digest mismatch returns
   `RATE_SHAPING_BINDING_INVALID`; on success emits a **separate** bound ACL
   block (`application_publication_rate_shaping_bound_profiles`) after the
   declare-only intents block.
5. Declare-only intent ACL remains unchanged—no numeric rpm / token_bucket in
   the intent block.

## Explicit non-owners

- Delivery process rate-limit middleware / live token-bucket enforcement
- Full Gateway node apply of bound profiles (documented residual; compile emit
  is enough for this gate)
- REST/OpenAPI/client/CLI catalog management
- Edge `PublishRoute` / SSE / browser / `McpRoutePolicy` conflation
- Re-opening empty intent compile bypasses audited by APP0.3-C19

## Consequences

- Snapshot ACL carries both declare-only refs and Gateway-bound scalars.
- Empty intents still compile unchanged (no bound block).
- Next first-principles residual: Gateway runtime apply of bound rate profiles
  on the node (not Delivery middleware) — tracked as APP0.3-C21 / ADR `0150`.
  Exact-release routing / rollback remains a separate APP0.3 track.

## Evidence

- `cargo test -p a3s-cloud-contracts --lib -- gateway_rate_shaping_bound_profile`
- `cargo test -p a3s-cloud-control-plane --lib -- rate_shaping`
- `cargo test -p a3s-cloud-control-plane --lib -- publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib -- edge_gateway_rate_shaping_binding`
