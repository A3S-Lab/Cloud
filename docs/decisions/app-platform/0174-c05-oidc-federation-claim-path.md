# 0174. Proven C0.5 OIDC federation claim path

- Status: Accepted
- Date: 2026-09-15
- Gate: C0.5-C1

## Context

APP0.6 production foundation (ADR `0173`) is `implemented` for APP0.6-gated
enterprise. Remaining public-release blockers include Identity gate `C0.5`
(`planned`) owning `enterprise.saml-oidc-scim` and `enterprise.audit-security`.

Identity already ships OIDC login/callback under `/identity/oidc` and
organization-scoped link under `/organizations/.../identity/oidc/.../link`.
No SAML IdP federation controller or SCIM provisioning surface exists today.
Inventing SAML/SCIM would overfit. Leaving proven OIDC federation
`unavailable` blocks honest C0.5 progress after APP0.6 foundation.

## Decision

1. **Production claim path** for `enterprise.saml-oidc-scim` is the existing
   Identity OIDC public login/callback plus organization link controllers.
   SAML IdP federation and SCIM directory provisioning remain deferred inside
   this capability until first-principles SAML/SCIM slices exist.

2. **Parity availability:** mark `enterprise.saml-oidc-scim` `internal` under
   gate `C0.5` with ADR/test/implementation evidence.

3. **Gate `C0.5`:** move from `planned` to `in_progress`. Do not mark
   `implemented` or `verified`. Do not set public `parity_claim`. Do not flip
   `enterprise.audit-security` in this slice.

4. **Focused conformance:** add an Identity presentation lib test that freezes
   OIDC login, callback, and link claim paths.

## Consequences

- Starts truthful C0.5 progress without inventing SAML/SCIM.
- Does not declare C0.5 or public APP0.6 production-complete.
- Full production release still requires remaining C0.5 surfaces, `S0`,
  `H0.5`, verified gates, and public parity.

## Evidence

- This ADR; plan row `C0.5-C1`; architecture + parity updates
- Prior: ADR `0173`; Identity `oidc_controller`
- Tests: `cargo test -p a3s-cloud-control-plane --lib enterprise_saml_oidc_scim_claim_path`
  plus `app_platform_parity_manifest`

