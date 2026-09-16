# 0173. APP0.6 production foundation for claimable enterprise

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.6-C3

## Context

APP0.6-C1 and APP0.6-C2 marked Edge-owned `enterprise.custom-domain-branding`
and platform-owned `enterprise.isolation-quota-retention` `internal` through
non-invented claim paths (domain-claims; Files quota + Audit/Inference
retention). Gate `APP0.6` remains `in_progress`.

Unlike APP0.5, no further capabilities are gated on `APP0.6`. Remaining
enterprise inventory (`enterprise.saml-oidc-scim`, `enterprise.audit-security`,
`enterprise.byok-residency-airgap`, `enterprise.ha-disaster-recovery`,
`enterprise.external-identity`) is owned by Identity/`C0.5`, platform/`S0`,
platform/`H0.5`, or Identity/`C0.3`. Inventing APP0.6-owned SSO, HA, BYOK, or
rebinding foreign gates under APP0.6 would overfit.

Leaving `APP0.6` `in_progress` after every APP0.6-gated capability is honestly
claimed blocks the production-foundation pattern already accepted for APP0.2
(ADR `0164`), APP0.3 (ADR `0162`), APP0.4 (ADR `0167`), and APP0.5 (ADR `0170`).

## Decision

1. **APP0.6 production scope is the two claimable APP0.6 enterprise
   internals**, not full enterprise parity across Identity/HA/Secrets gates.
   Production foundation means `enterprise.custom-domain-branding` and
   `enterprise.isolation-quota-retention` are `internal` with Proven C1/C2
   claim-path evidence (ADR `0171` / `0172`).

2. **Explicit first-principles deferral:** keep SSO/SCIM, audit-security,
   BYOK/residency/airgap, HA/DR, and external-identity on their declared
   gates (`C0.5`, `S0`, `H0.5`, `C0.3`). Do not invent APP0.6-owned
   replacements or flip those gates in this slice.

3. **Parity gate `APP0.6`:** move state from `in_progress` to `implemented`
   and extend evidence with this ADR plus C1/C2 tests. Do not mark `verified`
   or set public `parity_claim`. `public_claim_gate` remains `APP0.6`.

4. **Still refused:** inventing SAML/OIDC/SCIM product surfaces as APP0.6
   close-out; inventing HA/BYOK modules; marking APP0.6 `verified` / public;
   treating foreign-gate enterprise caps as APP0.6 deferrals that must flip
   unavailable??nternal here.

## Consequences

- Epic `APP0.6` may move to production-ready for claimable enterprise while
  SSO/HA/BYOK stay on their own gates.
- Full production release still requires verified dependency gates and public
  parity, not merely APP0.6 `implemented`.

## Evidence

- This ADR; plan row `APP0.6-C3`; architecture + parity gate refresh
- Prior: ADR `0171` / `0172`; two APP0.6 enterprise caps = `internal`
- Tests: `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
  (APP0.6 foundation assertions); existing custom-domain / isolation-quota
  claim-path suites

