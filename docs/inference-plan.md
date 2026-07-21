# A3S Cloud Inference Platform Plan

## 1. Status and objective

**Status: Planned.** None of the capabilities in this document are shipped
until their owning exit gates pass. E0 is verified as of 2026-07-20, so I0
implementation may now proceed through the ordered slices below. Production
capability claims remain blocked until their owning I0 and H0 exit gates pass.

I0 adds an optional inference product profile to A3S Cloud. Its target is a
clearly gated GPUStack-like multi-node model-serving capability set, not API,
UI, or implementation compatibility with GPUStack. The finished profile will:

- inventory and health-check accelerator devices on enrolled nodes;
- resolve immutable model revisions and maintain content-addressed node caches;
- run typed inference backends on exact accelerator claims;
- scale independent serving replicas across nodes;
- run one distributed serving replica as an atomic multi-node placement group;
- expose OpenAI-compatible streaming APIs through A3S Gateway;
- apply model-level authorization, weighted routing, fallback, and rate limits;
- record auditable request and token usage without coupling scheduling to
  commercial billing; and
- recover truthfully from process, node, device, network, and control-plane
  failure.

The first published provider combination is NVIDIA plus Docker plus vLLM.
A3S Power is the second backend adapter and proves that the Cloud contract is
not vLLM-specific. Additional accelerator vendors, Runtime providers, and
inference engines remain unavailable until they pass the same real-provider
conformance gates.

The capability boundary is explicit:

| Capability area | Decision | Owning gate or context |
| --- | --- | --- |
| Enrolled-node accelerator inventory, health and exclusive allocation | Included in the first profile | `I0.0`/`I0.1`, Fleet and Workloads |
| Immutable model catalog, source resolution and node cache | Included | `I0.2a`, Artifacts and Inference |
| Typed vLLM serving and same-node tensor parallelism | Included first | `I0.2a` |
| OpenAI Models, Chat Completions, Completions and Embeddings APIs | Included with an exact endpoint contract | `I0.2b`, Gateway and Inference |
| Model authorization, rate limits, weighted targets, fallback and usage | Included | `I0.2b`/`I0.2c` |
| Independent replicas on multiple nodes | Included | `H0.3` + `I0.3` |
| One Ray/vLLM replica spanning multiple GPU nodes | Included after independent replicas | `H0.3` + `I0.4` |
| External OpenAI-compatible provider targets | Included as a later isolated slice | `I0.2d` |
| Production Gateway/control-plane HA and autoscaling | Reuse the generic platform implementation | `H0.4`/`H0.5` + `I0.5` gates |
| Power, hardware partitions and additional accelerator vendors/backends | Deferred until conformance passes | `I0.5` |
| Responses API and Jina-compatible rerank | Deferred beyond the initial data-plane gate | `I0.5` candidate |
| Soft fractional GPU/VRAM sharing | Not a production capability in I0 | Future hardware-enforced or isolated design only |
| GPU cloud-instance provisioning and SSH credential custody | Outside Inference | Future Fleet/Compute provider, if justified |
| Training, fine-tuning and notebook lifecycle | Outside I0 | Separate owning profile required |
| Kubernetes as an alternative workload scheduler | Excluded | It may package Cloud, but Workloads remains authoritative |

## 2. Fixed architecture decisions

1. `inference` owns model-serving semantics. It does not own nodes, provider
   processes, accelerator inventory, generic replica identity, TLS, or Flow
   operation state.
2. Workloads remains the only deployment engine. An inference deployment is
   deterministically compiled into an inference-managed Workload revision.
3. Workloads owns generic accelerator requirements and placement policy.
   Runtime receives only exact accelerator bindings selected by Cloud and
   exposes generic enforcement/allocation evidence. It never receives model
   catalogs, vLLM flags, tenant identity, or scheduler policy.
4. Fleet owns node identity, liveness, accelerator and cache observations, and
   the node-control protocol. Scheduling consumes those observations through a
   typed application port.
5. Workloads owns desired replica count, effective placement and autoscaling
   policy, the sole autoscaling evaluator, durable replica/member identity,
   capacity reservation, Runtime lifecycle, and health. Inference compiles
   typed serving intent into those policies and queries their projections.
6. Edge owns domains, TLS, complete Gateway snapshots, transport target sets,
   and exact applied acknowledgements. Inference owns model aliases and target
   policy, but never stores instance IP addresses.
7. PostgreSQL remains authoritative for desired state and reservations. A3S
   Flow coordinates long operations, the transactional outbox publishes
   committed facts, and periodic reconciliation repairs missed delivery.
8. Model files, backend images, and accepted launch plans are immutable and
   digest-addressed. A mutable branch, tag, or model alias is resolved once.
9. Whole-device exclusive allocation is the first supported sharing mode.
   Hardware-enforced partitions such as MIG may follow. Soft VRAM overcommit on
   an otherwise unisolated device is not a production capability.
10. All product configuration remains validated A3S ACL. Backend-specific raw
    maps and alternate product configuration formats are forbidden.

## 3. Bounded-context ownership

| Context | Authoritative facts | Explicit exclusions |
| --- | --- | --- |
| Inference | Models and immutable revisions, backend revisions, inference deployment revisions and scaling intent, model routes and grants, provider targets, usage ledger | Effective placement/autoscaling policy, Node state, Runtime observations, effective replica state, device availability, instance endpoints, TLS state |
| Workloads | Managed Workload spec, desired replica count, effective placement/autoscaling policy and evaluator, replica/member identity, placement generation, Runtime convergence, health | Model formats, tokenizer semantics, backend catalogs, model grants |
| Fleet | Node identity and eligibility, inventory snapshots, accelerator devices and topology, artifact-cache observations, node commands and acknowledgements | Model deployment policy, backend arguments, desired replica count |
| Edge | Domain claims, certificates, complete target-set snapshots, Gateway revision and acknowledgement | Model catalog, replica scheduling, token usage |
| Operations | Parent/child operation graph, progress, cancellation, terminal outcome | Copies of aggregate desired state |
| Artifacts | Model and image descriptors, digests, provenance, registry/object-store locations | Node cache truth and mutable model aliases |
| Secrets | Encrypted secret identities and immutable versions, delivery, rotation and revocation | Provider/model metadata and route policy |
| Identity | Principals, memberships, API credentials, scopes and revocation | Model routing and deployment state |

Cross-context mutation uses commands or application ports. No inference
repository writes Workloads, Fleet, Edge, Identity, or Operations tables.

An inference-managed Workload carries exactly one immutable owner reference:

```text
ManagedOwnerRef {
  kind: InferenceDeployment,
  owner_id,
  owner_generation,
  owner_spec_digest
}
```

Ordinary Workloads mutation APIs reject a managed Workload. Inference commands
delegate scale, update, stop, and rollback to Workloads. Workloads is the sole
authority for the effective desired replica count and every replica/member
state; inference query handlers compose those projections for users.

## 4. Inference domain model

### 4.1 Aggregates

`InferenceModel` is the tenant-scoped logical model. An immutable
`ModelRevision` contains:

- a typed source: Hugging Face, ModelScope, or an existing Artifact reference;
- the resolved upstream commit/revision and canonical manifest digest;
- file digests, model format, architecture, parameter metadata, context limit,
  tokenizer digest, and license/trust decision.

Model bytes never enter PostgreSQL. A node-local path is not a portable model
source and is excluded from I0. `ResolveModelRevision` starts a durable
`ModelResolutionAttempt` Operation. Only a successful, fully verified attempt
creates and seals a ModelRevision; failure and retry state remain in the
attempt/Operation rather than mutating a supposedly immutable revision.

`InferenceBackend` is a catalog entry with immutable `BackendRevision` values.
A revision binds a typed backend profile, digest-pinned OCI image, supported
model formats, accelerator and distribution requirements, declared ports,
readiness contract, and compiler version. I0 initially exposes closed, typed
vLLM and Power profiles. A user-supplied image remains a generic Workload until
a typed backend manifest and conformance gate exist.

`InferenceDeployment` owns model-serving intent and immutable deployment
revisions. A revision binds one model revision, one backend revision, typed
inference parameters, a serving topology, model-aware scaling intent, and the
canonical compiled-input digest. The backend compiler validates model/backend
compatibility and emits only generic resource, capability, network, placement,
and autoscaling inputs to Workloads. The revision does not contain effective
placement/autoscaling policy, replica rows, node IDs, device IDs, or operation
status.

`InferenceRoute` owns an external model name and aliases, primary and fallback
targets, integer weights, explicit fallback conditions, and an access-policy
revision containing references to Identity principals. A target may reference
a local InferenceDeployment or an `ExternalModelProvider`. Edge owns the
compiled transport target set.

`ExternalModelProvider` stores an OpenAI-compatible endpoint descriptor,
provider-model mapping, timeout and health policy, and an immutable Secret
version reference. Secrets owns credential creation, rotation, revocation, and
delivery; Inference only validates and replaces the bound version reference.

`InferenceUsageRecord` is an append-only business fact keyed by a stable
request/event ID. It records tenant, principal, credential, route, selected
target, status, prompt/completion/cached tokens when reported, timestamps, and
whether the measurement is complete or estimated. It never stores prompts or
responses. Price, balance, invoicing, and settlement remain outside Cloud.

### 4.2 Typed topology

Serving replica count and distributed world size are different values:

```text
ServingTopology
  SingleDevice
  SingleNodeTensorParallel { devices }
  Distributed {
    nodes,
    tensor_parallel,
    pipeline_parallel,
    distribution: Ray
  }
```

The backend compiler converts one InferenceDeployment revision into a generic
Workload execution plan. One Workload replica is either one Runtime Service or
one placement group containing a leader and bounded member set. Workloads owns
the replica, group, members, generation, and lifecycle. This avoids a second
`inference_replicas` source of truth.

### 4.3 Commands and queries

Initial management commands are:

- `RegisterInferenceModel` and `ResolveModelRevision`;
- `PublishInferenceBackendRevision`;
- `CreateInferenceDeployment`, `ReviseInferenceDeployment`,
  `ScaleInferenceDeployment`, `StopInferenceDeployment`, and rollback;
- `PublishInferenceRoute` and `RetireInferenceRoute`; and
- `RegisterExternalModelProvider` and `BindExternalProviderSecretVersion`.

Queries compose authoritative readers:

- deployment detail = Inference spec + Workloads replicas + Operations;
- model availability = ModelRevision + Fleet cache observations;
- route status = Inference route intent + Edge applied revision; and
- usage = append-only usage records and rebuildable time-bucket rollups.

The web application adds model/backend catalogs, node accelerator inventory,
deployment revisions and replicas, cache state, model routes, usage, and one
correlated operation timeline. It does not infer readiness from optimistic UI
state or raw metrics.

Event keys include `inference.model.registered`,
`inference.model-revision.resolved`, `inference.backend-revision.published`,
`inference.deployment.created`, `inference.deployment.revised`,
`inference.route.changed`, and `inference.usage.recorded`. Events accelerate
coordination but are never the only way to discover unfinished desired state.

### 4.4 Management and internal APIs

Management paths are versioned under `/api/v1`. The following table uses
`ORG = /organizations/{organization_id}` and
`ENV = ORG/projects/{project_id}/environments/{environment_id}` as shorthand:

| Method and path | Scope | Result |
| --- | --- | --- |
| `POST ORG/inference/models` | `inference:write` | Create organization-scoped logical model |
| `POST ORG/inference/models/{model_id}/resolution-attempts` | `inference:write` | `202` ModelResolution Operation; successful attempt seals a revision |
| `GET ORG/inference/models` | `inference:read` | Cursor-paginated model catalog |
| `GET ORG/inference/models/{model_id}` | `inference:read` | Model and immutable revision detail |
| `GET ORG/inference/backends` | `inference:read` | Eligible platform and organization backend revisions |
| `POST /platform/inference/backend-revisions` | `platform:write` | Publish a typed platform backend revision |
| `POST ORG/inference/providers` | `inference:write` | Register organization-scoped external provider descriptor |
| `GET ORG/inference/providers` | `inference:read` | Cursor-paginated provider catalog without Secret material |
| `GET ORG/inference/providers/{provider_id}` | `inference:read` | Provider descriptor and bound Secret version identity |
| `POST ORG/inference/providers/{provider_id}/secret-bindings` | `inference:write` | Bind an existing immutable Secret version; does not rotate it |
| `POST ENV/inference/deployments` | `inference:write` | `202` create/reconcile Operation |
| `POST ENV/inference/deployments/{id}/revisions` | `inference:write` | `202` update Operation |
| `POST ENV/inference/deployments/{id}/scale` | `inference:write` | `202` desired-replica change through Workloads |
| `POST ENV/inference/deployments/{id}/stop` | `inference:write` | `202` stop Operation through Workloads |
| `POST ENV/inference/deployments/{id}/rollback` | `inference:write` | `202` rollback Operation through Workloads |
| `GET ENV/inference/deployments` | `inference:read` | Cursor-paginated composed deployment projection |
| `GET ENV/inference/deployments/{id}` | `inference:read` | Inference, Workloads and Operations detail |
| `POST ENV/inference/routes` | `inference:write` | `202` route/access-policy publication Operation |
| `POST ENV/inference/routes/{id}/revisions` | `inference:write` | `202` immutable route revision publication |
| `GET ENV/inference/routes` | `inference:read` | Cursor-paginated route and applied-state projection |
| `GET ENV/inference/routes/{id}` | `inference:read` | Intent plus exact Edge/Gateway applied state |
| `GET ENV/inference/usage` | `inference:read` | Cursor-paginated request records or bounded rollups |

Every management mutation requires `Idempotency-Key`, uses the standard A3S
response wrapper, and documents synchronous validation versus `202` Operation
semantics. Stable errors include `MODEL_REVISION_NOT_READY`,
`BACKEND_INCOMPATIBLE`, `UNSUPPORTED_CAPABILITY`, `NO_ELIGIBLE_PLACEMENT`,
`RESOURCE_CLAIM_CONFLICT`, `ROUTE_NOT_READY`, and `USAGE_GAP`.

Internal mTLS APIs are separately versioned and never accept tenant bearer
tokens:

| Method and path | Caller | Purpose |
| --- | --- | --- |
| `POST /v1/node-control/session:hello` | Node Agent | Negotiate all session contract versions |
| `POST /v1/node-control/inventories` | Node Agent | Submit bounded inventory generation/digest |
| `POST /v1/inference-control/usage-batches` | Gateway | Ingest ordered spool events and return contiguous ack/gaps |

`inference:invoke` is not a management token scope. It is a data-plane grant on
a separate inference-key audience as defined in Section 10.

## 5. Proposed module shape

```text
crates/control-plane/src/modules/inference/
|-- domain/
|   |-- entities/          # model, backend, deployment, route, provider, usage
|   |-- value_objects/     # source, manifest, topology, policy, typed profiles
|   |-- repositories/
|   |-- services/          # traits only
|   `-- events/
|-- application/
|   |-- commands/{use_case}/
|   `-- queries/{use_case}/
|-- infrastructure/
|   |-- persistence/
|   |-- model_sources/
|   |-- backend_compilers/
|   |-- integrations/      # Artifacts, Secrets, Workloads, Fleet, Edge, Operations
|   `-- reconciliation/
`-- presentation/
    |-- controllers/
    |-- dto/request/
    |-- dto/response/
    `-- inference_module.rs
```

Domain code has no Boot, SQL, HTTP, Runtime, Flow, Event, Gateway, or backend
imports. Backend compilers are infrastructure implementations of domain ports
and return typed generic execution plans.

## 6. Accelerator and node protocol

### 6.1 Versioned contracts

Accelerator support requires an explicit compatibility release rather than
adding fields silently to existing closed schemas. The planned protocol set is:

- Runtime capabilities v4;
- Runtime unit spec v3;
- Runtime observation v3;
- Runtime apply and inspection envelopes v2;
- Cloud node inventory v1;
- Cloud enrollment, heartbeat, and observation batch v2; and
- the complete Cloud command lease/envelope/payload/result/ack family v2.

`NodeSessionHello` runs on every authenticated agent start and reconnect. It
advertises all readable and writable contract versions plus the agent instance
ID; the control plane returns the exact negotiated set for that session.
Enrollment records initial support but is not the only negotiation point,
because an already enrolled node may upgrade its agent in place.

The control plane reads old and new node protocols during a bounded migration
window. Old agents remain eligible for compatible CPU workloads and are
ineligible for accelerator workloads. Side-by-side versioned structs or tagged
envelope variants prevent a v2 nested Runtime value from being decoded through
a v1 outer schema. A Runtime release, exact Cloud Cargo dependency, gitlink,
Cargo lock, every affected protocol lock level, and `compat/cloud-stack.acl`
update land as one compatibility change.

Workloads owns a scheduling value object equivalent to:

```text
AcceleratorRequirement {
  count,
  minimum_memory_bytes,
  allowed_device_classes,
  allowed_vendors,
  partition_policy,
  topology_policy,
  exclusivity
}
```

After placement, infrastructure maps the accepted claim into a Runtime type
equivalent to:

```text
AcceleratorBinding {
  claim_id,
  claim_generation,
  placement_generation,
  fencing_epoch,
  node_id,
  inventory_generation,
  inventory_digest,
  replica_id,
  member_id,
  runtime_unit_id,
  runtime_unit_generation,
  exact_device_ids,
  exact_partition_ids,
  topology_digest,
  claim_digest
}
```

Runtime apply receives only the exact binding selected and prepared by Cloud.
Runtime capabilities advertise supported binding/enforcement modes, not
placement preferences. Runtime observations echo allocation evidence,
including stable device IDs, generations, fencing epoch, and claim digest.

Node command v2 adds idempotent `PrepareResourceClaim` and
`ReleaseResourceClaim` payloads. `RuntimeApply` may reference only a claim that
the target agent has durably prepared at the same generation and fencing epoch.
An exact replay returns the journaled result; conflicting devices, digest,
generation, node, or epoch fail closed.

### 6.2 Inventory and telemetry

The node agent reports a versioned, digest-addressed inventory snapshot with:

- allocatable CPU, RAM, ephemeral storage, dedicated Artifact-cache storage,
  provider mount support, and bounded host/private-network port ranges;
- stable vendor device or partition UUID, never an ordinal such as `cuda:0`;
- vendor, product, architecture, driver/runtime, total memory, health, and
  allocatable state;
- PCI address, NUMA node, parent partition, and hardware partition profile;
- local links such as NVLink or XGMI; and
- private-network and optional fabric/RDMA capabilities needed by distributed
  backends.

Heartbeat references the latest inventory generation and digest. Full bounded
inventory uses a separate idempotent report endpoint. High-frequency
utilization, temperature, KV-cache pressure, TTFT, and throughput go to the
metrics pipeline; they are not desired state or hard capacity truth.

Inventory projections use generation compare-and-swap. Claim prepare and
Runtime apply both revalidate the exact inventory generation/digest, resource
health, partition identity, and topology digest. A MIG reconfiguration, device
replacement, port-range change, or capacity regression invalidates the old
candidate instead of being accepted as an equivalent node.

The node agent implements a small `AcceleratorDetector` infrastructure trait.
I0 supplies a deterministic virtual detector and a real NVIDIA detector. It
persists a resource-claim journal, translates stable IDs to CDI or Docker
device requests, labels provider resources with claim/fencing/spec identity,
and proves that an unclaimed device is invisible. Inference engine names never
enter the node-control protocol.

## 7. Resource reservation and scheduling

Fleet persists inventory. The general Workloads scheduler owns placement and
resource reservation through a Fleet inventory/claim port. The scheduler uses
a deterministic filter, score, reserve, prepare, and bind pipeline.

One resource claim follows this durable state machine:

```text
reserved_in_db
  -> preparing_on_agent
  -> prepared_on_agent
  -> bound_to_runtime_unit
  -> releasing
  -> released
  +-> orphaned
```

`placement_generation` is advanced by the WorkloadReplica whenever its member
assignment changes. `claim_generation` is advanced by the Workloads
ResourceClaim aggregate for each lifecycle revision. `fencing_epoch` is
advanced transactionally for each stable resource slot when Workloads grants a
new claim; the node journal rejects an older epoch. Every claim binds node and
inventory identity, replica/member identity, Runtime unit ID/generation, exact
resources, topology, and a canonical claim digest.

The reservation transaction creates `reserved_in_db`. Persisting the Fleet
prepare command advances it to `preparing_on_agent`; only the exact command
acknowledgement advances it to `prepared_on_agent`. Dispatching Runtime apply is
not a commit. `bound_to_runtime_unit` requires a matching Runtime observation or
inspection that proves unit, generation, claim digest, and allocation evidence.
After a crash between provider create and acknowledgement, reconciliation
inspects and adopts that exact unit before retrying or releasing anything.
Prepared-TTL cleanup applies only to a never-bound claim and reconciles both
the server row and agent journal. Failure to obtain release or trusted fencing
evidence produces an operator-visible `orphaned` claim that continues blocking
the old resource.

Hard filters are evaluated before scoring:

1. tenant quota, node pool, ready/fresh/non-draining state;
2. negotiated node, Runtime, provider, and compiled required capabilities;
3. accelerator vendor/class/features, driver, health and partition;
4. device count and safe memory requirement;
5. CPU, RAM, ephemeral disk, artifact-cache capacity, ports and secrets;
6. same-node topology requirements; and
7. for distributed groups, compiled private-network, fabric, topology and peer
   reachability requirements.

Scores are integer, explainable, and deterministic. They consider complete
model-cache locality, topology quality, binpack/spread policy, fragmentation,
failure-domain separation, and inventory freshness. Real-time GPU utilization
may be a low-weight signal but cannot create capacity. Stable node and device
IDs break ties.

Every final candidate contains the complete member-to-node assignment for GPU
or partition, CPU, RAM, cache disk, and host/private ports. PostgreSQL
atomically reserves all hard resources for the whole placement group.
Concurrent schedulers use row locking and retry; a partial group is never
dispatched. Agents prepare claims concurrently. Failure of any prepare
compensates the entire group. The Workloads reconciler reads durable Fleet
command/ack state and retries the handshake; success events are not the only
evidence.

A prepared lease may expire only before Runtime apply and only under the
bounded clock-skew contract. A binding committed to a Runtime unit does not
expire because a wall clock or control-plane heartbeat elapsed. Active resource
uniqueness is removed only after an exact Agent release acknowledgement,
same-epoch provider `NotFound` evidence, or a trusted Compute-provider
power-off/instance-generation fence. An uncertainty timeout alone is not
fencing evidence.

Initial exclusive reservations enforce a partial uniqueness constraint over
active `(node_id, accelerator_id)` claims. Hardware partitions use their stable
partition identity. Release occurs only after stop/remove or fencing evidence
proves that the old generation can no longer use the device.

## 8. Model artifacts and node cache

Model source resolution produces an immutable file manifest before placement.
Private source credentials use Secret references and never enter model digests,
Flow history, events, command journals, logs, or cache keys.

I0 resolves private upstream sources through a bounded Artifact-ingestion Task
and copies verified bytes into the Cloud-owned Artifact store. Nodes do not
receive Hugging Face or ModelScope credentials. An authenticated materializer
issues a short-lived, node- and digest-scoped download URL; that transient URL
never changes the stable Artifact reference, manifest digest, or cache key.
Any future direct-node source adapter requires a separate Secret-delivery and
redaction gate.

Resolvers enforce provider/host policy, bounded manifest and file sizes, safe
normalized relative paths, redirect policy, media types, per-file digests, and
tenant quotas. Symlinks, path traversal, duplicate normalized paths, and a
partially verified manifest fail closed. Executable remote model code is
disabled by default and requires a separately audited backend capability; a
model repository cannot turn resolution into control-plane code execution.

The node cache is content-addressed and generic to Artifact mounts. It provides:

- resumable range download, bounded concurrency and cross-process file locks;
- digest verification, partial-file quarantine, and atomic rename;
- read-only Runtime mounts;
- crash-safe pins and quota-aware LRU eviction; and
- bounded cache observations keyed by artifact/manifest digest.

Cache entries are observations, not model authority. Missing, duplicated,
stale, or reordered cache reports cannot change the accepted ModelRevision.
Scheduling prefers ready cache content only after all hard resource filters
pass. A cache pin binds artifact digest, Runtime unit ID/generation, and claim
ID. The agent journals the pin before apply and reconstructs it from provider
labels and mounts after a crash; eviction requires no live or reconstructable
pin. Prefetch holds a disk reservation but does not hold scarce GPU claims for
a long model download.

Preparation order is explicit:

```text
tentative placement plus cache-disk reservation
  -> prefetch and verify every Artifact
  -> revalidate inventory and candidate generation
  -> atomically reserve and prepare compute, accelerator and ports
  -> Runtime apply
```

If final revalidation fails, scheduling starts a new candidate. Verified cached
content may remain under quota and become locality evidence for that attempt.

## 9. Backend compilation and distributed execution

An `IInferenceBackendCompilerService` validates compatibility, estimates bounded
resource-plan variants, and compiles the immutable model/backend revision into
a generic Workload execution plan. Resource estimates are explicit inputs to
scheduling and include a safety margin; failure to produce a safe plan is an
unsupported-capability error, not a best-effort launch.

Backend adapters emit typed Runtime process, mount, network, health, and claim
fields. They may use typed CLI arguments or environment bindings, but Cloud
never writes a backend's alternate product configuration file. Power is
eligible only after its Cloud adapter is ACL-native at the product boundary or
proves a fully typed CLI/environment launch without generating non-ACL config.

The vLLM adapter first supports one GPU and same-node tensor parallelism. It
then adds a typed Ray distribution plan. Ray is not a Runtime provider. It is a
backend launch graph over ordinary Runtime Service units:

```text
tentative placement, disk reservation and verified prefetch
  -> revalidate and atomically prepare compute, accelerator and port claims
  -> create private endpoints and a short-lived rendezvous secret
  -> start and verify the CPU-only ray-head unit
  -> start one ray-worker unit per member, each owning its node's GPU claim
  -> continuously verify exact membership, ranks, world size and generation
  -> start the CPU-only vllm-driver/server unit without a duplicate GPU claim
  -> probe and publish only the server endpoint as a healthy target
```

A backend compiler may combine roles only when its typed execution plan proves
process supervision and non-overlapping claims. Atomic group means atomic hard
resource reservation, no partial activation, and eventual compensation; it
does not claim atomic process creation across nodes.

One missing or stale member makes the complete serving replica unavailable.
The first implementation restarts the complete group rather than claiming
elastic Ray recovery. Cleanup removes the Gateway target first, stops the
server, head and workers, revokes rendezvous secrets and endpoints, then
releases port/device claims and the database reservation. Planned
scale-down and rollout wait for the target-removal acknowledgement and a
bounded in-flight drain window before stop; timeout follows explicit abort or
cleanup-pending policy rather than silently terminating an active stream.

Serving-node loss or an unhealthy accelerator first removes the affected target
through an acknowledged snapshot on an independently placed Gateway. After the
configured uncertainty window, Workloads creates a replacement with the same
durable replica identity and a newer placement/resource epoch on different
capacity; the old device claim remains fenced and unreleased until trusted
evidence closes it. A returning node must inspect and remove every stale-epoch
unit before it becomes schedulable, and its old observation can never reactivate
a target or claim.

A database epoch cannot stop a partitioned process physically. Network policy
therefore permits serving traffic only from eligible Gateways, and no public or
tenant-routable endpoint bypasses them. If the Gateway scope itself cannot
produce a target-removal acknowledgement, a replacement may prepare but cannot
become publicly active; the route reports unavailable. Restart budgets and
bounded backoff prevent an incompatible model or failing device from creating
an endless reschedule loop.

Cross-node execution is unavailable until H0 proves a typed cluster-private
network with workload identity, isolation, bounded port allocation, partition
behavior, and recovery. The control mTLS address is not reused implicitly as a
tenant data-plane address.

## 10. Gateway, authorization, and usage

### 10.1 Route and rollout model

A3S Gateway remains the transport data plane. Cloud preserves two routing
layers instead of flattening provider policy and replica balancing:

```text
InferenceRoute
  -> weighted primary/fallback deployment or external-provider targets
      -> Edge target set for one local deployment revision
          -> healthy Workload replica endpoints
```

Inference owns the first layer, including model rewrite and fallback order.
Edge owns the second layer, including replica health, load-balancing weight,
cluster-private endpoint, source generation, and per-Gateway acknowledgement.

During update, a target set may contain the active prior revision and a verified
candidate revision under one explicit rollout generation. The old revision
remains eligible until candidate health, traffic weight, and the complete
Gateway snapshot are acknowledged. Unrelated, stale-generation, and unhealthy
replicas are excluded. Promotion atomically selects the candidate only after
the rollout policy passes; failure restores prior weights without inventing a
new backend observation.

Cross-node targets use typed, identity-bound cluster-private endpoints rather
than today's node-local loopback origin. Every dedicated or replicated Gateway
records its own revision/digest acknowledgement. H0 defines `minimum_ready` and
`max_unavailable` for Gateway rollout: only exact-revision-ready instances pass
the external load balancer readiness check, the route may serve after the
configured minimum is ready, and rollout success still requires every desired
Gateway replica or an explicit degraded terminal result. No global atomic
Gateway reload is assumed.

I0.3 requires a Gateway placed outside the serving-node failure domain. Its
node-loss gate covers a serving node. Gateway process/node loss and mixed
revision recovery are I0.5 gates over H0 Gateway HA.

### 10.2 Inference protocol and dispatch

OpenAI requests choose a model from the request body, so the current host/path
router and request-parts middleware are insufficient. I0 adds a native optional
A3S Gateway inference-dispatch stage configured by a complete Cloud-generated
ACL snapshot. A separate proxy and the control-plane API are not alternate hot
paths for I0.

The first protocol matrix is closed:

| Endpoint | I0 gate | Contract |
| --- | --- | --- |
| `GET /v1/models` | I0.2b | Grant-filtered OpenAI-compatible model list |
| `POST /v1/chat/completions` | I0.2b | Non-streaming and SSE streaming with terminal `[DONE]` |
| `POST /v1/completions` | I0.2b | Non-streaming and SSE streaming with terminal `[DONE]` |
| `POST /v1/embeddings` | I0.2b | Non-streaming OpenAI-compatible response |
| `POST /v1/responses` | Deferred to I0.5 | Enabled only after backend and SDK conformance |
| `POST /v1/rerank` | Deferred to I0.5 | Jina-compatible, not described as an OpenAI endpoint |
| Images, image edits, speech, transcription, and Anthropic Messages | Outside I0 | Require separate typed protocol profiles |

Management APIs retain the standard A3S response wrapper. These data-plane
endpoints retain their protocol-specific success and error shapes. I0.2b fixes
one OpenAI SDK/version contract and validates status, error object, stream event
framing, `[DONE]`, usage chunks, and disconnect behavior against real backends.

Dispatch accepts `application/json`, enforces an 8 MiB I0 hard request-body
limit, buffers a body at most once, and rejects malformed JSON, a missing or
invalid model, unsupported endpoints, and excess input with stable errors.
Connection/header, first-token, idle-stream, and total-operation policies are
independent. Retry or fallback is allowed only before any response byte is sent
to the client. Authentication and input 4xx responses never fall back. Every
attempt receives its own stable attempt ID and usage outcome, because an
upstream may consume tokens even when fallback later succeeds.

### 10.3 Tenant naming, authorization, and limits

I0 routes one organization/project/environment through a tenant-owned hostname;
model aliases are unique only within that environment and hostname. A future
shared global hostname must use an unambiguous organization-qualified model
name and is outside I0.

Management tokens and inference keys are separate credentials and audiences.
Management scopes include `inference:read`, `inference:write`, and
`inference-backend:write`; they cannot invoke a model. An inference key has the
`cloud-inference` audience, owner and consumer organization, expiration,
revocation state, allowed environment/model aliases, allowed endpoints, and an
`invoke` grant; it cannot call management APIs.

I0.2b initially supports organization-owned inference keys. Human, group, and
fine-grained principal grants require the C0 membership/principal sub-gate and
reuse Identity rather than creating another user model. Request authorization
evaluates hostname/tenant, credential audience and revocation, route visibility,
model/endpoint grant, then rate-limit policy. `/v1/models` returns only granted
aliases. Unknown and unauthorized aliases use the same non-enumerating 404
shape and bounded timing policy.

Revocation blocks the next request and stream reconnect. An already established
stream may finish within its route's bounded total timeout; emergency route or
key disable removes new traffic and may invoke an explicitly audited abort
policy. Gateway authorization fails closed when its verifier/grant snapshot is
expired or unavailable.

The immutable InferenceRoute access-policy revision owns typed request,
concurrency, token-budget, and rate-limit policy. One Gateway can enforce an
exact local counter. Production replicated Gateways use a shared Redis-backed
counter for limits advertised as globally exact; without that provider, limits
are explicitly per-Gateway approximations and cannot enforce a tenant quota or
billing entitlement.

### 10.4 External provider targets

ExternalModelProvider is deferred to I0.2d, after local routing, authorization,
and durable usage pass. It uses one typed, inference-managed egress Workload.
The egress adapter receives an immutable Secret version through the E0 delivery
boundary, strips the client credential, injects target-specific authorization
headers, rewrites the model name, applies provider timeout policy, and emits the
same attempt/usage protocol as a local target. Gateway snapshots contain only
the internal egress endpoint and provider target ID. Provider plaintext never
enters ACL text, usage records, access logs, or traces.

### 10.5 Durable usage and observability

Auditable usage requires a durable Gateway spool rather than a best-effort
event after response completion. Before upstream dispatch, Gateway appends
`request_started` and `attempt_started` under a stable request/attempt ID. It
then appends attempt and request terminal records after success, failure,
fallback, cancellation, or disconnect. Each Gateway spool has a durable
`gateway_id`, boot epoch, monotonic sequence, bounded retention, and backpressure.
A route that requires auditable usage fails closed if its spool is unavailable
or full.

Gateway sends ordered batches to an authenticated ingestion endpoint. The
control plane deduplicates event IDs, acknowledges the highest contiguous
sequence, detects gaps, and requests replay while retained. A started request
without a terminal event becomes a visible `unknown` outcome after recovery;
it never becomes zero usage. Measurement completeness is a closed enum:
`complete`, `estimated`, `incomplete`, or `unknown`.

The request record snapshots owner and consumer organization, credential ID,
endpoint, external alias, resolved model/deployment/backend revision, route
policy revision, selected provider/target, and timestamps. It has one-to-many
attempt records. Deleting a model, route, provider, or key preserves these
historical descriptors. Prompt, response, key material, and provider secrets
are never stored.

PostgreSQL usage tables are time partitioned, indexed by tenant/time and stable
request ID, and governed by explicit retention/export policy. Rebuildable daily
rollups do not replace request facts. Commercial price, balance, invoice, and
settlement remain outside Cloud.

Prometheus-compatible telemetry uses a documented label-cardinality budget and
contains no tenant, principal, credential, prompt, or response labels. It
covers request/error rate, latency, TTFT, TPOT, token throughput, fallback and
auth rejection, usage-ingestion lag/gaps, Gateway revision skew, claim leaks,
cache failures, accelerator health, and autoscaling metric age. Dashboards and
alerts preserve the request/operation correlation chain and report stale data
as unknown.

The Inference compiler maps queue depth, active requests, TTFT, token
throughput, and backend capacity signals such as KV-cache pressure into a typed
Workloads autoscaling policy. The H0 Workloads autoscaler is the only evaluator
and writer of desired replica count. Missing or stale metrics preserve a
configured safe count. Scale-to-zero is enabled only with bounded Gateway
buffering, an explicit cold-start SLO, and a model-cache policy that has passed
overflow, timeout, and restart gates.

## 11. Persistence slices

Migration numbers are assigned only when an implementation slice lands. The E0
prerequisite is already satisfied; the logical order and ownership are:

| Slice | Owner | Tables or changes |
| --- | --- | --- |
| Protocol negotiation | Fleet | session hello/epoch, readable/writable contract sets, selected versions, downgrade guard, nested v2 command/lease/result/ack schemas, and expanded command-kind constraints |
| Resource inventory | Fleet | immutable inventory snapshots, general capacity and port ranges, accelerator devices/links, cache observations, generation/digest CAS |
| Artifact foundation | Artifacts | immutable file manifests, ingest attempts, storage descriptors, scoped materialization grants and revocation |
| Managed Workload foundation | Workloads | owner reference, one durable replica/member, protected mutation and replacement of the current single-deployment uniqueness rule |
| Replica/group foundation | Workloads | replica sets, placement groups/members, rollout generation and private endpoint/port reservations |
| Resource claims | Workloads | full claim state machine, reservation members for all hard resources, inventory/generation/fencing binding, partial uniqueness and orphan state; Fleet dispatches commands |
| Inference catalog | Inference | models, model revisions, backends, backend revisions, external providers |
| Inference serving | Inference | deployments/revisions, routes/aliases, two-level targets, access-policy revisions, model/endpoint grants referencing Identity credential IDs, and typed rate limits |
| Inference credentials | Identity | credential audience, owner/consumer organization, expiry/revocation and non-secret key identity |
| Usage | Inference | append-only request/attempt events, Gateway epoch/sequence cursors, gap state, time partitions, retention and rebuildable rollups |
| Edge target sets | Edge | source/rollout owner reference, prior/candidate target-set revision/digest, private endpoints, per-Gateway ack/readiness |
| Autoscaling | Workloads | immutable effective policy, evaluator lease, decision record, stabilization/cooldown state and desired-count mutation |
| Operation composition | Operations | parent/child operation relations and workflow identity |

Every tenant row carries `organization_id`. Revision keys are unique by
aggregate and generation. Closed typed specifications store a canonical digest.
Gang reservations and all members commit in one PostgreSQL transaction. Raw
high-frequency metrics do not enter these tables.

## 12. Delivery gates

### I0.0: contracts and mixed-version safety

- Land Runtime binding contract tests, every nested Cloud v2 envelope,
  `NodeSessionHello`, and protocol-selection persistence.
- Prove new control plane plus old agent keeps CPU workloads working.
- Prove an enrolled agent can upgrade and renegotiate without re-enrollment.
- Fail closed on unknown schemas, downgrade, digest conflict, and stale fencing.
- Update the compatibility lock only with the tested Runtime and Cloud pair.

### I0.1: single-node accelerator substrate

- Depend on H0.1 managed-owner, single-replica and generic resource-claim
  foundations; I0 does not create a private claim implementation.
- Add virtual and NVIDIA inventory, exclusive claim reservation, agent journal,
  Docker/CDI enforcement, allocation evidence, and recovery.
- Prove 100 concurrent reservations never allocate one device twice.
- On a real NVIDIA host, expose exactly the claimed UUID and no other device.

### I0.2a: model, cache, and backend health

- Extend the existing Artifacts and E0 Secret foundations with immutable model
  file-manifest ingest/materialization, then land Inference model/backend
  catalogs, immutable resolution, cache, the vLLM compiler, one
  inference-managed Workload, and a real backend health/inference probe.
- Prove source failure, corrupt/partial cache, incompatible backend, OOM, stop,
  and process restart without a public route or duplicate unit.
- Keep Power unavailable in this slice; add it only after it passes the same
  backend conformance profile in I0.5.

### I0.2b: OpenAI data plane, model route, and authorization

- Depend on H0.2 cardinality-one Edge target-set/private-endpoint projection.
- Land Gateway inference dispatch, the closed endpoint matrix, organization
  inference keys, model/endpoint grants, typed limits, TLS, and streaming.
- Prove filtered model listing, non-enumerating denial, revocation, request
  bounds, SSE framing, pre-first-byte fallback, and exact route acknowledgement.

### I0.2c: durable usage and rollout

- Land the Gateway durable spool, batch/cursor/gap protocol, request/attempt
  ledger, retention/rollup queries, observability, update and rollback.
- Prove a failed model/backend revision leaves the prior healthy revision
  serving and every response/attempt has a terminal or visible unknown usage
  outcome after crash and replay.

### I0.2d: external provider targets

- Land only after I0.2b/I0.2c. Add the typed inference egress Workload, Secret
  version binding, model rewrite, header isolation, provider fallback and usage.
- Prove client and provider credentials cannot cross, rotate, leak, or select a
  cross-tenant provider target.

### I0.3: multi-node independent replicas

- Depend on H0.3 multi-node replicas, drain/evacuation, private endpoints, and
  independently placed Gateway foundations.
- Scale manually across at least three nodes, kill a non-Gateway serving node,
  and prove traffic contains only allowed rollout revisions, with no duplicate
  provider unit or released-but-live claim.

### I0.4: one distributed replica across nodes

- Depend on H0.3 placement-group, gang-claim and cluster-private-network gates;
  add the typed Ray launch graph, continuous membership, group health and
  compensation.
- Pass a real two-node, four-GPU vLLM gate. Every injected 3-of-4 preparation,
  membership, rank, port, partition, and process failure must converge to
  either every declared GPU claim/member ready or no active target and no
  releasable committed claim. Runtime unit count need not equal GPU count.

### I0.5: production hardening and provider breadth

- Depend on completed H0.4 production deployment/control-plane/Gateway HA and
  H0.5 autoscaling foundations; I0 does not own or reimplement them.
- Pass inference-specific Gateway loss, mixed-revision, autoscaling, quota,
  disaster-recovery, load, cache-pressure, and usage-backlog gates on those
  profiles.
- Add hardware partitions and each new vendor/backend only through the exported
  accelerator/backend conformance suites.
- Do not claim fractional sharing, multi-vendor support, or distributed
  recovery from capability advertisement alone.

Cloud GPU host creation, SSH credential custody, and cloud-instance lifecycle
are not owned by Inference. If required, they land later as a separate typed
Fleet/Compute provider that produces an ordinarily enrolled Node and passes the
same fencing, drain, cleanup, cost-quota, and recovery gates. Kubernetes also
remains an optional deployment profile rather than a second scheduler.

## 13. Mandatory verification

In addition to the repository verification matrix, I0 requires:

- pure domain tests for immutable revisions, route weights/fallback, typed
  topology, access policy, and usage completeness;
- PostgreSQL concurrency tests for reservation, fencing, replica identity,
  idempotency, and tenant isolation;
- golden protocol tests for old/new agent negotiation and canonical digests;
- virtual accelerator and real NVIDIA Docker conformance;
- real model-source download, resume, corruption, deduplication, disk pressure,
  and cache eviction tests;
- real vLLM streaming, same-node tensor parallel, weighted target distribution,
  fallback, auth revocation, and usage deduplication;
- real multi-node placement, drain, node return with stale epoch, and route
  convergence; and
- real Ray/vLLM gang tests with process kills and network faults at every
  durable boundary.

New crash points include:

1. an enrolled agent upgrades and reconnects without re-enrollment;
2. inventory or MIG topology changes between candidate, prepare and apply;
3. reservation commit before any Agent prepare;
4. Agent prepare journal commit before acknowledgement;
5. some Agents prepare before another member rejects;
6. Runtime/provider create before apply acknowledgement;
7. release succeeds before acknowledgement, including prepared-TTL clock skew;
8. Artifact rename or cache-pin journal before cache acknowledgement;
9. concurrent disk reservation and attempted eviction of an in-use Artifact;
10. all Runtime members ready before serving health projection;
11. asymmetric Ray partition, stale head or membership loss after readiness;
12. target-set reload before exact Gateway acknowledgement;
13. target removal acknowledgement before Runtime stop;
14. client response completes before terminal usage spool append;
15. usage batch send before contiguous-sequence acknowledgement;
16. serving-node loss, Gateway node loss, and old fenced node still running; and
17. fencing-epoch advance before the old node reconnects.

Each test asserts one authoritative rollout generation, only explicitly allowed
prior/candidate revisions, no duplicate claim, no stale target, no false
success, visible usage gaps, bounded cleanup, and a complete audit/correlation
chain.

## 14. First implementation backlog

The recommended merge order is:

1. accelerator contract ADR, failing protocol fixtures, and Runtime vNext;
2. Cloud nested v2 contracts, session negotiation, and compatibility fixtures;
3. H0.1 managed-owner, one-replica and generic claim state machine;
4. virtual inventory, then NVIDIA detection/enforcement and the real-host gate;
5. model file-manifest ingest/materialization/cache over the existing Artifacts
   and E0 Secret foundations;
6. Inference domain/application skeleton and model/backend repositories;
7. I0.2a vLLM backend health without a public route;
8. H0.2 cardinality-one target set/private endpoint, then I0.2b Gateway
   dispatch, organization inference keys, authorization and streaming;
9. I0.2c durable usage spool/ledger, observability, update and rollback;
10. I0.2d external-provider egress and Secret-version replacement;
11. H0.3 replica sets, multi-node placement/drain and dedicated Gateway;
12. I0.3 independent replica failover and rolling update;
13. H0.3 placement-group/private-network gate, then I0.4 Ray/vLLM;
14. H0.4 production deployment/HA and H0.5 sole autoscaling controller;
15. I0.5 inference HA, load, quota and disaster gates; and
16. Power and additional vendor/backend adapters through conformance.

No slice weakens the verified E0 path, creates a parallel deployment path, or
marks a capability available before its real-provider and recovery gates pass.
