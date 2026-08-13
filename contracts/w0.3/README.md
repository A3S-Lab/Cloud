# W0.3 conformance contracts

## Immutable step descriptors

`step-descriptor-registry.acl` is the canonical conformance fixture for the
Workflow-owned immutable descriptor registry contract. It proves exact SemVer
identity, typed ports, the existing coarse `WorkflowStepKind`, semantic owner,
execution class, configuration/default-policy digests, required bindings,
allowed existing `CapabilityType` values, typed failure behavior, compiler
compatibility, admission metadata, and a separate presentation digest.

The fixture contains two representative admitted descriptors so both a
Workflow-local step and the existing finite Executions application port are
covered. It is not the production built-in catalog, does not advertise all 23
application-platform nodes, and does not change public parity availability.
`step-descriptor-bindings.acl` freezes the separate, presentation-independent
step-to-descriptor authority. Migration `103` persists bindings, the exact
recoverable registry snapshot, and variables as immutable WorkflowRevision
children. `cloud.workflow.plan.v2` pins each semantic descriptor digest; the
`plan.v1` replay shape remains unchanged.

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
availability claim. Authorized inspection, digest-only defaults, composite
frames/exports, and Applications port dispatch remain open. No Flow history
shape or existing Workflow replay behavior changed.

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
