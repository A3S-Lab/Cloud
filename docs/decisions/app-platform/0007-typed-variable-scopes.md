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

The current contract can validate and canonically digest variable semantics
without changing `cloud.workflow.plan.v1`. Production use requires persistent
revision binding and an explicit next plan/compiler schema that pins the exact
descriptor and variable-contract digests. Runtime materialization and
inspection must project through the existing Workflow run and Flow history;
Cloud does not add another variable store, event log, scheduler, or queue.
