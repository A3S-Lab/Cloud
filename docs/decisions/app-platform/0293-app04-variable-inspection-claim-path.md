# 0293. APP0.4 claim path for Workflow-owned variable inspection

- Status: Accepted
- Date: 2026-09-16
- Gate: APP0.4-C4

## Context

ADR `0167` closed APP0.4 production foundation on six `application.*` modes and
deferred remaining APP0.4 toolkits as `unavailable`, stating a HEAD audit found
no Applications CQRS owners for those surfaces. That deferral remains correct
for moderation, TTS/STT, snippets, templates, MCP facade, and related toolkits.

`toolkit.variable-inspection` is different. It is Workflow-owned (`owner =
"workflow"`, dependency `W0.3`) and already has first-principles product
evidence:

- ADR `0010` freezes Flow-derived `cloud.workflow-run.variable-inspection.v1`
- Domain query `GetWorkflowRunVariables` + Flow-backed reader/materializer
- Management route `GET .../workflow-runs/{workflow_run_id}/variables` on the
  Nest-macro `WorkflowQueriesController` (org-scoped deferred-project admission)
- Domain inspection tests and REST/client/CLI/MCP contract history from ADR `0010`

Leaving this capability `unavailable` after the owning path exists understates
honest inventory and blocks production-release progress without inventing a new
domain.

## Decision

1. Mark `toolkit.variable-inspection` `internal` on the proven WorkflowRun
   variables claim path. Do not invent a second variable map, Applications
   toolkit domain, or public advertisement.
2. Keep every other APP0.4 toolkit/`publication.internal`/`publication.mcp`
   capability `unavailable` per ADR `0167`.
3. Keep parity gate `APP0.4` `implemented` (already closed by C3). Extend gate
   evidence with this ADR and the claim-path test. Do not mark `verified` or set
   `parity_claim=true`.
4. Refuse flipping opener/follow-up, embed/web publication, moderation, TTS/STT,
   node-test, snippets, error-policy, version-control, or hosted MCP façade as
   part of this slice.

## Consequences

- One more Cloud-owned APP0.4 capability becomes honestly `internal`.
- Public APP0.6 / `parity_claim` remain blocked on foreign planned gates and
  verified public owners.
- Nest preference is satisfied by reusing the existing Nest-macro workflow
  queries controller; no new imperative controller wrapper is introduced.

## Evidence

- This ADR; plan row `APP0.4-C4`
- ADR `0010`; `GetWorkflowRunVariables` handler; workflow queries variables route
- Focused `toolkit_variable_inspection_claim_path`
- `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
