# 0154. Gateway HTTP publication admit (resolve + rate catalog)

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C25

## Context

APP0.3-C24 installed a resolvable exact-release
`ApplicationPublicationRouteBindingTable` beside the rate-shaping catalog.
Distributed-api ?8 assigns burst/rate shaping to Gateway. Delivery remains the
`/delivery` and `/anonymous-delivery` traffic owner; inventing publication
paths, PublishRoute channel ownership, or Delivery rate middleware would be
overfitting. There was still no request-path enforcement connecting resolve to
`try_admit`.

Publication traffic must declare exact-release identity without inventing a new
router vocabulary. The binding table already keys on
`(application_release_digest, channel)`.

## Decision

In the Gateway crate (`a3s-gateway`):

1. Treat HTTP requests as claimed publication traffic only when both identity
   headers are present:
   - `x-a3s-application-release-digest`
   - `x-a3s-publication-channel`
2. Incomplete claims (exactly one header) fail closed with `400`.
3. Claimed traffic `resolve`s the binding table, enforces embed `Origin`
   allowlist when applicable, then `try_admit`s the bound rate profile.
4. Wire evaluation early in `handle_http_request` after route match and before
   the direct-HTTP fast path so admit cannot be skipped.
5. Absent both headers remains `NotApplicable` ? ordinary routed traffic is
   unchanged.

Failure contract for claimed traffic:

| Condition | Status |
| --- | --- |
| Bindings or catalog missing | `503` |
| No binding for digest+channel | `404` |
| Embed origin denied/required | `403` |
| Rate exhausted | `429` |

Cloud work for this gate is documentation and plan tracking only. Delivery
stamping of identity headers and rollback rails remain later residuals.

## Explicit non-owners

- Delivery process rate-limit middleware
- Inventing publication URL paths / `PublishRoute` channel ownership
- SSE/browser UI / rollback rails
- Durable Redis distributed limiter certification
- Cloud REST/OpenAPI changes

## Consequences

- Exact-release publication rate shaping is enforceable on Gateway when traffic
  owners stamp the identity headers.
- Unclaimed traffic does not pay publication admit cost or false rejects.
- Next residuals: Delivery/edge identity stamping, durable catalog, rollback.
  Retry-After / rate-policy reject metadata delivered in APP0.3-C26 / ADR `0155`.

## Evidence

- Gateway: `cargo test --lib -- application_publication` in `crates/gateway`
- Cloud plan row `APP0.3-C25` (this ADR)
