# 0178. C0.3 production foundation for claimable enterprise identity access surfaces

- Status: Accepted
- Date: 2026-09-15
- Gate: C0.3-C2

## Context

C0.3 now has both Identity-gated enterprise capabilities `internal`:
`enterprise.organizations-workspaces` (prior) and `enterprise.external-identity`
(C0.3-C1 via Resource Grants + Workload Trust). Gate `C0.3` remains
`in_progress`.

Directory-product IdP UI, SCIM sync console, and SIEM-bound identity CMS remain
deferred **inside** `enterprise.external-identity`, matching the APP0.5/APP0.6/
C0.5 production-foundation pattern.

Leaving `C0.3` `in_progress` after every C0.3-gated capability is honestly
claimed blocks that same foundation pattern.

## Decision

1. **C0.3 production scope is the two claimable C0.3 enterprise internals**,
   not inventing deferred directory/SCIM/SIEM CMS surfaces.

2. **Explicit first-principles deferral:** keep directory IdP UI, SCIM console,
   and SIEM identity CMS deferred inside the claimed capabilities until
   first-principles slices exist.

3. **Parity gate `C0.3`:** move state from `in_progress` to `implemented` and
   extend evidence with this ADR plus C1 tests. Do not mark `verified` or set
   public `parity_claim`. `public_claim_gate` remains `APP0.6`.

4. **Still refused:** inventing BYOK/HA (`S0`/`H0.5`) as C0.3 close-out;
   marking C0.3 `verified` / public; inventing directory CMS as foundation.

## Consequences

- Epic `C0.3` may move to production-ready for claimable enterprise identity
  access surfaces while deferred product depths stay unfinished inside the caps.
- Full production release still requires `S0`, `H0.5`, verified dependency
  gates, and public APP0.6 parity.

## Evidence

- This ADR; plan row `C0.3-C2`; architecture + parity gate refresh
- Prior: ADR `0177`; both C0.3 enterprise caps = `internal`
- Tests: `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`

