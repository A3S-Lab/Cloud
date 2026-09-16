# 0157. Durable Postgres Gateway rate-shaping profile catalog

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C28

## Context

APP0.3-C22/C23 wired a shared *process-local* `InMemoryGatewayRateShapingProfileCatalog`
into production snapshot compilers and optional Edge ACL seeds. That catalog
did not survive restart and was re-seeded independently per boot path.
APP0.3-C25?C27 already enforce Gateway HTTP publication admit against the
runtime catalog; missing durable authority left rate-policy revisions
ephemeral across control-plane processes.

Probe for C28 identity `request_headers` emission found no non-invented
intent?route association beyond the existing Edge projection mapper (C15
DTO-only). Inventing path?channel or PublishRoute-as-channel-owner was
refused. The production residual that *does* have first-principles ownership
is a durable Edge/Gateway rate-shaping profile catalog.

## Decision

In Cloud control-plane (`a3s-cloud-control-plane`):

1. Add migration `211_gateway_rate_shaping_profiles` for upsertable
   token-bucket/GCRA profile revisions keyed by `profile_id`.
2. Add `IGatewayRateShapingProfileDurableStore` with in-memory and Postgres
   adapters, plus `install_gateway_rate_shaping_catalog` that hydrates the
   process-local catalog from durable rows, then applies ACL seeds (and
   persists those seeds).
3. Api-worker boot installs one shared catalog + Postgres store; management
   receives the same `Arc`s via `ManagementApplicationDependencies` (no second
   empty catalog).
4. `RegisterGatewayRateShapingProfileHandler` dual-writes durable store then
   process-local catalog.
5. Runtime admission remains process-local; Applications continue to declare
   only opaque profile digests.

## Explicit non-owners

- Inventing path?channel / PublishRoute-as-channel-owner / SSE-first browser
  identity coupling
- Delivery-local rate-limit middleware
- REST catalog CRUD / Redis distributed limiter certification
- Automatic Control-plane emission of identity `request_headers` without a
  non-invented intent?route binding
- Publication rollback rails (closed as contract coverage in APP0.3-C29;
  not invented here)

## Consequences

- Rate-shaping profile revisions survive process restart and stay shared
  between api-worker and management without inventing a second admit owner.
- Remaining residuals: optional non-invented upstream identity stamping;
  publication rollback verification closed by APP0.3-C29 / ADR 0158;
  REST catalog CRUD deferred.

## Evidence

- Migration `migrations/211_gateway_rate_shaping_profiles.sql` registered as Cloud
  migration `211` (`CLOUD_MIGRATION_COUNT` / `LATEST_CLOUD_MIGRATION_VERSION`)
- `gateway_rate_shaping_profile_durable_store.rs` + register handler dual-write
- Focused `cargo test -p a3s-cloud-control-plane --lib gateway_rate_shaping`
  (10 passed)
- Cloud plan row `APP0.3-C28` (this ADR)
