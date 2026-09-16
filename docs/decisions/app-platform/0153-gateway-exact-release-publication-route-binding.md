# 0153. Gateway exact-release publication route binding table

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C24

## Context

APP0.3-C21 installed a reloadable rate-shaping catalog from bound ACL profiles
and retained intent?profile digests. ADR 0150 left two residuals: HTTP
request-path admit, and exact-release route attachment before admit can bind
to a real traffic owner. Delivery `/delivery` and `/anonymous-delivery` remain
the HTTP traffic owners; inventing paths, PublishRoute as publication-channel
owner, or Delivery rate middleware would be overfitting.

Gateway already receives declare-only `application_publication_route_intents`
with release digest, channels, optional embed origin allowlist, and rate
profile refs. Those intents were not yet indexed as a resolvable exact-release
binding table on the runtime.

## Decision

In the Gateway crate (`a3s-gateway`):

1. Install `ApplicationPublicationRouteBindingTable` from
   `application_publication_route_intents` alongside the rate-shaping catalog.
2. Index bindings by intent id and by
   `(application_release_digest, channel)`, retaining release/channel/origin
   allowlist/rate-shaping refs needed for later admit.
3. Expose `resolve`, `resolve_intent`, and fail-closed `allows_embed_origin`
   (empty allowlist admits no Origin).
4. On duplicate `(digest, channel)`, keep the lexicographically last intent id
   after sorted install (deterministic).
5. Keep HTTP request-path enforcement as the next residual: admit must
   `resolve` then `try_admit` on the bound profile ? not invent routers or
   Delivery middleware in this gate.

Cloud work for this gate is documentation and plan tracking only; intent ACL
emission already lives in APP0.3-C16?C20.

## Explicit non-owners

- HTTP request-path admit / middleware
- Delivery process rate-limit middleware
- Inventing publication routers / `PublishRoute` channel ownership
- SSE / browser UI / rollback rails
- Cloud REST changes / Secrets / Issue coupling
- Physical TEE/SEV

## Consequences

- Exact-release publication intents are attachable on Gateway without claiming
  a new traffic owner.
- HTTP admit can be a thin residual that resolves a binding and consults the
  rate catalog.
- Duplicate release+channel intents are deterministic last-wins; compile-time
  uniqueness remains a possible later tightening, not this gate.

## Evidence

- Gateway: `cargo test --lib -- application_publication` in `crates/gateway`
- Cloud plan row `APP0.3-C24` (this ADR)
