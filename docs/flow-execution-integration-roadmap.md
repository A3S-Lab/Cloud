# A3S Cloud and A3S Flow Execution-Integration Roadmap

**Planning baseline: 2026-09-05.**

This document plans the Cloud work that consumes the A3S Flow execution
kernel. It does not redefine Flow history, replay, timers, retries, or worker
leases. The Flow repository owns those generic contracts in its
[execution-kernel roadmap](https://github.com/A3S-Lab/Flow/blob/main/docs/ROADMAP.md).
The [Cloud product roadmap](../ROADMAP.md), [workflow and evolution plan](workflow-evolution-plan.md),
and [technical architecture](architecture.md) remain authoritative for Cloud
product outcomes and bounded-context ownership.

## 1. Boundary

### A3S Flow owns

- deterministic workflow decisions and replay;
- append-only execution history and projections;
- durable Activity/Step delivery, attempts, fencing, heartbeats, retries, and
  result commitment;
- timers, signals, callbacks, cancellation, child workflows, and
  continuation chains;
- runtime-build and protocol compatibility;
- generic worker, store, payload, and event contracts; and
- execution telemetry context and conformance evidence.

### A3S Cloud owns

- tenants, namespaces, projects, identity, authorization, quotas, billing, and
  secret policy;
- Workflow assets, revisions, ACL schemas, node descriptors, capability
  bindings, publication, and product semantics;
- Operations, transactional Outbox, audit policy, Search/visibility indexes,
  REST/OpenAPI, CLI, Management MCP, and the user-facing control plane;
- recurring schedules, calendars, time zones, jitter, overlap, catch-up,
  backfill, and bulk lifecycle operations;
- Workloads/Fleet placement, Lane admission, worker fleet deployment,
  autoscaling, regional failover, backup/restore, and SLO policy; and
- Agent, Function, Inference, Connector, HumanTask, Durable Cell, and other
  product adapters.

Cloud must not create a second workflow history, scheduler, retry daemon,
activity queue, or replay engine. It persists product intent and semantic
state, then delegates execution to Flow through public ports and immutable
facts.

## 2. Integration principles

1. **Authorize before replay.** Cloud resolves tenant and product policy before
   submitting a Flow command. Flow then enforces deterministic and durability
   invariants; it does not evaluate Cloud authorization.
2. **One owner per fact.** Cloud PostgreSQL owns product desired state and
   Operations. Flow owns execution history. Lane owns post-admission pressure,
   and Workloads/Fleet own placement and capacity.
3. **Intent before dispatch.** A Cloud transaction commits intent, idempotency,
   and Outbox facts before Flow or Lane dispatch. Lost dispatch acknowledgements
   are repaired from those facts.
4. **Opaque product metadata.** Flow may carry a bounded opaque tenant scope,
   revision, trace, and correlation identity. It must not interpret tenant
   roles, credentials, node catalogs, or model policy.
5. **Unknown is not success.** Provider process exit, telemetry, or broker
   delivery cannot close a Cloud operation without the authoritative owner
   receipt. Ambiguous Activity attempts become Flow `unknown` and remain
   suspended until Cloud reconciles them. Cloud must persist the Flow
   `attempt_id` and `idempotency_key` on its Outbox receipt, and accept a
   result only with the current `fencing_token` after any
   `activity_lease_acquired` redelivery event.
6. **ACL remains canonical.** Cloud parses and emits product configuration only
   through `a3s-acl`; it constructs Flow DAG inputs programmatically and does
   not add a second graph parser.

## 3. Ordered Cloud slices

The slices are dependency-ordered. They are gates, not calendar promises.

| Slice | Cloud outcome | Flow dependency | Exit evidence |
| --- | --- | --- | --- |
| `CFLOW-0` Contract lock | Pin the exact Flow revision, protocol versions, event/command fixtures, identity fields, error classes, and compatibility policy in `compat/cloud-stack.acl` | `FLOW-R1` | Clean revision bundle passes ACL, Flow, Cloud, ORM, Event, Boot, and integration fixture checks; no private Flow import or duplicate state owner |
| `CFLOW-1` Operation-to-Activity bridge | Map Cloud `Operation` and owner Outbox facts to Flow Activity IDs, attempt IDs, idempotency keys, deadlines, cancellation, and reconciliation | `FLOW-R2` | Kill/restart after dispatch, provider success, response loss, and lease expiry; exactly one owner receipt closes the operation |
| `CFLOW-2` Product adapters | Bind Workflow, Agent, Function, Inference, Connector, HumanTask, Durable Cell, and nested Workflow steps through owner Application ports | `FLOW-R2`, `W0`, `A1`, `FN0`, `I0`, `CELL0` | Each adapter preserves one Cloud semantic identity plus one Flow execution identity; no product context writes Flow history directly |
| `CFLOW-3` Visibility and operations | Build tenant-authorized projections for current state, history summaries, attempts, suspensions, traces, audit, and reconciliation; expose pause/resume/cancel/terminate/reset/redrive through Cloud control APIs | `FLOW-R3`, `FLOW-R5` | Queries use rebuildable projections; every mutation is authorized, idempotent, audited, and safe under repeated requests |
| `CFLOW-4` Scheduling and pressure | Implement recurring schedules, calendar/time-zone policy, overlap/catch-up/backfill, Lane admission, quotas, backpressure, and Workloads/Fleet placement | Flow timers/retries only; Cloud schedule and Lane contracts | Missed schedules, overlapping runs, dependency outage, queue pressure, and worker drain follow declared policy without changing Flow semantics |
| `CFLOW-5` Versioned delivery | Govern Workflow/Agent/Function revisions, runtime-build pinning, canary rollout, old-build drain, rollback, and compatibility reports | `FLOW-R1`, `FLOW-R5` | New runs and retained runs route to compatible builds; old builds are retired only after reachability and replay evidence |
| `CFLOW-6` Recovery and regional operation | Compose backup/restore, retention, disaster recovery, multi-replica reconciliation, regional routing, and incident runbooks | `FLOW-R3`, `FLOW-R6`, `S0`, `H0` | Database, queue, Flow worker, provider, and region failures meet declared RTO/RPO; no acknowledged operation is lost |
| `CFLOW-7` Public platform release | Publish REST/OpenAPI, TypeScript client, CLI, Management MCP, UI, usage, SLO dashboards, and support runbooks over the same contracts | All preceding slices | A clean tenant-scoped environment can create, run, inspect, pause, recover, upgrade, and remove a Workflow without direct database access |

## 4. Shared contract

Cloud and Flow exchange one bounded execution identity:

```text
tenant_scope (opaque to Flow)
workflow_type / workflow_revision / plan_digest
workflow_id / run_id / execution_chain_id
runtime_build_id / protocol_version
operation_id / activity_id / attempt_id / idempotency_key
trace_id / span_id
```

Cloud owns the meaning and authorization of product fields. Flow validates
identity shape, event ordering, deterministic replay, fencing, deadlines, and
durability. Any new field must be versioned and added to the exact-revision
compatibility fixture.

The current Flow Activity and projection-cache contract is published by Flow
`main` at revision
`cb50056b58f35eb10aad841288548e24730b06b5` (durable per-attempt deadlines,
fenced and idempotent unknown-outcome reconciliation, tip-validated
disposable projection checkpoints, durable dead-letter redrive, and bounded
worker drain fairness budgets, versioned worker capability negotiation, and
bounded history export pages, plus stable attempt correlation in event
bridges).
This revision also provides safe CLI/Skill CRUD for local workflow DSL files,
including bounded NDJSON operation streams and optimistic base-digest checks.
That stream is an authoring transport: Flow still publishes a portable,
validated snapshot for the local file. For hosted collaboration, Cloud should
persist the ordered operation journal and materialized snapshots in its own
tenant-authorized stores, then hand validated documents to Flow; Cloud remains
responsible for hosted publication, authorization, asset lifecycle, conflict
resolution, and multi-tenant platform behavior.
The same revision must be recorded in the Cloud compatibility lock before a
release bundle is published. Cloud integration code must
consume the public `ScheduleActivity`, `ActivityInvocation`,
`ActivitySnapshot`, `ActivityResolution`, `heartbeat_activity`, and
`resolve_unknown_activity` APIs only; it must not import Flow internals or
duplicate the Activity projection.

When a Cloud operation supplies `timeout_ms`, Flow persists the value with the
Activity definition and derives the attempt deadline from the durable
`activity_started` timestamp. A timeout is an unknown provider outcome, not a
successful cancellation or an implicit retry; Cloud must reconcile it using
the recorded attempt and idempotency identities.

Cloud visibility and operations may request a Flow projection checkpoint after
an execution transition. The checkpoint is an acceleration cache only: Flow
accepts it when `run_id`, `last_sequence`, `last_event_id`, and the snapshot
SHA-256 digest validate against the history tip, and otherwise replays the
authoritative event stream (or only its indexed tail). Cloud must not copy
or mutate checkpoint snapshots as a second execution history; its own
tenant-authorized visibility projection remains rebuildable from Flow facts.
For archive/export and visibility rebuilds, Cloud should consume
`FlowEngine::history_page` with its exclusive sequence cursor rather than
loading an unbounded history into memory.

## 5. Non-duplication checklist

Before accepting a Cloud change, verify:

- no Workflow, Agent, Evaluation, Build, or Deployment module creates a retry
  table, sleep loop, scheduler, or second task queue;
- no Cloud repository stores a copy of Flow event history as business truth;
- no Flow runtime imports Cloud repositories or product DTOs;
- no telemetry, process exit, or broker acknowledgement is treated as an
  owner receipt;
- no tenant credential or Secret material is persisted in Flow payloads;
- no schedule evaluator is added to Flow for Cloud's recurring schedule
  product; and
- every projection declares its source, rebuild procedure, retention, and
  authorization boundary.

## 6. Integration release gates

The Cloud release can mark a Flow-backed capability verified only when all of
the following pass on the exact revision bundle:

1. **Ownership:** one writer exists for each product and execution fact.
2. **Authorization:** tenant and system-admin decisions occur before replay or
   external dispatch.
3. **Durability:** process death, response loss, duplicate delivery, provider
   loss, and database reconnect converge without lost acknowledged work.
4. **Compatibility:** retained Flow histories, Cloud revisions, migrations,
   clients, and workers pass mixed-version replay.
5. **Security:** payloads are bounded, secrets are referenced rather than
   copied, and audit records are tenant-scoped and redacted.
6. **Operations:** visibility, traces, metrics, reconciliation, SLOs,
   runbooks, backup/restore, and cleanup evidence are present.
7. **User outcome:** the public API, client, CLI, Management MCP, and UI use
   the same authorized projections and idempotency contract.

The Cloud platform is ready to claim a Workflow outcome only after `CFLOW-7`
passes. A green Flow kernel test alone is not a hosted Cloud availability
claim; conversely, a Cloud mock must not weaken Flow's durable execution gates.
