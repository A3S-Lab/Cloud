# 0023: Fold terminal Execution failure into an exact default output

Status: Accepted

## Context

The immutable step descriptor already distinguishes failure-branch,
default-output, and unsupported fallback. The finite Execution runtime already
observes dispatch rejection, terminal child failure, and terminal child
cancellation through one authority-bound hook. Before this decision, choosing
default-output in a descriptor could not be published or executed, so callers
had to model an error branch or accept fail-fast behavior.

Closing that gap must not add a scheduler, retry engine, provider client,
node-run record, event history, or second orchestration path. The exact fallback
value must be immutable and replayable, and Plans v1 through v3 plus WorkflowRun
versions 1 through 6 must retain their canonical bytes and behavior.

## Decision

Default-output fallback is admitted only for the Executions-owned finite
`Execution` application port bound to one exact `ExecutionTemplate`. Its
descriptor must declare owner-classified failure, no error output or failure
branch, and exactly one required, static, single output port. The Workflow step
must bind the descriptor's exact `default_policy_digest`; a handled edge from
that step is rejected.

The immutable policy payload owns the exact fallback material. Policy v3 stores
one canonical JSON value, its digest, and its output port. Policy v2 remains the
only provider-retry policy schema, so retry and default-output ownership cannot
be combined. The existing step `policy_digest` is the single policy authority;
Plan does not copy that digest into another field.

The compiler emits `cloud.workflow.plan.v4` only when at least one step uses
this fallback. Plan v4 retains every descriptor failure contract from Plan v3
and adds only the typed output-port contract required to interpret the pinned
policy. It compiles to WorkflowRun input/runtime/Flow version 7 and runtime
build `a3s-cloud-workflows@7`; runtime generations 1 through 6 remain explicit
replay entries.

The existing Execution hook remains the sole failure observation path. A
dispatch rejection, terminal failure, or terminal cancellation is classified
as the same bounded `cloud.workflow.step-failure.v1` value used by failure
routes. Instead of selecting an error edge, runtime returns the exact
digest-bound policy value as the ordinary successful graph value and attaches
`cloud.workflow.step-default-output.v1` evidence. Projection records the exact
policy digest, port, and terminal failure observation beside the completed
result. Replay validates all of them against immutable Run input.

Migration `122` adds one nullable evidence column to the existing step
projection and corrects its selected-handle constraint for the already accepted
Execution failure-route behavior. It adds no Workflow table, queue, worker,
cache, provider lifecycle, or history. REST/OpenAPI `1.41.0` and the maintained
client expose the optional Plan contract and projection evidence through the
existing Workflow surfaces.

## Consequences

Finite Execution can now degrade deterministically to one exact typed value
without hiding why the provider failed. Failure routing and default output are
two descriptor-selected interpretations of the same terminal observation and
the same A3S Flow history, not separate engines. Descriptor, policy, Plan,
runtime, result, or projection drift fails closed.

Applications-owned variables, Answer frames, non-Execution error semantics,
compensation, and remaining Agent, MCP, model, Tool, Service, Memory, and
composite provider behavior remain separate gates. A later provider may adopt
default output only through its owning application port and a new explicit
runtime generation.
