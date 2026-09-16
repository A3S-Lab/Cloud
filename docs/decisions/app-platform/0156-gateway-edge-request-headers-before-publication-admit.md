# 0156. Gateway applies Edge `request_headers` before publication admit

- Status: Accepted
- Date: 2026-03-22
- Gate: APP0.3-C27

## Context

APP0.3-C25/C26 enforce HTTP publication admit on claimed traffic using identity
headers `x-a3s-application-release-digest` and `x-a3s-publication-channel`.
Cloud Edge desired-state already expresses those identity stamps as ordinary
route `headers.request_headers`. Gateway's `headers` middleware applied them
only inside the later request pipeline, *after* publication admit, so Edge
desired-state stamps could not activate admit without inventing a second
Delivery-owned rate or identity path.

## Decision

In the Gateway crate (`a3s-gateway`):

1. Add `Middleware::apply_request_header_stamps` (default no-op) and
   `HeadersMiddleware` / `Pipeline` implementations that insert configured
   `request_headers` without running the full request pipeline.
2. In `handle_http_request`, after the matched `RoutePlan` is resolved and
   *before* `evaluate_http_admit`, call
   `route_plan.pipeline.apply_request_header_stamps(req.headers_mut())`.
3. Keep the full middleware `process_request` path unchanged (headers may be
   re-inserted idempotently later).
4. Do not invent Delivery rate middleware, automatic path?channel mapping,
   PublishRoute-as-channel-owner, or SSE/browser identity coupling.

Cloud work for this gate is documentation and plan tracking only. Edge continues
to own desired-state `request_headers`; Gateway applies them early enough for
admit.

## Explicit non-owners

- Delivery-local rate-limit middleware or Delivery-owned Retry-After inventing
- Automatic inventing of release digest / channel from URL path or PublishRoute
- Durable Postgres rate catalog / Redis distributed limiter certification
- Rollback rails / REST catalog CRUD / SSE-first browser overfitting

## Consequences

- Edge-stamped publication identity participates in admit on the Gateway hop
  that owns rate shaping, without a second Delivery rate authority.
- Remaining residuals after C27: optional Delivery/CP *upstream* identity
  stamping only when a non-invented binding from published traffic to intent
  exists; durable catalog (now APP0.3-C28 / ADR 0157); rollback.

## Evidence

- Gateway: `cargo test --lib -- application_publication` in `crates/gateway`
  (includes `edge_request_headers_stamp_activates_admit`)
- Cloud plan row `APP0.3-C27` (this ADR)
