# 0010: Flow-derived Workflow Variable Inspection

Status: Accepted

## Context

Operators need to inspect typed Workflow values while a run is pending,
running, waiting, or terminal. The existing Plan v2 runtime already
materializes invocation inputs, node outputs, and deterministic run assignments
from the immutable `WorkflowRunInput` and the correlated A3S Flow snapshot and
history. Persisting another variable map or copying step results into an
inspection table would create a second run authority and allow inspection to
drift from replay.

Plan v1 has no exact typed-variable contract. Secret variables are opaque
references, and an inspection surface must never reveal Secret material merely
because a declaration is typed as an object.

## Decision

Cloud exposes one project-authorized WorkflowRun variable query. The query
resolves the existing WorkflowRun access grant, restores the exact variable ACL
carried by immutable run input, and asks one Flow-backed reader to observe the
correlated run. That reader reuses the existing Flow identity, sequence,
runtime-build, local-step, HumanTask Hook, and Execution Hook validation before
calling the same domain materializer used by execution.

The versioned `cloud.workflow-run.variable-inspection.v1` response is ordered by
canonical declaration name and identifies the exact plan revision, variable
contract digest, observed Flow sequence, and observation time. Each declaration
reports `materialized` or `unavailable`, its type/scope/storage/mutation
metadata, a canonical value digest when materialized, and the inline value when
it is safe to expose. Secret-reference values are redacted while retaining
their digest. The complete response is bounded to 16 MiB.

Before Flow creates a run, immutable invocation values may be returned at
sequence zero. Once Flow exists, snapshot and history must agree on the same
sequence; a bounded retry handles an in-flight transition. Plan v1 returns a
conflict instead of inventing untyped variable semantics.

REST contract `1.33.0`, the maintained client, `workflow-runs variables`, and
`a3s_cloud_workflow_run_variables_get` all execute this one query and response
projection.

## Consequences

Inspection cannot mutate a value, resume a run, admit a descriptor, or become
an execution input. It adds no migration, variable table, cache, event log,
projection worker, scheduler, or Flow primitive. PostgreSQL recovery tests
reconnect to the same Flow history and reproduce the exact inspection while
asserting that no `workflow_run_variables` table exists.

Decision 0011 makes digest-bound defaults part of immutable Revision and Run v2
input. Because inspection and execution share the materializer, those values
appear without adding a read model or changing this inspection schema.
Composite-region frames/exports and Applications-owned ports remain separate
unfinished semantics and must extend the single runtime materializer first.
