# W0.3 conformance contracts

## Immutable step descriptors

`step-descriptor-registry.acl` is the canonical conformance fixture for the
Workflow-owned immutable descriptor registry contract. It proves exact SemVer
identity, typed ports, the existing coarse `WorkflowStepKind`, semantic owner,
execution class, configuration/default-policy digests, required bindings,
allowed existing `CapabilityType` values, typed failure behavior, compiler
compatibility, admission metadata, and a separate presentation digest.

The fixture contains five representative admitted descriptors so Workflow-local
Input, Transform, If / Else, and Output steps plus the existing finite Executions
application port are covered. It is not the production built-in catalog, does not advertise all 23
application-platform nodes, and does not change public parity availability.
`step-descriptor-bindings.acl` freezes the separate, presentation-independent
step-to-descriptor authority. Migration `103` persists bindings, the exact
recoverable registry snapshot, and variables as immutable WorkflowRevision
children. `cloud.workflow.plan.v2` pins each semantic descriptor digest; the
`plan.v1` replay shape remains unchanged.

## Descriptor-bound finite failure route

The `executions.finite` descriptor declares one required static object error
output named `error`, owner-classified retry, and failure-branch fallback. A
Workflow graph may opt into that contract with one ordinary handled edge from
the Execution step plus at least one unhandled success edge. The handle must be
exactly `error`; no authoring label, presentation metadata, or runtime guess may
select it.

That graph emits `cloud.workflow.plan.v3`, which copies every exact descriptor
failure contract into the canonical Plan. Immutable WorkflowRun
input/runtime/Flow v4 converts dispatch rejection, terminal Execution failure,
or terminal cancellation into a bounded `cloud.workflow.step-failure.v1`
object and selects the same ordinary edge. The Execution projection stays
failed while the reachable error path may complete its parent. Without the
edge, historical fail-fast behavior remains. Plan v1-v2 and Run input v1-v3
retain their byte and replay shape.

The same finite Execution descriptor may instead select mutually exclusive
default-output fallback: no error port or handled edge is then permitted. Its
exact `default_policy_digest` must equal the Workflow step policy digest, and
policy v3 contains one canonical value for the descriptor's required static
single output port. That graph emits Plan v4 and immutable WorkflowRun
input/runtime/Flow v7. Dispatch rejection, terminal failure, or cancellation
returns the exact policy value and retains bounded
`cloud.workflow.step-default-output.v1` evidence in the completed projection.
Plan v1-v3 and Run inputs v1-v6 retain their exact bytes and replay behavior.

This is conformance for the existing finite Execution port only. It adds no
retry engine, error queue, node-run table, provider lifecycle, or Flow history.
Application composite Answer frames are implemented by `APP0.2-C13` without
changing this finite-Execution contract. Compensation and other provider error
branches remain unavailable.

## Descriptor-bound Workflow-local Transform failure route

The `workflow.transform` descriptor declares one required static object error
output named `error`, a non-retryable classification, and failure-branch
fallback. Selecting that port emits `cloud.workflow.plan.v8` and immutable
WorkflowRun input/runtime/Flow v16. A failed local evaluation runs exactly
once, resumes normal DAG interpretation, and produces the bounded redacted
`cloud.workflow.step-failure.v5` value. The failed Transform projection records
the exact `error` handle while its ordinary failure sink may complete the
parent run.

Runtime v16 never copies the template evaluator's raw error into handled DAG
data or the public step projection. Migration `145` only admits failed
Transform selected-handle evidence in the existing projection constraint, and
runtime build `a3s-cloud-workflows@18` retains `@1` through `@17` for exact
replay. Plans v1-v7 and Run inputs v1-v15 keep their bytes and behavior. This
slice adds no table, column, retry engine, queue, worker, scheduler, or second
Flow mechanism.

## Descriptor-bound Workflow-local Output failure route

The `workflow.output` descriptor declares one required static object error
output named `error`, a non-retryable classification, and failure-branch
fallback. Selecting that port emits `cloud.workflow.plan.v9` and immutable
WorkflowRun input/runtime/Flow v17. Template or output-schema evaluation failure
runs exactly once, resumes normal DAG interpretation, and produces the bounded
redacted `cloud.workflow.step-failure.v6` value. The failed source Output
projection records the exact `error` handle while its ordinary failure sink may
complete the parent run.

Runtime v17 never copies evaluator diagnostics into handled DAG data or the
public step projection. Migration `143` already admits failed Output
selected-handle evidence and still rejects completed aliases. Runtime build
`a3s-cloud-workflows@19` retains `@1` through `@18` for exact replay. Plans
v1-v8 and Run inputs v1-v16 keep their bytes and behavior. This slice adds no
table, column, retry engine, queue, worker, scheduler, or second Flow mechanism.

## Descriptor-bound Workflow-local Branch failure route

The `workflow.branch` descriptor with semantic profile `workflow.if-else`
declares one required static object error output named `error`, a non-retryable
classification, and failure-branch fallback. Configuration routes and the
default remain ordinary business handles and must be disjoint from `error`.
Selecting the descriptor error port emits `cloud.workflow.plan.v10` and
immutable WorkflowRun input/runtime/Flow v18. Missing or invalid selector
evaluation runs exactly once, resumes ordinary DAG interpretation, and produces
the bounded redacted `cloud.workflow.step-failure.v7` value. The failed Branch
projection records the exact `error` handle while its error sink may complete
the parent run.

Runtime v18 never copies selector diagnostics into handled DAG data or the
public step projection. The existing failed Branch selected-handle projection
already carries the exact descriptor handle, so no migration is required.
Runtime build `a3s-cloud-workflows@20` retains `@1` through `@19` for exact
replay. Plans v1-v9 and Run inputs v1-v17 keep their bytes and behavior. This
slice adds no table, column, retry engine, queue, worker, scheduler, or second
Flow mechanism.

## Bounded runtime evidence references

The existing `WorkflowStepProjection.evidenceReferences` field now retains a
closed, deterministic correlation to owning-context evidence for current
finite Execution, Connector, and HumanDecision steps. A verified terminal
Execution resume projects the exact
`urn:a3s:cloud:executions:execution:<uuid>` and
`urn:a3s:cloud:operations:operation:<uuid>` identities. Each verified received
Connector observation projects its exact
`urn:a3s:cloud:connectors:attempt:<uuid>` identity. A verified received
HumanDecision resume projects its exact
`urn:a3s:cloud:workflow:human-task:<uuid>` and
`urn:a3s:cloud:workflow:workflow-decision:<uuid>` identities; an interactive
submit, approve, or reject additionally projects the accepted
`urn:a3s:cloud:forms:submission:<uuid>` identity. Automatic expiry and
cancellation intentionally project no FormSubmission reference. Repeated
observations of one attempt are deduplicated, distinct retries remain visible,
and the final array is sorted and bounded to 32 entries.

The projection is reconstructed only from immutable WorkflowRun input and
verified A3S Flow Hook history. A provider dispatch rejection that has no
owning-context evidence projects no reference. References never contain a
request, response, provider message, credential, or evidence body, and do not
grant access to the referenced resource. Existing WorkflowRun authorization
still guards the projection, while each owning context independently guards its
resource. Historical terminal projections are not mutated or backfilled. This
slice reuses the existing JSON field and persistence path; it adds no table,
column, evidence store, event log, route, or OpenAPI shape.

## Typed variable scopes

`variable-contract.acl` is the canonical conformance fixture for
`cloud.workflow.variable-contract.v1`. The Workflow-owned immutable contract
declares invocation inputs, node outputs, composite-local values, deterministic
run values, and Applications-owned values. It freezes typed declarations,
reads, ordered assignments, explicit composite exports, exact root and leaf
schema digests, and one exact compiler-schema version.

Graph admission validates step identity, reachability, dominance, source-schema
ancestry, deterministic writer order, branch-local optionality, and unique
consumer-port bindings. Secret and large values remain opaque typed references;
they cannot be dereferenced or copied into mutable Workflow state. Application
values require an Applications port plus optimistic revision and idempotency
evidence. Plan v1 still rejects those reads and writes. Plan v2 can prove the
descriptor owner during compilation. WorkflowRun input/runtime/Flow v2 also
freezes the exact variable ACL and reconstructs invocation, node-output,
deterministic run-assignment, direct-read, and opaque-reference values from the
immutable input plus existing Flow history. Explicit reads are authoritative
for their step and are consumed only through `current`; steps without reads
retain their legacy dependency input.

This is a persisted compiler/runtime contract, not a variable store or a public
availability claim. `variable-defaults.acl` is the companion canonical
`cloud.workflow.variable-defaults.v1` fixture. The variable contract retains
only each default digest; the companion revision child supplies exact bounded
canonical JSON, must cover the digest-backed declarations exactly, participates
in the semantic-contract-set digest, and is copied into immutable Run v2 input.
The shared execution/inspection materializer applies it only when the declared
source value is absent. The deterministic composite frame
materializes region-local input and reduces child output through declared
assignments, Run updates, and exports. The region reducer folds those results
in stable ordinal order. Runtime v3 dispatches those frames through exact Flow
hooks and deterministic ordinary child WorkflowRuns; Applications-owned
variable access remains open.
No mutable variable table, event log, scheduler, queue, Flow history shape, or
existing Workflow replay behavior was added.

## Immutable composite-region policies

`composite-regions.acl` is the canonical conformance fixture for
`cloud.workflow.composite-regions.v1`. It freezes the Workflow-owned policy
for every admitted Iteration and Loop descriptor: stable step identity,
bounded items or iterations, bounded concurrency or time, failure behavior,
and the Loop termination-value path. Policies are sorted by stable step ID and
must exactly cover descriptors whose execution class is `composite_region`
and whose semantic profile is `workflow.iteration` or `workflow.loop`.

Each covered graph step remains the existing `subworkflow` kind and must bind
`workflow.run` to one exact, non-nil child WorkflowRevision. The canonical
contract is limited to 512 regions and 512 KiB. Iteration admits at most 10,000
items and concurrency from 1 through 10; Loop admits at most 10,000 iterations,
a 30-day time budget, and 32 portable termination-path segments.

Migration `108` permits this optional fifth immutable WorkflowRevision child
without adding a table. New publication fails closed when a composite
descriptor has no exact region contract, while pre-migration revisions remain
readable. The semantic-contract-set digest, Plan v2
`compositeRegionsDigest`, and immutable Run v3 input pin the exact ACL and
digest for composite execution; non-composite Plan v2 runs retain Run v2.
REST/OpenAPI `1.35.0`, the maintained client, CLI, and Management MCP
only transport that bounded material as optional `compositeRegionsAcl`.

The `cloud.workflow.composite-frame.v1`,
`cloud.workflow.composite-frame-result.v1`, and
`cloud.workflow.composite-region-result.v1` runtime values bind the exact Plan,
variable/composite digests, zero-based bounded ordinal, and child
WorkflowRevision. They deterministically project child input, reduce bounded
child output through one pre-write assignment snapshot, restore arbitrary
completion observations to contiguous ordinal order, apply the immutable
Iteration failure mode, require Loop's boolean termination path, and fold Run
updates and explicit exports in ordinal order. These JSON values are runtime
state, not product configuration; the authorizing contracts remain A3S ACL.

WorkflowRun runtime/Flow v3 executes these regions through authority-bound
`workflow-composite:<step>:<ordinal>` hooks. A Workflow-owned port derives one
deterministic ordinary child Goal, Plan, WorkflowRun, and Operation from each
frame, uses the existing Outbox/Flow start path, records an exact
`workflow_run` child reference, and resumes the parent with a digest-bound
frame resolution. Iteration dispatch is sequential in ordinal order, so the
declared concurrency remains an enforced upper bound. Loop also enforces its
iteration/time bounds and passes terminal output into the next frame. Parent
cancellation and timeout adopt, cancel, and await children before the parent
terminates. No mutable region store, scheduler, queue, worker, event history,
or second Flow mechanism was added.

## Finite Execution

`execution-template.acl` is the single ACL-native finite-task definition used
by the Workflow PostgreSQL/process-death fixture and the REST/Management MCP
cross-surface gate. Both paths parse it through the Executions-owned
ExecutionTemplate domain contract; neither Workflow nor a presentation adapter
maintains a second template representation.

The fixture pins a digest-addressed OCI artifact, bounded process and resource
settings, and the exact `execution.run` capability surface. Invocation input,
child lifecycle, Operation/Flow history, and cleanup state remain runtime data,
not product configuration.

Migrations `098` and `099` own the immutable template and exact child binding.
Migration `100` only evolves the existing WorkflowStepProjection kind
constraint to admit `execution`; it creates no parallel projection or child
execution mechanism.

The clean Linux H0 gate certifies finite persistence and seven process-death
boundaries. The clean C0.2 Management MCP/A3S Box/PostgreSQL gate certifies the
same ACL fixture across REST and MCP with `8/8` persistence, replay, rollback,
immutability, and tenant non-disclosure checks. These results verify the finite
Execution sub-gate while W0.3 remains in progress.
