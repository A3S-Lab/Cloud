# Coordination and Data-Plane Project Roadmaps

This group turns already-admitted intent into durable coordination, bounded
dispatch, external traffic, model execution, and object bytes. The central
rule is that a data plane consumes immutable desired-state projections and
publishes observations; it does not silently become a management plane.

## A3S Flow

**Mission:** provide deterministic workflow compilation, append-only history,
durable replay, and worker-independent coordination for steps, timers,
signals, callbacks, approvals, and child workflows.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `FLOW-R1` | Stabilize the generic workflow document, node/edge typing, deterministic compiler, immutable runtime build identity, history schema, and patch markers | Invalid graphs fail before start; replay under the same build is byte-stable; incompatible code is rejected visibly |
| `FLOW-R2` | Complete expected-sequence append, activities, durable timers, signals, callbacks, cancellation, compensation markers, child workflows, continue-as-new, and bounded history | Concurrent append, lost response, worker replacement, timer recovery, parent/child crash windows, and cancellation races pass |
| `FLOW-R3` | Publish worker leasing, task routing, history archival, search attributes, execution diagnostics, metrics, and operational repair contracts | A worker can disappear after any transition without losing an acknowledged decision or repeating a committed one |
| `FLOW-R4` | Qualify Cloud node adapters for Agent, Function, Inference, Connector, HumanTask, Durable Cell, and nested Workflow through owner ports | A heterogeneous Flow preserves one WorkflowRun history while every side effect retains its own product operation identity |

Flow owns generic graph and replay semantics. The Cloud Workflow bounded
context owns tenant Workflow assets, revisions, publish policy, node catalog
bindings, authorization, credentials, quotas, and product outcomes. Flow does
not invoke Cloud repositories directly and Cloud does not duplicate Flow
history or timers.

## A3S Lane

**Mission:** enforce priority, concurrency, pressure, and fairness for work
that already has a durable owner record.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `LANE-R1` | Stabilize typed lane definitions, priority ordering, concurrency permits, deadlines, cancellation, drain, pressure signals, and deterministic metrics | Admission order and active limits hold under contention; cancellation settles permits and never leaves phantom capacity |
| `LANE-R2` | Qualify Redis-backed multi-process dispatch with ownership tokens, visibility, bounded redelivery, duplicate delivery, and queue reconstruction | Redis loss and queue deletion rebuild from owner records; stale workers cannot settle another lease |
| `LANE-R3` | Add hierarchical project/class fairness adapters and capacity feedback for management commands, FaaS, Agents, Workflow activities, builds, inference, and telemetry | Hard quota remains in the owner transaction; Lane provides no path around a denied durable admission |
| `LANE-R4` | Publish operational lag, wait, saturation, shed, and drain contracts plus overload conformance | Operators can distinguish durable backlog from dispatch lag and can replace workers without losing owner truth |

Lane does not own Workflow history, product retries, schedules, DLQ business
decisions, idempotency truth, desired replicas, hard quotas, or Runtime logs.
Its optional job, retry, repeat, Flow, and log helpers are library features, not
parallel Cloud authorities.

## A3S Gateway

**Mission:** be the only externally reachable request plane for Cloud APIs,
Agent/Function/Workflow/Cell endpoints, inference, Git/OCI/Use/model supply,
object delivery, and tenant static Web applications.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `GATEWAY-R1` | Stabilize listener, host/path/protocol match, TLS, authentication, authorization-input, target, timeout, body, streaming, and error-policy snapshots | A complete snapshot is accepted atomically; partial or incompatible snapshots never serve |
| `GATEWAY-R2` | Complete healthy-generation target sets, weighted rollout, drain, retry safety, circuit breaking, connection limits, request/body bounds, and protocol adapters | Stale or fenced endpoints receive no new traffic; non-replayable requests are never retried implicitly |
| `GATEWAY-R3` | Implement hierarchical rate shaping, safe response/metadata caching, cache invalidation epochs, static-object range/conditional serving, SPA fallback, and origin protection | Hard quota remains Cloud-owned; cache keys include tenant, authorization, release, vary inputs, and policy epoch; private objects cannot leak across principals |
| `GATEWAY-R4` | Qualify Agent streaming, FaaS invocation, modern sessionless MCP, OpenAI-compatible inference, Git/OCI protocols, Registry/object downloads, and Web delivery | Protocol-specific conformance, overload, disconnect, timeout, and accounting tests pass through the same edge |
| `GATEWAY-R5` | Add multi-region ingress snapshots, locality-aware failover, usage delivery, abuse telemetry, certificate automation, and zero-downtime upgrades | Region loss follows declared consistency and retry semantics; usage events reconcile to accepted requests |

Gateway never creates workloads, chooses desired replica counts, stores tenant
business state, evaluates model-provider inventory, publishes a target that
Cloud has not admitted, or becomes a second admin API. Direct public access to
internal Cloud processes or provider services is prohibited.

## A3S Power

**Mission:** execute model inference efficiently on selected CPU/GPU resources,
including distributed serving mechanisms inspired by llm-d, while remaining
separate from model governance and cluster placement.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `POWER-R1` | Complete bounded local model load, tokenizer/tensor contracts, batching, deadlines, cancellation, memory accounting, health, and usage evidence | Shape, resource, cancellation, leak, and confidential performance suites pass on declared devices |
| `POWER-R2` | Stabilize replica and worker-group contracts for tensor, pipeline, data, expert, and prefill/decode disaggregation | Exact model/weight/runtime revisions bind every worker; partial groups remain unhealthy and cannot receive traffic |
| `POWER-R3` | Add KV-cache ownership and transfer, prefix-cache evidence, RDMA-capable transport negotiation, failure fencing, and topology-aware health | Worker loss or stale KV ownership cannot corrupt another request; transport fallback never violates the requested capability |
| `POWER-R4` | Publish queue/cache/load observations and a versioned routing input contract for Gateway without embedding global routing policy | Cloud/Gateway can choose from immutable healthy sets; Power reports facts and never overrides tenant/model policy |
| `POWER-R5` | Qualify rolling model revision changes, mixed accelerators, scale-out/in, checkpoint/cache cleanup, observability, and confidential execution | Long-running load, chaos, upgrade, billing-reconciliation, and final GPU-memory/resource cleanup pass |

Power does not own logical Models, external provider accounts, weight licenses,
tenant grants, ModelScope resolution, placement, quotas, autoscaling, public
routing, or request authorization. Cloud Model Supply, Inference, Fleet, and
Gateway retain those authorities.

## RustFS / S3-compatible object provider

**Mission:** serve as a qualified, replaceable S3-compatible byte store for
artifacts, files, model weights, checkpoints, logs, and static Web releases.
RustFS is an external provider dependency, not an A3S bounded context.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `OBJECT-R1` | Freeze the Cloud object-provider port around immutable put/get/head/range, multipart upload, conditional writes, delete markers, checksums, version identity, and presigned access | Provider contract passes against RustFS and at least one independent S3-compatible implementation |
| `OBJECT-R2` | Qualify encryption, replication/erasure behavior, lifecycle rules, capacity/health, backup, restore, and disaster-recovery procedures | Corruption, partial multipart upload, node loss, credential rotation, restore, and inventory reconciliation pass |
| `OBJECT-R3` | Add bounded performance profiles for weights, logs, artifacts, files, and static assets with cache-friendly metadata | Large weights, small objects, range downloads, concurrent uploads, and retention sweeps meet declared SLOs |

Cloud owns object namespaces, tenant authorization, immutable manifests,
reference counts, retention holds, legal/audit policy, and deletion operations.
Callers never receive provider-wide credentials, and no product depends on a
RustFS-private API.

## Integration exit

This group is ready when:

- a Workflow can replay after all workers are replaced while its external
  side effects are protected by owner idempotency records;
- deleting Redis or replacing Lane workers loses no acknowledged product work;
- Gateway routes only a complete, versioned, admitted snapshot and is the sole
  public path;
- a distributed Power deployment is fenced by exact model, weights, runtime,
  topology, and worker generations; and
- object-provider loss and restore preserve manifest-to-byte integrity without
  making the provider database a product authority.

