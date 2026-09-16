# 0166. Proven APP0.4 Chatflow/Workflow claim path (exact Workflow release)

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.4-C2

## Context

APP0.4-C1 marked the four preset application modes `internal` and left
`application.chatflow` / `application.workflow` `unavailable` because the
authoring-profile / C4 preset path fails closed for those experiences
(ADR `0034` / `0101` / `0165`).

APP0.1 already owns `CreateApplication` and `PublishApplicationRelease` over
one exact `ApplicationReleaseContract` that retains an immutable
`ApplicationWorkflowBinding`. Existing CQRS suites already exercise Chatflow
releases through that path. Inventing a Chatflow/Workflow preset compiler,
graph editor, or second mode controller would overfit. Leaving the proven
exact-Workflow publication path `unavailable` blocks honest APP0.4 mode
progress.

## Decision

1. **Production claim path** for Chatflow and Workflow application modes is
   Applications-owned `CreateApplication` / `PublishApplicationRelease` with an
   exact user-authored Workflow revision admitted through
   `IApplicationWorkflowRevisionPort`. Experience is immutable per Application
   identity. Chatflow uses conversation delivery; Workflow uses invocation
   delivery. No preset authoring-profile path, graph editor, or mode-specific
   execution engine.

2. **Fail-closed invariant retained:** `PublishApplicationAuthoringProfile` /
   C4 preset compilation continues to reject Chatflow and Workflow (ADR
   `0165`).

3. **Parity availability:** mark `application.chatflow` and
   `application.workflow` `internal` under gate `APP0.4` with ADR/test/
   implementation evidence. Keep gate `APP0.4` `in_progress` (not
   `implemented` / `verified` / public). Remaining APP0.4 toolkit/channel
   work stays open.

4. **Focused conformance:** add a Cloud lib test that creates and publishes
   Chatflow and Workflow Applications through the exact Workflow binding, and
   re-asserts preset authoring fail-closed.

## Consequences

- Aligns APP0.4 mode parity with the already-shipping APP0.1 release authority
  for graph-backed experiences.
- Does not declare APP0.4 production-complete or invent Chatflow/Workflow
  authoring UIs.
- Full production release across APP0 still requires remaining APP0.4+ gates
  and APP0.6 for public claim.

## Evidence

- This ADR; plan row `APP0.4-C2`; architecture + parity updates
- Prior: ADR `0026`, `0034`, `0101`, `0165`
- Tests: `cargo test -p a3s-cloud-control-plane --lib application_chatflow_workflow_claim_path`
  plus existing `application::tests` create/publish suite;
  `app_platform_parity_manifest`

