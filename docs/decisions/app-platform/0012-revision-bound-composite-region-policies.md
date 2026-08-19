# 0012: Bind composite-region policy to WorkflowRevision

Status: Accepted

## Context

Iteration and Loop are Workflow-owned composite graph regions, not generic
A3S Flow node types. Replaying either region requires exact scheduling,
failure, termination, and child-Workflow identity. A descriptor profile alone
cannot reconstruct those semantics, while a mutable policy lookup would allow
historical plans to drift.

Cloud must also preserve the existing ownership boundary. Workflow owns graph
meaning and bounded region policy; A3S Flow owns durable history, replay,
scheduling, retries, waits, cancellation, and child-operation linkage.
Implementing a Workflow-local scheduler, queue, worker, or event history would
duplicate that authority.

## Decision

A compiler-schema-2 Workflow revision may own one optional canonical
`cloud.workflow.composite-regions.v1` child. New publication requires it
whenever an admitted descriptor has execution class `composite_region`. The
contract must exactly cover those descriptors, match
`workflow.iteration` or `workflow.loop` profiles, and remain sorted by stable
step ID.

Iteration freezes maximum items, concurrency, and one of `terminate`,
`continue_null`, or `remove_failed`. Loop freezes maximum iterations, time
budget, and a bounded portable termination-value path. Every covered graph step
uses the existing `subworkflow` kind and binds `workflow.run` to one exact,
non-nil child WorkflowRevision. The contract admits at most 512 regions and
512 KiB.

The optional child participates in the semantic-contract-set digest.
`cloud.workflow.plan.v2` pins its `compositeRegionsDigest`, and composite
WorkflowRun compilation copies the exact ACL and digest into immutable Run v3 input.
Historical compiler-schema-2 revisions without the child remain restorable,
but cannot gain or infer composite execution semantics.

## Consequences

Migration `108` expands the existing revision-child kind constraint and permits
three mandatory children plus optional default and composite material. It adds
no table. The existing trigger now proves that all three mandatory kinds are
present when the total is three, four, or five.

REST/OpenAPI `1.35.0`, the maintained client, CLI publication file, and
Workflow Management MCP inputs accept optional `compositeRegionsAcl`. Existing
Plan v1 and Run v2 bytes without composite material remain unchanged.

This policy decision alone did not claim Iteration or Loop execution and
introduced no A3S Flow change. Decisions 0016 through 0018 subsequently add
frames, exports, deterministic result ordering, and Flow-backed region
dispatch through existing hooks, child-operation links, and ordinary
WorkflowRuns. They add no Cloud-local scheduler, queue, retry engine, worker,
event history, or second child lifecycle.
