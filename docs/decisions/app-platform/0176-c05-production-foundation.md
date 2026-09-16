# 0176. C0.5 production foundation for claimable enterprise identity surfaces

- Status: Accepted
- Date: 2026-09-15
- Gate: C0.5-C3

## Context

C0.5-C1 and C0.5-C2 marked Identity-gated `enterprise.saml-oidc-scim` and
`enterprise.audit-security` `internal` through non-invented claim paths (OIDC
login/callback/link; shared Audit export/manifest/list/retention). Gate `C0.5`
remains `in_progress`.

No further capabilities are gated on `C0.5`. SAML IdP federation, SCIM
directory provisioning, SIEM product connectors, investigation UI, and
PII-redaction CMS remain deferred **inside** the two claimed capabilities,
matching the APP0.5/APP0.6 production-foundation pattern (ADR `0170` / `0173`).

Leaving `C0.5` `in_progress` after every C0.5-gated capability is honestly
claimed blocks that same foundation pattern.

## Decision

1. **C0.5 production scope is the two claimable C0.5 enterprise internals**,
   not inventing deferred SAML/SCIM/SIEM/redaction product surfaces.

2. **Explicit first-principles deferral:** keep SAML IdP, SCIM provisioning,
   SIEM connectors, investigation UI, and PII-redaction CMS deferred inside
   the claimed capabilities until first-principles slices exist. Do not invent
   them as C0.5 close-out.

3. **Parity gate `C0.5`:** move state from `in_progress` to `implemented` and
   extend evidence with this ADR plus C1/C2 tests. Do not mark `verified` or
   set public `parity_claim`. `public_claim_gate` remains `APP0.6`.

4. **Still refused:** inventing SAML/SCIM/SIEM/redaction as foundation
   close-out; marking C0.5 `verified` / public; treating `S0`/`H0.5` BYOK/HA
   as C0.5 work.

## Consequences

- Epic `C0.5` may move to production-ready for claimable enterprise identity
  surfaces while deferred product depths stay unfinished inside the caps.
- Full production release still requires `S0`, `H0.5`, verified dependency
  gates, and public APP0.6 parity.

## Evidence

- This ADR; plan row `C0.5-C3`; architecture + parity gate refresh
- Prior: ADR `0174` / `0175`; two C0.5 enterprise caps = `internal`
- Tests: `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
  (C0.5 foundation assertions); existing OIDC / audit-security claim-path suites

