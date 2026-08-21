# AI Application Platform Decisions

These accepted decisions define the authority boundaries used by the versioned
AI application platform parity manifest.

| Decision | Outcome |
| --- | --- |
| [0001](0001-flow-preservation.md) | Preserve A3S Flow as the sole durable orchestration authority |
| [0002](0002-application-delivery.md) | Deliver all application modes through one immutable release and invocation path |
| [0003](0003-step-descriptor-registry.md) | Add immutable Cloud-owned semantic descriptors without widening Flow into a product graph |
| [0004](0004-trigger-authority.md) | Create new invocations only through Automations |
| [0005](0005-file-authority.md) | Separate user-file metadata from shared immutable bytes and runtime working files |
| [0006](0006-knowledge-authority.md) | Keep Knowledge corpus truth separate from Workflow ontology and Search indexes |
| [0007](0007-typed-variable-scopes.md) | Freeze Workflow-owned typed scopes without adding a second mutable state engine |
| [0008](0008-revision-bound-plan-v2.md) | Persist revision-owned semantic contracts and pin exact execution semantics in Plan v2 |
| [0009](0009-workflow-node-catalog-projection.md) | Compose read-only built-in node discovery without creating another descriptor or execution authority |
| [0010](0010-flow-derived-variable-inspection.md) | Inspect runtime variables by replaying immutable input and the sole A3S Flow history |
| [0011](0011-digest-bound-variable-defaults.md) | Materialize digest-bound defaults from immutable WorkflowRevision and Run input without a variable store |
| [0012](0012-revision-bound-composite-region-policies.md) | Bind bounded Iteration and Loop policy plus exact child Workflow identity without adding another execution engine |
| [0013](0013-single-flow-dag-compiler.md) | Reuse A3S Flow as the sole portable DAG structural compiler while Cloud retains ACL and product semantics |
| [0014](0014-exact-flow-runtime-registry.md) | Route every Flow workflow and step through one startup-validated exact registry with no default runtime |
| [0015](0015-versioned-flow-runtime-builds.md) | Give each deployed Flow replay-code generation one explicit build identity and admit older generations only by declaration |
| [0016](0016-deterministic-composite-frames.md) | Reduce exact child Workflow inputs and outputs through one deterministic composite frame without adding another orchestrator |
| [0017](0017-ordered-composite-region-results.md) | Reconstruct arbitrary composite child observations by stable ordinal before reducing output, failure policy, updates, and exports |
| [0018](0018-authority-bound-composite-child-workflow-runs.md) | Execute composite frames as deterministic ordinary child WorkflowRuns through existing Operation, Outbox, Flow, cancellation, and recovery authority |
| [0019](0019-descriptor-bound-execution-failure-routes.md) | Route finite Execution failures through descriptor-bound ordinary DAG edges without another retry or orchestration mechanism |
| [0020](0020-flow-owned-connector-attempt-waits.md) | Keep deterministic Connector observation, retry, and wait decisions in Flow while C6 remains the sole provider-attempt authority |
| [0021](0021-connector-immutable-response-objects.md) | Store accepted Connector response bytes before terminal evidence and retain only their exact immutable reference in Flow |
| [0022](0022-terminal-evidence-authorized-connector-response-reads.md) | Resolve Connector response bytes only through environment authorization, accepted terminal evidence, and the shared immutable-object authority |
| [0023](0023-descriptor-bound-default-output-fallback.md) | Fold terminal finite Execution failure into exact policy-owned output with typed evidence and no second runtime mechanism |
| [0024](0024-schema-bound-connector-json-response-projection.md) | Project an authorized Connector response into typed Workflow JSON through one history-verified no-retry Flow step |
| [0025](0025-descriptor-bound-connector-failure-routes.md) | Route Connector failures through descriptor-bound ordinary DAG edges without another error or retry mechanism |
| [0026](0026-single-application-release-authority.md) | Bind all six application experiences to one immutable Applications-owned release and exact Workflow revision |
| [0027](0027-atomic-application-release-persistence.md) | Persist immutable Application releases, exact Workflow evidence, idempotency, audit, and Outbox facts in one Applications-owned transaction |
| [0028](0028-authorized-application-release-cqrs.md) | Authorize Application release CQRS before replay and resolve only exact immutable Workflow evidence |
| [0029](0029-single-application-management-interface.md) | Expose authorization-first Application management through one CQRS and persistence authority |
| [0030](0030-canonical-user-file-admission.md) | Admit scoped user files through one canonical Files lifecycle and the shared immutable-object authority |
| [0031](0031-single-application-session-authority.md) | Keep release-pinned sessions and exactly-once Workflow semantic effects in one Applications authority |
| [0032](0032-atomic-application-session-persistence.md) | Persist the single Application session and semantic-effect authority atomically without copying WorkflowRun or Flow state |
| [0033](0033-typed-application-workflow-run-composition.md) | Compose exact Application invocations into ordinary deterministic Workflow Goals, Plans, and Runs without bypassing Workflow or Flow |
| [0034](0034-deterministic-application-preset-workflows.md) | Compile stable preset wrapper Workflows through Workflow's sole canonical ACL publication authority |
| [0035](0035-durable-application-invocation-execution-authority.md) | Persist exact invocation execution authority atomically for restart-safe Workflow composition and cancellation |
| [0036](0036-authorized-application-delivery-cqrs.md) | Authorize project-member session, invocation, cancellation, and cursor replay before adopting exact persisted state |
| [0037](0037-workflow-application-semantic-effect-port.md) | Apply exact Workflow Answer, final-output, variable, and terminal effects through the sole Applications session authority |
| [0038](0038-project-member-application-delivery-admission.md) | Admit Principal-owned project-member sessions and invocations through one idempotent Applications/Workflow path |
| [0039](0039-versioned-application-workflow-lifecycle-projection.md) | Project v10 Application WorkflowRun final output and terminal state before saving the replayed Workflow projection |
| [0040](0040-descriptor-bound-application-answer-effects.md) | Commit descriptor-bound v11 Answer messages through Applications before resuming the Workflow DAG |
| [0041](0041-descriptor-bound-application-variable-effects.md) | Commit descriptor-bound v12 Application variable snapshots and CAS revisions before resuming the Workflow DAG |
| [0042](0042-project-member-application-lifecycle-replay.md) | Expose project-member close, cancellation, and complete session replay through the existing Applications/Workflow authority |

All forty-two decisions are normative for `APP0`, `K0`, `AUT0`, and the remaining
`W0` application-platform work. A later change requires a superseding decision
and a new parity-manifest revision; it cannot silently reinterpret `v1`.
