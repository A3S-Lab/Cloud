# 0172. Proven APP0.6 isolation-quota-retention claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.6-C2

## Context

Gate `APP0.6` is `in_progress` after ADR `0171` claimed Edge-owned
`enterprise.custom-domain-branding`. The next APP0.6-gated enterprise
capability is `enterprise.isolation-quota-retention` (owner `platform`).

The plan assigns this surface to **owning contexts plus shared policy and
audit**, not a new platform isolation product. Control-plane already ships:

- Files org quota query (`USER_FILE_QUOTA_ROUTE`)
- Audit retention status under `.../audit-records/retention`
- Inference usage retention under `.../inference-usage/retention`

Inventing a platform isolation/concurrency module or second policy plane would
overfit. Leaving the proven quota/retention HTTP `unavailable` blocks honest
APP0.6 progress.

`C0.5` remains non-`verified`; this slice does not flip Identity platform RBAC.

## Decision

1. **Production claim path** for `enterprise.isolation-quota-retention` is the
   existing Files quota + Audit retention + Inference usage retention
   controllers under `/organizations`. Do not invent platform isolation,
   concurrency governors, or retention CMS.

2. **Parity availability:** mark `enterprise.isolation-quota-retention`
   `internal` under gate `APP0.6` with ADR/test/implementation evidence.
   Keep dependency on `C0.5` as declared (non-Verified deps allowed for
   internal caps).

3. **Gate `APP0.6`:** remain `in_progress`. Do not mark `implemented` or
   `verified`. Do not set public `parity_claim`.

4. **Focused conformance:** add a modules-level lib test that freezes the
   three owning-context claim path fragments.

## Consequences

- Continues truthful APP0.6 progress without inventing platform HA/isolation.
- Does not declare APP0.6 production-complete.
- Full production release still requires remaining APP0.6 enterprise surfaces
  and public parity.

## Evidence

- This ADR; plan row `APP0.6-C2`; architecture + parity updates
- Prior: ADR `0171`; Files/Audit/Inference retention/quota controllers
- Tests: `cargo test -p a3s-cloud-control-plane --lib enterprise_isolation_quota_retention_claim_path`
  plus `app_platform_parity_manifest`

