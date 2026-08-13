# 0001: Preserve A3S Flow

Status: Accepted

## Context

A3S Flow already owns durable histories, scheduling, retries, backoff, timers
inside an existing run, Hooks, cancellation, timeout, progress, replay, and
child-operation linkage. Cloud Workflow owns versioned business graphs, plan
compilation, typed semantic state, and projections.

## Decision

A3S Flow remains the only durable orchestration authority. Application nodes
compile to existing Flow commands through Cloud Workflow and Operations. Cloud
may add a domain-neutral Flow primitive only after a conformance test proves the
currently pinned Flow contract cannot express the requirement.

Cloud must not add a run journal, scheduler, retry daemon, timer queue, Hook
store, node executor, or replay authority. Flow must not acquire tenant,
application, graph, model, Knowledge, Tool, ACL, or presentation semantics.

## Consequences

Existing histories and pinned runtime builds remain replayable. Any accepted
Flow extension is backward compatible, versioned in Flow and the Cloud
compatibility lock, and covered by Build, Deployment, Executions, and Workflow
recovery regressions. Cloud semantic projections stay rebuildable from owning
execution facts rather than becoming a second history.
