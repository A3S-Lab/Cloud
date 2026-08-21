# 0043: Bind repeated Application Answer frames to one root invocation

Status: Accepted

## Context

Decision 0040 dispatches descriptor-bound Answers from an Application's root
WorkflowRun. Composite Iteration and Loop regions instead execute ordinary
child WorkflowRuns. The same logical child Answer can therefore execute once
per frame while every child has a different Run identity.

Applications resolves semantic effects from the WorkflowRun bound to the sole
invocation. Addressing an Answer by the child Run would find no invocation;
addressing every frame by the root Run and raw child step would collapse
repeated effects. Letting a child also publish final output or terminal state
would incorrectly terminate the root invocation. Persisting a frame table or
adding a delivery scheduler would duplicate existing Workflow, Flow, and
Applications authorities.

Nested composite regions add another ambiguity: sibling ordinals of one
logical region must share an Answer identity, while equivalent inner steps
under different outer frames must not collide. This distinction must be
derived from immutable Run, Plan, region, child-revision, and frame material,
not caller input or process memory.

## Decision

Application composition with composite regions emits immutable WorkflowRun
input/runtime/Flow v13 and root projection
`cloud.workflow-run.application-projection.v5`. The root projection retains
the v10-v12 final-output, Answer, and variable capabilities and additionally
admits frame authority. Application runs without composite regions retain
their exact v10, v11, or v12 representation.

For a semantic composite child, Workflow derives one immutable
`cloud.workflow.application-frame-authority.v1` value from the validated
parent Run and exact `WorkflowCompositeFrame`. It pins Organization, Project,
root Application WorkflowRun, parent Run and Plan, parent execution path,
region step, zero-based ordinal, frame digest, deterministic child Run,
child Workflow definition/revision/digest, logical path, and execution path.
The child is compiled as v13 with
`cloud.workflow-run.application-projection.v4`; tenant, child Run, and child
Plan drift fail closed. Nested children inherit the same root Application Run
and their parent's exact execution path.

The logical path includes the ancestor execution path plus the current parent
Plan, region, and child revision, but excludes the current frame ordinal. An
Answer effect step is a deterministic UUIDv5 of that logical path and the
local descriptor-bound step ID. Sibling frames therefore use one stable
logical effect step and distinct zero-based `effect_ordinal` values. Because
an outer ordinal is already part of the inherited execution path, equivalent
inner Answers under different outer frames remain distinct.

The v13 Answer Hook and resume evidence carry the exact frame authority. The
coordinator addresses C7 with the root Application WorkflowRun, the synthetic
logical Answer step, and the frame ordinal, and resumes only after matching
committed-message evidence. Lost responses repeat the same request. Projection
v4 children never publish Application final output or terminal lifecycle;
only the root v5 projection owns those effects.

Application-scoped variable reads, writes, and exports remain prohibited
inside composite frames. A child revision without semantic-contract material
retains the existing standalone compilation path even when its parent is v13.
The optional frame field on the composite request is transport of immutable
authority only and creates no caller-selected scope.

Flow registers `cloud.workflow-run@13`, and deployment build
`a3s-cloud-workflows@13` explicitly retains `@1` through `@12` for replay.
Inputs and histories v1-v12 cannot acquire frame semantics by aliasing version
or projection fields. No migration, table, queue, scheduler, retry rail,
public delivery surface, or non-ACL product configuration is added.

## Consequences

- Repeated Iteration and Loop Answers commit in stable zero-based ordinal order
  against the one invocation-bound root WorkflowRun.
- Nested frames are collision-free without persisting a path registry or
  changing the existing Applications effect key.
- Child completion cannot prematurely append root final output or observe the
  root invocation terminal state.
- Exact replay after a lost Answer response reuses the same root, logical step,
  attempt, ordinal, content, and commit evidence.
- Focused contract, compiler, runtime, coordinator, lost-response,
  production-adapter, variable, and Connector compatibility tests cover the
  v13 boundary. The [retained PostgreSQL 17 C6-C13 job](https://github.com/A3S-Lab/Cloud/actions/runs/32486698014/job/96784727028)
  exercises two ordinals and commit-before-response replay through the existing
  production Applications repository and records C13 as verified.
- Public blocking/streaming delivery and the remaining `APP0.2` records and
  interfaces remain later gates.
