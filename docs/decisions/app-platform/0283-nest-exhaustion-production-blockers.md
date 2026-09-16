# 0283. Nest claimable-surface exhaustion and production-release foreign blockers

- Status: Accepted
- Date: 2026-09-16
- Gate: production-release-honesty / PROD-R2

## Context

After Nest migrations through ADR `0282`, a HEAD audit of
`crates/control-plane` shows seven remaining `ControllerDefinition::new`
surfaces without `#[controller]`:

- `build_run_commands_controller` — 100% deferred project scope
- `mcp_service_profile_{commands,queries}_controller` — 100% deferred
- `smart_http_controller` — 100% deferred
- `notifications/controller` — 100% deferred
- `operations_query_controller` — 100% deferred (SSE/query)
- `presentation/management_mcp/module` — protocol-special `/mcp` verb surface

None of those leftovers are claimable Nest hybrids. Inventing Nest wrappers
around deferred admission or MCP protocol verbs would overfit presentation
preference into false release progress.

Remaining APP-parity `planned` gates are still foreign owners with plan-only
evidence:

| Gate | Why it stays planned |
| --- | --- |
| `I0.6` | LLM / OpenAI data-plane productization beyond `I0.2` route foundation |
| `S0` | BYOK / residency / air-gap product path absent |
| `K0.2` / `K0.3` / `K0.5` | Datasource / transform / pipeline product claims beyond Nest knowledge HTTP |
| `U0.4` | Executable plugin surfaces (Tool/MCP/Secret/UI adapters); U0.1–U0.3 assignment foundations are not `U0.4` |
| `MCP0.5` | Joint Cloud/Runtime/Gateway committed-revision process evidence required |

`APP0.6` remains `implemented` with `parity_claim=false` and
`public_claim_gate=APP0.6`. Public advertisement still requires verified owning
gates for every public capability.

This ADR also records that `workflow-node-profiles.acl` must pin the live
parity-manifest digest; a stale pin after Nest evidence inserts is a release
blocker and must be repaired with the current canonical digest.

## Decision

1. Treat claimable Nest attribute-macro coverage as **exhausted** for
   production-foundation purposes. Keep the seven deferred/protocol leftovers
   imperative.
2. Keep `I0.6`, `S0`, `K0.2`, `K0.3`, `K0.5`, `U0.4`, and `MCP0.5` `planned`
   until real owning implementation slices exist (MCP0.5 requires joint pinned
   revisions, not Nest/`mcp0.1` alone).
3. Refuse inventing BYOK/S0, integration-trigger availability without `U0.4`,
   HA/ops console productization, SAML-SCIM beyond claimed C0.5 internals,
   Knowledge datasource/transform product claims from Nest alone, and MCP0.5
   joint closure from presentation evidence alone.
4. Do not set `parity_claim=true`, do not mark `APP0.6` `verified`, and do not
   advertise unavailable capabilities as `internal`.
5. Pin `workflow-node-profiles.acl` `parity_manifest_digest` to the current
   canonical parity-manifest digest whenever the manifest changes.

## Consequences

- Production release is blocked only on honest foreign owners, not on further
  Nest presentation churn.
- Next claimable work must show domain/application/infrastructure proof for a
  specific capability ID (or joint MCP process evidence) before any gate or
  availability change.
- ADR `0251` remains the refusal baseline; this ADR is the post-Nest-exhaustion
  audit that proves blockers-only.

## Evidence

- This ADR
- HEAD audit of `ControllerDefinition::new` without `#[controller]`
- `contracts/app-platform/v1/parity-manifest.acl` planned-gate inventory
- `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
- `cargo test -p a3s-cloud-contracts --test workflow_node_profiles`
