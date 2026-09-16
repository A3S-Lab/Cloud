# 0175. Proven C0.5 audit-security claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: C0.5-C2

## Context

Gate `C0.5` is `in_progress` after ADR `0174` claimed Identity-owned
`enterprise.saml-oidc-scim` via proven OIDC routes. The remaining C0.5-gated
enterprise capability is `enterprise.audit-security` (owner `identity`, shared
audit per the platform plan).

Control-plane already ships organization-administrator Audit query routes for
record list, signed export, export manifest, and retention status under
`/organizations/.../audit-records/...`. No SIEM connector product,
investigation workspace, or PII-redaction policy CMS exists as a first-class
surface today. Inventing those would overfit. Leaving the proven Audit export
rail `unavailable` blocks honest C0.5 progress.

## Decision

1. **Production claim path** for `enterprise.audit-security` is the existing
   Audit query controller under `/organizations` (list, export, export
   manifest, retention). SIEM product integration, investigation UI, and
   PII-redaction CMS remain deferred inside this capability until
   first-principles slices exist.

2. **Parity availability:** mark `enterprise.audit-security` `internal` under
   gate `C0.5` with ADR/test/implementation evidence.

3. **Gate `C0.5`:** remain `in_progress`. Do not mark `implemented` or
   `verified`. Do not set public `parity_claim`.

4. **Focused conformance:** add an Audit presentation lib test that freezes
   export/manifest/list/retention claim paths.

## Consequences

- Continues truthful C0.5 progress without inventing SIEM/redaction products.
- Does not declare C0.5 or public APP0.6 production-complete.
- Full production release still requires C0.5 foundation close-out, `S0`,
  `H0.5`, verified gates, and public parity.

## Evidence

- This ADR; plan row `C0.5-C2`; architecture + parity updates
- Prior: ADR `0174`; Audit `audit_query_controller`
- Tests: `cargo test -p a3s-cloud-control-plane --lib enterprise_audit_security_claim_path`
  plus `app_platform_parity_manifest`

