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

All twenty-two decisions are normative for `APP0`, `K0`, `AUT0`, and the remaining
`W0` application-platform work. A later change requires a superseding decision
and a new parity-manifest revision; it cannot silently reinterpret `v1`.
