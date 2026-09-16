# 0171. Proven APP0.6 custom-domain claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.6-C1

## Context

Gate `APP0.5` is now `implemented` (production foundation, ADR `0170`). Gate
`APP0.6` is still `planned`. Among APP0.6-gated enterprise capabilities,
`enterprise.custom-domain-branding` is owned by `edge_gateway` and already ships
environment-scoped domain-claim create/list/get/verify/revoke under Edge
`/organizations` management routes.

No branding CMS/theme product surface exists today. Inventing branding would
overfit. Leaving the proven domain-claim surface `unavailable` blocks honest
APP0.6 progress after APP0.5 foundation.

## Decision

1. **Production claim path** for `enterprise.custom-domain-branding` is the
   existing Edge domain-claim command/query controllers under `/organizations`.
   Branding product UI remains deferred inside this capability until a
   first-principles branding slice exists; do not invent a theme store.

2. **Parity availability:** mark `enterprise.custom-domain-branding` `internal`
   under gate `APP0.6` with ADR/test/implementation evidence.

3. **Gate `APP0.6`:** move from `planned` to `in_progress`. Do not mark
   `implemented` or `verified`. Do not set public `parity_claim`.

4. **Focused conformance:** add an Edge presentation lib test that freezes
   environment-scoped domain-claim list/create and verify claim paths.

## Consequences

- Starts truthful APP0.6 progress without inventing branding or platform HA.
- Does not declare APP0.6 production-complete.
- Full production release still requires remaining APP0.6 enterprise surfaces
  and public parity.

## Evidence

- This ADR; plan row `APP0.6-C1`; architecture + parity updates
- Prior: ADR `0170`; Edge domain-claim controllers
- Tests: `cargo test -p a3s-cloud-control-plane --lib enterprise_custom_domain_claim_path`
  plus `app_platform_parity_manifest`

