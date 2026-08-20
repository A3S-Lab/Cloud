# 0019: Route typed Execution failures through descriptor-bound DAG edges

Status: Accepted

## Context

The immutable step descriptor already declares a typed error output, retry
classification, fallback mode, and whether a failure branch exists. The
Workflow graph previously rejected every handled edge from a non-branch step,
while the finite Execution adapter terminated its parent WorkflowRun for every
dispatch rejection, failed child, or cancelled child. The admitted descriptor
and the executable graph therefore expressed different behavior.

Closing that gap must not add another error queue, retry engine, node-run
table, event history, or provider lifecycle. Historical Plan and WorkflowRun
inputs must also retain their exact bytes and replay behavior.

## Decision

One Execution step may declare one handled outgoing edge in addition to at
least one unhandled success edge. The handled edge is an ordinary Workflow DAG
edge. Its `source_handle` must exactly equal the immutable descriptor's error
output name, and the descriptor must declare failure-branch fallback with one
required, static, single object value. Branch steps keep their existing named
route semantics. No other non-branch step kind is admitted in this slice.

The compiler emits `cloud.workflow.plan.v3` only when the graph opts into this
route. Every Plan v3 step carries the exact descriptor failure contract so the
canonical Plan digest pins the interpretation. Plans v1 and v2 omit that
optional field and retain their canonical bytes. Plan v3 compiles to immutable
WorkflowRun input/runtime/Flow version 4 and runtime build
`a3s-cloud-workflows@4`; versions 1 through 3 remain explicit replay entries.

The existing authority-bound Execution hook remains the sole observation
path. Dispatch rejection, terminal failure, or terminal cancellation is
converted deterministically into a bounded
`cloud.workflow.step-failure.v1` object. It records the step, classification,
message, and authority-bound Execution detail when a child exists. The local
result selects the descriptor error handle. Dependency resolution compares the
result handle with every ordinary edge handle, so the failure edge activates
and unhandled success edges become inactive. Without an admitted failure edge,
the same observations retain the historical fail-fast behavior.

The Execution projection remains `failed`, stores its bounded error, and
exposes the selected handle; the parent WorkflowRun may still complete after
the reachable failure branch resolves. Downstream values, variable
materialization, replay, and inactive-step calculation all derive the same
typed result from immutable input and the one A3S Flow history.

## Consequences

Finite Execution now has one descriptor-bound typed failure path without
copying the Executions lifecycle or introducing a second orchestration
mechanism. Owner-classified retry remains with Executions; Workflow only
interprets the terminal authority-bound result. Replay drift in the Plan,
descriptor port, hook payload, error classification, selected handle, or
projection fails closed.

Default-output fallback, Answer frames, compensation, and failure branches for
Agent, MCP, model, Tool, Service, Memory, or composite steps remain separate
gates. Supporting one of them requires its owning provider contract and a new
runtime generation; it cannot silently reuse the finite Execution claim.
