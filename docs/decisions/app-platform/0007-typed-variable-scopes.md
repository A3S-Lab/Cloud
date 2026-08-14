# 0007: Workflow-owned typed variable scopes

Status: Accepted

## Context

Application workflows need values from invocation input, step output,
Iteration and Loop frames, run-local state, Applications state, Secrets, and
large immutable objects. Treating all of them as an untyped shared map would
erase ownership, leak opaque material, and make branch and replay behavior
ambiguous. Giving Cloud a second mutable state engine would duplicate A3S Flow
and owning application services.

## Decision

Workflow owns one immutable, canonical ACL variable contract for each compiled
Workflow revision. It declares typed scopes, reads, deterministic assignments,
and explicit composite exports. Compilation validates exact schema ancestry,
step reachability and dominance, deterministic mutation order, and region
boundaries.

Secrets and large values are opaque typed references. Applications-owned values
are read and changed only through a descriptor-bound Applications port; changes
require optimistic revision and idempotency evidence. Workflow run-local values
are deterministic semantic state, while Flow remains the sole durable run and
replay authority.

## Consequences

Migration `103` now persists the contract with its Workflow revision, and
`cloud.workflow.plan.v2` pins its exact digest together with exact per-step
descriptor semantics. Plan v1 remains unchanged. WorkflowRun input/runtime/Flow
v2 materializes the initial supported subset by replaying immutable run input
and existing Flow history. Authorized inspection now exposes that same
materialization through one bounded read projection. Decision 0011 adds
digest-bound defaults as immutable revision and Run input material;
composite-region state/exports and Applications-owned reads/writes remain
fail-closed. Cloud does not add another variable store, event log, scheduler,
or queue.
