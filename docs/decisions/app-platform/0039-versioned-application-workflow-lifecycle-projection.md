# 0039: Project Application WorkflowRun lifecycle through a versioned reconciliation contract

Status: Accepted

## Context

Decision 0037 introduced the Applications-owned semantic-effect port, but no
Workflow runtime called it. An Application invocation could therefore create
and execute an ordinary WorkflowRun without receiving the run's aggregate
output or terminal status. Detecting Application ownership by calling the port
for every terminal WorkflowRun and treating `NotFound` as absence would turn a
repository miss into runtime routing authority. Adding an optional intent to
the existing v9 input would also make new inputs unreadable by old v9 workers
during a mixed deployment because the immutable input rejects unknown fields.

WorkflowRun and Applications persistence are separate authorities. A process
can consequently commit an Applications effect and lose its response, or apply
both lifecycle effects and then lose the WorkflowRun projection save. The
integration must make those retries exact without adding a second execution
history or weakening either repository's optimistic checks.

## Decision

Application composition uses `WorkflowRunCompiler::compile_for_application`
and emits WorkflowRun input/runtime/Flow v10. The immutable input contains one
`cloud.workflow-run.application-projection.v1` value with only the exact final
Output step ID. The compiler derives that ID from the pinned Plan and requires
exactly one Output. Callers cannot provide Application, release, session, or
invocation identities. Inputs v1 through v9 require the projection to be
absent, so their bytes and routing semantics remain unchanged.

Flow registers `cloud.workflow-run@10`, and the deployment build becomes
`a3s-cloud-workflows@10` with builds `@1` through `@9` explicitly replay
compatible. Runtime v10 composes the existing Plan v2-v5 execution authorities,
including optional composites, finite-Execution policies, and Connectors. The
new generation does not by itself admit Answer execution or Application-scoped
variable reads and writes; those require their own descriptor-bound runtime
gate.

After replaying authoritative Flow state and constructing a changed
WorkflowRun projection, the coordinator examines only the immutable v10
Application projection:

- `Completed` appends one final-output message using the projected Output step,
  its positive attempt, ordinal zero, the aggregate output, and the Flow finish
  time, then observes the invocation as `Succeeded`.
- `Failed` and `TimedOut` observe the invocation as `Failed`.
- `Cancelled` observes the invocation as `Cancelled`.
- non-terminal runs and every v1-v9 run produce no Applications call.

The Applications effects run before the coordinator returns the WorkflowRun
projection for persistence. Any missing port, missing invocation correlation,
authority drift, or write failure prevents that projection from being saved.
On retry, the same Flow history regenerates the same effect identities and
times; decision 0037's deterministic recovery adopts prior commits. Final
output always precedes terminal observation.

Production composition injects one `WorkflowApplicationEffectsService` backed
by the existing Applications session repository. No public delivery surface,
table, queue, scheduler, Application identity in Workflow state, or duplicate
Flow history is added.

## Consequences

- Application-run lifecycle projection is explicit, versioned, and cannot be
  inferred from a repository miss.
- A committed final output or terminal observation is safe to redeliver after
  a lost response or failed WorkflowRun projection save.
- Old WorkflowRun generations never depend on Applications and remain replay
  compatible under their exact recorded inputs.
- Completed Application runs fail closed if their single final Output
  projection, attempt, result, or Applications port is missing or drifted.
- Answer frames, Application conversation-variable materialization and
  assignment, public blocking/streaming delivery, and real PostgreSQL recovery
  evidence remain later `APP0.2` gates.
