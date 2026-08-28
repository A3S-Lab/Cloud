# Elastic Service Deployment Architecture

## 1. Decision

A3S Cloud has one tenant-workload reconciliation path and one placement
authority:

```text
product intent
  -> Workload revision + control policy
  -> Workloads scaling decision
  -> Fleet inventory + Resource Claims
  -> A3S Runtime Task/Service
  -> A3S Box
  -> exact observation
  -> A3S Gateway target snapshot
```

Stateless Services, stateful Agent Services, Durable Cell providers, finite
Functions, Workflow work, and CPU/GPU model workloads do not get independent
schedulers, replica stores, node queues, or Runtime classes. They share the
same lifecycle primitives and differ only where state, traffic, and recovery
invariants are genuinely different.

The current replica, placement-generation, Claim, evacuation, retirement,
writer-fence, Runtime observation, and complete Gateway snapshot foundations
remain authoritative. The autoscaling and stateful-movement contracts in this
document extend those foundations under `H0.5`; they are not yet a claim of
production availability.

## 2. First-principles model

Elastic deployment is not "start or kill more containers." It is a controlled
change to durable intent under four irreducible facts:

1. **Demand**: accepted work, queued work, active sessions, request concurrency,
   or model demand that must be served within a stated objective.
2. **Capacity**: committed CPU, memory, storage, port, accelerator, topology,
   and isolation Claims, not optimistic metric estimates.
3. **State ownership**: the exact authority that can prove whether a process is
   disposable, checkpointed, single-writer, or quorum-replicated.
4. **Transition cost and safety**: cold-start time, checkpoint/restore time,
   image/model loading, connection drain, writer fencing, and availability
   budget.

Every scaling decision must therefore bind all of the following:

- tenant, project, environment, Workload, revision, and managed owner;
- current Workload-control version and placement-policy generation;
- immutable scaling-policy revision and state-safety profile digest;
- bounded signal window and source digests;
- previous and requested desired capacity;
- quota and resource-admission result;
- reason, decision time, cooldown horizon, and decision identity; and
- eventual replica, Claim, Runtime, target, drain, fence, and cleanup evidence.

Metrics are evidence. The accepted scaling decision and resulting Workload
control generation are desired-state truth.

## 3. Canonical abstractions

Only the following generic concepts may cross product boundaries:

| Concept | Meaning | Owner |
| --- | --- | --- |
| `WorkloadRevision` | Immutable executable, resources, endpoints, health, Secrets, and deployment safety input | Workloads |
| `WorkloadControl` | Current managed owner, placement shape, desired replicas, and generation-fenced mutation authority | Workloads |
| `ScalingPolicyRevision` | Immutable min/max/targets/windows/rates/zero policy and state-compatible constraints | Workloads |
| `ScalingSignalWindow` | Bounded, source-attributed demand/capacity summary; never desired state | Observability source through a Workloads port |
| `ScalingDecision` | Idempotent evaluated mutation against one exact control/policy/signal fence | Workloads |
| `WorkloadReplica` | Stable logical ordinal whose incarnation is generation-bound | Workloads |
| `PlacementGroup` | All-or-none multi-member replica topology | Workloads |
| `ResourceClaim` | Committed resource allocation truth | Workloads + Fleet transaction boundary |
| `RuntimeUnit` | Generic finite `Task` or long-running `Service` | A3S Runtime |
| `DrainLease` | Admission closure plus a bounded deadline for in-flight work or sessions | Workloads; product adapter supplies readiness evidence |
| `WriterFenceReceipt` | Proof that a prior writer generation cannot acknowledge new mutations | Workloads with state-provider evidence |
| `TrafficTargetSet` | Complete set of healthy, exact-generation public targets | Edge/Gateway |
| `CapacityIntent` | Required node-pool capacity produced from pending Claims and safety buffers | Future Compute owner; never a placement decision |

`ScalingPolicyRevision`, `ScalingDecision`, and `DrainLease` are planned names
for `H0.5`; implementation must extend the existing Workloads aggregate and
repositories instead of creating an Autoscaling bounded context.

Product contexts contribute only immutable intent or product-specific proof:

- Agents supplies session/checkpoint/recovery readiness;
- Workflow supplies queued runnable work and deadline classes;
- Function release/invocation supplies Task versus Service mode and cold-start
  objective;
- Durable Cells supplies shard, writer epoch, seal, restore, and replication
  proof;
- Inference supplies model identity, device/topology intent, load cost, and
  serving readiness; and
- Edge/Gateway supplies bounded traffic demand and drain observations.

None of them mutates desired replicas or Claims directly.

## 4. Deployment profiles

The profile is a closed combination of existing Runtime class plus explicit
state and traffic semantics. It is not a new Runtime class.

| Profile | Runtime projection | Scaling unit | Safe scale-down prerequisite | Typical consumers |
| --- | --- | --- | --- | --- |
| Finite task | `Task` | Accepted invocation/execution concurrency | Task terminal or explicitly cancelled with evidence | Workflow activity, finite Function, build, evaluation |
| Stateless service | `Service` | Interchangeable replica | Remove target, drain bounded requests, stop/remove | HTTP FaaS, sessionless MCP, SSR/BFF, APIs |
| Checkpointed session service | `Service` | Warm replica plus bound sessions | Stop admission, checkpoint every bound session, verify, then retire | A3S Code and other stateful Agents |
| Single-writer partition service | `Service` | Independent state partition/provider shard | Quiesce, replicate/seal, fence old writer, restore, then publish new writer | Durable Cell provider, explicitly adapted state service |
| Replicated state service | `Service` or placement group | Provider-defined member set | Provider confirms quorum-safe membership change and backup posture | Future managed databases or consensus services |
| Gang model service | Placement group of `Service` units | Complete model replica | Remove complete target, drain, stop all members, release all Claims | Distributed GPU inference |

Static Web releases are not Services. They are immutable objects served by
Gateway and scale through Gateway/cache capacity. A server-rendered Web
application is an ordinary stateless or explicitly stateful Service.

## 5. One reconciliation state machine

All Service profiles use the same outer state machine:

```text
desired
  -> placement planned
  -> Claims prepared/committed
  -> Runtime applied
  -> exact healthy observation admitted
  -> target eligible
  -> serving
  -> admission closed
  -> drained or product-safe
  -> target removed
  -> Runtime fenced/stopped/removed
  -> Claims released
  -> retired
```

The ordering of `product-safe`, target removal, and Runtime fence is selected
by the immutable state-safety profile:

- stateless replicas remove new traffic, drain, then stop;
- checkpointed sessions close new admission, checkpoint and verify, remove the
  target/binding, then stop;
- single-writer state removes or freezes write admission, quiesces, seals,
  fences the prior epoch, restores the successor, and only then publishes it;
- a gang target is admitted or removed as a complete placement group; and
- a failed safety step holds the replica in a visible blocked state. It never
  falls back to a forceful state-losing move.

Node maintenance, deployment rollout, manual scale, autoscale, and failure
recovery all request transitions through this state machine. They cannot each
implement their own stop/move algorithm.

## 6. Stateless Service elasticity

### 6.1 Scale up

1. The scaler evaluates one fresh signal window against one policy and control
   generation.
2. Workloads persists one idempotent decision and advances desired replicas.
3. Stable replica ordinals are created or reactivated; placement reserves
   Claims from one fenced Fleet inventory.
4. Runtime applies the exact Service specification through Box.
5. Only exact-generation, semantically converged, healthy endpoints enter the
   complete Gateway target snapshot.
6. The decision completes only when desired ready capacity is observed or a
   bounded terminal reason is recorded.

### 6.2 Scale down

Candidate selection is deterministic and safety-aware: non-ready and
non-serving replicas first, then the highest removable ordinals, constrained
by failure-domain spread, rollout state, active requests, and minimum ready
capacity. Workloads marks retirement before Edge removes the target. Gateway
then stops new admission, drains connections until zero or the declared
deadline, acknowledges the exact target generation, and only then permits
Runtime stop/remove and Claim release.

An expired drain does not silently terminate a request. The policy must select
one declared outcome: cancel with protocol evidence, extend within a bound, or
fail the scale-down decision and retain capacity.

### 6.3 Rollout

Stateless rollout uses the existing immutable Deployment generation and a
bounded `max_surge`/`max_unavailable` policy. A new replica becomes traffic
eligible before the corresponding old replica retires unless the explicit
availability budget permits otherwise. Canary and blue/green are target-set
selection policies over the same revisions and replicas, not separate
deployment mechanisms.

## 7. Stateful Agent Service elasticity

A stateful Agent has three different kinds of state and must not conflate them:

- durable conversation, approval, checkpoint lineage, and semantic events are
  owned by the Agent/Flow authorities in PostgreSQL and shared objects;
- mutable workspace state is owned by its declared checkpoint/storage profile;
  and
- the Runtime Service process, local caches, sockets, and credentials are a
  disposable execution incarnation once recovery evidence exists.

A warm Agent replica may host zero or more bounded sessions. Each session has
one exact binding to the Workload revision, replica/generation, Runtime unit,
provider run, checkpoint lineage, and lease. New sessions can be balanced only
among ready, non-draining replicas with compatible release/profile and enough
committed capacity. Existing sessions remain sticky to their binding until an
explicit recovery transition rotates it.

Safe retirement is:

1. mark the replica draining and reject new session leases;
2. enumerate the exact bounded session set at one binding fence;
3. wait for an idle boundary or request provider-neutral suspension;
4. persist and verify each required checkpoint/workspace digest in shared
   storage;
5. close or rotate the old session leases and remove the old target/binding;
6. stop/remove the Runtime Service and release Claims; and
7. lazily recover a session on a compatible successor, or eagerly recover it
   when the service objective requires it.

Scale is driven by active-session pressure, pending-session age, available
session slots, memory/resource pressure, and cold-start cost. Token count or
model latency alone cannot create replicas. Scale down is prohibited while any
session lacks verified recovery evidence.

The Agent product may request a minimum warm pool and an idle release policy,
but Workloads owns the resulting desired replica mutation. This preserves one
Agent Flow and one workload lifecycle.

## 8. Durable Cell and other single-writer state

Named Cells scale inside a provider replica by hibernating inactive objects.
Cloud does not create one Runtime Service per Cell. Horizontal scale therefore
means changing the number or placement of provider shards and moving named
partitions under a stable ownership map.

Each partition has one stable identity, shard-map generation, writer epoch,
storage namespace, and accepted replication policy. A move follows this
protocol:

1. freeze new assignments to the source shard-map generation;
2. close or proxy write admission for the moving partition;
3. drain in-flight turns and WebSocket/alarms under the declared policy;
4. replicate and seal the exact state prefix;
5. persist a successful writer-fence receipt for the source epoch;
6. restore and verify on the destination Claim/Runtime generation;
7. advance the ownership-map generation atomically;
8. publish the destination route and reactivate alarms/connections; and
9. grace-delay deletion of the fenced source state.

No destination can acknowledge writes before the prior epoch is fenced.
Ordinary replica count is therefore not a correct Cell scaling API; shard
capacity and partition movement compile into the same Workload replica,
Placement, Claim, Runtime, and Gateway mechanisms.

Durable Cell is a first-class collaboration-state service. Its later portfolio
sequence changes neither its domain status nor its safety requirements. Its
multi-node elasticity cannot be advertised until the retained RPO=0,
writer-loss, alarm, WebSocket, partition-move, restore, and stale-node tests
pass.

## 9. Tasks, Functions, and Workflow demand

A finite Runtime Task is already a unit of work. It is not represented as a
long-running replica set. Elasticity is controlled at admission:

- maximum global/tenant/project/release concurrency;
- queued runnable count and oldest accepted age;
- per-invocation CPU, memory, storage, accelerator, timeout, and cost budget;
- fair-share and deadline class; and
- Fleet Claim availability.

The Execution owner accepts and durably tracks each invocation; Workloads/Fleet
places its Task through the common path. A scheduler backlog can request node
pool `CapacityIntent`, but it cannot be converted into fake Service replicas.

High-request-rate Functions and sessionless MCP use the stateless Service
profile. External FaaS uses a Connector and the external provider's scaling;
Cloud still owns invocation identity, policy, timeout, evidence, and result,
but owns no external replica count.

A Workflow node may start or adopt an Agent Service interaction, a Function
Task, a hosted Function Service call, or an external Connector call. A3S Flow
records only stable owner identities and result evidence; it never becomes a
scheduler or autoscaler.

## 10. Scaling policy and deterministic evaluation

An immutable policy contains at least:

- `min_capacity`, `max_capacity`, and whether zero is allowed;
- one or more typed targets such as concurrency per replica, queue age,
  sessions per replica, or accelerator utilization;
- separate scale-up and scale-down stabilization windows;
- maximum absolute and proportional change per decision;
- scale-up, scale-down, and failure cooldowns;
- drain/checkpoint/model-load deadlines;
- minimum ready capacity and rollout/maintenance availability budget;
- quota/cost ceiling and node-pool constraints; and
- behavior for missing, stale, conflicting, saturated, or low-confidence
  signals.

For a target-based signal the evaluator uses a deterministic baseline:

```text
candidate = ceil(observed_demand / target_capacity_per_replica)
desired   = clamp(candidate, policy_min, policy_max, admitted_quota)
```

Multiple safety-relevant demand signals combine by their maximum candidate;
capacity or policy constraints clamp the result and surface unmet demand.
Scale up uses the newest complete window and may react quickly within its step
bound. Scale down uses the maximum recommendation across the entire down
stabilization window and proceeds only when every required safety signal is
fresh. Missing or stale demand never causes scale down.

Each evaluation is idempotent on:

```text
workload + control_generation + policy_digest + signal_window_digest
```

Concurrent evaluators race through optimistic control-version admission. Only
one advances the placement generation; later evaluators reload and recompute.
Leader leases improve efficiency but are not correctness authority.

## 11. Scale to zero and activation

Scale to zero is opt-in and is never inferred merely from low traffic.

It requires:

- no minimum availability or active state lease;
- a declared cold-start objective and bounded activation queue;
- immutable release/image/model/cache identity;
- state-safe retirement proof for every current replica;
- a trusted activation source; and
- an overload outcome when the cold-start deadline or queue limit is exceeded.

Gateway does not mutate replicas. For HTTP Service activation it authenticates
and rate-limits the request, emits or calls one Cloud-owned activation command,
and either holds a strictly bounded request, returns an accepted invocation
identity, or returns an explicit retry/unavailable response. The Workloads
scaler is the only component that converts activation demand into desired
replicas.

Recommended defaults are:

- finite FaaS: no idle Service exists; accept a Task invocation;
- hosted stateless FaaS/MCP: zero allowed only after the bounded activation path
  and cold-start SLO pass;
- stateful Agent: zero only after all sessions are checkpointed and leases are
  closed; a minimum warm pool is preferred for interactive Code workloads;
- Durable Cell provider: no scale-to-zero in the first production profile;
  hibernate individual named Cells instead; and
- GPU inference: zero only for explicitly cold models; popular/large models
  use a warm floor because image/model load and accelerator Claims dominate
  latency.

## 12. CPU and GPU capacity elasticity

Replica scaling and node provisioning are distinct aggregate decisions:

- Workloads owns how much executable capacity a Workload desires;
- the one scheduler maps that demand to Claims and reports pending reasons;
- Fleet owns observed node inventory, health, maintenance, and Claim delivery;
  and
- a future Compute provider may own `CapacityIntent` and machine lifecycle.

The Compute provider cannot select workload placement, forge Fleet inventory,
or mark Claims committed. It receives an aggregate pool requirement derived
from pending Claims, committed headroom, failure-domain policy, startup time,
and a bounded safety buffer. Newly provisioned machines become usable only
after normal Fleet enrollment and inventory admission.

Node scale-down is safe only when a candidate is cordoned, every Claim is
released or its Workload has completed the common evacuation protocol, the
node command journal is settled, credentials are revoked, and the provider
confirms termination. Stateful safety may block node removal indefinitely;
capacity cost is never permission to discard state.

GPU capacity additionally accounts for exact device/partition identity, VRAM,
driver/runtime compatibility, NUMA/fabric topology, reset epoch, model load
time, and fragmentation. Distributed model replicas scale as complete
placement groups. A partial group supplies zero serving capacity and must be
fully compensated on failure. Predictive warm capacity is permitted as an
immutable policy input; an independent GPU scheduler or Power-owned replica
queue is not.

### 12.1 Distributed inference shapes

Distributed inference has three orthogonal axes and the control plane must not
flatten them into one replica count:

1. **Independent serving replicas** increase request-level capacity and are
   ordinary Workload replicas.
2. **Intra-replica model parallelism** uses tensor, pipeline, expert, or another
   typed topology across devices/nodes and is one all-or-none placement group.
3. **Phase disaggregation** separates aggregated, encode, prefill, and decode
   role pools because their compute, memory, cache, and scaling curves differ.

One immutable Inference deployment revision projects stable managed Workload
role slots. Aggregated serving has `serve`; P/D has `prefill` and `decode`;
multimodal E/P/D adds `encode` only after its independent gate. Each slot has
one generic Workloads scaling policy. Each replica within a slot is either one
Runtime Service or one placement group of Runtime Services. This permits
independent phase scaling and multi-node gang execution without an inference
scheduler or a new Runtime class.

The request path is equally explicit:

```text
client
  -> A3S Gateway authorization / admission / model route
  -> one compatible serving cohort
  -> request-scoped decode + optional prefill/encode endpoint selection
  -> A3S Power phase execution
  -> opaque, lease-bound KV/embedding transfer over the private fabric
```

Gateway's request selector is not the resource scheduler. It sees only
complete, exact-revision endpoint cohorts plus bounded load/cache-affinity
observations. Workloads/Fleet alone owns hardware placement and Claims. Power
alone owns engine state and transfer correctness. Cloud stores no prompt,
token, embedding, or exact KV block index.

The useful llm-d-like outcomes map onto existing A3S owners: an inference pool
is a read projection, endpoint picking is a Gateway strategy, a model server
is a Power Runtime Service, and P/D or E/P/D is a typed Inference topology.
No Kubernetes resource, EPP database, second discovery registry, model
scheduler, or `a3s-llm-d` control process becomes authoritative.

## 13. System-plane elasticity

Cloud cannot depend on its tenant scheduler to keep that scheduler alive. The
operator/A3S OS system plane therefore owns Cloud's own capacity:

- API replicas are stateless and Gateway-balanced;
- Workers and Relays use PostgreSQL/Flow/Outbox leases and can be replicated;
- Gateway uses complete snapshot acknowledgement and independent replicas;
- the migrator is a singleton terminating job; and
- PostgreSQL, NATS, S3, Hosted Git storage, OCI Registry, and A3S Use Registry
  follow their provider-specific HA, backup, and recovery contracts.

The system plane may reuse Runtime/Box mechanics and the same signal schema,
but it must not write tenant Workload control records. Initial production uses
operator-set replica floors and failure-domain placement. Automatic
system-plane scaling requires a separately retained bootstrap-controller gate,
because a Cloud-local autoscaler is a circular failure dependency.

## 14. Multi-tenant admission and fairness

Scaling is always authorization- and quota-bounded before mutation. Admission
evaluates organization/project/environment/release identity; replica,
concurrency, CPU, memory, storage, accelerator, public-route, object, and cost
limits; node-pool policy; and current committed plus pending Claims.

A noisy tenant cannot gain capacity by emitting metrics. Signal sources are
authenticated, typed, replay-bounded, and attributed to an accepted Workload
or invocation. Fair queueing applies to finite Tasks. Service scale-up that
cannot be admitted remains visible as unmet demand with an exact quota or
capacity reason; it does not silently steal Claims from another tenant.

## 15. Observability and evidence

Every transition exports correlated but secret-free evidence:

- policy and decision digests;
- signal source, interval, sample count, freshness, and confidence;
- desired, placed, ready, serving, draining, blocked, and retired capacity;
- pending Claim reason and node-pool/accelerator fragmentation;
- cold start, image/model load, health convergence, drain, checkpoint, fence,
  recovery, and cleanup latency;
- active session/connection/task counts within bounded cardinality; and
- Runtime unit/generation/spec, Box provider, node inventory, Gateway snapshot,
  and product owner identities.

Logs and dashboards are projections. A retained gate must reconstruct why a
replica was added, retained, moved, or removed from PostgreSQL decisions and
exact external evidence after process death.

## 16. Delivery gates

| Gate | Required outcome |
| --- | --- |
| `H0.5-C1` | Freeze the scaling policy, state-safety profile, signal window, idempotent decision, drain lease, and evidence schemas in Workloads without another bounded context |
| `H0.5-C2` | Stateless Service scale up/down, complete Gateway target transitions, rollout coexistence, stale/duplicate/burst signal safety, and process-death replay on CPU nodes |
| `H0.5-C3` | Finite Task concurrency/fair queueing plus hosted Function/MCP activation and bounded scale-to-zero behavior without fake replicas or Gateway mutation authority |
| `H0.5-C4` | Agent session-aware warm scaling, checkpointed drain/recovery, single-writer fence integration, maintenance evacuation, and failure-injection evidence |
| `H0.5-C5` | CPU pool capacity intent and node drain/termination; exact GPU device, warm-model, fragmentation, and complete placement-group scaling evidence |
| `H0.5-C6` | Quota, overload, failover, restore, dependency outage, oscillation, cost-bound, and multi-tenant noisy-neighbor certification with published SLOs |

`CELL0.6` owns named-partition rebalancing behavior and must consume `H0.5-C4`
rather than copy it. `I0.4` owns distributed model semantics and must consume
`H0.5-C5`. `FN0` and `MCP0` consume `H0.5-C3`. `AR0` consumes the Agent slice
of `H0.5-C4`.

## 17. Non-goals

- One autoscaler per product or node pool.
- Treating Gateway, A3S Power, Runtime, Box, Prometheus, NATS, Redis, or an
  external cloud autoscaler as desired-state authority.
- Scaling finite Tasks by inventing idle Service replicas.
- Moving state before checkpoint, quorum change, or writer-fence proof.
- Creating one Runtime Service per Agent turn or named Durable Cell.
- Soft fractional GPU isolation without hardware/provider certification.
- Terminating nodes that still own Claims, unsettled journals, live sessions,
  unfenced writers, or incomplete placement-group members.
- Scheduling Cloud's bootstrap database, API, Gateway, or scheduler through
  the tenant control plane they are required to start.
