# A3S Cloud Technical Architecture

## 1. Status and decisions

`BX0` is in progress. A3S Box is the sole Cloud execution and image-build
provider, and A3S Power is the required local inference Service boundary. The
previous Docker/Bollard implementation and its R0 through E0 evidence are
historical regression records only; they do not certify the Box-only release.
The paragraphs below that describe completed Docker/BuildKit gates record that
historical behavior until `BX0.5` removes the retired implementation and ports
the evidence to exact Box revisions.

R0 through E0 behavior was previously implemented and certified. E0 has durable Edge route
ownership, exact and wildcard domain claims, managed Gateway certificate
provisioning, HTTPS-only snapshot compilation, Fleet dispatch, exact
acknowledgement projection, and injected-time renewal/revocation convergence
with delayed provider-serial revocation. It also has tenant-scoped Secret
identities, immutable encrypted versions, rotation and version revocation APIs,
and metadata-only events and idempotency records, typed workload bindings, and
late Docker environment/file injection plus authenticated registry pulls. Committed
rotation events now drive an idempotent worker that derives a new resolved
revision for each affected active workload, advances only matching Secret
references, and atomically records the deployment operation, causal event, and
restart checkpoint. A
dedicated Linux acceptance gate now uses real PostgreSQL
authorization/decryption and Docker to prove active-version environment and
`0400` tmpfs-file injection, and uses a separate encrypted credential to pull
an uncached digest from a registry that rejects anonymous access. The
first-node log slice now
projects active Runtime targets durably, persists bounded batches before mTLS
upload, redacts bound Secret values at the Docker boundary, stores verified
chunk objects through a typed filesystem or S3-compatible adapter, indexes
metadata in PostgreSQL, and exposes tenant-scoped cursor queries with explicit
provider/missing/corrupt gaps. Typed Runtime cursor-loss and source-disconnect
boundaries are persisted and replayed by the node, stored atomically with batch
headers in PostgreSQL, and merged into the same sequence pages and bounded
resumable SSE feed. A bounded control-plane worker removes object bodies after
the configured receipt age while durable `retained` tombstones preserve every
cursor position. An independently configured bounded worker later replaces old
per-chunk tombstones with coalesced durable sequence ranges, and queries expose
those ranges as explicit `compacted` gaps. A dedicated digest-pinned MinIO CI
job defines the real S3-compatible lifecycle gate, and a separate remote
Gateway job exercises managed TLS plus a forced reload-before-acknowledgement
agent crash. The Linux acceptance gate also captures real Docker stdout/stderr
after redaction, persists immutable filesystem objects and PostgreSQL metadata,
kills a child control plane after object publication but before receipt
persistence, adopts the exact object after restart, corrupts a non-secret
record, and reads its ordered `corrupt` gap through the REST API. The Docker
gate also preserves an exact log cursor across provider restart, while the
pinned-MinIO gate verifies deliberate corruption and immutable repair
rejection. The one-node update slice now commits complete
immutable replacement templates, runs candidates on the previous Runtime node,
gates routed cutover on health plus an exact Gateway acknowledgement, and
recovers deterministic old-revision retirement after activation. Unhealthy,
mismatched, and rejected outcomes preserve the previous active revision and
route rows. Manual rollback now selects an older successfully activated
revision, derives a new generation from its exact resolved template, and reuses
the version 3 deployment, Claim, cutover, and retirement workflow. Versions 1
and 2 remain replay-compatible for histories persisted before the Claim
protocol. PostgreSQL API
coverage proves the durable clone and replay contract, the routed suite proves
exact Gateway cutover, and the isolated Docker suite proves real rollback apply
and retirement. Workload queries now project the complete immutable requested
template with reference-only Secret bindings, and operation queries expose
explicit rollback lineage. The React console consumes those authoritative
projections for deployment history, route/certificate state, complete-template
differences and updates, eligible rollback, and browser-local terminal
operation cleanup. Production now performs bounded DNS TXT ownership
verification through the host resolver. The clean-host release gate now builds
the exact clean Cloud, Runtime, and Gateway revisions, starts pinned PostgreSQL
and registry fixtures, the control plane, and one outbound Docker node, binds
the enrolled node identity to a managed Gateway, then certifies bootstrap
through A→B→cloned-A TLS cutover, ordered resumable logs, durable stop, source
cleanliness, host-inventory equality, and credential-safe cleanup. This closes
the first release. The current G0 slices
add a Sources context with canonical GitHub repository identities, an exact
allow/deny policy, provider-neutral anonymous-first branch/tag/commit
resolution, full
immutable commit IDs, explicit digest-bound Dockerfile recipes, atomic webhook
source-identity reservation, PostgreSQL persistence, and tenant-scoped REST
acceptance/query. A separate public GitHub ingress authenticates the exact raw
body with HMAC-SHA256 and stores only a typed branch-push identity and payload
digest in a durable provider-level replay inbox. A provider-neutral checkout
port and Git adapter fetch an accepted commit under isolated Git configuration,
accept an ephemeral repository-bound credential only for the provider fetch,
reject unsafe tree entries, strip `.git`, and commit an immutable content
receipt. The Artifacts context also owns deterministic PostgreSQL-backed
`BuildRun` attempts and a production reconciler that reserve one initial build
per accepted revision, create fresh child attempts for failed or cancelled
runs, and enqueue one exact `cloud.build@3` Operation for each attempt. The
registered Flow replays and packages the credential-free checkout, selects a
compatible node, dispatches a digest-pinned BuildKit client as a Runtime Task,
validates the complete OCI output graph, persists a deterministic digest-only
registry target, publishes and remotely verifies every reachable descriptor,
generates SPDX and SLSA documents, signs and locally verifies their DSSE
envelope with an Ed25519 local or Vault Transit provider, persists the complete
evidence, and durably removes both the Task and checkout before terminal
completion. The
node-control boundary now also provides command-bound mTLS Artifact streaming:
the control plane stores content-addressed directory archives, and the node
agent verifies, safely materializes, mounts, captures, replays, and reclaims
their exact Runtime input/output identities. A
tenant-scoped GitHub App connection boundary now binds one
verified installation/account to one Cloud organization using single-use
installation and OAuth state, S256 PKCE, and transient GitHub user-token
verification. Environment-owned repository
subscriptions now bind that verified installation to exact
repository/branch/recipe policy,
and the provider inbox atomically fans out immutable revisions only through
the exact active connection. Signed GitHub installation, installation-target,
and App-authorization deliveries reconcile versioned connection status,
retain terminal history, and write typed replay receipts plus outbox facts.
Installation-token authentication and private checkout are
implemented with local provider evidence: the App PEM key and token are
materialized only per attempt, and no credential enters source state, URLs,
receipts, responses, or events. The operator-supplied real private-repository
gate is implemented but has not been run with operator credentials. External
private-provider certification therefore remains unrecorded. Signed build
evidence is implemented and restored fail-closed from PostgreSQL. The companion
external Registry/Vault workflow implements two real `SIGKILL` recovery
boundaries and has passed a local real-provider rehearsal, but that rehearsal
is not operator certification. Content-addressed BuildKit cache trust is
implemented: cache-required BuildRuns persist an exact validated cache graph,
and retries can stage only the matching immediate parent's read-only Artifact.
The published-build
deployment handoff is implemented: it
accepts an artifact-free service template only for an exact tenant-owned
successful BuildRun whose source revision and remotely verified digest match,
then reuses `cloud.deployment@3` with durable source/build lineage.
Registry publication is implemented and covered by hostile-protocol fixtures
plus an authenticated private Distribution CI gate. The combined
Runtime/BuildKit/Registry gate provisions the operator-controlled shared socket
volume, validates and removes a cache-producing parent, prunes the BuildKit
worker, proves a child cache hit from the parent Artifact alone, then records
the exact isolated Task, publication, signed evidence, replay, and cleanup
evidence.
Unimplemented portions of later milestone sections remain accepted design
until their own exit gates pass. A3S Cloud ships as a Rust modular monolith, a
separate Linux node agent, and a React web application.

The following decisions are fixed for the first architecture:

- A3S Runtime is the required provider-neutral data-plane contract.
- A3S Box is Cloud's sole local Runtime and image-build provider. Cloud never
  selects or falls back to a Docker-compatible provider.
- A3S Power is the required local inference boundary and runs as an ordinary
  Box-hosted Runtime Service. It owns serving and attestation, not scheduling,
  device claims, routing, authorization, or usage accounting.
- A3S Runtime is general purpose. Candidate and Judge remain Bench concepts and
  do not appear in the Runtime core contract.
- PostgreSQL stores business desired state.
- A3S Flow stores durable operation history and coordinates long-running work.
- A transactional outbox publishes committed facts through A3S Event.
- Node agents connect outward over mutually authenticated HTTPS. Nodes never
  receive PostgreSQL or NATS credentials.
- A3S Gateway receives complete, versioned configuration snapshots.
- Asset hosting supports exactly Agent, MCP, and Skill.
- AHP is not a dependency.

## 2. System shape

```mermaid
flowchart LR
    browser[Web browser] --> web[React web]
    web --> api[A3S Boot control-plane API]

    subgraph control[Control plane]
      api --> modules[DDD modules]
      worker[Flow worker and reconcilers] --> modules
      relay[Outbox relay] --> event[A3S Event]
      modules --> pg[(PostgreSQL)]
      worker --> flow[(A3S Flow store)]
      modules --> registry[OCI registry]
      modules --> objects[Object storage]
      git[Hosted Git service] --> gitstore[(Durable Git storage)]
      git --> modules
    end

    agent[Node agent] -- outbound mTLS long poll --> api
    agent --> runtime[A3S Runtime]
    runtime --> box[A3S Box]
    box --> power[A3S Power Service]
    agent --> gateway[A3S Gateway]
    gateway --> workload[Healthy Runtime unit]
```

The API, Flow worker, outbox relay, and reconcilers initially ship in one
control-plane binary with selectable process roles. They share modules and
ports but not in-memory correctness assumptions. A production deployment may
run the roles as separate processes without splitting the domain into network
services.

## 3. Universal A3S Runtime boundary

### 3.1 Resolved Runtime prerequisite

The earlier Runtime contract encoded Candidate and Judge roles, Bench-specific
validation, and caller-owned provider policy. R0 replaced that surface with a
small provider-neutral execution unit. The same managed client now runs finite
Tasks and long-running Services, including ports, health, restart policy,
capability matching, durable identity, and idempotent recovery. Bench-specific
profiles remain outside Runtime.

### 3.2 Core model

The core noun is `RuntimeUnit`, not Candidate, Judge, Asset, Deployment, or
Cloud Workload. A unit has an immutable specification and one of two lifecycle
classes:

```text
RuntimeUnitClass
├── Task       # finite execution: build, evaluation, migration, one-off job
└── Service    # long-running execution: application, Agent, MCP server
```

The general contract contains typed fields for:

- stable `unit_id`, monotonically increasing `generation`, and spec digest;
- a digest-pinned runnable artifact and process definition;
- artifact, volume, and secret-reference inputs;
- resource limits and an isolation requirement;
- network mode, declared service ports, and egress policy;
- optional health checks and restart policy;
- named output artifacts for finite tasks;
- an optional semantics-profile digest used for higher-level attestation.

Mutable image tags, provider command lines, organization IDs, Cloud deployment
states, and arbitrary provider option maps do not belong in this contract.
Providers advertise accepted artifact media types and capabilities before an
application submits a unit.

The Runtime core may own `ProviderId`, provider factories, and a provider
registry for reuse by other products. Cloud binds exactly one provider ID:
A3S Box. It schedules by required capabilities and fails closed when Box cannot
satisfy them; it does not select a provider from login state or configuration
and has no fallback. Bench and Code own their own explicit selection policies.

The provider-neutral client surface is:

```text
capabilities()       -> RuntimeCapabilities
apply(request)       -> RuntimeObservation
inspect(unit_id)     -> RuntimeObservation
stop(request)        -> RuntimeObservation
remove(request)      -> RuntimeObservation
logs(query)          -> ordered log chunks       # capability-gated
exec(request)        -> attached execution       # capability-gated
```

`apply` covers both initial creation and convergence to a newer generation.
Every mutating request has an idempotency key and deadline. Repeating the same
key and canonical request returns the same logical result; reusing the key for
different content is a conflict. A lower generation is rejected, and provider
loss is reported as `unknown`, never silently recreated under a new identity.

Observations distinguish desired convergence from lifecycle state. A Task may
reach `succeeded`; a Service converges while `running` and healthy. The common
states are `accepted`, `preparing`, `starting`, `running`, `stopping`,
`stopped`, `succeeded`, `failed`, and `unknown`. Removal is represented by an
explicit not-found observation rather than a fabricated successful execution.

Capabilities use structured sets instead of provider names or a growing list
of product-specific booleans. They describe supported unit classes, artifact
media types, isolation levels, network modes, mount kinds, health-check kinds,
resource controls, logs, exec, durable identity, and cancellation. Scheduling
fails closed when the required capability set is unavailable.

### 3.3 Domain profiles stay outside Runtime

Bench owns Candidate/Judge validation and converts a validated Bench execution
profile into a Task `RuntimeUnitSpec`. Candidate checkpoints, submission
snapshots, Judge protected results, and their privacy rules are interpreted by
Bench. Runtime only enforces the generic mounts, output descriptors, isolation
requirements, resource policy, and bound semantics-profile digest.

A3S Cloud performs a similar projection from an immutable `WorkloadRevision`
to a Service `RuntimeUnitSpec`. Runtime does not import Cloud domain types.
Builds and migrations use the same client with Task units. Agent and MCP are
ordinary Service units at this boundary; Skill is an immutable input binding,
not a runnable Runtime class.

The planned Inference profile follows the same boundary. Runtime may advertise
generic accelerator capabilities and enforcement modes, accept exact device
bindings, mount Artifacts, and report allocation evidence. Models, inference
backends, tensor/pipeline parallelism, model routes, usage, Inference scaling
intent, and the Workloads-owned effective autoscaling policy remain Cloud
concepts. A typed backend compiler converts an immutable
Inference deployment revision into an inference-managed Workload execution
plan. The compiler targets A3S Power, while Runtime and the node agent remain
inference-neutral and never branch on Power or an internal engine name.
Inference route revisions persist only a validated same-environment reference to
an Edge-owned DomainClaim, logical Gateway scope, hostname and binding
generation. Edge remains authoritative for certificate, target-set and applied
Gateway state.
The complete design and release gates are in
[`inference-plan.md`](inference-plan.md).

Runtime deliberately does not own:

- tenants, projects, environments, assets, or releases;
- scheduling across nodes;
- build graphs, deployment workflows, routes, certificates, or DNS;
- Candidate/Judge rules or evaluation scoring;
- caller authentication state or default-provider selection policy;
- provider installation and cluster membership.

This keeps Runtime reusable without turning it into a second control plane.
Function invocation, schedules, interactive sessions, and batch fan-out are
higher-level profiles or orchestration patterns over Task and Service; they do
not require more product-role variants in the core lifecycle enum.

### 3.4 Provider conformance

`RuntimeClient` owns protocol semantics. `RuntimeDriver` owns provider calls.
The shared managed client owns idempotent reservation, monotonic generation,
reattachment, terminal-state protection, and durable operation identity.
Cloud composes the shared A3S Box driver directly. It does not implement a
second Box lifecycle adapter or retain another provider driver. Other Runtime
consumers may certify other drivers without expanding Cloud's provider set.

The Runtime repository must expose a conformance suite. Each provider must
prove duplicate apply, process restart and reattachment, stale-generation
rejection, capability mismatch, stop/remove idempotency, bounded cancellation,
typed Service endpoints, every advertised health-probe kind, threshold and
timeout behavior, current inspection, log ordering, and truthful loss reporting
against a real provider. Cloud consumes that one provider-neutral observation
through the existing Node Agent command journal; it does not add a probe worker
or health registry.

## 4. Control-plane modular monolith

The control plane uses A3S Boot modules, typed dependency injection, CQRS, the
request pipeline, OpenAPI, configuration validation, and lifecycle hooks. Each
business module follows the repository's four-layer DDD rules in Rust form:

```text
modules/{context}/
├── domain/
│   ├── entities/
│   ├── value_objects/
│   ├── repositories/
│   ├── services/             # traits only
│   └── events/
├── application/
│   ├── commands/{use_case}/
│   └── queries/{use_case}/
├── infrastructure/
│   ├── persistence/
│   └── integrations/
├── presentation/
│   ├── controllers/
│   └── dto/{request,response}/
└── module.rs
```

Domain code has no A3S Boot, SQL, HTTP, Runtime, Flow, Event, or provider
imports. Application handlers depend on domain repository and service traits.
Infrastructure implements those ports. Controllers only validate transport
input, establish tenant context, and dispatch a command or query.

Cross-context mutation happens through application ports or commands, never by
writing another module's tables. Domain events are integration facts after the
originating transaction commits; they are not a substitute for an invariant
inside the same transaction.

| Module | Commands owned by the module | Important outbound ports |
| --- | --- | --- |
| Identity | create organization, manage membership/token | password/identity provider, audit |
| Projects | create project/environment, request deletion | operation coordinator |
| Sources | verify and own a provider installation; authenticate and accept provider webhook delivery; resolve and accept immutable external source revision | GitHub App authorization, provider webhook verifier, source resolver, build coordinator |
| Assets | create asset, accept Git revision, publish/yank release | Git store, artifact registry |
| Artifacts | build, register, verify, sign, retain artifact | A3S Box build port, OCI registry, object store, signer |
| Fleet | issue enrollment, accept node observation/log batch, drain/revoke node | certificate authority, node control, log object store |
| Workloads | create revision, deploy, stop, update, roll back | scheduler, Runtime dispatch, Flow, Fleet log metadata |
| Inference (planned I0) | register model/backend revisions, create/revise/scale model service, publish model route | artifact resolver, managed Workloads, Fleet inventory, Edge target sets, Identity principals, metrics |
| Edge | claim domain, publish/remove route | DNS verifier, Gateway publisher, ACME |
| Data | provision database/volume, back up, restore | Runtime dispatch, object store |
| Secrets | create version, bind, rotate, revoke | envelope encryption, node secret delivery |
| Operations | start/cancel operation, rebuild projection | A3S Flow, audit, notification |

### 4.1 Management web delivery

The production React output is not served by the control-plane API and A3S
Gateway does not read application files. `a3s-cloud-web-server` is a bounded
private HTTP service for the immutable `web/dist` tree. It provides exact
content types, non-cached HTML entrypoints, immutable caching for hashed
assets, client-route fallback, path containment, and browser security headers.
It reserves `/api` so bypassing Gateway cannot turn an API request into an HTML
success response.

The shipped Gateway ACL profile is the public same-origin boundary. Exact
`/api` and `/api/*` routers have higher priority and preserve the request path
to the control plane; the catch-all router sends every other path to the SPA
service. Gateway owns the listener, observability, and deployment TLS while
both upstreams remain private. This avoids embedding generated frontend bytes
in the business API, avoids a second public origin and CORS policy, and does not
require Cloud to deploy its own UI as a tenant Workload during first bootstrap.

Local development intentionally remains separate: the Rsbuild server owns hot
reload and proxies `/api` directly. The monorepo `just cloud` command starts the
API and development web process under one signal boundary; `just
cloud-gateway` exercises the production topology after building the SPA.

## 5. Data and consistency ownership

PostgreSQL is authoritative for aggregates, desired state, idempotency records,
the outbox, and UI projections. A3S ORM supplies parameterized queries,
transactions, migrations, and PostgreSQL access. All database access must go
through A3S ORM; direct driver access from a business persistence adapter is
forbidden. New or modified ordinary reads, JOINs, ordering, aggregation,
inserts, updates, deletes, and concurrency controls must use typed tables and
query builders. If the typed AST cannot preserve a required database
primitive, the capability must be filed, implemented, and tested in A3S ORM,
then Cloud must upgrade its pinned revision before the business behavior
ships. A local raw-SQL workaround is not an accepted permanent boundary for
new or modified persistence. Each aggregate row carries a version; commands
use optimistic concurrency rather than last-write-wins.

The Flow event store uses a separate PostgreSQL schema. A business transaction
does not attempt a distributed transaction with Flow. The deployment command
first commits a `Deployment` and outbox row. An idempotent operation starter
then ensures the Flow run exists with `deployment_id` as its business key.
Periodic reconciliation repairs a crash between those two actions.

The outbox relay publishes through A3S Event and records delivery attempts.
Consumers deduplicate by `event_id`. In a single-process installation Event may
use its local provider; scaled installations may use NATS. Event delivery is
never the only way to discover unfinished desired state.

## 6. Deployment and reconciliation

A deployment follows these durable steps:

1. Commit an immutable requested template and queued deployment.
2. Resolve the source to a commit SHA and/or OCI digest.
3. For an initial deployment, enumerate ready nodes whose reported Runtime
   capabilities and current Fleet inventory satisfy the spec; for an update,
   require the previous Runtime node to remain eligible and consider only that
   node.
4. Compile canonical CPU, memory, and optional ephemeral-storage requirements,
   reserve the exact current-inventory capacity under a deterministic Claim ID,
   and then persist the node assignment. Replay recovers the assignment from a
   claim committed before a process crash.
5. Lease an exact Claim prepare command to that node. The Agent revalidates its
   current inventory and journals the binding before acknowledging preparation.
6. Lease a Runtime apply command carrying that prepared binding. The Agent
   rejects a missing or changed binding and keeps Cloud placement identity
   outside the nested Runtime request.
7. Wait for an observation that matches the Runtime unit/generation, Claim ID,
   and binding digest, then persist the Claim as bound.
8. Run the declared health check through the actual service path.
9. When the previous revision owns routes, stage a complete Gateway snapshot
   and a durable `GatewayRouteCutover` without mutating the active route rows.
10. Wait for an `applied` acknowledgement matching the exact node, command,
   Gateway revision, and snapshot digest.
11. Replace all affected route targets atomically and select the healthy
    candidate as active. The deployment enters `retiring` when a previous
    Runtime revision exists.
12. Issue the deterministic stop command for the previous Runtime revision and
    require durable stopped-or-absent evidence.
13. Release the previous Claim with an exact higher Claim generation/digest and
    Agent acknowledgement before the deployment becomes terminal `active`.

The `H0.1` persistence foundation overlays this existing workflow rather than
creating another deployment engine. `WorkloadControl` owns the managed-owner
reference and effective placement policy. One stable `WorkloadReplica` and
`WorkloadReplicaMember` currently represent canonical ordinal zero.
`DeploymentReplicaBinding` freezes the deployment revision, replica
generation, member, node, placement generation, and opaque Runtime unit
identity. Runtime and Gateway never receive organization, logical placement,
replica, member, or claim identities.

Fleet separately owns immutable node resource inventories. One strict snapshot
contains an enrolled node and Agent identity, positive generation,
content-addressed digest, observation time, and a canonical sorted set of
stable resource slots. The digest covers only the versioned canonical slot
content, so the same capacity remains addressable across Agent restart.
Migration 042 persists every historical snapshot, normalized slots, and one
current head. An exact historical replay is accepted without moving that head;
a new snapshot must advance generation exactly once, advance observation time,
and change content.

The node agent currently proves host CPU and state-filesystem capacity and
Linux `MemTotal` when available. It omits memory on unsupported hosts and never
fabricates accelerators, ports, volumes, or networking. Its local secure state
retains inventory generation and digest across restart and advances them only
when the canonical slots change. A v2 heartbeat is accepted only when it binds
the current Fleet generation and digest for that exact Agent identity. Legacy
v1 observation batches remain readable during the protocol migration.

Hard resources are reserved by an independent Workloads repository. A
canonical request names the exact stable slots and allocation shapes. The
repository transaction locks and verifies the exact current Fleet inventory
head before it reserves any slot. CPU, memory, and ephemeral storage are shared
scalar capacities: each stable slot is serialized, active allocations are
summed, and an over-capacity request is rejected. Accelerators, host ports, and
volumes remain exclusive. Every successful reservation assigns a monotonically
increasing slot generation and a new fence token, then persists one
digest-bound claim:

```text
reserved_in_db
  -> preparing_on_agent
  -> prepared_on_agent
  -> bound_to_runtime_unit
  -> releasing
  -> released
  +-> orphaned
```

`orphaned` remains an active allocation, including when a timeout caused the
uncertainty. Release requires exact slot generation/token evidence from the
Agent, a provider NotFound result, or a trusted compute fence. PostgreSQL keeps
immutable per-claim slot evidence and a current slot ledger. Releasing a claim
and marking every claim slot released are one transaction; exclusive release
also clears the matching ledger owner. Migration 043 limits active-slot
uniqueness to the exclusive resource kinds and allows multiple bounded scalar
claims on one CPU, memory, or ephemeral-storage slot.

Migrations 040, 041, and 043 own these tables. Migration 044 admits the
versioned `resource_claim_prepare` and `resource_claim_release` kinds to the
durable Fleet command queue. The complete Workloads repository uses A3S ORM
typed table definitions and query builders for ordinary CRUD, aggregate reads,
the claim/slot JOIN, generation lookup, idempotency, and outbox writes. The same
typed AST owns transaction-scoped advisory locks, targeted row locks,
`SKIP LOCKED`, and parameterized JSONPath Secret-binding predicates.
Architecture tests reject raw SQL or direct drivers anywhere in Workloads
production persistence.

The current requirements compiler deterministically maps CPU, memory, and
optional ephemeral storage to inventory-backed scalar slots and one topology
digest. PID limits remain Runtime-local. Deployment Flow reserves before
placement, skips a candidate only for a typed capacity conflict, and recovers
the reservation-before-placement crash gap. A stopped normal path may cancel
only an unissued `reserved_in_db` claim with database evidence.

The Agent command journal reconstructs exact prepare, bound Runtime,
stop/remove fencing, and release state after restart. It rejects a Runtime
apply that omits or changes an active prepared binding, revalidates that binding
against current inventory, and adds the Claim ID plus binding digest to apply
and inspection observations. A bound Claim cannot release until the journal
contains stopped-or-absent evidence for the same Runtime generation. Cloud
validates the prepare/release acknowledgement against the exact command,
persists allocation-binding evidence, retries release by advancing Claim
generation and digest, and retains ownership when stop or release evidence is
rejected or ambiguous. Provider `not_found` is fencing evidence only when it is
the successful inspection carried by a Runtime stop result; a rejected
`not_found` or `stale_generation` outcome is never accepted as release
authority.

New deployment operations use `cloud.deployment@3`. Versions 1 and 2 are
registered only to replay runs persisted before routed-update and
resource-Claim semantics respectively. Create, update, rollback, source
handoff, and Secret-rotation derivation use the same application-owned current
identity. At most one nonterminal deployment may exist for a workload.
Cancellation is available during resolution, scheduling, and apply, but closes
when the deployment enters `verifying`, because health-verified work may
already be participating in a Gateway cutover.

Manual rollback enters this same workflow through:

```text
POST /api/v1/organizations/{organization}/workloads/{workload}/rollback
{"revisionId":"<older-revision-id>"}
```

The application accepts only an active running workload and an older revision
of that same workload whose deployment reached `active` with an activation
timestamp. It never changes `active_revision_id` back to the source identity.
Instead, it clones the source's exact resolved template and template digest into
the next monotonically increasing generation, pins the request to the resolved
artifact digest, and revalidates all referenced Secret versions. The new
operation input carries `rollbackSourceRevisionId`, allowing the workflow to
reject a candidate that does not exactly clone its declared source or whose
source was never active.

The rollback idempotency scope is bound to organization and workload. Durable
replay is checked before mutable workload and Secret validation, so an exact
retry returns the first committed revision, deployment, and operation even
after the workload later stops or its referenced Secret state changes. A
different source revision under the same key is an idempotency conflict. Once
accepted, rollback has no special data-plane branch: health, routed cutover,
activation, `retiring`, and deterministic cleanup of the revision it replaces
use steps 7–10 above.

The reconciler compares database desired state with the last accepted node and
Gateway observations. It periodically scans all nonterminal and stale records,
so a lost event, restarted worker, or expired command lease cannot strand work.
Only one reconciler lease may advance an aggregate generation at a time.

Each external step has its own attempt timeout, retry policy, and total
deadline. Source resolution, image pull, Runtime apply, health stabilization,
Gateway publication, certificate issuance, log idle time, and Flow run lifetime
must not share one global timer. Cancellation stops new steps, propagates to the
active Runtime request, waits for a bounded acknowledgement, and records any
cleanup that still requires reconciliation.

A deployment succeeds only when observed Runtime generation equals desired
generation, required health is real and current, and the requested Gateway
revision is active. A failed update leaves the prior healthy revision selected.
If the coordinator restarts after activation, it adopts or replays the
deterministic retirement command and finishes only from durable Runtime
stopped-or-absent evidence.

The PostgreSQL crash gate makes that boundary a real process failure. Its parent
holds retirement command access closed while a child reconstructs Flow and
atomically selects the candidate revision as `retiring`. After proving the
workload points at the candidate and no retirement command exists, the parent
sends `SIGKILL`. A fresh coordinator replays the completed activation, enqueues
one deterministic stop for the previous immutable Runtime, and reaches
terminal `active` only after stopped-or-absent evidence. The same probe passes
inside both the Linux Secret/log gate and the isolated real-Docker Cloud
consumer gate.

## 7. Node agent and control protocol

The node agent is intentionally small. It discovers provider capabilities and
provable host capacity, leases commands, calls the local Runtime provider,
persists command outcomes, reports inventories and observations, streams
bounded log chunks, and publishes local Gateway snapshots. It does not
schedule workloads or evaluate tenant authorization.

Enrollment uses a short-lived one-time token. The node creates its private key
locally and exchanges a proof for a short-lived client certificate. Normal
traffic is outbound mutually authenticated HTTPS:

```text
POST /v1/node-control/commands:lease       # bounded long poll
POST /v1/node-control/commands/{id}:ack
POST /v1/node-control/inventories
POST /v1/node-control/observations
POST /v1/node-control/log-chunks
POST /v1/node-control/gateway-acks
POST /v1/node-control/gateway-certificates:sign
```

A command envelope contains `command_id`, `node_id`, `sequence`,
`aggregate_id`, `generation`, `payload_schema`, `payload_digest`, `issued_at`,
`not_after`, and a correlation ID. The server may redeliver until a durable
acknowledgement exists. The agent rejects expired, regressed, mismatched, or
digest-conflicting commands and returns the previous result for an exact
duplicate.

Before sending a v2 observation batch, the agent detects its current resource
slots, atomically restores or advances its local inventory record, and reports
that exact snapshot through the authenticated inventory endpoint. The control
plane accepts exact replay, rejects skipped or content-conflicting generations,
and returns a receipt bound to the same node, generation, and digest. Only
after that receipt does the Agent send a heartbeat referencing the inventory.
The control-plane transaction rejects an unknown or stale reference instead of
updating node observations against ambiguous capacity.

Gateway publication is a distinct node command and never enters A3S Runtime.
Its payload carries one complete ACL snapshot, a positive revision, the
expected installed revision, a typed certificate request when TLS is required,
an exact SHA-256 digest over the ACL bytes, and an independent issue/expiry
window. Before the first certificate-bearing apply, the node generates or
reuses its private key and CSR, obtains public certificate material through the
authenticated signing endpoint, and verifies identity, SANs, serial,
fingerprint, validity, server usage, CA chain, and private-key match. It then
calls Gateway's native apply endpoint with independent validation and reload
deadlines and queries exact readiness. Before certificate provisioning or
snapshot mutation, it reads `a3s.gateway.version.v1` and selects the complete
advertised management protocol tuple. The exact pre-descriptor Gateway v1
response is a bounded compatibility fallback; unknown version/API schemas,
duplicate protocol declarations, and inconsistent request/status schemas fail
before apply. Gateway's managed-state journal is the sole installed-snapshot
authority; the node agent records only its command outcome. Its v4
acknowledgement binds `command_id`, `node_id`, `gateway_id`, revision, ACL
digest, expiry, applied metadata, readiness, selected protocol, and discovery
mode. The enclosing command acknowledgement is v2. The control plane also
accepts the legacy Gateway-ack-v3/command-ack-v1 pair during migration and
rejects mixed outer/inner schema pairs.

The agent persists its command journal and last accepted generation locally.
Provider labels also bind resources to unit ID, generation, and spec digest so
the journal can be reconstructed after partial disk loss. SSH remains an
explicit break-glass operator action, never the control protocol.

Workload revisions bind Secrets as typed immutable
`secret_id + version + target` records. Runtime specs and Fleet commands carry
only canonical `a3s-cloud-secret://` references. During authoritative artifact
resolution, the control plane performs an anonymous manifest request first. A
Basic or Bearer challenge causes it to reload the exact bound Secret version,
revalidate tenant/project/environment and active-version scope, decrypt only
for the request, and discard the redacted, zeroizing credential afterward.
Only the resolved digest and original reference are persisted. When Docker
must create or restart a container, the driver resolves references through the
existing authenticated node-control mTLS client. The control plane authorizes
the exact revision, assigned node, tenant scope, deployment state, Secret
state, and version before node-boundary decryption. Environment material is
passed directly into the Docker create boundary; file material is written only
beneath the configured Linux tmpfs root and mounted read-only. The node
resolves a registry credential only when the digest-pinned artifact is absent
locally, and derives its registry address from that artifact before Docker
receives pull authentication. Registry credentials never become container
environment, files, or log-redaction inputs. Runtime state files, command
journals, Flow input, events, and provider labels never receive the plaintext.

Rotation restart orchestration begins only from the committed
`secret.version.created` outbox row. A worker locks that event, reloads the
authoritative current version, and ignores an unavailable version or an older
event superseded before work began. It selects only active revisions of
running workloads in the Secret's project and environment. A workload with a
nonterminal deployment is deferred. Otherwise the worker clones the resolved
template, keeps the exact OCI artifact digest and every unrelated field,
advances all bindings for that Secret, and creates the next immutable
generation. The revision, deployment, `cloud.deployment@3` operation,
`workload.deployment.requested` event whose `causation_id` is the Secret event,
idempotency record, and per-workload restart record commit in one PostgreSQL
transaction. A terminal event checkpoint is written only when no affected
workload remains. Advisory locking and unique event/workload records make
concurrent workers and post-commit process loss converge to one deployment.
The later operation reconciler cannot dispatch a Runtime command until that
transaction is visible, so the Secret version is necessarily durable first.

The isolated Cloud consumer gate exercises the rotated Runtime apply across
both provider and agent process death. A child durably reserves the exact
Runtime request, creates the healthy Docker container with materialized Secret
bindings, and pauses before completing the pending receipt. The parent verifies
the receipt and provider identity, restarts only the labeled isolated Docker
provider, proves the same container remains, and sends `SIGKILL` to the child.
A reconstructed Runtime client rebinds the same node and Secret transport,
reattaches the exact container, completes and locally replays the original
receipt, verifies `0400` file material and fully redacted logs, then removes the
container and tmpfs material. Runtime receipts, command state, and provider
labels remain reference-only throughout.

Successful Runtime apply/remove completions are also projected from the command
journal into restart-safe active log targets. A separate node-agent loop reads
ordered provider chunks after the durable cursor and stages at most one batch
through the shared `outbound_batch::DurableOutboundBatch` primitive before
upload. The typed batch protocol validates the exact receipt before settlement,
and restart replays the identical batch ID and content before reading more
provider logs. `LogShippingState` embeds the primitive in its existing
version-1 JSON field, so per-unit-generation cursor advancement and pending
removal publish atomically. ACL configuration bounds polling independently and
closes each batch at 256 chunk/gap records and 16 MiB of log text.

Before Docker returns stdout/stderr, the driver resolves every immutable Secret
reference bound to that Runtime unit. Authorization or materialization failure
fails the log read closed. Exact values are redacted in overlap-safe order, and
the temporary raw Docker text buffer is zeroized before the sanitized chunks
leave the driver. A missing requested cursor returns the typed permanent
`cursor_lost` Runtime boundary. A durable Runtime unit whose Docker source is
absent, including an explicit Docker 404 during the read, returns
`source_disconnected`; transient Docker transport and availability errors stay
retryable and never become gaps.

The Linux Secret/log acceptance path binds one active encrypted PostgreSQL
Secret version to both an environment variable and a `0400` file, plus a
separate encrypted credential to an authenticated private registry. It proves
anonymous registry access fails, resolves the exact digest through the
credential-aware production control-plane resolver, removes the cached fixture
image, and pulls the private digest through the production node Secret
materialization path.
The workload proves both injected values agree without embedding plaintext in
its Runtime spec, emits the value on real stdout and stderr, and verifies that
only redaction markers leave the Docker driver. The node-side fixture runs as
root, matching the isolated release runner, while the workload container stays
unprivileged with every capability dropped. A child test process writes part of
the sanitized batch through the production immutable filesystem adapter and
exits immediately after the synced object publication, before PostgreSQL
receipt persistence. The parent proves no batch or chunk metadata committed,
reconstructs the repository/store/handler boundary, adopts every exact object,
and receives one non-replayed receipt followed by an exact replay. It then
overwrites only the non-secret recovery marker, requires replay to leave the
accepted immutable object untouched, and queries the same position as an
ordered `corrupt` gap through the tenant-authorized REST API while both
redacted Secret records remain readable. The gate scans control-plane rows,
Flow history, node state, and durable log objects for both Secret plaintexts
and requires its run-specific tmpfs Secret root to contain no files after
cleanup. The separate real Docker recovery profile retains the pre-restart log
cursor across isolated provider process death.

The node validates the discontinuity's exact unit, generation, and requested
cursor, assigns the next monotonic Cloud sequence, and includes the gap in the
same durable replay protocol as chunks. An acknowledged gap clears the provider
cursor but retains the delivery watermark. The next read starts at the earliest
available provider record, and each returned source sequence is rebased to at
least the prior delivery sequence plus one. A continuous source disconnect is
reported once; a successful source read re-arms detection for a later,
independent disconnect.

For either selected object adapter, control-plane `all` and `worker` roles also
run a bounded retention scan. Eligibility uses the durable Fleet `received_at`
timestamp, not a node-supplied observation time. The worker first performs an
idempotent object deletion and only then compare-and-sets `retained_at` on the
metadata row. A deletion failure leaves active metadata for retry; a metadata
commit interruption repeats the idempotent deletion on the next scan. Multiple
workers may inspect the same row safely. Persisted batch replays are recognized
before object writes, so an acknowledged retained batch cannot recreate its
body.

An independent `all`/`worker` loop selects at most the configured number of
tombstones whose durable `retained_at` predates the tombstone retention cutoff.
One PostgreSQL transaction locks eligible rows with `SKIP LOCKED`, deletes their
batch memberships and per-chunk metadata, and inserts continuous sequence-range
markers. Adjacent markers for one node, unit, and generation are coalesced
across cycles. Batch headers and payload digests remain durable for exact replay,
and the maximum live, provider-gap, or compacted sequence is a durable watermark
that rejects an unseen non-advancing sequence. Queries surface each marker as an
explicit `compacted` gap. Original provider cursors, observation times, and
stream values are intentionally discarded, so stream-filtered queries
conservatively include compacted ranges.

The S3-compatible adapter uses conditional create for every immutable object.
An exact replay compares the existing bytes and returns the original logical
result; different bytes at the same key are a conflict. Reads enforce the same
size, schema, report, and checksum validation as the filesystem adapter, and
deletion is idempotent. Readiness uses a unique write/read/delete probe.
Credentials are resolved only from configured environment-variable names.
Production ACL must select the S3 adapter and forbids HTTP endpoints; custom
HTTP endpoints remain an explicit development-only option.

## 8. Gateway and edge publication

For the first vertical slice, A3S Gateway runs on the workload node. Edge owns
a logical `GatewayScope` inside one organization, project, and environment;
it stores an ordered desired physical member set, a membership generation, and
`min_ready`/`max_unavailable` rollout policy. The first member remains the
bootstrap primary for the current cardinality-one route compiler. Routes
persist both logical and physical identities, while the managed snapshot
protocol remains node-addressed and Gateway does not interpret Cloud tenancy.
A publication may target only the workload's active immutable revision, a
declared TCP port, and a current healthy Runtime observation whose node matches
the bootstrap primary. Runtime observations expose the selected node-local
socket as a typed Service endpoint; one stateless Edge adapter compiles its TCP
socket into Gateway's canonical HTTP origin. Box execution, forwarding, probe,
and port-binding details do not cross into the Route domain.

The durable `RouteTarget` projection binds that origin to the immutable
workload revision, deterministic
`workload:{workload_id}:revision:{revision_id}` Runtime unit, positive Runtime
generation, declared port, and canonical observation time. Initial publication
accepts only the observation owned by the active deployment's exact Runtime
command. Routed update accepts only the candidate deployment command's
observation at the desired healthy generation. A future, stale, mismatched, or
forged observation cannot create a route target.

Hosted MCP routes reuse that same evidence path. Each `McpRoutePolicy` also
pins one tenant-qualified `DomainClaim`; migration 053 enforces that exact
foreign-key binding. Publication candidates accept the hostname only while the
claim is verified, covers it, and has not changed after the observation time.
The Claim ID and aggregate version remain attached to the candidate so later
revocation cannot race publication. The MCP target compiler accepts only a
canonical policy, its exact immutable Service profile, a profile/release-bound
`WorkloadRevision`, and already resolved healthy Runtime targets. It
revalidates tenant, Workload, Asset, AssetRelease, profile digest, Runtime port,
health path, Unit ID, generation, and node-local endpoint alignment.
AssetRelease identity and profile digest are copied only from the immutable
revision binding; the endpoint is copied only from `RouteTarget`.
Callers control only priority and positive weight. Targets are sorted
canonically and receive a stable UUIDv5 identity derived from route, node,
Runtime Unit, and generation. Empty, duplicate-node, mixed-revision,
non-contiguous-priority, and overflowing-weight sets fail closed. Credential
authority resolution, scope-complete planning, complete snapshot composition,
and durable staging are separate `MCP0.3` layers rather than target-compiler
responsibilities.

The MCP projection planner is the read-side orchestration boundary. It verifies
that the mutable route policy, immutable profile binding, Workload revision,
and desired Gateway scope have the same organization, project, environment,
AssetRelease, Workload, and validity window before consulting Runtime state.
It then asks the existing `IRouteTargetReader` for exactly one current healthy
target per desired Gateway member, using the profile's declared Runtime port.
Partial or wrong-node sets fail before projection. Because the current Runtime
contract exposes node-local loopback sockets, Cloud then selects only the
target whose node is the physical Gateway receiving this snapshot. That local
target has priority zero and weight one, and the router name derives from the
route ID. A remote member's `127.0.0.1` endpoint must never be interpreted as a
socket on the receiving Gateway; remote target routing remains unavailable
until `H0.3` defines and proves the cluster-private endpoint contract. Later
traffic policy remains Cloud-owned rather than becoming a Runtime or Gateway
scheduling decision.

The one-route Gateway projection planner then resolves exactly the credential
IDs named by that route's grants. The repository query is bounded to 10,000
unique non-nil IDs and one exact organization/project/environment scope; it
does not list or project unrelated environment credentials. Missing or
cross-scope identities, stale generations, expiry, and revocation fail closed.
The resulting projection contains one immutable profile, its one route, only
the referenced verifier generations, and the earlier of route-policy expiry
and credential expiry. It is validated as one complete Gateway contract before
return. The planner returns a node-bound value that rejects any Runtime target
whose node differs from the receiving physical Gateway.

The pure MCP projection assembler accepts one to 1,000 independently planned
one-route fragments only when every fragment is bound to that same physical
Gateway. It revalidates each fragment at one canonical observation time,
requires exactly the route's profile and credentials, deduplicates only
field-for-field-equivalent shared profiles and credential authority, rejects
route/router ownership collisions, takes the earliest expiry, sorts the merged
collections canonically, and validates the final complete contract. It cannot
assign a managed revision or publish.

Durable desired-route enumeration is an exact typed A3S ORM read over one
organization, project, environment, logical Gateway scope, and canonical
observation time. It excludes expired policies in PostgreSQL, joins each
immutable Service profile in the same query, sorts by route identity, and
requests 1,001 rows so exceeding the 1,000-route snapshot bound fails instead
of truncating. Cross-scope and cross-tenant policies are never materialized
into the candidate set. A durable worker must still resolve every enumerated
route to its active release-bound Workload and healthy local target.

The projection-input reader performs that materialization with at most 16
concurrent reads. Every policy must resolve to its immutable profile, verified
DomainClaim, a running Workload's current active revision, and the exact
organization, project, environment, Asset, AssetRelease, hostname coverage,
and profile-digest binding. A missing, stopped, revoked, future, stale, or
differently bound input aborts the complete candidate; it is never skipped.
The complete-set planner then plans at most 16 routes concurrently for one
receiving Gateway and feeds the entire result into the conflict-safe assembler.
It retains canonical hostname/path/router bindings and rejects duplicate
ingress ownership before consulting Runtime. Each accepted credential
contributes both its grant generation and its credential aggregate version to
the candidate. The assembler rejects fragments that observe different
authority versions for the same credential. This matters because revocation
advances the aggregate version without having to advance the generation. An
empty active set is represented explicitly as no MCP projection.

The pure complete-snapshot composer now joins that MCP candidate with the
physical `GatewayScopeState` and every active ordinary `Route` plus its exact
verified `DomainClaim`. It requires one canonical observation/issue time, the
next physical revision, the exact installed-revision expectation, and the
receiving node's logical-scope membership. It rejects stale or cross-tenant
domain authority, duplicate ownership, and any ordinary `PathPrefix` that
would cover an exact MCP path. Certificate DNS names are the canonical union
of ordinary and MCP Claim patterns. The emitted A3S ACL is one complete
managed document containing ordinary routers/services, exact MCP ingress
routers, fail-closed and healthy target services, the top-level `mcp` policy,
and management configuration. An empty MCP candidate omits the complete MCP
surface, removing the final stale `mcp` block while preserving ordinary
routes.

The compiled value retains the physical scope state, ordinary Route versions,
all ordinary and MCP DomainClaim versions, and the complete MCP plan; that plan
already retains the policy/digest, Workload/revision, and credential
generation/version evidence.

Durable staging now consumes that complete value in one PostgreSQL
transaction. It locks the physical Node, exact logical scope and ordered
membership, physical scope, complete active ordinary Route set, complete active
MCP policy set at the planning observation, every DomainClaim, every referenced
Workload/active revision, and every credential generation. Policy create and
update lock the same logical scope row before their policy row, so a concurrent
insert cannot appear as an unobserved active-set phantom. Ordinary Route
publication already serializes through the same Node and physical-scope
authority. Any scope, membership, installed revision, route, policy, claim,
Workload rollout, credential rotation, revocation, or pending-publication drift
rejects the candidate before commit.

An accepted stage writes the pending `GatewayPublication`, optional provisioning
certificate, immutable `mcp_gateway_snapshot_publications` kind/tenant marker,
next physical scope revision, and one secret-free
`edge.mcp-gateway.snapshot-staged` Outbox fact atomically. The event binds the
logical and physical identities, command, revision, snapshot digest, ordinary
and MCP Route IDs, DomainClaim IDs, and optional certificate ID without
including credential verifiers. Migration 056 binds the marker to the exact
logical scope, receiving Node, publication revision, command, and digest. The
marker is durable recovery evidence rather than another state machine:
`GatewayPublication` remains the sole delivery-state authority, and
acknowledgement code does not infer publication kind from Outbox retention.

The bounded `McpGatewaySnapshotReconciler` scans pending marker/publication
joins and enqueues the existing idempotent Fleet
`GatewaySnapshotInstall` command. A queue failure leaves the publication
pending for the next process or cycle; a repeated enqueue uses the same command
ID and becomes Fleet replay rather than another mutation. A command that
reaches its exact deadline becomes `unavailable` atomically with any still
provisioning or issued certificate, without advancing the installed revision.
Clock regression fails closed.

The existing Agent CSR/signing path can issue the staged certificate after
command delivery. The acknowledgement projector recognizes the immutable MCP
marker before ordinary Route publication kinds, validates the exact
node/command/revision/digest and zero-or-one certificate projection, and
atomically records `Rejected` or `Applied`. Only Applied with valid issued
certificate material advances certificate readiness and the physical installed
revision; any active ordinary routes are rebound to the same complete snapshot
identity. Terminal acknowledgement replay remains idempotent.

The PostgreSQL integration fixture rejects a candidate after policy revision
and injects failure at the final Outbox insert to verify no publication,
certificate, marker, event, or scope advance leaks before a successful retry.
It then compiles Fleet dispatch/replay, certificate issuance, and exact Applied
projection. Focused in-memory tests exercise queue interruption followed by
restart, idempotent redispatch, deadline expiry, and future-clock rejection.
Those fixtures are compiled in the normal test gate; an environment-backed
PostgreSQL execution is still required before claiming real database evidence.

Migration 057 gives each immutable marker a second digest for logical desired
state and an exact MCP route count. The digest canonically binds compiler
configuration, logical scope and membership policy, semantic ordinary Route
and DomainClaim authority, complete MCP policy/profile/target projection, and
credential generation/version evidence. It deliberately excludes the
physical revision, command and certificate UUIDs, observation time, mutable
ordinary Route publication binding, and target observation timestamp. A
successful zero-route marker is therefore durable removal evidence without
making later ordinary-only changes MCP-owned. Existing migration-056 rows use
a conservative non-empty sentinel and legacy snapshot digest, causing one
safe repair publication instead of assuming their logical contents.

The registered `McpGatewayDesiredStateReconciler` scans logical scopes that
have an unexpired policy or prior MCP publication. Its ordered UUID cursor
rotates a bounded batch so an old unchanged scope cannot starve later scopes.
Migration 058 makes those scopes triggers rather than publication owners. Each
immutable marker records the canonical desired logical-scope ID set, while
`mcp_gateway_snapshot_heads` points from one physical node to exactly one
latest MCP-owned complete snapshot. The worker unions and deduplicates target
nodes across the scanned triggers, including a head whose node is no longer a
current member. For each node it first reads physical pending-publication and
head state. Any pending complete snapshot defers planning. Otherwise it loads
every active MCP scope containing the node, plans each scope independently,
and merges the complete projections into one node-bound projection. Cross-
scope route, router, ingress, profile, credential, Runtime-observation, and
tenant conflicts fail closed.

The compiler reads the complete physical ordinary Route set and produces one
candidate for all active scopes. Its v2 desired digest binds the ordered scope
set and per-route scope identity but excludes the historical publication
anchor. Staging locks every candidate scope and membership in canonical order,
rechecks the node's complete active scope set, then locks all policy, ordinary
Route, Claim, Workload, credential, and physical-scope versions before
advancing the immutable marker and mutable head in the same transaction. A
first empty set is a no-op. Changed desired state, transition from non-empty to
empty, or a non-empty applied snapshot displaced by another physical revision
stages a replacement. Equal applied state is unchanged; equal rejected or
unavailable state retries only after a bounded delay. An exact Applied
zero-route acknowledgement deletes only the mutable head, leaving immutable
history for replay while ending future historical scans. Dispatch remains the
separate durable marker worker, preserving commit-before-send and idempotent
Fleet replay.

Migration 059 makes marker ownership explicit. MCP-originated snapshots remain
owned by the MCP dispatch reconciler. A single-member, replicated, or
deployment-cutover ordinary Route publication instead uses
`GatewayNodeDesiredStatePlanner` to observe the physical scope, all active
ordinary Route/DomainClaim authority, and every active MCP logical scope at one
timestamp. The Route compiler produces the post-mutation complete snapshot but
retains the pre-write version vector. PostgreSQL locks that full vector before
atomically staging the ordinary Route/rollout/cutover publication,
certificate, MCP marker/head, and both secret-free events. The ordinary
dispatcher and acknowledgement projector remain authoritative for
Route/rollout/cutover lifecycle; the marker is projected orthogonally so an
Applied zero-MCP publication releases its head and a non-empty MCP publication
remains current.

Migration 057 also replaces the original marker-to-primary-scope foreign key
with the logical scope's exact tenant boundary. Physical Node and publication
foreign keys remain exact, while current secondary membership is checked
under the staging transaction's ordered membership locks. Historical
publication evidence therefore neither rejects secondary members nor blocks a
later membership change.

Exact rollback and certificate convergence/renewal still need to consume the
unified node plan. Proactive MCP-only certificate renewal, revoked-credential
cleanup, public lifecycle surfaces, audit, an executed PostgreSQL gate, and
joint real-process recovery remain required before this path can close
`MCP0.3`.

Hosted MCP service credentials are distinct from Cloud management API tokens.
An API token is organization-scoped management authority with the `a3s_`
format and a SHA-256 lookup digest; it cannot be projected into Gateway.
The Edge-owned `McpCredential` aggregate is instead bound to one organization,
project, and environment, uses the fixed `cloud-mcp` audience and
`a3s_mcp_` lookup prefix, and retains only a bounded Argon2id PHC verifier.
Rotation replaces both prefix and verifier, advances the credential generation,
and leaves the stable credential ID unchanged. Revocation is terminal and
advances only the aggregate version. Debug and serialized projection views
redact the verifier. Migration 055 stores the aggregate through typed A3S ORM
with an exact environment foreign key, globally unique fixed-length prefixes,
bounded verifier/version checks, optimistic updates, and tenant-filtered reads;
every restored row is revalidated by the aggregate and shared Gateway
contract. Exact route-grant resolution now uses only the requested IDs within
the route's tenant scope and requires the persisted generation to remain
active at projection time.

The internal credential issuer obtains 64 bits of random fixed-length lookup
prefix plus 256 bits of bearer secret and a separate 128-bit salt, derives the
bounded Argon2id verifier on the blocking pool under a four-operation
semaphore, and persists the aggregate before returning the bearer value. The
result owns the secret in zeroizing memory and is neither cloneable nor
serializable; Debug output is redacted. A uniqueness conflict discards the
entire candidate and retries with fresh identity, prefix, secret, salt, and
verifier at most four times. Credential lifetime is positive and capped at 365
days. This primitive is intentionally not a public lifecycle surface yet:
idempotent one-time delivery must recover or compensate a
commit-before-response failure without persisting plaintext or returning a
second secret. Rotation delivery and durable atomic removal or replacement of
revoked grants also remain unfinished. Cloud management credentials must never
be accepted as a shortcut.

The compiler sorts every active route plus the proposed route for the physical
node and emits one deterministic, versioned ACL snapshot. Physical
`GatewayScopeState` permits only one pending complete snapshot per node. Its
PostgreSQL transaction binds Route, logical scope, physical revision, snapshot
digest, command ID, original correlation ID, idempotency record, and outbox
fact. A replay therefore reuses the first Fleet command identity even when the
retry arrives under a new HTTP request ID. The application checks this durable
replay before consulting current workload health, so later observation expiry
or workload-state drift cannot turn an already accepted identical request into
a conflict.

Each generated service carries the target revision, Runtime unit, and
generation in the canonical ACL bytes. The snapshot digest therefore changes
when the Runtime generation changes even if the node-local origin is reused.
Gateway applies the resulting complete traffic policy; it never derives a
Runtime target or generation itself.

Incremental route mutation is forbidden because a partial retry could expose a
route to the wrong tenant or revision. Snapshot publication uses compare-and-
swap against the previous installed revision. Fleet persists a Gateway
acknowledgement before projecting it into Edge. Only an `applied`
acknowledgement matching the exact node, command, revision, and digest moves a
route from `publishing` to `active`; rejection is terminal and replay is
idempotent. Rejected direct publications and revoked-claim convergence release
their hostname/path ownership only after they are no longer reachable, so a
later verified claim can publish the same tuple without weakening uniqueness
for `publishing` or `active` routes.

Routed workload updates use a separate `GatewayRouteCutover` record because the
candidate is healthy but is not yet the active workload revision. Staging
stores the previous and candidate generations, candidate route projections,
and the complete publication identity while leaving every live route row
byte-identical. It rejects an equal or stale generation, a reused immutable
revision, a changed declared port, or any active route that does not share the
same prior revision, generation, and node. A mismatched acknowledgement is
rejected without changing the cutover, route rows, or active revision. A
matching `rejected` acknowledgement makes only the cutover terminal and
preserves the prior revision, unit, generation, origin, and observation. A
matching `applied` acknowledgement atomically replaces all of those target
fields for every affected route; deployment activation may select the
candidate only after that applied cutover is durable.

Migration 035 backfills legacy routes and serialized cutover projections from
immutable workload revisions. PostgreSQL then enforces the deterministic unit
identity, positive and increasing generations, observation ordering, and
composite workload/revision/generation references. Migration 036
deterministically creates one logical scope per legacy
organization/project/environment/node binding, backfills Route, cutover,
and idempotency documents, preserves certificate-convergence route-version
records, and adds composite tenancy/node foreign keys. Recreated repository
tests prove exact target and scope recovery, while migration probes reject
forged identities, revision-generation pairs, cross-environment scopes, and
wrong-node bindings. Migration 037 adds the selected management protocol,
request/status schemas, and discovery mode to Gateway acknowledgements. Legacy
rows remain null because the migration does not invent negotiation evidence;
new rows must store either the complete supported tuple or no tuple.

Migration 038 preserves every legacy primary as the first scope member and
backfills the single-replica rollout policy. Migration 039 adds one durable
`GatewayRollout` plus a per-member projection. Each member has an independent
Gateway revision, command, snapshot digest, expiry, optional certificate, and
terminal outcome. Reaching the policy threshold makes the aggregate ready to
serve, but only exact acknowledgement from every desired member makes it
succeeded. Once every member is terminal, any rejected or unavailable member
makes the result explicitly degraded. Staging and acknowledgement transitions
are transactional, versioned, and recoverable through PostgreSQL without
assuming an atomic reload across Gateway processes. A worker-role reconciler
selects a bounded set of active rollouts, restores each aggregate, replica, and
publication with one typed A3S ORM CTE/JOIN query, and idempotently writes every
pending publication to the durable Fleet command queue. A queue or process
failure is retried from PostgreSQL; once the exact command deadline passes, the
same optimistic aggregate transition records that member as unavailable.
Per-member healthy target derivation and snapshot compilation are still
required before this foundation becomes a replicated traffic path.

Every Edge PostgreSQL adapter uses A3S ORM typed tables, queries, and
expressions. This includes logical Gateway scopes and membership, publications,
routes, cutovers, acknowledgement projection, DomainClaims, managed
certificates, certificate convergence, and replicated rollouts. Correlated
`EXISTS`, scalar aggregate subqueries, `COALESCE`/`LEAST` ordering, optimistic
updates, row locks, and the DomainClaim table lock remain inside the typed AST.
A source architecture test rejects raw SQL and direct database drivers in Edge
production persistence, and the PostgreSQL 17 foundation gate exercises the
combined recovery paths.

Domain claims are organization, project, and environment scoped. Canonical
exact names cover only themselves; a wildcard covers exactly one label. A route
can compile only from verified claims that cover every hostname in the complete
snapshot. Development uses a deterministic local proof verifier, while
production constructs an asynchronous resolver from the host DNS configuration
and fails startup closed when that configuration is unavailable. The production
verifier requires the caller's proof to exactly match the issued challenge
before lookup, joins split TXT fragments in wire order, and accepts only an
exact constant-time match from a bounded response. An absent or stale TXT value
leaves the claim `pending` without consuming the idempotency key so the same
request can be retried; timeout and resolver failures expose only a sanitized
temporary-unavailability error.

The compiler emits one HTTPS entrypoint with TLS 1.2 as the minimum, unions and
sorts the required SAN patterns, and binds one typed certificate request into
snapshot schema v3. The exact digest covers the ACL bytes; certificate intent is
validated independently and may be omitted when validity renewal reuses the
already installed certificate paths. The control plane stores claim state, CSR
digest, serial, fingerprint, leaf certificate, and CA bundle. It never receives
or persists the Gateway private key. The node creates that key and CSR under
its configured managed directory, keeps the key at mode `0600`, reuses the pair
after interruption, and atomically writes the verified certificate chain before
Gateway validation and native apply.

The node command journal is committed before Gateway mutation. The dedicated
real-process crash gate pauses immediately after A3S Gateway accepts the reload,
verifies that the new listener and Gateway journal are live while no Cloud
acknowledgement exists, and sends `SIGKILL` to the child agent. A reconstructed
executor rebinds the same command ID to a new lease, repeats native apply
idempotently, queries exact Gateway readiness, persists the applied command
outcome and acknowledgement, then survives another reconstruction without a
second Gateway mutation. Only the simulated command-ack receipt advances the
durable command-journal cursor.

Gateway certificates move from `provisioning` to `issued`, then become `ready`
only after the exact applied Gateway acknowledgement; provisioning may fail and
a ready certificate may be revoked. The development Gateway CA is separate from
the Fleet/node CA and overrides CSR SANs with the desired set. Production uses
an independently selected Vault Gateway PKI provider, mount, and role over the
same bounded HTTPS/token client used by node PKI and Transit. The request sends
only the CSR, requested DNS set, and lifetime; Vault returns the public leaf, CA
bundle, and provider serial. The private key remains on the node. The adapter
accepts only one non-CA ServerAuth leaf with the exact requested DNS set, a
matching provider serial, bounded actual validity, and a CA-only bundle. It
records actual certificate validity, revokes by that provider serial, sanitizes
provider failures, and bounds each successful Vault response to 2 MiB.
Transport, timeout, HTTP 429, and server failures leave the certificate
`provisioning` so the node can retry its same persisted CSR; invalid or
policy-rejected responses remain terminal.

The worker/all process roles run `GatewayCertificateReconciler` with
`run_once(now)` as the injected-time seam. Each cycle first redispatches durable
pending publications, then scans installed scopes against independent
`now + certificate_renewal_window_ms` and
`now + snapshot_renewal_window_ms` bounds. Certificate renewal,
provider-certificate revocation, revoked domain ownership, projection drift,
and snapshot validity renewal stage a separate
`GatewayCertificateConvergence` record with a deterministic node/revision
command identity. Staging does not mutate active route rows. Snapshot renewal
copies the exact installed ACL bytes, retains the digest and active certificate,
omits certificate intent, and advances the validity window to 24 hours. A
matching rejected acknowledgement preserves the previous installed certificate
and routes. A matching applied acknowledgement atomically binds every retained
route to either the replacement or retained certificate, rejects revoked-claim
routes, and advances the installed scope revision. When no verified routes
remain, the complete management-only snapshot intentionally carries no
certificate request.

Provider revocation is a later retryable phase. A ready certificate is selected
only after a newer revision is installed and no active route references it.
The provider serial is revoked first, then the public certificate projection is
marked `revoked`; provider or projection failure leaves the certificate
eligible for another idempotent attempt. The REST command
`POST /api/v1/organizations/{organization_id}/domain-claims/{claim_id}/revoke`
is idempotent under `route:write`, emits `edge.domain-claim.revoked`, and never
removes reachability before the exact route-less or filtered snapshot
acknowledgement.

Production configuration requires Vault for node PKI, Gateway PKI, and Transit
and fails startup closed without valid credentials or provider names. A
dedicated Ubuntu CI job builds Cloud's pinned Gateway revision and proves the
node-generated key, managed chain, native exact apply/readiness, trusted
DNS/SNI HTTPS request, durable revision against a loopback upstream, and forced
process-death recovery at the apply-before-acknowledgement boundary. A second
real-binary gate replaces both the independently signed certificate and target
origin, proves the prior CA and exact selector no longer work, removes the
superseded certificate directory, and restarts Gateway to recover only the
replacement target. Gateway's native journal is the sole applied-snapshot
recovery authority.

I0 extends this projection from one upstream to complete healthy target sets.
Inference owns model aliases, primary/fallback intent, access policy, and usage
semantics; Edge owns transport targets and Gateway revision state. OpenAI model
selection runs in an optional Gateway inference-dispatch stage, not in a
control-plane HTTP handler. Gateway receives only complete, versioned ACL
snapshots, forwards only healthy Workload revisions explicitly allowed by the
current prior/candidate rollout generation, and durably spools ordered usage
facts without becoming the authority for models or tenants.

## 9. Source, build, and asset hosting

The generic source pipeline is:

```text
source reference -> immutable revision -> build/provenance -> artifact digest
                 -> workload revision -> deployment
```

External Git inputs resolve a branch or tag once, then build the pinned commit
with a Runtime Task. OCI inputs resolve a tag once and deploy only the manifest
digest. Build cache keys include tenant scope, immutable source digest,
canonical recipe digest and platforms, digest-pinned builder, operator
BuildKit socket-volume identity, cache schema, and execution-semantics profile.

The GitHub App connection boundary owns installation authorization, not
repository subscription or checkout credentials. An organization-authorized
`POST /api/v1/organizations/{organization_id}/source-connections/github`
creates or replaces one short-lived awaiting-installation flow and returns the
fixed GitHub App installation URL. PostgreSQL stores only the SHA-256 digest of
its random 32-byte state. GitHub returns to the public
`GET /api/v1/source-connections/github/setup`; Cloud atomically consumes that
state, records the positive installation ID, rotates to a second random
state, and redirects to GitHub OAuth with an S256 PKCE challenge.

The PKCE verifier is not server-side state. It is carried only in a bounded
`Secure`, `HttpOnly`, `SameSite=Lax` callback-path cookie while PostgreSQL
stores its digest. The public OAuth callback matches both digests, reads the
current client secret from its configured environment variable, exchanges the
bounded code without following redirects, and uses the transient user token
for `GET /user` plus at most ten 100-entry pages of
`GET /user/installations`. The setup-provided installation ID is accepted only
when it is present in that user-token intersection; the setup query alone is
never installation authority. Provider bodies and requests are bounded, and
OAuth codes, client secrets, access/refresh tokens, PKCE verifiers, and
provider response buffers are never durable.

Completion atomically consumes the flow, persists one active
`GithubConnection`, and writes `source.github-connection.created` to the
outbox. A Cloud organization has at most one current active/suspended
connection; current GitHub installation ID and account identity are exclusive
across organizations. Terminal history is retained under its original
connection ID. Durable state contains only numeric
installation/account/verifying-user IDs, account kind, display logins, status,
aggregate version, and connection/update times. The tenant GET prefers the
current connection and otherwise returns the latest terminal record, including
`status` and `updatedAt`. Flow responses use no-store and no-referrer policy.
Explicitly disabled GitHub App ACL fields construct a closed unavailable
adapter rather than partial provider behavior.

Connection status is `active`, `suspended`, `verification_revoked`,
`installation_deleted`, or `account_changed`. Only `active` is provider
authority. Both `active` and `suspended` prevent a competing connection;
terminal states require the full installation/OAuth proof again and create a
new connection ID. A terminal record cannot be reactivated by a webhook, and
subscriptions retain their old connection ID rather than inheriting a new
proof.

The durable connection does not enumerate repositories or contain a token.
After anonymous resolution reports unavailable, the application may use only
the same tenant's active verified installation ID to mint a bounded App JWT and
request one repository-scoped installation token with `contents: read`. The
App PEM key is read from its configured environment variable for each attempt. The
provider must confirm selected-repository scope and only read-only contents plus
implicit metadata permission. Any issuance or authenticated-provider
failure collapses to the same unavailable source result. Repository binding and
fanout remain separate transactions beneath this verified ownership record.
Already-issued credentials are provider-managed and may remain usable until
expiry or revocation.

An environment-owned `GithubRepositorySubscription` binds the organization's
verified connection and installation to one canonical GitHub repository, one
exact safe branch, and one explicit canonical build recipe. Tenant commands and
queries use:

```text
POST /api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-subscriptions/github
GET  /api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-subscriptions/github
POST /api/v1/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-subscriptions/github/{subscription_id}/deactivate
```

Creation requires `source:write`, the configured exact repository policy, an
existing environment in the complete tenant hierarchy, and the same
organization's active verified connection. Composite PostgreSQL foreign keys
bind both the environment hierarchy and connection/installation identity. The
transaction locks and rechecks the exact connection in `active` state, so a
concurrent lifecycle change cannot authorize a stale creation. Active natural
identity is environment, connection, repository, branch, and recipe digest;
idempotency and canonical duplicates return the original binding. Explicit
deactivation changes `active` to `inactive` and retains the historical record.
Creation and deactivation atomically persist their idempotency response and the
`source.github-repository-subscription.created` or
`source.github-repository-subscription.deactivated` outbox fact. Subscription
state contains neither a provider credential nor a credential reference.

`POST /api/v1/webhooks/github` is a public provider boundary. It requires JSON
plus GitHub event, delivery, and
`X-Hub-Signature-256` headers. The verifier rejects bodies beyond the configured
bound, reads the configured secret environment variable for each request, and
authenticates the exact raw bytes with HMAC-SHA256 before parsing. The accepted
signature syntax is exactly `sha256=` plus 64 lowercase hexadecimal digits.
Bearer authentication cannot substitute for or bypass this proof.

Deleted pushes, non-branch refs, unsupported lifecycle actions, and unrelated
authenticated events are acknowledged without persistence. A branch push is
reduced to the GitHub provider, bounded delivery ID, canonical repository
identity, positive installation ID, safe branch, full commit object ID,
exact-payload SHA-256 digest, and canonical receipt time. The PostgreSQL inbox
is keyed by provider and delivery ID. An exact-payload replay returns the
stored fact; reusing the key with any changed typed identity or raw-body digest
conflicts in the same transaction. Neither secret material nor the raw payload
is stored.

Only a newly inserted push delivery may fan out. Before the transaction, Cloud
resolves the currently authoritative connection ID for the installation. In
the inbox transaction it joins that exact connection and selects only active
subscriptions whose installation, repository identity, and branch match while
requiring `connection.status = 'active'`. Share locks on the connection and
subscription serialize fanout against lifecycle reconciliation. The immutable
commit is derived directly from the authenticated delivery without resolving
the branch again. Each matching
environment/recipe natural identity creates one `ExternalSourceRevision` and
one `source.revision.accepted` outbox fact. Multiple environments and recipes
fan out independently; no match creates no tenant revision. Tenant delivery
reservations bind one organization and provider delivery to the
repository-plus-commit source identity, so multiple recipes in that
organization remain legal while changed identity conflicts.

Inbox insertion, reservations, every new revision, and every outbox fact are
one PostgreSQL transaction. An outbox failure rolls back the provider inbox as
well. Exact replay never re-evaluates subscriptions, preventing duplicate or
retroactive fanout. This transaction still does not create a build or
deployment. The optional source-revision `webhookDeliveryId` remains a separate
authenticated mutation-time entry to the same tenant reservation invariant.

The same signed ingress recognizes `installation` `suspend`, `unsuspend`, and
`deleted`; `installation_target` `renamed`; and
`github_app_authorization` `revoked`. These deliveries are reduced to typed
event/action, installation-or-user subject, exact-payload digest, and receipt
time in a separate lifecycle inbox. Raw provider bodies and credentials are
not stored. The first accepted fact locks matching active/suspended
connections, applies the state transition, advances aggregate version/update
time, and atomically emits `source.github-connection.reconciled`. Exact replay
does not reconcile again; changed event, subject, action, or digest under the
same lifecycle delivery ID conflicts.

Suspend and unsuspend preserve a same-identity account login; rename updates
that login without losing active/suspended state. Numeric account or account
kind mismatch fails closed to `account_changed`. Installation deletion is
terminal. App-authorization revocation invalidates every current connection
whose proof was supplied by that user; it is not interpreted as installation
deletion. Because GitHub does not supply one uniformly reliable event time for
all of these deliveries, this slice orders first acceptance by local receipt
for each signed fact. Bounded authoritative polling now repairs missed or
out-of-order installation and account lifecycle changes, disambiguates delayed
facts across reconnection, and requires a fresh provider check immediately
before private resolution or checkout. Verifying-user authorization revocation
remains webhook-authoritative because GitHub exposes no tokenless current-user
grant query and Cloud does not persist OAuth tokens.

`POST .../source-revisions` accepts a typed branch, tag, or full Git object ID,
normalizes an exact GitHub HTTPS locator, enforces the configured exact
allow/deny policy, and resolves the reference through a provider-neutral port.
The GitHub adapter uses only the fixed HTTPS API origin, disables
redirects, confirms the response repository identity, requires exact ref
echoing, bounds annotated-tag peeling, and verifies full commit IDs. The
application checks for an idempotent response before contacting the provider;
after one resolution it validates `a3s.cloud.build-recipe.v1`, computes the
canonical digest, and atomically stores the environment-owned immutable
revision, idempotency response, optional webhook repository-plus-commit
reservation, and `source.revision.accepted` outbox fact. Natural identity is
environment, repository, commit, and recipe digest. Mutable ref names and
credential references are not durable source-revision state. Resolution is
anonymous first. Only an anonymous `Unavailable` result may look up the same
organization's verified GitHub connection, issue one short-lived credential
bound to the canonical repository, and retry with a Bearer header. Public
success, non-availability provider failures, and idempotency replay never issue
a token. Token-service and authenticated-provider errors are sanitized so even
a defective adapter cannot reflect a credential. The Build Flow consumes only
the resulting immutable revision and transient checkout authority described
below.

The provider-neutral source-checkout port accepts only a canonical repository,
one full commit object ID, and an immutable checkout ID. Its Git adapter uses a
fresh staging directory and empty Git home, disables system/global
configuration, redirects, credential helpers, hooks, unsafe protocols, tags,
and recursive submodules, and fetches the accepted object ID directly. A valid
repository-bound credential is converted to transient Basic authentication for
`x-access-token:TOKEN` and supplied only through Git's
`--config-env=http.extraHeader=...`; it never enters the remote URL or argument
list. The adapter
requires the detached `HEAD` and tree to match, bounds file count, content
bytes, command output, and total time, rejects unsupported Git tree modes,
gitlinks, unsafe paths, and symlinks that escape the checkout root, then
removes `.git`. Atomic publication records the repository, commit, Git tree,
and a deterministic SHA-256 filesystem digest without credentials. Replaying
the same checkout ID recomputes that digest; another source identity conflicts
and mutated content fails integrity validation. Existing checkout replay never
requires a credential because it performs no provider access. The dedicated
public GitHub CI gate resolves a branch and exercises checkout/replay. A local
smart-HTTP Git fixture proves authenticated fetch, exact header transport, and
credential-free receipt/replay. The ignored real GitHub App test is ready for
operator credentials but has not produced external private-repository
evidence.

Every accepted source revision now has one deterministic initial `BuildRun`
identity. Failed or cancelled runs may form a linear retry chain: attempt
identity is deterministic from the source revision and positive attempt number,
each child records its immediate parent, and every attempt owns a fresh
Operation with the same UUID as its BuildRun. PostgreSQL reservation uses row
locking and a source/attempt uniqueness constraint so concurrent reconcilers
create one initial row. Atomic retry creation locks the parent, permits at most
one child per parent, records the idempotency response in the same transaction,
and rejects nonterminal or successful parents. The aggregate binds the
organization/project/environment, source revision, attempt and parent,
operation ID, immutable input and Runtime artifact identities, exact
node/command identities, validated OCI output, cancellation/failure outcome,
cleanup command, timestamps, and optimistic version. Its state transitions are
exact-replay no-ops; storage accepts only one transition generated by the
aggregate and rejects stale or forged state. A separate reconciler repairs the
durable gap after source or retry commit by enqueuing the same deterministic
`cloud.build@3` request for that attempt. The PostgreSQL gate covers concurrent
reservation and retry, one-child parent lineage, the pre-enqueue crash gap,
operation replay, tenant ownership, foreign-key integrity, cleanup order, and
optimistic conflicts. The production worker runs this reconciler before the
generic operation coordinator; a closed Flow router keeps
`cloud.deployment@1/@2/@3`, `cloud.workload.stop@1`, and
`cloud.build@1/@2/@3` on their own Runtime implementations.

The Artifacts presentation layer exposes environment-scoped BuildRun lists and
tenant-scoped detail. Its public projection includes source/Operation lineage,
status, timestamps, validated OCI metadata, publication state, a bounded
evidence summary, and bounded failure, while excluding node/command identities
and internal input or Runtime Artifact URIs. A tenant-scoped evidence resource
returns the complete immutable SPDX, SLSA provenance, DSSE envelope, and public
signing-key identity; the web console loads and downloads that JSON only on
demand. `build:write` cancellation persists the aggregate transition and
idempotency response atomically. It is deliberately cooperative: the Flow
continues through publication-race adoption, attestation, Runtime removal, and
checkout cleanup before projecting a terminal cancellation. The `build:write`
retry endpoint atomically creates a queued child BuildRun and fresh Operation
only for a failed or cancelled parent; exact request replay returns that same
child, while another key or a second child conflicts. Public REST and web
projections show the attempt number and parent BuildRun and offer retry only
when eligible.

The Artifact transport prerequisite is implemented below that Flow boundary.
Typed download and upload requests bind the authenticated node, durable command
ID, Runtime specification digest, exact mount or output name, media type,
SHA-256 digest, and byte size. The mTLS node-control API authorizes those fields
against the persisted unexpired `RuntimeApply` command before opening a blob or
accepting a body. It streams raw bytes rather than base64, returns explicit
length/content/digest metadata, applies a total transfer deadline, and persists
content-addressed blobs plus atomic replay receipts. A blob/receipt crash gap
is repaired only after the bytes are rehashed.

The node agent independently verifies downloaded and captured bytes before
admission. Its persistent cache separates immutable read-only blobs from
spec-bound mount/output receipts. Directory archives are planned and hashed
before extraction, reject absolute or parent paths, escaping links, devices,
FIFOs, duplicates, non-directory ancestors, and configured entry/file/expanded
limits, and are reverified by path, type, content, link identity, and
permissions after restart. Artifact views are reference-counted by durable spec
receipts; Runtime removal deletes the view and reclaims only blobs with no
remaining reference.

Docker advertises `MountKind::Artifact` and `OutputArtifacts`; node startup
binds this manager before it begins command processing. Artifact inputs become
exact read-only host binds. A successful finite Task is archived through the
Docker API with the declared directory contents at the Artifact root, then the
command executor uploads the verified node-local blob and replaces the local
URI with the control-plane content URI. The safe tar boundary accepts at most
one leading empty `./` directory marker and does not expose a provider-specific
basename when the Artifact is mounted again. Exact command replay, node/client
restart, inspection, and removal retain or retire the same output identity.
The registered `cloud.build@3` Flow
now composes this transport with checkout replay, BuildKit execution, OCI
validation, authoritative registry publication, and cleanup. A separate
Workload command resolves only the deterministic successful BuildRun for the
exact organization, project, environment, and source revision, converts its
verified publication to a digest-pinned Workload artifact, and reuses
`cloud.deployment@3`. Its idempotency identity covers the BuildRun, published
digest, name, and complete artifact-free service template. The resulting
revision retains an `ExternalBuildReference` across rollback and Secret
rotation so Workload and Operation projections expose the originating source
revision and build without trusting a caller-supplied artifact locator.
`cloud.build@2` remains executable for persisted publication-era runs that did
not require evidence, while `cloud.build@1` drains upgrade-invalidated
pre-publication work without changing either persisted step history.

The Artifact-owned `IBuildService` accepts one immutable build ID, an absolute
materialized source directory, the source content receipt digest, and the
accepted recipe. The BuildKit adapter resolves only recipe-owned context and
Dockerfile paths beneath that directory, runs `buildctl` with an empty client
home and no credential, SSH, cache import/export, push, or
privileged-entitlement inputs, and exports an OCI image layout. Unix sockets
and mTLS are the production-capable transports; unauthenticated TCP is
constructible only through an explicitly named conformance option and only for
a literal loopback address.

Acceptance requires the BuildKit metadata digest and descriptor to agree, the
OCI root to bind that descriptor, every reachable index, manifest,
config, and layer to have its declared size and SHA-256 bytes, the inventory to
contain no unreferenced blob, and the config platforms to equal the recipe.
The bounded result and receipt publish atomically by build ID; replay validates
the whole graph again, changed input conflicts, and changed output fails
integrity validation. The local-context CI gate still certifies this adapter
directly.

The production Build Flow closes the previously separate source, Runtime, and
validation boundaries. `SourceBuildInputPreparer` checks tenant identity,
materializes the exact commit anonymously or with one ephemeral installation
token, packages deterministic archive bytes, admits them to the Artifact store,
then performs an offline checkout receipt replay to reject package-time change.
Only nodes advertising Task, container isolation, Artifact/Volume mounts,
Tmpfs mounts, output Artifacts, resource controls, `NetworkMode::None`, and the
builder media type are eligible. The projected Task mounts source and the
BuildKit socket read-only, drops Runtime networking, and also passes
`force-network-mode=none`; it accepts no secret, SSH, or entitlement channel.
Cache-required runs export a BuildKit OCI cache alongside the image output.
The cache validator requires one exact reachable graph, supported media types,
no missing or unreferenced blobs, and an empty ingest directory. A retry may
mount only its immediate terminal parent's key-matching Artifact read-only. It
copies the validated cache tree into a size-bounded, non-executable tmpfs
because BuildKit needs a writable local lock, then imports only that staging
copy. Successful output is rehashed from the mTLS Artifact store and its
complete OCI and cache graphs are validated before a deterministic
`RuntimeRemove` command and checkout deletion. Cache reuse never skips
publication or signed-evidence generation. Flow history persists dispatch
identities before replay, so a crash cannot duplicate apply or removal.

The Runtime gate uses the exact projector, node command journal, Docker driver,
Artifact upload, and OCI validator. Its Dockerfile requires a `RUN` environment
without `eth0` and a failed `wget` attempt while the overall build succeeds.
CI provisions the operator-controlled rootless BuildKit socket volume and
authenticated registry for this implemented gate. Authoritative registry
publication, locally verified signed evidence, evidence API/web inspection,
and explicit published-build deployment are implemented. The gate cancels and
removes the cache-producing parent, prunes all worker state, requires the child
to parse the imported cache manifest and emit a real `CACHED` record,
revalidates identical OCI/cache graphs, publishes and signs the child, and
leaves no managed Task. The manual external-provider workflow now adds private
GitHub resolution, operator Registry/Vault signing, and real process death
after publication and evidence persistence. A local real-provider rehearsal
passes; operator-owned execution and retained revision-bound evidence remain
before G0 verification. BuildRun status, cancellation, retry-as-new-attempt,
ordered log page/SSE, and web controls are implemented; the log projection
reuses Fleet metadata and object storage while redacting node and internal
Runtime identities.

The target hosted-asset publication chain is:

```text
Asset -> Git commit -> validation/release gate -> Artifact -> Listing -> Deployment
```

The implemented `A0.1` foundation defines exact Agent, MCP, and Skill `Asset`
aggregates plus immutable `AssetRelease` identities. Migration 051 and one
tenant-scoped A3S ORM repository enforce organization-name and release-version
uniqueness, optimistic lifecycle transitions, replay through the shared
idempotency table, and atomic typed events through the existing Outbox. Real
PostgreSQL tests prove tenant isolation, archival, publication immutability,
yanked addressability, and rollback of rejected writes.

The first `A0.2` slice adds one `IAssetGitRepository` domain port and a local
durable adapter. The adapter addresses repositories by the tenant-qualified
identity `{organization_id}/{asset_id}.git`, initializes `main`, records and
revalidates immutable schema, organization, and Asset metadata, enables Git
receive and transfer integrity checks, and atomically publishes a staged bare
repository before syncing its parent directories. Concurrent provisioning
converges on one repository, while symlinked paths and changed identity fail
closed. It and Source checkout share the same hardened Git process runner.

This filesystem slice adds no database access, queue, scheduler, or object
storage path. Smart HTTP authorization, PostgreSQL write leases and quotas
through A3S ORM, backups through the shared immutable-object boundary, and
pinned ACL admission through `a3s-acl` remain before `A0.2` is complete. No
publication command, deployment, Skill binding, or catalog read is exposed.

PostgreSQL stores asset and release metadata. The implemented local profile
keeps the authoritative bare Git repository on durable POSIX storage and
addresses it by `(organization_id, asset_id)`; the production profile adds an
A3S ORM-backed PostgreSQL single-writer lease before ref updates. Smart HTTP is
implemented before SSH. The existing immutable-object boundary will hold
atomic repository bundles, backups, release archives, and other
content-addressed artifacts, not a live file-by-file mirror of Git objects.

`.a3s/asset.acl` accepts only Agent, MCP, or Skill manifests. Published releases
bind commit SHA, manifest digest, and artifact digest. Agent and MCP releases
may produce Service units. Skill releases produce bundles that may be bound as
immutable inputs; they are not deployed alone.

## 10. Secrets and security

- Every tenant-owned aggregate row includes `organization_id`; child rows are
  reachable only through tenant-bound foreign keys. Repository methods require
  tenant context and cross-tenant references fail before persistence.
- Secret versions use authenticated provider encryption with a key identifier.
  Ciphertext, key ID, and metadata are stored separately from access policy;
  production Transit/KMS providers own their internal key hierarchy.
- Secret mutation idempotency rows store only the Secret ID and immutable
  version number, then reload authoritative records. Domain events and API
  responses contain metadata but no key ID, ciphertext, or plaintext.
- Rotation restart rows, derived templates, operation inputs, causal events,
  and reconciliation checkpoints contain only Secret/revision/version
  references. The restart worker never invokes a decryption provider.
- A node receives only the exact versions referenced by a revision currently
  assigned to it, inside its authenticated mTLS session and a short-lived,
  non-cacheable material response.
- Plaintext secrets are excluded from Runtime specs, events, Flow payloads,
  command journals, logs, traces, and API responses.
- Build network access is deny-by-default and separately configurable from a
  deployed service's network policy.
- Node certificate revocation immediately prevents new command leases. Drain
  and revoke are different operations.
- Artifact digest verification is mandatory. A successful source build also
  requires locally verified Ed25519-signed evidence bound to that digest;
  admission policy can be tightened without changing workload identity.

## 11. API, web application, and typed clients

HTTP APIs are versioned under `/api/v1`. Mutating endpoints accept an
`Idempotency-Key`. Fast commands return the committed resource; long-running
commands return `202` with an Operation link. The shared A3S Boot interceptor
wraps every response in the repository-standard shape:

```json
{
  "code": 200,
  "message": "Success",
  "data": {},
  "requestId": "uuid",
  "timestamp": "2026-07-14T00:00:00.000Z"
}
```

Errors include HTTP `code`, stable business `statusCode`, safe `details`,
`requestId`, and `timestamp`, and are documented in OpenAPI. Queries use cursor
pagination. Operation updates use resumable SSE with an event sequence; the UI
always reloads the authoritative projection after reconnecting.

REST major version 1 has one committed OpenAPI 3.0.3 contract at
`openapi/v1.json`, served as public raw JSON at `/api/v1/openapi.json`. It is
outside the normal response envelope but uses the same request-ID and contract
version headers. Resolved-route tests generate the candidate document and
require exact snapshot parity. Stable operation IDs, explicit public or bearer
security, mutation headers, request media types, success/error responses, and
shared envelope schemas make the document usable without inferring behavior
from controller code. The client default base path, document metadata, and all
API responses pin contract `1.0.0`.

Pull requests compare the candidate with the base contract. Version 1 cannot
remove a path, method, accepted input, response status, or response schema
field, add a required input, or narrow an input constraint. Additive semantic
changes require a contract version increment. A deprecated operation records
`x-a3s-deprecated-since`, `x-a3s-deprecated-on`,
`x-a3s-sunset-not-before`, and `x-a3s-replacement-operation`; the replacement
must exist and the sunset must remain at least 180 days after announcement.
Removal requires a new REST major contract rather than silently changing v1.

The `packages/cloud-client` TypeScript package is the single REST transport for
the web console and `a3s-cloud` CLI. It validates both envelope variants and the
HTTP/envelope status match, bounds request time, maps invalid responses and
transport failure to stable client errors, and never places a bearer token in a
URL or error. The first `C0.1` CLI slice is presentation-only: it resolves
non-secret context from flags or environment, reads the token only from
`A3S_CLOUD_TOKEN`, and invokes public tenant-guarded queries. It does not persist
context, read PostgreSQL, contact nodes, or infer authorization from hidden
output. The operational read slice adds workload, deployment, route, BuildRun,
signed-evidence, and cursor-paginated workload/build logs through the exact
existing REST queries. Resource identifiers are validated before transport;
log cursors remain opaque and limits stay bounded. Later mutation and MCP
surfaces must continue through this API and the same application commands and
queries.

The first `C0.2` management MCP surface is a presentation adapter beside REST,
not a new application or infrastructure layer. `POST /api/v1/mcp` implements
stateless Streamable HTTP JSON-RPC for MCP `2025-06-18`; global authentication
loads the current API token through the Identity A3S ORM repository on every
request. The adapter derives its organization only from the authenticated
principal, filters and rechecks mutation tools by effective scope, rejects
batches and foreign origins, and dispatches Project, Environment, search, Node,
Operation, Workload, Deployment, Route, and BuildRun tools plus bounded
cursor-paginated Workload and BuildRun logs, signed BuildRun evidence, Workload
stop/rollback, Deployment cancel, and BuildRun cancel/retry to the same
`CommandBus` and `QueryBus` handlers used by REST. The operational commands
require caller-owned idempotency keys and their existing `workload:write` or
`build:write` scopes. Domain-specific MCP
adapters reuse the REST response DTOs; the protocol handler owns transport only
and does not accumulate resource dispatch logic. Tool structured content
contains the standard API success or business-error envelope. It has no session
database, direct repository access, Redis path, node transport, live log
stream, or business rules. The dedicated production-binary gate uses PostgreSQL
17 through the same A3S ORM repositories to prove exact scope-derived catalogs,
strict arguments and annotations, operational read and command semantics,
Project REST-to-MCP replay, exact Workload-stop replay,
hidden-mutation zero-write, indistinguishable foreign and missing Project
errors, and next-request token revocation while scanning responses, logs,
evidence, and the database dump for plaintext credentials. See the
[management MCP contract](management-mcp.md).

The first CLI mutation slice exposes Workload stop and rollback, Deployment
cancel, and BuildRun cancel and retry. Every invocation requires a caller-owned
visible-ASCII idempotency key of at most 255 bytes; the shared client validates
it before transport and sends it only as `Idempotency-Key`. Replaying the same
command with the same key returns the API's durable `replayed` result. The CLI
does not synthesize a key or add a confirmation side channel.

Core resource parity exposes Organization, Project, and Environment creation
through their existing scoped commands. Node `ready`, `drain`, and `revoke`
also reuse the existing Fleet command and require both an idempotency key and
the current positive aggregate version. The CLI validates the UUID and safe
integer before transport; stale versions remain an authoritative Cloud
conflict. No command writes a projection directly or bypasses the A3S ORM
repository adapter.

Administrative diagnostics reuse the existing public `/platform`,
`/health/live`, and `/health/ready` endpoints. The shared client omits the
Authorization header for this operation even when the CLI environment contains
a token. A health endpoint may return HTTP `503` with the standard success
envelope carrying a truthful down report; the client preserves that report as
data. A `503` error envelope is still a `CloudApiError`. The CLI writes the
complete diagnostic result to stdout and returns stable exit code `8` whenever
liveness or readiness is down, so automation can inspect both state and
process status without treating an unhealthy report as a malformed response.

Edge automation uses the existing tenant-guarded DomainClaim, logical
Gateway-scope, and Route controllers. The shared client and CLI expose
DomainClaim list/get/create/verify/revoke, Gateway-scope list/create, and Route
publication; they never read Edge tables or contact Gateway members directly.
A logical scope request carries one through 100 unique member node IDs and
explicit `minReady`/`maxUnavailable` thresholds. Cloud application commands
remain authoritative for ownership, membership, threshold, and route-target
validation, and their production repositories remain typed A3S ORM adapters.

DomainClaim create/verify/revoke and Gateway-scope create now return the
complete public projection plus a durable `replayed` flag. An initial create
uses HTTP `201`, an accepted verify or revoke uses `202`, and replay uses
`200`. Route publication already returns the Route, managed certificate,
request replay state, and Gateway-command replay state. These response
contracts let automation retry safely without reconstructing state from
internal persistence.

Source automation uses the existing Source controllers and application
handlers. The shared client and CLI expose source-revision list/resolve,
GitHub-connection get/begin, and repository-subscription
list/create/deactivate. They submit the closed GitHub repository,
branch/tag/commit, and `a3s.cloud.build-recipe.v1` Dockerfile contracts to
Cloud; they never resolve a Git reference, contact GitHub, read PostgreSQL, or
construct a Source aggregate locally. Source policy, tenant ownership,
provider access, immutable commit resolution, and A3S ORM-backed persistence
remain authoritative in Cloud.

Revision resolution and subscription create/deactivate require explicit
caller-owned idempotency keys and return the durable replay projection.
GitHub-connection begin deliberately preserves the existing security boundary:
it returns a short-lived no-store installation URL and has no replay contract.
The CLI does not persist that URL and recommends JSON output when the complete
value must be copied; provider setup and OAuth callbacks remain browser-facing
Cloud endpoints.

Secret automation uses the existing tenant-guarded Secret query and command
controllers. The shared client and CLI expose metadata list/get plus
create/add-version/revoke-version. Value-bearing CLI commands accept material
only through an explicit `--value-stdin` flag, read at most 1 MiB plus one byte
for overflow detection, use fatal UTF-8 decoding, preserve accepted bytes
without trimming, and clear the input byte buffer. They do not accept
plaintext through arguments, environment, configuration, or a CLI-managed
file. Safe result projection selects only Secret metadata and version state;
value-bearing API errors are replaced by a stable non-secret mutation error.
Cloud remains the sole tenancy, encryption, idempotency, rotation-effect, and
A3S ORM persistence authority, and the CLI never reads Secret tables or
contacts nodes.

Identity automation uses the existing tenant-guarded API-token controller plus
dedicated list/get queries. Both reads require `token:write`, apply the
organization tenant guard, and return only ID, organization, name, scopes,
aggregate version, creation time, optional expiry, and optional revocation
time. Create and revoke retain the existing caller-owned idempotency contract.
The CLI accepts a newly created credential only through `--token-stdin`, reads
69 bytes to detect overflow, requires exactly `a3s_` plus 64 lowercase
hexadecimal digits, uses fatal UTF-8 decoding, and clears the byte buffer. Safe
result projections remove unexpected credential fields, and mutation errors
are replaced by a stable non-secret error. PostgreSQL list/get/create/revoke
all remain in the Identity repository and use typed A3S ORM queries; the CLI
never reads Identity tables.

The API-token surface is:

- `GET /organizations/{organization}/api-tokens`
- `GET /organizations/{organization}/api-tokens/{api-token}`
- `POST /organizations/{organization}/api-tokens`
- `DELETE /organizations/{organization}/api-tokens/{api-token}`

Node bootstrap extends the same typed client with the existing
`POST /organizations/{organization}/enrollment-tokens` Fleet command; it does
not add a second enrollment endpoint or persistence path. The caller supplies
one exact `a3sn_` credential through bounded fatal-UTF-8 standard input, the
CLI clears the input bytes, and the request retains `node:write`, tenant guard,
caller-owned idempotency, one-time use, and the server's maximum 24-hour
lifetime. Successful output selects only credential-free enrollment metadata;
credential-bearing API errors become one stable non-secret failure. The Fleet
repository continues to persist only the digest with typed A3S ORM operations.

The CLI does not install software itself or contact a node. It prints a Bash
invocation that downloads one caller-selected HTTPS Agent binary, verifies its
exact SHA-256 before `sudo install`, prompts for the credential on the target,
and starts the Agent with an already provisioned absolute `.acl` file. The
credential never enters the invocation, argv, configuration, output, or error.
The release URL and digest must be obtained from trusted signed A3S release
metadata; the checksum check prevents byte substitution but does not establish
the trustworthiness of caller-supplied metadata. Cloud never accepts an SSH
password or private key through this path.

Organization-scoped cross-resource search is one query surface:

- `GET /organizations/{organization}/search?q=<query>&limit=<1..50>`

The controller applies `OrganizationTenantGuard` before dispatch, normalizes a
query of 1 through 128 safe characters, and defaults the limit to 20. Migration
050 registers credential-free metadata for Project, Environment, Node,
Workload, Deployment, Route, DomainClaim, logical Gateway scope, BuildRun,
SourceRevision, Secret, and Operation in one PostgreSQL view. Secret values and
all credentials are absent. `PostgresSearchRepository` uses typed A3S ORM
expressions and bound values to rank exact title or ID matches, then title
prefixes, then contained projection text; it deduplicates resource kind and ID
before enforcing the caller's limit. The view is derived from authoritative
tables and never becomes a command, lock, queue, or state authority. No Redis,
external search engine, or presentation-side broad resource read is required.

The TypeScript client and CLI validate the same query and limit bounds and call
only this public endpoint. The React console waits 250 milliseconds after
input, cancels superseded requests, supports pointer and keyboard selection,
and checks each returned contextual hash against its organization, project,
environment, kind, and ID before updating browser history. Route and Deployment
results select their related Workload, BuildRun results select the build panel,
and Operation results open the operation drawer. This `C0.1` boundary is
organization authorization only. Grant-derived filtering and role-focused
search remain `C0.3` work and must be enforced by Cloud queries rather than
hidden navigation.

The `C0.1` release gate starts the production control-plane binary with the
shipped ACL and PostgreSQL 17. Raw REST bootstraps the tenant, the exact
`CloudApi` import consumed by React creates a Project, and the independently
compiled CLI replays that command with the same idempotency key. The reverse
REST-to-CLI replay covers Environment creation, while both client consumers
must return the same authorized search IDs. The gate also verifies stable
conflict responses, Web and CLI cross-tenant denial, next-request token
revocation, the two expected digest-only API-token rows through A3S ORM, and a
credential-free PostgreSQL dump. This evidence exercises presentation adapters
over one application and persistence path; it does not move authorization or
idempotency rules into the client packages.

Workload create/update and SourceRevision deployment use a versioned A3S ACL
admission boundary. The CLI reads at most 64 KiB of valid UTF-8 and transports
the exact bytes as `application/vnd.a3s.acl`; it does not parse or normalize
the document. The Cloud workload controller uses `a3s-acl 0.3.0` with explicit
document, nesting, collection, token, and diagnostic limits, then applies a
closed version-1 schema before constructing the existing request DTO. Direct
create/update requires an `artifact`; source deployment forbids it and derives
the artifact through the proven BuildRun. An ACL update also binds the named
Workload to the targeted Workload ID, preventing a valid manifest from being
sent to the wrong aggregate. JSON Web requests remain compatible and converge
on the same application command and canonical idempotency input. The Cloud CLI
accepts product configuration only as A3S ACL.

Secret mutations require the `secret:write` scope. The initial resource API is:

- `POST /organizations/{organization}/projects/{project}/environments/{environment}/secrets`
- `POST /organizations/{organization}/secrets/{secret}/versions`
- `POST /organizations/{organization}/secrets/{secret}/versions/{version}/revoke`
- `GET /organizations/{organization}/projects/{project}/environments/{environment}/secrets`
- `GET /organizations/{organization}/secrets/{secret}`

Mutation bodies accept a plaintext `value`, but request debugging redacts it
and every response returns version metadata only.

Workload log snapshots use:

- `GET /organizations/{organization}/workloads/{workload}/revisions/{revision}/logs`

The query validates organization, workload, and revision ownership before it
selects the newest assigned deployment for that revision. `cursor=v1:<sequence>`
pages strictly after that sequence; omitting the cursor includes sequence zero.
`limit` is closed to 1 through 256, and an optional
`stream=stdout|stderr` filter preserves sequence order. Each active object key
is validated, then its bounded size, JSON schema, report checksum, and expected
metadata are verified before its body is returned. The filesystem adapter also
rejects non-files and symbolic links. Deleted and invalid objects remain
visible as ordered `missing` and `corrupt` gap records. A row whose body was
removed by the configured retention worker remains visible as a `retained` gap
without an object-store read. Object-storage unavailability is an error rather
than a fabricated gap. Durable provider discontinuities appear as
`provider_cursor_lost` or `provider_disconnected`; they carry the exact nullable
requested cursor and observation time but no stream, so they remain visible
under a stream filter. Once old tombstones are compacted, the query returns a
`compacted` gap with inclusive `fromSequence` and `throughSequence` bounds plus
the number of compacted chunks. Its source cursor, observation time, and stream
are null; paging advances to the terminal sequence. Compacted ranges remain
visible under a stream filter because their original stream metadata no longer
exists.

Live reads use:

- `GET /organizations/{organization}/workloads/{workload}/revisions/{revision}/logs/stream`

The controller validates ownership with the same query before opening SSE.
Each one-second poll requests at most 16 records, and the encoder truncates only
at an ordered record boundary so one event never exceeds 8 MiB of JSON.
`records` event IDs are the terminal `v1:<sequence>`; `Last-Event-ID` resumes
strictly after that position, and idle feeds send keepalives every 15 polls.
Query or object-storage failure terminates the feed instead of fabricating a
gap. The React client reconnects with bounded backoff, deduplicates replayed
sequences, resets when the revision or stream filter changes, and retains at
most 500 records in memory.

The React application is organized by the same bounded contexts. It never
derives success from an emitted event or an optimistic spinner. Deployment,
health, route, operation, log data, and explicit log gaps remain visually
distinct. Authorized search calls the bounded server query and never filters a
broad local resource cache. Workload list and detail polling supply complete
immutable requested templates and exact route projections; organization
polling supplies managed certificate projections; operation SSE remains the
live progress path. Update
dialogs retain one idempotency key for their lifetime, validate and compare the
complete JSON template, and refresh all authoritative projections only after
the command commits. Rollback choices are older `active` deployments with a
persisted `activatedAt`, and operation lineage identifies the selected source
revision. Terminal-operation cleanup changes only a browser-local dismissed-ID
set; Flow history, operation projections, and audit records remain durable.

## 12. Observability and audit

Every API request, command, Flow run, Runtime unit, Gateway revision, event, and
log stream carries the same correlation chain. Control-plane structured logs
redact by field. Unstructured Docker stdout/stderr additionally redacts exact
bound Secret values at the provider boundary before durable shipping. Metrics
cover queue age, reconcile lag, command lease expiry, Runtime convergence,
health latency, Gateway publication, outbox lag, certificate expiry, and node
heartbeat age.

Audit records capture actor, tenant, command, target, result, request ID, and
time. They are append-only business records, not copies of debug logs. A3S
Observer and A3S Sentry remain optional until their identity and redaction
boundaries satisfy these requirements.

## 13. Dependency policy

| Component | Decision | Owned capability |
| --- | --- | --- |
| A3S Boot | Required | HTTP, DI, CQRS, validation, OpenAPI, lifecycle |
| A3S ACL | Required | Typed block-structured product configuration and asset manifests |
| A3S ORM | Required | PostgreSQL queries, transactions, migrations |
| A3S Runtime | Required after generalization | Provider-neutral Task and Service lifecycle, endpoints, and health observations |
| A3S Flow | Required | Durable deployment/build/backup operations |
| A3S Event | Required | Committed integration-fact API over local or NATS providers |
| A3S Gateway | Required for public routes | Proxy, TLS, ACME, atomic reload target |
| A3S Box | Required | Sole node-local workload/build execution, isolation, networking, health probes, mounts, logs, snapshots, and cleanup |
| A3S Observer / Sentry | Conditional | Telemetry or wire security after boundary review |
| A3S Power | Required for I0 | Sole local inference serving and attestation boundary; never model, placement, device, route, authorization, or usage authority |
| A3S Lane | Not initially used | Flow's PostgreSQL task leases already own durable work |
| AHP | Excluded | No required Cloud capability |
| A3S Code, Memory, Search, Bench, Updater | Not product dependencies | No capability in the first Cloud loop |

The A3S dependency list is not the complete infrastructure design. Build,
artifact, key-management, certificate, telemetry, and storage capabilities are
provided by the middleware below rather than reimplemented in A3S Cloud.

## 14. Distributed middleware and infrastructure

### 14.1 Adoption matrix

| Capability | Local / first-node profile | Distributed production profile | Decision |
| --- | --- | --- | --- |
| Transactional state and coordination | PostgreSQL | HA PostgreSQL; PgBouncer when measured connection pressure requires it | Required from F0; remains the source of desired state and leases |
| Durable workflow work | A3S Flow PostgreSQL store and task queue | The same store/queue with multiple leased workers | Required; do not add another job queue for the same work |
| Integration event fan-out | A3S Event local provider | NATS JetStream durable streams and consumers | NATS is required when API, workers, or integrations run as independent replicas |
| OCI build execution | Typed A3S Box build boundary | Isolated Box build workers selected by platform/architecture | Required when external Git or hosted source builds are enabled; build plans are closed A3S ACL and preserve OCI identity/provenance |
| OCI artifact storage | Existing external registry for image-only deployment | CNCF Distribution or Harbor with retention, replication, and access policy | Cloud-owned registry is required when Cloud owns builds |
| Object and log-segment storage | Filesystem adapter for development | S3-compatible storage such as RustFS, MinIO, or a managed S3 service | Required for production logs, asset archives, backups, SBOMs, and provenance |
| Hosted Git repository storage | Local durable POSIX filesystem | Replicated POSIX/block storage with PostgreSQL single-writer leases and object-store backups | Required only when hosted assets are enabled; live loose Git objects do not use S3 |
| Workload persistent volumes | Node-local, single-writer volume provider | Ceph RBD or another fenced attach/detach provider | Required for stateful services; multi-node failover is forbidden without fencing evidence |
| Secret key encryption | Development-only local key provider | OpenBao/Vault Transit or a cloud KMS/HSM-backed key provider | A production profile may not keep the master key as a plain environment variable |
| Node certificate authority | Development intermediate CA | OpenBao/Vault PKI, step-ca, or an HSM/KMS-backed intermediate | Required from node enrollment; root material stays outside the control-plane database |
| Gateway certificate authority | Separate development Gateway CA | Dedicated Vault PKI mount and server-only role | Node private keys never enter the control plane; production validates the returned SANs, usage, serial, validity, and CA bundle before persistence |
| Metrics and traces | Structured logs plus local OpenTelemetry export | OpenTelemetry Collector, Prometheus-compatible metrics storage, and Tempo/Jaeger when trace retention is enabled | Required before production support; backends remain replaceable |
| Durable log search | PostgreSQL metadata plus S3 chunk objects | Loki or ClickHouse only when cross-node ad-hoc search/retention measurements justify it | Do not put high-volume log bodies in PostgreSQL |
| Cache and distributed rate limits | Bounded in-process cache | Redis only when multiple API replicas need shared ephemeral counters/cache | Optional; never an authority for deployments, sessions, or operations |
| Identity federation | Bootstrap owner and scoped API tokens | External OIDC provider such as Zitadel or Keycloak | Optional until SSO is required; authorization remains in the Cloud domain |

NATS does not replace the transactional outbox or reconciliation. JetStream
provides durable distributed delivery after commit; PostgreSQL still proves
whether a command is required. NATS subjects carry event IDs and compact fact
payloads, not secret material, logs, or Runtime command authority.

The Box build boundary is not a second Runtime or scheduler. Build Flow submits
one isolated Runtime Task with an immutable ACL build plan and content-addressed
inputs. Box owns local build execution, cache isolation, snapshots, and cleanup;
Artifacts owns OCI graph validation, publication, SPDX/SLSA evidence, and
retention. Runtime remains unaware of build-system syntax and registry policy.
Missing Box build capability fails closed rather than opening a daemon socket
or selecting a compatibility path.

Registry publication is a separate typed Artifacts port. The Flow first commits
an immutable target containing registry, repository, root digest, media type,
and size. The publisher reopens and revalidates the admitted OCI layout, streams
non-manifest blobs, publishes deeper manifests before the root index or
manifest, and accepts success only after `HEAD` verifies the complete remote
graph. Upload `Location` values may not leave the configured registry origin
and repository. Basic and Bearer credentials are read from the configured
environment reference for each attempt, zeroized after use, and never enter
BuildRun JSON or Flow history. Production rejects anonymous or HTTP
publication; development requires explicit opt-ins. Registry lookup adopts a
completed push after response loss, Flow event loss, or a cancellation/CAS
race without changing the durable target.

The implemented first-node log path writes ordered, checksummed report objects
through an immutable filesystem or S3-compatible adapter and keeps node, unit,
generation, cursor, sequence, observation time, stream, checksum, and object key
metadata in PostgreSQL. Reads revalidate the object and surface missing or
corrupt objects without putting log bodies in PostgreSQL. The retention worker
deletes expired bodies first and then records durable `retained_at` tombstones,
so snapshot queries never silently skip old positions. The production profile
requires HTTPS S3-compatible storage, and the dedicated CI job provisions
digest-pinned MinIO to exercise conditional create, exact replay, verified
read, deliberate corruption, immutable repair rejection, and idempotent
deletion. A separate bounded worker compacts aged tombstones into coalesced
sequence ranges while preserving batch replay and durable sequence watermarks.
Provider cursor loss and source disconnects use typed Runtime errors, durable
node replay, and ordered PostgreSQL gap metadata without creating object
bodies. Real Docker provider restart, control-plane object-before-receipt
process death, filesystem REST corruption projection, and real MinIO
corruption are certified independently. Loki or ClickHouse is introduced only
when product requirements demand global text search at a volume that the chunk
index cannot serve.

### 14.2 Middleware deliberately not selected

- Kafka or RabbitMQ: NATS JetStream covers current event fan-out; Flow/Postgres
  covers business work queues.
- Redis as a primary requirement: no authoritative state, lock, or queue needs
  it in the first architecture.
- etcd or Consul: PostgreSQL leases and the Fleet registry already own control-
  plane coordination and node discovery.
- Elasticsearch/OpenSearch: PostgreSQL metadata search is sufficient until a
  measured asset or audit search requirement exceeds it.
- Kubernetes and a service mesh: neither is required to operate the outbound
  node protocol or first-node deployment loop.
- Temporal: A3S Flow owns durable workflows; adding a second workflow authority
  would split operation history.

Every optional middleware is hidden behind a typed application port. A new
provider is selected by validated A3S ACL and capability discovery, never by
branching on raw backend-name strings inside a domain module.

## 15. Deployment profiles

The verified E0 profile runs one control-plane process, PostgreSQL, a local
registry and object-store adapter, one node agent, Docker, and A3S Gateway.
Production adds external KMS/PKI, S3-compatible storage, OpenTelemetry
collection, and NATS JetStream when roles are replicated. Git builds add
BuildKit and an owned OCI registry. Correctness must remain unchanged: all
coordination uses
PostgreSQL/Flow leases, idempotent commands, and observed state rather than
process memory.

Multi-node scheduling, stateful failover, provider-specific autoscaling, and
federated control planes are later capabilities. The verified E0 release does
not claim them through placeholder abstractions.
