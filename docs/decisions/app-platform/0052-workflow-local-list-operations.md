# 0052: Process typed lists through one Workflow-local step

Status: Accepted

## Context

Workflow authors need to filter, select, order, and limit an array without
dispatching a provider or embedding those operations in an untyped template.
The operation order, filter operands, item type, and result ports must be
immutable publication material so that persisted runs replay deterministically.

Reading arbitrary dependency output would bypass the Workflow variable
contract. Adding a list-operation service, mutable intermediate store, or
second scheduler would duplicate authority already owned by A3S Flow.
Reinterpreting WorkflowRun inputs 1 through 20 would also change historic
replay behavior.

## Decision

List Operator remains the existing `transform` step kind and uses the versioned
ACL payload `cloud.workflow.configuration.list-operator.v1`. Its exact
descriptor ID and semantic profile are `workflow.list-operator`; the descriptor
is Workflow-owned, Workflow-local, and has no capability, policy, required
binding, or allowed capability type.

The configuration freezes one required array source, an item type of `object`,
`string`, `number`, or `boolean`, at most 64 ordered filter conditions, an
optional one-based extraction index, an optional typed ordering rule, and an
optional positive limit. Source arrays, extraction indices, and limits are
bounded to 10,000. Conditions are unique and use contiguous zero-based
ordinals. String conditions admit contains, prefix, suffix, equality,
membership, emptiness, and their closed negative forms. Number conditions
admit equality and ordered comparisons; boolean conditions admit equality and
inequality. Operands are either bounded canonical JSON literals or exact typed
input ports.

Publication requires the input data schema, descriptor ports, and direct
variable reads to cover the source and every dynamic operand or extraction
port exactly. The source port and read are required; operation ports and reads
are optional so an empty source does not force unused parameter resolution.
The output schema and descriptor must expose one
required array `result` plus optional item-typed `first_record` and
`last_record` ports. Reserved descriptor identity or semantic profile without
the exact versioned configuration is rejected, as is the configuration bound
to any custom descriptor.

The compiler retains Plan schemas v2 through v11 and emits immutable
WorkflowRun input/runtime/Flow v21 whenever a revision contains this
configuration. Runtime v21 composes all admitted v2-v20 behavior and consumes
only the authoritative typed projection. It validates every source item, then
applies operations in the fixed order filter, extract, order, limit. Multiple
filters run sequentially by ordinal. An empty source succeeds before operation
parameters are resolved and returns an empty `result`; its optional record
outputs are absent. Non-array input, type drift, invalid operands, and
out-of-range extraction fail closed.

Runtime build `a3s-cloud-workflows@23` adds Flow version 21 and retains builds
`@1` through `@22` as explicit replay-compatible generations. REST/OpenAPI
1.62.0 documents the returned payload schema, and the maintained TypeScript
client enumerates it. Constraint-only migration `151` widens the existing
`workflow_revision_payloads` schema check. It adds no table or column, and
canonical ACL parsing remains the semantic authority.

## Consequences

- List processing is deterministic, typed, bounded, and provider-free.
- Dynamic values remain exact immutable variable reads rather than runtime
  expression lookup.
- Historic Plan, WorkflowRun, Flow, and runtime-build behavior is unchanged.
- Mixed graphs retain Variable Aggregator, Connector, Application, composite,
  default, and failure semantics from earlier runtime generations.
- Migration `151` changes one closed schema constraint; no table, column,
  mutable list store, queue, worker, scheduler, provider call, public route, or
  second orchestration authority is added.
