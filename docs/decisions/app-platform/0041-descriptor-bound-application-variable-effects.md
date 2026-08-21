# 0041: Dispatch descriptor-bound Application variable effects through snapshot and CAS hooks

Status: Accepted

## Context

Decision 0037 defines the sole Applications-owned conversation-variable read
and compare-and-swap port, while Decisions 0039 and 0040 connect final Output,
terminal state, and Answer effects to versioned Workflow runs. Workflow typed
variable contracts can already name `application` scope and optimistic
assignments, but historic runtimes intentionally reject that scope. Executing
an Application variable assigner as an ordinary Connector Service would either
invent a Connector capability or copy mutable session state into Workflow.

The coarse Workflow graph uses `service` for both Connector dispatch and
owning-application ports. It cannot classify a capability-free Service without
the immutable descriptor registry. Application assignment also spans two
external observations: the exact owner snapshot used for materialization and
the exact committed CAS revision. A process can lose either response, and a
concurrent Application write can make the frozen snapshot stale.

The desired values must remain reproducible from immutable Run input, the
variable contract, prior Workflow results, and Flow history. Storing them in a
new table, queue, mutable cache, or untyped Hook metadata would create another
variable or orchestration authority.

## Decision

Workflow semantic-contract validation admits `release_reference` for the exact
`application.conversation-variable-assign` descriptor only. It is owned by
Applications, has coarse kind Service, execution class
`owning_application_port`, no capability types, no failure route, and the
release reference as its sole required binding. Every such descriptor-bound
step must own at least one Application-scoped assignment, and every
Application read or assignment must occur at an exact Applications port.

The structural graph may defer a capability-free Service because it has no
descriptor material. A WorkflowRevision or Plan without semantic-contract
authority rejects that shape. Semantic validation then admits only the exact
Applications descriptor or requires the existing exact ConnectorRevision
capability. Standalone run compilation rejects all Applications-owned steps;
only Application composition can satisfy the release binding.

Application composition with no Application variable access preserves v10 or
v11 exactly. A graph that reads or assigns Application variables emits
WorkflowRun input/runtime/Flow v12 and
`cloud.workflow-run.application-projection.v3`. The projection pins the final
Workflow Output, ordered Answer IDs, ordered Application-variable port IDs,
and the assignment subset. Run validation binds those IDs to the exact Plan
descriptors and variable contract. Inputs and histories v1-v11 cannot acquire
this behavior by aliasing version fields.

Runtime v12 processes each projected variable port in immutable Plan order.
It first creates one deterministic snapshot Hook containing only Run, Plan,
contract, step, attempt, and configuration authority. The coordinator resolves
the sole Application invocation through the Decision 0037 port, validates the
owner snapshot, and resumes Flow with its exact release, session, invocation,
revision, digest, and bounded object values.

Flow materializes the step from that frozen snapshot and existing typed
variable reducer. For an assignment port it deterministically computes the
entire desired conversation-variable object, stores only its digest in a
second write Hook, and suspends. The coordinator recomputes the same object
from immutable Flow evidence, calls the existing Applications CAS port, and
resumes only after the returned child revision exactly matches the expected
parent, values, effect, digest, and Hook creation time. The committed object is
the reconstructed step result; later ports take a fresh Applications snapshot.

The variable contract's expected-revision and idempotency variable names are
structural graph-admission evidence only. Runtime v12 never trusts their
caller-provided values: the expected revision comes from the Applications
snapshot, while the effect identity comes from WorkflowRun, step, attempt, and
port ordinal authority.

A lost read or resume retries the same snapshot phase. A lost write response or
write-Hook resume repeats the same deterministic effect identity and exact CAS
request, so Applications returns the one committed revision. Stale CAS,
missing ports, missing invocation bindings, changed values, and drifted commit
evidence leave the Hook unresolved and fail closed. Snapshot and committed
values remain in redacted Hook payload history rather than Hook metadata.

An independent composite region may coexist in the same v12 contract, but its
frame cannot read, assign, or export Application-scoped values. Frame admission
recognizes only the two exact Applications descriptor IDs when validating the
whole graph, keeps their values outside captured region state, and rejects any
attempt to cross that owner boundary.

Flow-derived variable inspection reuses the same snapshot and validated write
history to show the latest materialized Application values. It does not query
or persist a second variable store. Flow registers `cloud.workflow-run@12`,
and deployment build `a3s-cloud-workflows@12` explicitly retains builds `@1`
through `@11` for replay. No migration, table, queue, scheduler, retry rail,
public delivery surface, or non-ACL product configuration is added.

## Consequences

- Application variable reads and writes execute at their descriptor-bound
  owner boundary without masquerading as Connector calls.
- Multiple assignment steps serialize in immutable Plan order and each uses a
  fresh owner snapshot; assignments within one step share one pre-write view.
- Ambiguous commit recovery cannot create a second variable revision, while a
  stale snapshot is never silently refreshed into a different effect.
- v1-v11 histories and v10/v11 Application composition preserve their exact
  bytes and behavior.
- The [retained PostgreSQL 17 C6-C11 recovery job](https://github.com/A3S-Lab/Cloud/actions/runs/32474020740/job/96746540732)
  proves production-composed Answer and variable commit-before-response loss,
  exact replay, final-output/terminal replay, and durable cardinalities.
- Public blocking/streaming delivery and repeated-frame Answer ordinals remain
  later `APP0.2` gates.
