# 0177. C0.3 external-identity claim path via Resource Grants and Workload Trust

- Status: Accepted
- Date: 2026-09-15
- Gate: C0.3-C1

## Context

Parity capability `enterprise.external-identity` (owner `identity`, gate `C0.3`)
is still `unavailable` while Organizations/Workspaces are already `internal` on
the same gate. Identity already ships Resource Grant CRUD/revocation under
`/organizations/.../resource-grants` and Workload Trust read/write under
`/platform/trust-domains/...` plus organization workload-identity policy routes.

Inventing a directory-product IdP UI, SCIM sync console, or SIEM-bound identity
CMS would overfit. Leaving proven fine-grained grants and workload trust
unclaimed blocks C0.3 progress without first-principles evidence.

## Decision

1. **Claim path:** mark `enterprise.external-identity` `internal` using the
   existing Resource Grant and Workload Trust HTTP surfaces as the production
   claim path.

2. **Explicit deferral:** keep directory-product IdP UI, SCIM sync console, and
   SIEM-bound identity CMS deferred inside this capability until first-principles
   slices exist.

3. **Parity:** extend capability evidence with this ADR and the claim-path
   tests. Move gate `C0.3` from `in_progress` toward continued progress; do not
   mark `verified` or public. `public_claim_gate` remains `APP0.6`.

4. **Still refused:** inventing BYOK/HA as C0.3 work; inventing SCIM/IdP CMS;
   double-claiming OIDC (owned by `enterprise.saml-oidc-scim` / C0.5).

## Consequences

- C0.3 gains an honest internal enterprise surface for fine-grained access and
  external workload identity.
- Full production release still requires `S0`, `H0.5`, remaining C0.3 foundation
  if needed, verified dependency gates, and public APP0.6 parity.

## Evidence

- Controllers: `resource_grant_controller`, `workload_trust_*_controller`
- Tests: `enterprise_external_identity_claim_path_tests.rs`
- Prior: ADRs `0079`, `0087`; organizations-workspaces already `internal` on C0.3

