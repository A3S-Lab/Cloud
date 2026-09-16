# 0251. Production-release blockers: refuse inventing AUT0.4, S0, and Knowledge datasource productization

- Status: Accepted
- Date: 2026-09-16
- Gate: production-release-honesty

## Context

After AUT0.2/AUT0.3 webhook and schedule foundations, and after A1.3/A1.4
provider and invocation-profile foundations (ADRs `0252`/`0253`), remaining
planned gates that still own unavailable inventory include:

- `AUT0.4` / `node.integration-trigger` → no control-plane integration or
  plugin-trigger implementation
- `S0` / `enterprise.byok-residency-airgap` → no BYOK/residency/air-gap product
  path
- `K0.2`/`K0.3`/`K0.5` Knowledge authoring inventory → Nest knowledge HTTP
  (ADR `0240`) is presentation-only and explicitly refuses retrieval
  productization; pipeline entrance digests are not datasource-file /
  transform product claims
- `U0.4` plugin inventory and `I0.*` inference nodes → foreign owners without
  a claimable internal slice in this pass
- `MCP0.5` → joint release gate that requires one Box-hosted MCP Service
  proven end-to-end through real Cloud, Runtime, and Gateway processes at
  exact committed revisions; Nest MCP controllers and `mcp0.1` contracts alone
  do not close it

Closing those gates to `implemented` or flipping capabilities to `internal`
without owning implementations would overfit catalogs to Nest presentation
work and falsely advance public `APP0.6` readiness.

`parity_claim` remains `false`. Public advertisement still requires verified
owning gates for every public capability.

## Decision

1. Keep `S0`, `K0.2`, `K0.3`, `K0.5`, `U0.4`, `I0.2`/`I0.6`, and `MCP0.5`
   `planned` until a real owning implementation slice exists (for `MCP0.5`,
   that means joint committed-revision evidence, not catalog-only evidence).
   `AUT0.4` component foundations (C1-C4) are claimed separately in ADR
   `0255` without advertising `node.integration-trigger`.
2. Refuse inventing BYOK/S0, HA/ops console, SAML-SCIM productization beyond
   already-claimed internal C0.5 surfaces, SIEM/PII CMS, Knowledge
   datasource/transform product claims from Nest controllers alone, and
   MCP0.5 joint closure from Nest/`mcp0.1` presentation evidence alone.
3. Do not set `parity_claim=true` or advertise public capabilities in this
   decision.
4. Treat Nest attribute-macro coverage as presentation preference evidence,
   not as public release completion.

## Consequences

- Production release remains blocked on honest owner evidence, not on more
  catalog flips.
- Next claimable work must show domain/application/infrastructure proof for
  the specific capability ID before any availability change; `MCP0.5` must
  show joint process evidence at pinned revisions.
