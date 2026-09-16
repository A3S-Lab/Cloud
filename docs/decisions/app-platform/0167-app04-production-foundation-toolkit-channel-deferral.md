# 0167. APP0.4 production foundation with toolkit and channel deferral

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.4-C3

## Context

APP0.4-C1 and APP0.4-C2 marked all six `application.*` mode capabilities
`internal` through non-invented claim paths (authoring-profile / C4 presets for
four modes; exact user-authored Workflow release for Chatflow/Workflow). Gate
`APP0.4` remains `in_progress` while many APP0.4-owned toolkit and publication
capabilities stay `unavailable`.

A repository audit shows those remaining APP0.4 surfaces have **no Applications
domain or CQRS owners** today (moderation, snippets, TTS/STT, templates catalog,
ACL import/export, collaborative revision, error policy, variable inspection,
version control, node test, hosted MCP facade, internal invocation, global
discovery, New Agent build-chat / skill files, `publication.internal`,
`publication.mcp`). Inventing any of them to close the gate would overfit.
Leaving the gate `in_progress` after the six modes are honestly claimed blocks
the same production-foundation pattern already accepted for APP0.2 (ADR `0164`)
and APP0.3 (ADR `0162`).

## Decision

1. **APP0.4 production scope is six application-mode internal availability**,
   not full APP0.4 toolkit/channel public or internal parity. Production
   foundation means the six `application.*` modes are `internal` with Proven
   C1/C2 claim-path evidence (ADR `0165` / `0166`).

2. **Explicit first-principles deferral:** keep every APP0.4-gated capability
   other than the six application modes `unavailable` until first-principles
   domain slices exist. Do not invent moderation/TTS/STT/MCP/internal
   publication product surfaces, New Agent build-chat engines, template
   catalogs, or mode-adjacent toolkit runtimes as the close-out of APP0.4.
   APP0.2 opener/follow-up deferral (ADR `0164`) remains unchanged.

3. **Parity gate `APP0.4`:** move state from `in_progress` to `implemented`
   and extend evidence with this ADR plus C1/C2 tests. Do not mark `verified`
   or set public `parity_claim`. `implemented` is the production-foundation
   gate state for application modes.

4. **Still refused:** inventing toolkit domains listed above; Chatflow/Workflow
   preset compilers; mode-specific controllers/engines; marking APP0.4
   `verified` / public; substituting APP0.5 monitoring or APP0.6 enterprise
   work as APP0.4 close-out.

## Consequences

- Epic `APP0.4` may move to production-ready for application modes while
  deferred toolkits/channels stay `unavailable`.
- Full toolkit/channel parity and APP0.6 public claim remain separate.
- Goal "reach production release conditions" advances to APP0.5 / APP0.6 as
  the next honest blockers, not invented APP0.4 surfaces.

## Evidence

- This ADR; plan row `APP0.4-C3`; architecture + parity gate refresh
- Prior: ADR `0165` / `0166`; six `application.*` = `internal`
- Tests: `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
  (APP0.4 foundation assertions); existing
  `application_mode_claim_path` /
  `application_chatflow_workflow_claim_path` suites

