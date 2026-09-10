# Coordination and Data-Plane Project Roadmaps

**Aligned with Cloud execution baseline: 2026-09-10.**

This group turns already-admitted intent into durable coordination, bounded
dispatch, external traffic, model execution, and object bytes. The central
rule is that a data plane consumes immutable desired-state projections and
publishes observations; it does not silently become a management plane.

Cloud Wave mapping:

| Local outcomes | Cloud wave / gates |
| --- | --- |
| `FLOW-R*` | Wave 2 `CD0`; Wave 3 `W0` / Operations |
| `LANE-R*` | Wave 2–3 pressure after durable admission |
| `GATEWAY-R*` | Wave 1–3 publish; `I0.2b`+; `WEB0` |
| `POWER-R*` | Wave 1 `PW0` then I0 Track B |
| Object provider | `S0` / Cell / WEB / model supply |

## A3S Flow

**Mission:** provide deterministic workflow compilation, append-only history,
durable replay, and worker-independent coordination for steps, timers,
signals, callbacks, approvals, and child workflows.

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `FLOW-R1` | Stabilize the generic workflow document, node/edge typing, deterministic compiler, versioned event envelope, immutable runtime-build identity, patch markers, payload limits, and store capability profile | Invalid graphs and incompatible stores fail before start; supported histories replay byte-stably; protocol fixtures and negative cases pass |
| `FLOW-R2` | Complete the Activity/Step side-effect contract: expected-sequence append, attempt identity, idempotency, fencing, heartbeats, checkpoints, timers, signals, callbacks, cancellation, compensation markers, child workflows, and continue-as-new | Worker/process loss, response loss, duplicate delivery, unknown outcomes, parent/child crash windows, and cancellation races converge to one durable owner result |
| `FLOW-R3` | Scale durable state and dispatch with snapshots, incremental projections, history archival, worker leasing, task routing, backpressure hooks, visibility projections, diagnostics, metrics, and repair contracts | Append/replay cost stays bounded as history grows; queue loss and worker replacement are reconstructible; stale leases cannot commit |
| `FLOW-R4` | Provide typed Query/Update, structured concurrency, dynamic bounded fan-out, versioned worker protocol, telemetry context, and Cloud adapter certification for Agent, Function, Inference, Connector, HumanTask, Durable Cell, and nested Workflow | Mixed-version workers and heterogeneous adapters preserve one Flow history while each side effect retains its own Cloud operation identity |
| `FLOW-R5` | Maintain cross-language fixtures, replay/chaos/load conformance, migration tooling, release automation, and exact-revision compatibility evidence | Real-provider failure, upgrade, security, cleanup, package, and Cloud integration gates pass before a public kernel claim |

Flow owns generic graph and replay semantics. The Cloud Workflow bounded
context owns tenant Workflow assets, revisions, publish policy, node catalog
bindings, authorization, credentials, quotas, and product outcomes. Flow does
not invoke Cloud repositories directly and Cloud does not duplicate Flow
history or timers.

The Cloud-owned execution integration is tracked in the
[Flow execution-integration roadmap](../flow-execution-integration-roadmap.md).
Recurring schedules, calendar/time-zone policy, visibility/search, pause/reset/
redrive APIs, Lane admission, Workloads/Fleet placement, worker fleet rollout,
and regional recovery remain Cloud capabilities; Flow supplies only the
durable primitives and versioned protocols needed to implement them.

### Cloud obligations

| Priority | Obligation | Forbidden |
| --- | --- | --- |
| `CD0` | Durable stage history and receipts for Delivery Pipelines (source→build→release→promote→rollback) | Parallel Cloud updater or second workflow engine |
| `W0` / Operations | Worker-independent replay for product adapters Cloud owns | Flow history as Agent transcript or product retry tables as truth |

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

**Cloud obligation:** post-commit fairness for CD0/workers/pressure only.
Redis/Lane never become quota or desired-state truth.

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

### Cloud obligations (Wave 1–3)

| Priority | Obligation | Forbidden |
| --- | --- | --- |
| `H0.2`+ | Exact Cloud-managed snapshot apply/ACK, recovery, certificate/target replacement | Partial snapshots; Cloud request-byte proxy |
| `I0.2b`+ | OpenAI-compatible dispatch, auth denial, fallback, streaming with **real** Power workers | Inventing workers when Edge snapshots omit them; storing bearers |
| `WEB0` | Read-only static-object target for immutable Web releases | Becoming east-west mesh control or a Dashboard backend |
| Dual-track `I0` | Keep fail-closed empty workers until `PW0` observation delivery | Marketing inference “available” on Track A alone |

Local roadmap detail: [Gateway ROADMAP](https://github.com/A3S-Lab/Gateway/blob/main/ROADMAP.md).

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

### Cloud `PW0` obligations (Wave 1, after `BX0`)

Power is an ordinary **Box-hosted Runtime Service**, not a control plane.
`compat/cloud-stack.acl` remains unbound for Power until `PW0` lands.

| Priority | Obligation | Forbidden |
| --- | --- | --- |
| `PW0` | ACL-native immutable Power Service profile; MicroVM/TEE evidence; health; inference; recovery; cleanup | Second scheduler, node channel, or desired-state store |
| Observation delivery | Versioned worker capability/observation facts Gateway and Cloud Edge can bind | Asking Cloud to invent `workers` ACL blocks or `InferenceDeployment` aggregates |
| Lock entry | Pin into `compat/cloud-stack.acl` with Gateway/Cloud revisions together | Claiming I0 data-plane availability without Box+Gateway Verified evidence |

Local roadmap detail: [Power ROADMAP](https://github.com/A3S-Lab/Power/blob/main/ROADMAP.md).

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
  topology, and worker generations **after** `PW0` observation delivery; empty
  workers remain fail-closed until then; and
- object-provider loss and restore preserve manifest-to-byte integrity without
  making the provider database a product authority.
