# 0253. A1.4 production foundation for Harness invocation profiles

- Status: Accepted
- Date: 2026-09-16
- Gate: A1.4-PF1

## Context

Parity gate `A1.4` is `planned` with only `doc:ROADMAP.md` evidence, while the
repository already freezes closed, digest-bound Harness invocation profiles:

- `crates/contracts/src/agent_provider/invocation.rs` (`HarnessInvocationProfileV1`)
- `crates/contracts/tests/agent_provider_contract.rs` (closed profile start,
  digest binding, reject unknown fields / secret material)
- control-plane Agent execution binding that materializes one immutable
  invocation profile before provider dispatch
  (`agent_execution_flow/binding.rs`)

`A0.5` Skill/MCP binding foundations are already `implemented`. Toolkit surfaces
that still depend on `A1.4` (`toolkit.new-agent-build-chat`,
`toolkit.new-agent-skill-files`) remain `unavailable` until their own owning
evidence lands. No app-platform capability uses `gate = "A1.4"`. Closing `A1.4`
to `verified`, inventing plugin/`U0.4` claims, or flipping `parity_claim` would
overfit.

## Decision

1. Move parity gate `A1.4` from `planned` to `implemented`.
2. Replace roadmap-only evidence with the invocation contract, focused provider
   contract tests, Agent execution binding implementation, this ADR, and the
   plan row.
3. Do not invent `A1.4`-gated app-platform capabilities.
4. Do not advertise new toolkit availability solely because the gate moves.
5. Do not mark `A1.4` `verified` or raise `parity_claim`.
6. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.

## Consequences

- A1.4 is an honest production foundation for closed Harness invocation-profile
  binding.
- `MCP0.5` joint Runtime/Box/Gateway committed-revision recovery, `U0.4` plugin
  surfaces, and public APP0.6 remain separately gated.
