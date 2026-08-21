# 0040: Dispatch descriptor-bound Answer effects before resuming Workflow

Status: Accepted

## Context

Decision 0039 projects one final Workflow Output after a v10 Application
WorkflowRun finishes. The coarse Workflow graph also represents Answer as an
`output` step, however. Counting every coarse Output as final therefore rejects
a graph containing Answer, while executing Answer as an ordinary local Output
would complete Flow without committing the ordered Applications-owned message
effect.

Answer and final Output have different authorities. An Answer is an
Applications-owned, release-bound interaction frame and may occur at more than
one graph step. Final Output is the single Workflow-owned value that completes
the run. A process can commit an Answer and lose the response before resuming
Flow, so dispatch must recover the same effect rather than append another
message. Historic WorkflowRun inputs cannot acquire this behavior implicitly.

Application-scoped variable reads and assignments need a separate asynchronous
snapshot and compare-and-swap protocol. Coupling that protocol to Answer would
make either boundary less reviewable and would not be required to close the
Answer execution gap.

## Decision

Workflow semantic-contract validation admits `release_reference` only for the
exact Applications-owned `application.answer` descriptor: coarse kind Output,
execution class `owning_application_port`, no capability binding, and the
release reference as its sole required binding. Application publication and
run compilation resolve descriptor bindings, require exactly one
Workflow-owned `workflow.output`, and classify every remaining coarse Output
as an exact Answer. A standalone run compiler rejects any Applications-owned
step because only Application composition can satisfy its release binding.

Application compilation without Answer remains byte- and behavior-compatible
v10. A graph with at least one Answer emits WorkflowRun input/runtime/Flow v11
and `cloud.workflow-run.application-projection.v2`. The projection retains the
exact final Output step ID and adds the Answer step IDs in immutable Plan order.
It must partition all coarse Output steps into one final Output and one or more
distinct Answers. Inputs v1-v10 cannot carry this projection generation.

Runtime v11 evaluates each Answer with the existing typed Output semantics,
including its configuration template and output schema, but does not schedule
it as a local Flow step. It creates one deterministic Answer hook containing
the exact organization, project, run, Plan, step, attempt, configuration,
content, and content-digest authority. The hook creation event supplies the
canonical effect time; attempt one and ordinal zero identify the one Answer
effect for that graph step.

The Workflow coordinator verifies the hook snapshot and its sole creation
event, calls `IWorkflowApplicationEffectsPort::append_answer`, validates the
returned Applications message against the request, and only then resumes the
hook with bounded commit evidence. Flow validates that evidence and restores
the precomputed typed Answer result before continuing the DAG. A lost port
response or lost hook-resume response therefore repeats the same deterministic
Applications effect. Missing ports, missing invocation bindings, drifted
messages, and write failures leave the hook unresolved and fail closed.

Final WorkflowRun output in v11 is taken only from the projected
`workflow.output`; Answer leaves are never aggregated into it. Projection and
history validation recognize Answer hooks only when the v2 marker names the
step. Flow registers `cloud.workflow-run@11`, and deployment build
`a3s-cloud-workflows@11` explicitly retains builds `@1` through `@10` for
replay. No table, queue, scheduler, second history, Application identity in
Workflow state, public streaming surface, or non-ACL product configuration is
added.

## Consequences

- Answer is dispatched at its durable graph boundary and cannot be mistaken
  for final Workflow output.
- Multiple distinct Answer leaves are serialized by deterministic Plan order;
  repeated loop or subworkflow frames require a later ordinal-aware extension.
- Applications message idempotency closes both commit-response loss and
  hook-resume loss without weakening Flow replay.
- v1-v10 histories never probe Applications for Answer and keep their exact
  recorded behavior.
- Application conversation-variable materialization and assignment, public
  blocking/streaming delivery, and real PostgreSQL recovery evidence remain
  later `APP0.2` gates.
