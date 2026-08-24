# 0051: Aggregate branch variables through one typed Workflow-local step

Status: Accepted

## Context

Mutually exclusive Workflow branches can produce the same logical value from
different steps. A downstream consumer needs one stable port regardless of the
selected branch. Template evaluation is not an adequate authority: it treats
missing values as evaluator errors, does not freeze candidate priority, and
cannot prove that every candidate and output has one compatible type.

Scanning dependency output would also bypass the immutable variable contract.
Adding mutable aggregation state or another execution service would duplicate
the state and scheduling authority already owned by A3S Flow. Reinterpreting
WorkflowRun inputs 1 through 19 would change replay behavior for persisted
histories.

## Decision

Variable Aggregator remains the existing `transform` step kind and uses the
versioned ACL payload
`cloud.workflow.configuration.variable-aggregate.v1`. Its exact descriptor ID
and semantic profile are `workflow.variable-aggregate`; the descriptor is
Workflow-owned, Workflow-local, and has no capability, policy, required
binding, or allowed capability type.

The configuration contains one to 32 groups, at most 64 candidates per group,
and at most 256 candidates in total. Every group freezes a concrete non-null
output type and unique output port. Candidate ordinals are unique, contiguous,
and zero-based. Simple mode has exactly one group named `output`. Grouped mode
may expose multiple group ports. A candidate reused by multiple groups must
retain the same type.

Publication requires the descriptor input ports, input data schema, and
optional direct variable reads to cover the unique candidate ports exactly.
Candidate ports are static, single, optional, and type-exact. The output
descriptor and schema are also exact: simple mode exposes one required typed
`output`; grouped mode exposes one required object per group. Reserved
descriptor identity or semantic profile without the versioned configuration is
rejected, as is configuration bound to any custom descriptor.

The compiler retains Plan schemas v2 through v11 and emits immutable
WorkflowRun input/runtime/Flow v20 whenever a revision contains this
configuration. Runtime v20 is cumulative with all admitted v2-v19 semantics.
The local step consumes only the authoritative typed projection, evaluates
candidates by ordinal, skips missing or null candidates, and chooses the first
available value. It validates the selected JSON value against the frozen group
type. Simple output is `{ "output": value }`; grouped output is
`{ "group": { "output": value } }`. No available candidate, non-authoritative
input, or type drift fails closed.

Runtime build `a3s-cloud-workflows@22` adds Flow version 20 and retains builds
`@1` through `@21` as explicit replay-compatible generations. REST/OpenAPI
1.61.0 documents the new returned payload schema, and the maintained
TypeScript client enumerates it. Constraint-only migration `149` widens the
existing `workflow_revision_payloads` schema check for this configuration and
the already supported policy v2/v3 schemas. It adds no table or column, and
canonical ACL parsing remains the semantic authority.

## Consequences

- Mutually exclusive branches converge through one deterministic typed node.
- Candidate priority and output shape are immutable publication material.
- Historic Plan, WorkflowRun, Flow, and runtime-build behavior is unchanged.
- Mixed graphs retain Connector, Application, composite, default, and failure
  semantics from earlier runtime generations.
- Migration `149` changes one closed schema constraint; no table, column,
  mutable variable store, queue, worker, scheduler, provider call, public
  route, or second orchestration authority is added.
