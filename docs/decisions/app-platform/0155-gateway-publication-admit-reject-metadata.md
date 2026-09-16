# 0155. Gateway publication admit reject response metadata

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C26

## Context

APP0.3-C25 enforces HTTP publication admit on claimed traffic and returns bare
JSON error bodies for rejects, including `429` when the bound rate profile is
exhausted. Distributed-api ?8 expects clients to receive a bounded retry hint
and stable rate-policy identity on shaping rejects. Existing Gateway local
rate-limit middleware already emits `Retry-After: 1`. Stamping those headers
from Delivery would invent a Delivery?Gateway coupling that is not the request
hop owning admit; that path remains a later residual for identity headers only.

## Decision

In the Gateway crate (`a3s-gateway`):

1. Extend `PublicationAdmitDecision::Rejected` with optional
   `retry_after_secs`, `rate_shaping_profile_id`, and
   `rate_shaping_policy_revision_digest`.
2. On rate exhaustion (`429`), set `retry_after_secs = Some(1)` and copy the
   resolved binding's profile id and policy revision digest.
3. On embed-origin rejects (`403`) after a binding resolve, attach the same
   profile/revision identity (no Retry-After).
4. In `handle_http_request`, map those fields onto response headers:
   - `retry-after` (when present)
   - `x-a3s-publication-rate-profile`
   - `x-a3s-publication-rate-policy-revision`
5. Keep status/message contracts from C25 unchanged; unclaimed traffic remains
   `NotApplicable`.

Cloud work for this gate is documentation and plan tracking only.

## Explicit non-owners

- Delivery process rate-limit middleware or Delivery-owned Retry-After inventing
- Changing admit status codes or inventing publication URL paths
- Durable Redis distributed limiter certification / cross-node Retry-After
- Delivery/edge identity header stamping (separate residual)
- SSE/browser UI / rollback rails / REST catalog CRUD

## Consequences

- Claimed publication `429` responses carry the same bounded Retry-After hint
  pattern as Gateway's local rate-limit path, plus stable profile/revision
  identity for client diagnostics and policy correlation.
- Next residual (now APP0.3-C27 / ADR 0156): apply Edge `request_headers`
  before admit so desired-state identity can claim traffic; then durable
  catalog and rollback.

## Evidence

- Gateway: `cargo test --lib -- application_publication` in `crates/gateway`
- Cloud plan row `APP0.3-C26` (this ADR)
