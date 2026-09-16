# 0162. APP0.3 production release for API channels with embed/web deferral

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C33

## Context

ADR `0160` production-release rule still left epic `APP0.3` open after C32:

1. parity gate `APP0.3` evidence must list Proven rows,
2. claimed publication capabilities need non-invented claim-path,
3. identity stamp closed via Delivery traffic-owner (ADR `0161`) with
   optional Gateway operator/client stamps.

`publication.api-blocking` and `publication.api-streaming` are already
`internal` with Delivery claim-path evidence. `publication.embed` and
`publication.web` remain without honest product owners: embed needs Origin /
browser surface work; generated web is a separate authoring/delivery product.
Inventing either would overfit and violate first principles.

SSE/browser UI, REST Gateway rate-catalog CRUD, and Redis limiter remain
explicit non-owners from ADR `0160`.

## Decision

1. **APP0.3 production scope is API-channel internal availability**, not full
   publication-channel public parity. Production release for APP0.3 means
   Delivery-owned blocking/streaming API channels are `internal` with Proven
   C1-C32 foundation and claim-path ADR `0161`.

2. **Explicit first-principles deferral:** keep `publication.embed` and
   `publication.web` `unavailable` under gate `APP0.3`. Do not invent embed
   Origin allowlist product UI, generated web apps, or browser-first SSE as
   the close-out of APP0.3. Later gates may own those surfaces.

3. **Parity gate `APP0.3`:** move state from `planned` to `implemented` and
   extend evidence with ADR `0161`/`0162`, claim-path tests, and the Proven
   ADR rows named by ADR `0160`. Do not mark `verified` until a later audit
   that wants public advertising or full channel parity; `implemented` is the
   production-foundation gate state for internal API channels.

4. **Product docs for optional Gateway stamps:** ADR `0161` plus this ADR are
   the accepted claim-only documentation: Delivery is the traffic owner;
   Gateway `x-a3s-application-release-digest` +
   `x-a3s-publication-channel` remain optional for claimed admit.

5. **Still refused:** path->channel invention, PublishRoute as channel owner,
   Delivery rate middleware, automatic Edge `request_headers` emission,
   REST catalog CRUD, Redis limiter certification, SSE/browser overfitting,
   and inventing a second APP0.3 Delivery restart/recovery rail beyond drain +
   rollback ACL retain (keep APP0.2/H0 rails).

## Consequences

- Epic `APP0.3` may move to production-ready for internal API channels after
  parity gate update + plan/architecture close, while embed/web stay deferred.
- Full public publication parity and APP0.6 public claim remain separate.
- Goal "reach production release conditions" becomes auditable against this
  scoped production meaning rather than inventing missing surfaces.

## Evidence

- This ADR; plan row `APP0.3-C33`; architecture + parity gate refresh
- Prior: ADR `0160`/`0161`; parity `publication.api-blocking` /
  `api-streaming` = `internal`
- Tests: `cargo test -p a3s-cloud-control-plane --lib publication_api_channel_claim_path`;
  `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
