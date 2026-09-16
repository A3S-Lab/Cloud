# 0252. A1.3 production foundation for provider-neutral Agent execution contracts

- Status: Accepted
- Date: 2026-09-16
- Gate: A1.3-PF1

## Context

Parity gate `A1.3` is `planned` with only `doc:ROADMAP.md` evidence, while the
repository already freezes provider-neutral Agent execution contracts:

- `contracts/a1.3/a3s-code-provider-profile.acl`
- `contracts/a1.3/reference-echo-provider-profile.acl`
- `crates/contracts/tests/agent_provider_contract.rs` (capability negotiation,
  profile-bound commands/receipts, event pages, approval checkpoints)
- control-plane `AgentExecutionProvider` registry with built-in Code and
  reference-echo providers

`A1.4` invocation-profile binding is claimed separately under ADR `0253`. No
app-platform capability currently uses `gate = "A1.3"`. Closing `A1.3` to
`verified` or inventing plugin/strategy product claims would overfit.

## Decision

1. Move parity gate `A1.3` from `planned` to `implemented`.
2. Replace roadmap-only evidence with the A1.3 contracts, focused provider
   contract tests, provider registry implementation, this ADR, and the plan
   row.
3. Do not invent `A1.3`-gated app-platform capabilities.
4. Do not mark `A1.3` `verified` or raise `parity_claim`.
5. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.

## Consequences

- A1.3 is an honest production foundation for the provider-neutral Harness
  contract slice.
- Plugin/`U0.4` surfaces stay deferred until their own claimable evidence lands;
  `A1.4` is claimed under ADR `0253`.

