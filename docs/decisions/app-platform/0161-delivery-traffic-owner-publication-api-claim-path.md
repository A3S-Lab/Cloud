# 0161. Delivery traffic-owner claim path for publication API channels

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C32

## Context

APP0.3-C31 / ADR `0160` left production release blocked on a non-invented
upstream identity stamp: Cloud `Edge::Route` has no `request_headers`, and
`PublishRoute` carries workload/path only. Inventing path->channel mapping or
automatic CP `request_headers` emission was refused.

ADR `0153` already names Delivery `/delivery` and `/anonymous-delivery` as the
HTTP traffic owners. Authenticated Delivery already exposes session/invocation
lifecycle plus blocking, streaming, and asynchronous observation polls under
`/delivery` (APP0.3-C5/C6). Gateway publication admit remains a *claimed-traffic*
rate/origin gate keyed by identity headers, not a second traffic owner.

ADR `0160` production-release rule clause 2 accepts claim-path evidence as
**traffic owner or Gateway ACL stamps + Delivery**. Clause 3 allows accepting
operator/client-stamped Gateway claim without inventing a binding.

## Decision

1. **Production claim path for `publication.api-blocking` and
   `publication.api-streaming`:** the Delivery process traffic owner. Clients
   invoke published Application APIs on `/delivery` (and anonymous
   `/anonymous-delivery` where applicable) with exact-release session authority.
   No PublishRoute-as-channel-owner and no invented path->channel.

2. **Gateway identity headers remain optional for the Gateway hop:** when traffic
   crosses Gateway with `x-a3s-application-release-digest` +
   `x-a3s-publication-channel` (operator ACL `headers.request_headers` or
   client stamps), C24-C27 admit/rate shaping applies. Absence of those stamps
   does not revoke Delivery traffic-owner availability.

3. **Parity availability:** mark `publication.api-blocking` and
   `publication.api-streaming` as `internal` under gate `APP0.3` with evidence from
   Delivery ADRs/tests plus this ADR. Keep `publication.embed` and
   `publication.web` `unavailable` (embed Origin/browser and generated web remain
   first-principles deferred). Keep inventing automatic identity stamps refused.

4. **Focused conformance:** add a Cloud lib test that freezes the claim-path
   contract: `ApplicationPublicationChannel::ApiBlocking` /
   `ApiStreaming` string forms match Gateway admit channel tokens, and Delivery
   authenticated observation exposes `blocking-observation` and
   `streaming-observation` under `/delivery`.

## Consequences

- Closes ADR `0160` blocked identity-stamp row for API channels without inventing
  Edge Route fields or PublishRoute channel ownership.
- Epic `APP0.3` remains open until embed/web (or accepted deferral) and any
  remaining production-release clauses are proven; API channel availability is
  no longer gated on invented upstream stamps.
- REST Gateway rate-catalog CRUD, Redis limiter, and SSE/browser stay deferred.

## Evidence

- This ADR; plan row `APP0.3-C32`; architecture + parity updates
- Prior: ADR `0131`-`0138` Delivery role/HTTP/client; ADR `0153`-`0156` Gateway
  claimed admit; ADR `0160` audit rule
- Focused: `cargo test -p a3s-cloud-control-plane --lib publication_api_channel_claim_path`
