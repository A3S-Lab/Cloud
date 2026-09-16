# 0256. I0.2 production foundation for inference route control plane

- Status: Accepted
- Date: 2026-09-16
- Gate: I0.2-PF1

## Context

Parity gate `I0.2` is still `planned` with only `doc:docs/inference-plan.md`
evidence, while the repository already ships the Inference route control-plane
slice:

- Nest-style presentation macros on
  `inference_route_queries_controller.rs` and
  `inference_route_commands_controller.rs` (ADRs `0194` / `0207`)
- Publish / revise / retire / list / get CQRS handlers with grant-credential
  and edge-binding admission tests
- Durable usage showback already claimed separately under `I0.2c` (ADR `0254`)

ADR `0251` correctly refuses inventing LLM node availability (`node.llm` and
related W0.4 dependents), OpenAI protocol productization (`I0.6`), and public
`APP0.6` advertisement from Nest presentation alone. Closing `I0.2` on the
existing route control-plane evidence without flipping those capabilities is
the same production-foundation pattern used for `I0.2c` and `AUT0.4`.

This decision does **not** claim I0.2a Power/cache health, I0.2b OpenAI data
plane endpoints, I0.2d external providers, or I0.2e enterprise self-service
consoles.

## Decision

1. Move parity gate `I0.2` from `planned` to `implemented`.
2. Replace inference-plan-only evidence with ADRs `0194`/`0207`, this ADR, the
   Nest route controllers, focused route admission/catalog tests, and the plan
   row.
3. Keep every `node.*` / `plugin.model` capability that depends on `I0.2`
   unchanged (`unavailable` where already unavailable); do not advertise LLM
   nodes.
4. Keep `I0.6` `planned`; do not invent provider-channel productization.
5. Do not mark `I0.2` `verified` or raise `parity_claim`.
6. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.

## Consequences

- I0.2 is an honest production foundation for environment-scoped inference
  route publish/list/revise/retire control plane.
- OpenAI data-plane serving, external providers, LLM nodes, S0, Knowledge,
  U0.4, MCP0.5 joint closure, and public APP0.6 remain separately gated.
