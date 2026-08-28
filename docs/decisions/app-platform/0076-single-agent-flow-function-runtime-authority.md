# 0076: Compose Agent, Workflow, and Function runtimes from one authority per concern

Status: Accepted

## Context

A3S Cloud is Agent-first, while A3S Flow is also a first-class orchestration
dependency. A Workflow may chain or parallel Agent nodes, A3S Code must invoke
external FaaS as governed Tools, Cloud must host Function profiles, and
sessionless MCP services may use stateless Function-style deployment.

Adding an `AgentRuntime`, `FunctionRuntime`, or `DurableCell` execution
substrate for each product would duplicate the existing AgentExecution,
WorkflowRun, Operation/Flow, Execution, Workload, Fleet, Runtime/Box, Secret,
object, route, and autoscaling authorities. Conversely, collapsing Agent,
Workflow, Function, and Connector semantics into a generic execution aggregate
would erase the invariants that make recovery and authorization safe.

## Decision

Agent Runtime is an application projection over the existing Agents and shared
execution authorities. It creates no new aggregate, table, repository,
scheduler, queue, event log, object client, Runtime unit class, or autoscaler.

Workflow and Agent execution remain independently first-class:

- WorkflowRun plus the outer A3S Flow owns graph order, parallel waves, step
  attempts, waits, retry, cancellation, and compensation.
- AgentExecution plus its child A3S Flow owns provider admission, dispatch,
  semantic events, approvals, checkpoints, forks, recovery, and cancellation.
- The outer Flow links the AgentExecution Operation and resumes only from an
  Agents-owned terminal observation. It never drives the Harness or copies its
  semantic log.

Stateful coding Agents use an ordinary warm Runtime Service pool. Every
execution binds the exact provider and invocation profiles, node, Workload,
revision, Deployment, replica, Runtime unit and generation, spec digest,
endpoint, provider run, and exclusive workspace lease before dispatch. Same-
generation process recovery retains that binding; node/replica recovery fences
the predecessor and creates one new binding only after a verified checkpoint.

Function Runtime is a product profile over the same substrate:

- finite durable calls use Executions and Runtime Task;
- low-latency stateless HTTP and sessionless MCP use Workloads and Runtime
  Service behind Edge/Gateway;
- external FaaS uses one exact Connector revision and attempt.

All callers carry one immutable invocation-authority envelope for tenant,
parent, slot/attempt, exact target, input digest/reference, deadline,
idempotency, authorization, and egress class. Each target retains a typed
owner-specific port and result. The shared envelope must not become a universal
executor or contain product lifecycle, provider, persistence, or transport
types.

A Harness invokes hosted or external Functions only as governed Tools through
the AgentExecution Flow. It receives no raw provider credential and owns no
retry, egress, or external-attempt state.

Durable Cell is an independent first-class named collaboration-state product
for humans and multiple Agents. It is not a hidden storage mechanism for
AgentExecution, WorkflowRun, Function profiles, workspace, checkpoint, Tool,
model, MCP, or memory semantics. A governed Agent or Workflow may call a Cell
as a service while each context retains its own history. `CELL0` may be
delivered after the Agent/Workflow/Function critical path without weakening
its architecture status or public contract.

## Consequences

- Agent and Workflow are both first-class without creating two owners for the
  same orchestration decision.
- A3S Runtime remains universally small: only Task and Service.
- Hosted and external FaaS reuse existing execution and integration evidence
  instead of creating another scheduler or provider control plane.
- Sessionless MCP may adopt Function-style Service deployment without moving
  MCP protocol or Gateway policy into Function Runtime.
- Durable Cell remains first-class without adding a Runtime Cell class, a
  second scheduler, or a second Flow history.
- Scaling changes capacity for new leases; active Agent executions remain
  pinned and migrate only through explicit checkpoint/fence/recovery.
- Cross-replica Agent recovery, hosted Function profiles, external FaaS Tools,
  and sessionless MCP remain unavailable until their named roadmap gates pass.
