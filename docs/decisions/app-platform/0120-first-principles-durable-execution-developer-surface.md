# 0120: First-Principles Durable Execution Developer Surface

Status: Accepted

## Context

A3S Cloud is an Agent-first, Flow-native, multi-tenant control plane. Product
intent lives in PostgreSQL owner aggregates; A3S Flow plus Operations remain
the only durable orchestration path; Gateway is the only public data plane;
Identity owns tenancy and authorization. That mission is not the same as a
code-as-workflow durable microservice runtime.

External durable-execution products (notably Restate) demonstrate useful
*outcomes*: developer-visible step journals, key-scoped serialized stateful
entities, explicit durable wait/awake points, harness-agnostic agent
durability, and single-invocation operations timelines. Those outcomes are
references, not authorities. Importing a second journal, scheduler, retry
daemon, timer queue, or code-as-workflow runtime would violate ADR
[0001](0001-flow-preservation.md) and the single-authority map in
`docs/architecture.md`.

First-principles review of the Restate comparison passes Cloud's feature gate
and yields seven architecture decisions (D1-D7) that strengthen Cloud without
changing its core mission:

1. Does it serve the mission? Yes — operators and product surfaces need one
   recoverable story for long-running AI work.
2. Does it strengthen architecture? Yes — only if projections reuse Flow,
   Operations, Agents, Applications, Connectors, and Durable Cells owners.
3. Is the problem real? Yes — durable facts exist but are fragmented across
   Flow history, Agent sequences, Application observation, Connector waits,
   HumanTasks, and node command journals.
4. Simpler alternative? Document and compose existing owners; do not clone
   Restate Server.

## Decision

### D1. Preserve the control-plane durable spine

No Cloud change may introduce a product-local durable execution engine,
invocation journal store, scheduler, retry loop, Hook store, or code-as-
workflow runtime. Flow remains the only durable orchestration authority.
Operations remains the idempotent long-running coordination facade. Product
contexts own semantic state and rebuildable projections only.

### D2. One developer-visible durable step timeline (projection)

Every long-running product invocation (Application invocation, WorkflowRun,
AgentExecution, and later Delivery Pipeline run) MUST be able to project an
authorized, ordered **durable step timeline** for operators and management
interfaces. The timeline is a read model composed from owner facts:

| Timeline facet | Authority |
| --- | --- |
| Orchestration steps, waits, retries, child linkage | A3S Flow history via Operations/Workflow ports |
| Semantic messages, variables, terminal observations | Applications or Agents owning sequences |
| Human approval / HumanTask | Agents or Workflow HumanTask owners |
| Connector attempt / wait / compensation | Connectors + Flow-owned wait interpretation |
| Placement / Runtime / Box evidence | Workloads, Fleet, Runtime receipts — never as semantic transcript |

The timeline MUST NOT become a second write history. Exact replay continues to
recover from owner aggregates and Flow bytes. Management MCP, CLI, and REST
may expose the same projection; a Dashboard is optional and non-authoritative.

### D3. One durable wait / awake contract

External completion (human approval, webhook, connector deferred result,
application answer, durable timer already owned by Flow) MUST share one
cross-cutting **durable wait handle** vocabulary:

- create/observe/complete/cancel through the semantic owner;
- Flow owns durable wait scheduling, cancellation, and coordinator replacement
  semantics already proven for Connectors and Workflow;
- product owners MUST NOT invent parallel sleep queues or wake tables.

New wait kinds extend the existing Flow wait/Hook model or an owner port that
Flow already observes. Awakeables-style DX is a Cloud projection over those
facts, not a Restate import.

### D4. Durable Cell as the key-scoped serialized state primitive

Durable Cell remains the Cloud outcome analogous to Restate Virtual Objects /
Deno Durable Objects: named key, single-writer serialization, provider-owned
SQLite state, hibernation, alarms, and fenced handoff. Architecture language
MUST treat Cell as that primitive for human/Agent rooms and multi-Agent
blackboards. Cell MUST NOT absorb WorkflowRun history, AgentExecution
transcripts, or Application session truth.

### D5. Harness under durability for Agents

Agents own one provider-neutral execution, event sequence, approval,
checkpoint, and recovery contract. Any Harness (A3S Code or external) runs
*under* that durability contract. A Harness MUST NOT own a private durable
scheduler, transcript store, or approval authority. This preserves Restate's
useful agent-runtime-under-the-SDK outcome without making Restate the agent
platform.

### D6. Side effects require stable effect identity

Connector calls, Tool invocations, finite Executions, and Application semantic
effects MUST retain stable effect identities and ambiguous-commit recovery
already required by Applications and Workflow ADRs. New custom nodes and
Automations outbound attempts MUST NOT perform non-journaled side effects
outside an owner port that can prove exact-once or explicit indeterminate
resolution.

### D7. Single invocation correlation for operations

Management and observation surfaces MUST correlate one stable product
invocation identity to the durable step timeline, session/message cursor,
blocking/streaming/async observation, and audit metadata. Gateway request IDs
remain transport correlation only; they do not replace product invocation
identity.

## Non-goals

- Replacing ACL/Workflow graph authorship with code-as-workflow as the primary
  product model.
- Embedding Restate Server, Temporal, or equivalent as Cloud's orchestration
  spine.
- Moving per-Cell SQLite state into PostgreSQL or Gateway.
- Collapsing Identity, Edge/Gateway, Workloads, or Fleet into a durable runtime.

## Consequences

- `docs/architecture.md` records Restate as an external reference outcome and
  names the durable execution developer surface under existing owners.
- Future APP0 / W0 / A1 / CELL0 / AUT0 slices that expose timelines, wait
  handles, or Cell DX MUST cite this decision and ADR 0001.
- Implementation remains gate-driven; accepting this ADR does not claim
  product availability.
- Fitness: any PR that adds a second durable journal, scheduler, or retry
  daemon fails closed against this decision and ADR 0001.
