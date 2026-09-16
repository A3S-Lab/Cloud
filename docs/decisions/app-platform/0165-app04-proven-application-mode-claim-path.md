# 0165. Proven APP0.4 application-mode claim path (four presets)

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.4-C1

## Context

Gate `APP0.4` is still `planned` while six `application.*` mode capabilities
remain `unavailable`. APP0.2-C29 / ADR `0101` already prove Applications owns
one immutable authoring-profile ? C4 preset Workflow ?
`ApplicationReleaseContract` path for **Chatbot, Text Generator, classic Agent,
and New Agent**. Chatflow and Workflow fail closed on that preset path by
design.

Flipping all six modes to available, inventing mode-specific controllers /
session stores / execution engines, or marking APP0.4 `implemented` /
`verified` would overfit. Leaving the four proven presets `unavailable`
blocks honest APP0.4 progress after APP0.2/APP0.3 production foundations
(ADR `0162`/`0164`).

## Decision

1. **Production claim path** for the four proven application modes is the
   Applications-owned `PublishApplicationAuthoringProfile` CQRS command and the
   resulting exact-release contract (ADR `0101` / C29), sharing one release and
   invocation spine with APP0.1/APP0.2 delivery. No mode-specific controller,
   session store, Agent/sandbox lifecycle, or second execution engine.

2. **Parity availability:** mark `application.chatbot`,
   `application.text-generator`, `application.classic-agent`, and
   `application.new-agent` `internal` under gate `APP0.4` with ADR/test/
   implementation evidence. Keep `application.chatflow` and
   `application.workflow` `unavailable` until first-principles graph/Workflow
   authoring slices exist (preset path continues to fail closed).

3. **Gate `APP0.4`:** move from `planned` to `in_progress`. Do not mark
   `implemented` or `verified`. Do not set public `parity_claim`. Remaining
   APP0.4 toolkit/channel work (opener/follow-up ownership transfer, MCP/
   internal publication, New Agent build-chat, etc.) stays open.

4. **Focused conformance:** add a Cloud lib test that freezes the six
   `ApplicationExperience` identities, conversation vs invocation interaction
   modes, and Chatflow/Workflow fail-closed authoring-profile creation.

## Consequences

- Restores truthfulness between verified C29 preset publication and APP0.4 mode
  parity for the four preset experiences.
- Does not declare APP0.4 production-complete or invent Chatflow/Workflow
  preset compilers.
- Full production release across APP0 still requires remaining APP0.4+ gates and
  APP0.6 for public claim.

## Evidence

- This ADR; plan row `APP0.4-C1`; architecture + parity updates
- Prior: ADR `0034`, `0101`; APP0.2-C29
- Tests: `cargo test -p a3s-cloud-control-plane --lib application_mode_claim_path`
  plus existing `authoring_profile` suite; `app_platform_parity_manifest`

