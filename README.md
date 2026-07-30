# A3S Cloud

<p align="center">
  <strong>Self-Hosted Desired-State Control Plane for A3S</strong>
</p>

<p align="center">
  <em>Deploy immutable workloads, converge infrastructure, and operate services on systems you own</em>
</p>

<p align="center">
  <a href="#overview">Overview</a> •
  <a href="#features">Features</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#platform-model">Platform Model</a> •
  <a href="#delivery-model">Delivery Model</a> •
  <a href="#gateway-relationship">Gateway Relationship</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Cloud** is a self-hosted control plane that stores desired state in
PostgreSQL and converges it through durable operations. Organizations, projects,
and environments define the tenancy boundary. Outbound node agents execute
provider-neutral A3S Runtime commands, apply identity-bound A3S Gateway
snapshots, and report durable observations to the control plane.

Cloud accepts intent rather than holding an HTTP request open for deployment
work. A mutation commits desired state and an operation identity, then A3S Flow,
reconcilers, and node command leases continue the work across retries and
process restarts.

Cloud is not a reverse proxy, an inference byte path, or a replacement Runtime.
It owns business state, scheduling and deployment policy, rollout and
autoscaling decisions, complete Gateway policy, operations, and management
surfaces. Runtime owns provider lifecycle mechanics, while Gateway owns
transport and request-path enforcement.

### Basic usage

From the Cloud repository directory:

```bash
just cloud
```

With `A3S_CLOUD_POSTGRES_URL` set to an existing PostgreSQL instance, this
starts the control-plane API and hot-reloading web console. The API listens on
`127.0.0.1:8080` by default. The legacy dependency bootstrap is being removed
under `BX0` and is not part of the Box-only release contract.

```bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
```

## Features

- **Tenant Model**: Isolate organizations, projects, environments, resources,
  commands, queries, and observations
- **Scoped Identity**: Bootstrap the first organization, issue, inspect, and
  revoke expiring scoped API tokens without storing token plaintext
- **Durable Operations**: Persist intent before execution and resume A3S Flow
  operations, leases, retries, cleanup, and projections after interruption
- **Outbound Node Control**: Enroll Linux nodes, rotate mTLS identities, lease
  idempotent commands, persist exact outbound batches until a typed receipt
  settles them, and receive observations without opening inbound node
  management ports
- **Versioned Node Inventory**: Detect real host CPU and state-filesystem
  capacity, report Linux memory when available, preserve content-addressed
  generations across Agent restarts, and bind v2 heartbeats to the current
  Fleet snapshot
- **Immutable Workloads**: Resolve OCI images to digests, create versioned
  workload revisions, schedule an eligible node, and activate only after
  Runtime health evidence
- **Box-Backed Service Health**: Compile the existing A3S ACL HTTP health
  policy into A3S Runtime, consume current kind-neutral health observations
  through the Node Agent command journal, and preserve the same typed endpoint
  for Gateway publication; Box alone executes and certifies HTTP, TCP, and
  command probes
- **Box-Only Execution Migration**: Converge every Task, Service, build,
  network, mount, Secret, log, output, and cleanup operation through the shared A3S Runtime
  contract and A3S Box, with no Docker-compatible daemon or fallback provider
- **Planned Power-Backed Inference**: Compile the first inference profile into
  an immutable A3S Power Service hosted by A3S Box; Cloud retains placement,
  device claims, routing, authorization, and usage authority
- **Managed Replica Foundation**: Persist an inference-neutral owner,
  effective placement policy, stable replica/member identity, exact deployment
  binding, current-inventory requirement compilation, shared scalar capacity,
  Agent-journaled Claim preparation and release, Runtime allocation-binding
  evidence, and process-death fencing without exposing Cloud placement identity
  to Runtime
- **Managed Reachability**: Create tenant-scoped logical Gateway scopes, persist
  ordered physical membership and explicit rollout thresholds, verify domain
  ownership, provision TLS, compile complete expiring ACL snapshots from
  command-bound healthy Runtime targets, atomically stage every physical
  projection, activate only at the acknowledgement threshold, renew unchanged
  policy without reissuing TLS, and recover or roll back each member from exact
  native Gateway state
- **Encrypted Secrets**: Store tenant-scoped immutable Secret versions,
  manage metadata and version lifecycle through the shared client and CLI
  without rendering plaintext, and materialize exact bindings only at
  authenticated registry or assigned-node boundaries
- **Durable Logs**: Ship bounded ordered Runtime logs, preserve explicit gaps,
  redact bound Secrets, replay one exact receipt-gated pending batch after
  restart, store immutable chunks, and expose cursor and resumable SSE queries
- **Safe Changes**: Replace an active workload immutably, preserve the prior
  healthy revision until cutover, and roll back by cloning a proven template
  into a new generation
- **Historical Source Delivery Evidence**: Resolve exact GitHub commits, run
  isolated builds, validate trusted content-addressed caches and complete OCI
  graphs, publish by digest, freeze locally verified signed SPDX/SLSA evidence,
  and hand successful builds to the existing workload deployment path
- **Hosted Asset Foundation**: Persist tenant-scoped Agent, MCP, and Skill
  identities plus draft, published, and yanked release state; enforce canonical
  SemVer, immutable Git and SHA-256 identities, typed OCI or Skill artifacts,
  optimistic concurrency, replay-safe writes, and transactional outbox events
  through A3S ORM
- **Focused Web Operations**: Navigate responsive Overview, Workloads,
  Delivery, and Edge workspaces; route authorized search and validated deep
  links to the owning section; and inspect deployment history, route and
  certificate state, Runtime health, logs, BuildRuns, updates, rollback,
  cancellation, and retry through the shared typed client
- **Typed Automation Slice**: Reuse one validated TypeScript client across the
  web console and `a3s-cloud` CLI, select tenant context without a credential
  file, inspect tenant and operational resources, BuildRun evidence, and paged
  workload or build logs as bounded tables or stable JSON, and request
  stop/rollback/cancel/retry operations with caller-owned idempotency keys;
  create, update, or deploy Workloads from bounded A3S ACL admitted by Cloud;
  create core tenant resources and transition nodes with explicit optimistic
  concurrency; create, verify, and revoke DomainClaims, create multi-member
  logical Gateway scopes with explicit rollout thresholds, and publish routes
  through replay-aware Edge commands; inspect GitHub connection authority,
  resolve immutable source revisions, and manage repository subscriptions
  through the existing Source commands; list Secret metadata, inspect version
  state, and create or rotate versions from bounded fatal-UTF-8 standard input
  without placing plaintext in arguments, environment, configuration, output,
  or errors; and inspect tokenless platform, liveness, and readiness
  diagnostics with a stable unhealthy exit status; manage API-token metadata,
  credential lifecycle, and one-time node bootstrap without rendering
  stdin-only credentials; and search bounded organization-authorized resource
  projections through the API, client, CLI, and Web without broad local reads;
  expose one public raw OpenAPI v1 document, pin the shared client to contract
  `1.0.0`, and reject incompatible or invalidly deprecated contract changes
- **Scoped Management MCP Operational Reads**: Serve the sessionless,
  initialization-based `2025-06-18` Streamable HTTP MCP through the same
  per-request API-token verifier, derive tenant context and tool visibility
  from the current principal, expose Project, Environment, search, Node,
  Operation, Workload, Deployment, Route, and BuildRun queries,
  bounded cursor-paginated Workload and BuildRun logs, and signed BuildRun
  evidence plus scope-gated idempotent Project and Environment commands through
  the existing application buses, reject batching, forged tenant input,
  invalid bounds, and cross-origin confusion, and preserve the standard API
  envelope inside tool results; prove scope-derived discovery, operational
  reads, REST-to-MCP idempotency replay, tenant non-disclosure, immediate
  revocation, and digest-only persistence against real PostgreSQL through A3S
  ORM without creating another persistence path

### Delivery capability matrix

| Gate | Product outcome | State |
| --- | --- | --- |
| `BX0` — Box-only platform | Sole A3S Box execution/build path and Box re-certification of the complete Runtime, deployment, source-delivery, recovery, and cleanup baseline | In progress |
| `PW0` — Power inference boundary | ACL-native immutable Power Service profile, Box MicroVM/TEE evidence, health, inference, recovery, and cleanup | Planned |
| `R0` — Universal Runtime | Task and Service contracts, durable identity, capability matching, and provider conformance | Historical evidence; Box re-certification pending |
| `F0` — Foundation | A3S Boot API, PostgreSQL, tenancy, identity, Flow operations, outbox, projections, and web shell | Verified |
| `N0` — Node control | Enrollment, outbound mTLS, command leases, observations, command journal, and sole Box driver | Historical evidence; Box re-certification pending |
| `D0` — OCI deployment | Digest-pinned revisions, one-node scheduling, apply, health, activation, stop, cancellation, and recovery | Historical evidence; Box re-certification pending |
| `E0` — Reachable service | Managed TLS, complete Gateway snapshots, encrypted Secrets, ordered logs, immutable update, cloned rollback, web operations, and clean-host recovery | Historical evidence; Box re-certification pending |
| `G0` — External source delivery | Pinned Git sources, isolated builds, trusted retry caches, OCI validation and publication, signed SPDX/SLSA evidence, and deployment handoff | In progress |
| `P0` — Developer workflows | Build detection, workload profiles, previews, monorepos, and closed Compose import | Planned |
| `C0` — Control surfaces | Stable REST, CLI, management MCP, grants, collaboration, notifications, audit, and bounded terminal access | In progress |
| `A0` — Release catalog | Immutable Agent and MCP release publication, Agent deployment, and Skill binding through the common source and artifact paths | In progress |
| `MCP0` — Hosted MCP services | Modern `2026-07-28` MCP release admission, Runtime Service hosting, Cloud orchestration, Gateway protocol enforcement, and joint recovery evidence | In progress; unavailable |
| `A1` — Agent execution | Durable conversations, executions, approvals, checkpoints, forks, and trajectories over existing Cloud control paths | Planned (`A1.0` verified) |
| `S0` — Stateful platform | Databases, volumes, fencing, backup, restore, and retention | Planned |
| `H0` — Production scale | Replicas, multi-node placement, private networking, Gateway replication, HA, and measured autoscaling | In progress |
| `I0` — Inference profile | Accelerator-backed serving, OpenAI-compatible traffic, scoped keys, Providers, routing, usage, and governed self-service | Planned |

The original `R0` through `E0` behavior was certified against the retired
Docker implementation. That evidence remains useful regression history, but it
does not certify the current Box-only product contract. `BX0` must reproduce
the complete baseline on exact A3S Box revisions before those gates are
published as verified again. Later gates must reuse the same deployment and
reconciliation path.

`BX0.1` is verified. Cloud pins one certified A3S Runtime/A3S Box pair, starts
the Node Agent with the shared Box Runtime driver, accepts only the closed ACL
`box` configuration, and contains no fallback provider. Local dependencies and
the C0 PostgreSQL gates run through checksum-pinned A3S Box runtime artifacts
plus an exact-revision Box CLI. C0 reaches its Sandbox-hosted PostgreSQL fixture
only through Box's generation-fenced loopback forwarder. The
[real provider gate](https://github.com/A3S-Lab/Cloud/actions/runs/30416879476)
passed every capability advertised by the pinned Box revision; Cloud does not
carry a second provider conformance implementation.

`BX0.2` is verified on the exact pinned Runtime/Box pair. The
[consumer-recovery and hard-resource Claim gate](https://github.com/A3S-Lab/Cloud/actions/runs/30425852930)
replays an Agent crash after Box apply but before command-journal completion,
preserves the exact durable Runtime receipt and provider identity, replaces a
running Service generation, and proves logs and inspection. It also prepares
an inventory-bound CPU/memory Claim, binds it to the exact Box Service
observation across Runtime and Agent executor restarts, rejects release before
durable stop evidence, then releases and removes the resource with empty Box
state. Deployment cancellation is verified by the
[exact Linux gate](https://github.com/A3S-Lab/Cloud/actions/runs/30429412890):
it requires an authoritative Runtime removal receipt before releasing the exact
Claim and recording terminal `Cancelled`. The
[final interruption gate](https://github.com/A3S-Lab/Cloud/actions/runs/30456965598)
kills the Agent after Box has durably removed the Service but before command
completion, then proves exact receipt adoption, Claim preservation until
acknowledgement, one release, terminal cancellation, and zero provider residue.

The first `BX0.3` networking slice is implemented by
[Runtime PR #8](https://github.com/A3S-Lab/Runtime/pull/8),
[Box PR #185](https://github.com/A3S-Lab/Box/pull/185), and
[Cloud PR #95](https://github.com/A3S-Lab/Cloud/pull/95). Runtime owns the
typed, generation-bound Service endpoint contract. Box alone binds loopback
TCP listeners, relays each connection through its existing execution connector,
and closes or reconstructs listeners with the matching execution generation.
Cloud deletes its former endpoint encoding and uses one stateless Edge adapter
to compile the Runtime TCP socket into the canonical HTTP origin consumed by
Gateway. The dedicated real-provider gate starts a Box Service, reads and
replays the exact Runtime observation, reaches the workload through the
compiled Gateway origin, removes the Service, and requires the listener to
close. Cloud adds no endpoint registry, forwarding process, Runtime driver, or
lifecycle store.

The second `BX0.3` slice pins
[Box PR #186](https://github.com/A3S-Lab/Box/pull/186). Box advertises and
provider-certifies HTTP, TCP, and command health through generation-fenced
ports and exec. Cloud keeps one health path: its existing Workload compiler
emits the Runtime policy, and the Node Agent journals the resulting
kind-neutral observation. The real consumer gate applies a health-enabled Box
Service, receives `Healthy`, reconstructs Runtime and the Agent executor,
replays the exact durable observation, obtains a fresh healthy inspection with
the same endpoint, reaches it through the canonical Gateway origin, and proves
removal, `NotFound`, and listener closure. Cloud adds no health worker,
registry, scheduler, or lifecycle state.

Mounts, Secrets, outputs, registry credentials, builds, isolation evidence,
and the clean-host release loop remain release-blocking `BX0.3` through
`BX0.5` work.

The `A0.1` hosted-asset identity foundation is verified against real
PostgreSQL. The domain accepts exactly Agent, MCP, and Skill assets; persists
organization-scoped names and immutable versioned release identities; enforces
the `active -> archived` and `draft -> published -> yanked` lifecycles; and
commits typed events, idempotency records, and aggregate changes atomically
through A3S ORM.

`A0.2` is now in progress. Its first slice provides one local durable bare-Git
adapter addressed by organization and immutable Asset ID, publishes newly
initialized repositories through atomic staging, records and verifies the
organization, Asset, schema, and `main`-branch identity, enables Git object
integrity checks, converges concurrent provisioning, and fails closed on
symlinked paths or changed identity. Source checkout and hosted repositories
reuse the same hardened Git command runner. No Smart HTTP route is public yet;
authorization, PostgreSQL write leases, quotas, backup and restore, and pinned
`.a3s/asset.acl` admission remain before `A0.2` can be verified. Release APIs,
build publication, Workload deployment, Skill binding, and catalog surfaces
remain `A0.3` through `A0.5`, so the complete `A0` gate remains in progress.

The `A1.0` consolidation gate is verified. Workload and BuildRun logs share one
sequence/SSE implementation, Operation snapshots reuse the same polling
transport without fabricating a sequence, log chunks and node Artifacts share
one namespaced immutable-object client behind typed adapters, and the node
agent uses one typed durable outbound-batch primitive for exact restart replay
and receipt-gated settlement. These are reusable foundations only; user-visible
Agent execution begins with `A1.1` only after `A0.3` supplies published,
immutable release identities.

`G0` already includes GitHub App connections and signed webhooks, private
repository access, immutable source revisions, BuildRuns, cancellation, retry,
ordered logs, command-bound Artifact transport, isolated BuildKit execution,
trusted content-addressed retry caches, complete OCI graph validation,
digest-only registry publication, deterministic SPDX/SLSA generation,
locally verified Ed25519 DSSE signing through persistent local or Vault Transit
providers, durable evidence restoration, evidence API/web download, replay
adoption, cleanup, and explicit deployment handoff. The manual external-provider
gate is implemented for an operator-owned private GitHub repository, HTTPS OCI
Registry, Vault Transit Ed25519 key, PostgreSQL 17, and rootless BuildKit. It
also terminates real child processes with `SIGKILL` after remote publication
and after evidence persistence, then proves exact replay and cleanup. A local
real-provider rehearsal passes, but no operator-owned run is recorded because
the repository has no G0 provider secrets. `G0` therefore remains in progress
until that external certification produces retained evidence.

The current `H0.1` foundation maps every existing single-instance Workload to
one stable replica and member. Replica identity survives immutable revision
changes, while every deployment records the exact replica generation, member,
node, placement generation, and opaque Runtime unit identity it projects.
Managed-owner references and the effective placement policy are durable Cloud
state; managed Workloads reject direct mutations that do not carry the exact
owner and policy. Workload list/detail responses expose this control and
placement state explicitly.

Hard-resource reservations define canonical sorted slot requests and a durable
claim state machine for database reservation, Agent preparation, Runtime
binding, release, and operator-visible orphaning. CPU, memory, and ephemeral
storage are shared scalar capacities; accelerator, host-port, and volume slots
remain exclusive. PostgreSQL serializes each slot, sums active shared
allocations, rejects over-capacity reservations, and advances a monotonic slot
generation with a new unguessable fence token. An orphan or timeout continues
to block its allocation; only exact Agent release, provider NotFound, or
trusted compute-fencing evidence makes it reusable.

The deployment scheduler now compiles CPU, memory, and optional ephemeral
storage requirements from the current Fleet inventory. Its PostgreSQL
reservation transaction locks and verifies the exact inventory head, reserves
capacity before persisting placement, and uses the Deployment ID as the
deterministic Claim ID. Replay recovers a crash between reservation and node
assignment from the durable claim, while a capacity conflict on one candidate
continues to the next eligible node. PID limits remain Runtime-local because
the current inventory contract has no PID slot. Cancellation, retirement, and
stop may release a claim with database-only evidence only while it is still
`reserved_in_db`; an issued, prepared, bound, or orphaned claim still requires
Agent or trusted fencing evidence.

Migrations 040 and 041 backfill the replica foundation and add claim,
slot-evidence, and current-lease tables. Migration 043 replaces universal
active-slot uniqueness with exclusive-kind uniqueness so shared capacities can
carry multiple bounded claims. Migration 044 admits the versioned
`resource_claim_prepare` and `resource_claim_release` commands to the durable
Fleet queue. The complete Workloads PostgreSQL repository uses A3S ORM typed
tables and builders for reads, JOINs, ordering, counts, inserts, updates,
idempotency records, outbox writes, PostgreSQL row and advisory locks,
`SKIP LOCKED`, and parameterized JSONPath Secret-binding predicates. No
Workloads production persistence file uses raw SQL or a direct database driver;
an architecture test enforces that boundary. In-memory and PostgreSQL 17 gates
cover exact replay, competing exclusive and shared reservations, over-capacity
rejection, stale inventory rejection, fencing, release, and generation/token
rotation.

Fleet now persists strict `NodeResourceInventory` snapshots, their normalized
slots, and one current generation/digest head per enrolled node. The node agent
detects CPU and state-filesystem capacity, adds Linux `MemTotal` when the host
can report it, and deliberately omits accelerators, ports, volumes, memory on
unsupported hosts, and networking it cannot prove. It stores the canonical
snapshot locally, keeps its generation across restart, and advances that
generation only when slot content changes. The authenticated
`POST /v1/node-control/inventories` endpoint accepts exact replay and exact
next-generation content changes while rejecting skips, reused generations, and
identity conflicts. New v2 observation batches are accepted only when their
heartbeat references the current inventory generation and digest; legacy v1
batches remain readable during migration. Migration 042 and the inventory
persistence adapter use only typed A3S ORM tables, expressions, transactions,
locks, inserts, updates, and joins. Contract, Agent, API, in-memory, and real
PostgreSQL 17 gates cover canonical digests, restart reuse, concurrent replay,
historical replay without head regression, stale-heartbeat rejection, and
claim rejection after the inventory head advances.

Deployment operations now use `cloud.deployment@3`. After database reservation
and placement, Flow sends an exact Claim prepare command and waits for the
Agent's durable journal evidence before it can dispatch Runtime apply. The apply
envelope carries that prepared binding; the Agent revalidates current inventory
and Runtime identity, then adds the Claim ID and binding digest to the Runtime
observation. Cloud persists that evidence before treating the Claim as bound.
Cancellation, failed-candidate cleanup, prior-revision retirement, and Workload
stop release a prepared or bound Claim only after stopped-or-absent Runtime
evidence and an exact higher-generation Agent release acknowledgement.

The Agent reconstructs prepare, bind, stop/remove, and release state from its
command journal after restart. Control-plane reconciliation adopts exact bound
Claims, retries release with a new generation and digest, and never interprets
a rejected `not_found` or `stale_generation` stop as fencing evidence.
PostgreSQL process-death tests cover reservation-before-placement,
activation-before-retirement, Secret-rotation recovery, and stop-before-release
ordering. `cloud.deployment@1` and `@2` remain registered only to replay
persisted histories; all newly created, updated, rolled-back, source-derived,
and Secret-rotation deployments use v3.

The isolated Docker provider suite now makes the `H0.1` process-death
certification mandatory. A child node Agent persists Claim prepare, starts the
bound apply, creates one real container, and pauses before either the Runtime
receipt or command acknowledgement can complete. The gate replaces the
isolated provider process, sends `SIGKILL` to the child, reconstructs both
Runtime state and the Agent command journal, and replays the exact command. It
requires the original single provider unit and exact Claim ID/binding-digest
evidence, rejects release and a capacity-conflicting Claim before fencing,
executes real stop and removal, accepts the higher-generation release, and only
then permits the competing Claim. The provider release gate requires the
stable certification marker plus empty provider and Artifact inventories.
`H0.1` is therefore implemented as a closed exact-SHA acceptance gate; `H0`
continues with logical reachability and multi-node placement.
The closing evidence is Cloud commit
`5cd7c4eebc21905cb2758856d0e96b31a111116c` in
[Docker provider conformance run 30157496417](https://github.com/A3S-Lab/Cloud/actions/runs/30157496417),
where both `Real Docker provider` and `Cloud consumer recovery` passed.

The verified `H0.2` gate adds Cloud-owned logical Gateway scopes plus a
Gateway-native snapshot and generation-bound private-target foundation. A scope belongs to
one organization, project, and environment. Its desired state can contain an
ordered set of physical Gateway members, a membership generation, and explicit
`minReady` and `maxUnavailable` policy; the legacy `nodeId` request remains the
single-member form. `POST` and `GET` on the environment's `/gateway-scopes`
resource create and list these scopes. The per-member planning boundary resolves
one exact active or retiring Deployment, replica binding, Runtime command,
generation, and fresh healthy node-local endpoint for every desired member. It
rejects missing, ambiguous, mixed-revision, and mixed-port target sets, then
compiles an independent complete snapshot, certificate, command, and staged
Route projection for every member. Single-member publication continues through
the established path. Replicated publication commits the logical Route, every
physical projection, rollout, publication, certificate, ownership row,
idempotency result, and outbox fact in one transaction; any conflict rolls back
the complete bundle. Gateway receives only its node-addressed managed snapshot
and does not become the owner of Cloud tenancy.

Every route persists its immutable workload revision, deterministic Runtime
unit identity, positive generation, declared port, canonical node-local HTTP
origin, and observation time. Cloud accepts the target only from the exact
deployment command's current healthy Runtime observation. The compiler binds
revision, unit, and generation into the ACL digest, so even a reused origin at
a new generation produces a distinct snapshot.

Cloud records an applied acknowledgement only after Gateway reports the same
identity, revision, digest, expiry, applied metadata, and ready state.
Before any mutation, the node agent reads Gateway's versioned capability
descriptor and selects the exact management protocol plus request/status
schemas. It also supports the closed pre-descriptor v1 response for an older
Gateway; unknown or inconsistent tuples fail before apply. New Gateway
acknowledgements carry that selection under v4 and the enclosing command
acknowledgement is v2. The control plane still reads the prior v3/v1 pair during
the migration window.

Generation cutover requires a different immutable revision, a strictly newer
generation, and the same logical scope; rejection preserves the prior target,
while an exact applied acknowledgement atomically replaces the complete target
projection. PostgreSQL migration 036 deterministically creates one logical
scope per legacy environment/node binding, backfills Route and recovery
documents, and enforces the full tenancy/node relationship. Recreated
PostgreSQL and migration gates verify recovery and reject cross-environment or
wrong-node publication. Migration 037 stores new protocol evidence without
inventing it for legacy acknowledgements.

The replicated control plane persists one `GatewayRollout`
aggregate with an independent revision, command, digest, expiry, certificate,
and terminal result for every desired physical member. Meeting the configured
threshold makes the rollout ready to serve; only exact success from every
member makes it succeeded, while a fully observed mixed result becomes
explicitly degraded. Migrations 038 and 039 preserve legacy single-member
scopes, add membership and rollout constraints, and recover the aggregate from
PostgreSQL. The complete Edge PostgreSQL repository now uses A3S ORM typed
tables and query builders for logical scopes and membership, publications,
routes, cutovers, acknowledgements, DomainClaims, managed certificates,
certificate convergence, and replicated rollouts. Typed expressions preserve
joins, correlated `EXISTS`, scalar aggregate subqueries, `COALESCE`/`LEAST`
deadline ordering, optimistic updates, row locks, and the DomainClaim table
lock. No Edge production persistence file uses raw SQL or a direct database
driver; a source architecture test enforces that boundary. Any missing typed
primitive must be filed, implemented, and tested in A3S ORM before new
persistence behavior ships. A worker-role reconciler loads each active rollout
and its publications through one typed CTE/JOIN query, idempotently
redispatches every pending member command after process or queue interruption,
and records a member as unavailable only after its exact command deadline
passes. Migration 045 adds atomic logical-to-physical Route projections;
migration 046 adds read-only snapshot observation commands; migration 047
persists per-member physical recovery; migration 048 adds deterministic exact
rollback; and migration 049 makes expired certificate convergence unavailable
without disturbing the prior applied state.

The real pinned-Gateway gates rotate independently signed certificates and
upstream targets, reject superseded certificates and selectors, and recover
only the replacement after restart. Two real Gateway processes use independent
identities, trust roots, native journals, and Agent journals; either member can
continue serving when its peer is lost, and the returning member reconstructs
the exact snapshot without another apply. Agent process death after native
apply but before Cloud acknowledgement also replays without duplicate mutation.
The PostgreSQL 17 gate proves atomic staging, threshold-driven Route activation,
prior-route retention, member observation, deterministic exact rollback,
certificate renewal/revocation convergence, and restart-safe Fleet dispatch.
These gates use Gateway commit
`7a146b6d53635861e5db4870fb4603a5c59c87ee` and close `H0.2`. Multi-node
placement and production HA remain in `H0.3` and `H0.4`, so the broader `H0`
gate remains in progress.

See the [Product Roadmap](ROADMAP.md) for dependencies, sub-gates, current
evidence, and the ordered product portfolio.

## Quick Start

### Requirements

- Rust 1.85 or later
- PostgreSQL 17 or a compatible supported release
- A3S Box for all node-local workload and build execution
- A3S Power for the inference profile
- The A3S Gateway source revision pinned in
  `tools/gateway-conformance/gateway-revision` for routed service operation
- Bun and Node.js 22 or later for the web console and CLI development
- NATS JetStream only when the NATS event provider is selected

Redis is not required by the current Cloud profile. PostgreSQL owns durable
state and leases, A3S Flow owns workflow work, and NATS owns distributed event
fan-out when selected. Planned `I0.2b` requires a typed Redis counter provider
only for limits advertised as globally exact across replicated Gateways;
without it, limits are explicitly per-Gateway approximations. Redis may also
back shared ephemeral cache, but it never becomes authority for desired state,
operations, sessions, locks, or queues.

### Run the control plane

The development recipe creates an ephemeral bootstrap token when one is not
provided and keeps the API and web process under one signal boundary:

```bash
just cloud
```

To run the API directly, provide PostgreSQL and the required
environment-backed credentials:

```bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

Database migrations run during startup. The default development profile uses
the in-memory event provider. The raw OpenAPI 3.0.3 contract is available
without authentication at
`http://127.0.0.1:8080/api/v1/openapi.json`. The served document is the
committed [`openapi/v1.json`](openapi/v1.json) snapshot for REST major version
1 and contract version `1.0.0`; it is not wrapped in the normal API envelope.

### Bootstrap an organization

The caller creates and retains the first API token. Cloud stores only its
SHA-256 digest.

```bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: ${A3S_CLOUD_BOOTSTRAP_TOKEN}" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"${A3S_CLOUD_ADMIN_TOKEN}\",\"expiresAt\":null}"
```

Subsequent requests use
`Authorization: Bearer ${A3S_CLOUD_ADMIN_TOKEN}`. Every mutation also requires a
stable `idempotency-key` header. REST responses advertise the exact contract in
`x-a3s-api-contract-version`. Use OpenAPI and the web console for the current
resource and operation surfaces instead of treating README examples as a
second API specification.

### Use the Cloud CLI

The verified `C0.1` automation surface uses the same typed client as the web console.
The token is accepted only from `A3S_CLOUD_TOKEN`; it is never accepted as an
argument or written to a context file.

```bash
bun install --cwd cli --frozen-lockfile

export A3S_CLOUD_TOKEN="${A3S_CLOUD_ADMIN_TOKEN}"
export A3S_CLOUD_URL="http://127.0.0.1:8080/api/v1"
export A3S_CLOUD_ORGANIZATION_ID="<organization-uuid>"
export A3S_CLOUD_PROJECT_ID="<project-uuid>"
export A3S_CLOUD_ENVIRONMENT_ID="<environment-uuid>"

bun run --cwd cli src/main.ts context show
bun run --cwd cli src/main.ts diagnostics status --output=json
bun run --cwd cli src/main.ts organizations list --output=json
bun run --cwd cli src/main.ts organizations create "Operations" \
  --idempotency-key="tenant:organization:<request-id>"
bun run --cwd cli src/main.ts api-tokens list
password-manager read "a3s-cloud/automation-token" | \
  bun run --cwd cli src/main.ts api-tokens create "Automation" \
    --token-stdin --scopes="project:write,build:write" \
    --expires-at="2027-01-02T03:04:05Z" \
    --idempotency-key="identity:token:create:<request-id>"
bun run --cwd cli src/main.ts api-tokens get "<api-token-uuid>" --output=json
bun run --cwd cli src/main.ts api-tokens revoke "<api-token-uuid>" \
  --idempotency-key="identity:token:revoke:<request-id>"
bun run --cwd cli src/main.ts projects list
bun run --cwd cli src/main.ts projects create "Cloud" \
  --idempotency-key="tenant:project:<request-id>"
bun run --cwd cli src/main.ts environments list
bun run --cwd cli src/main.ts environments create "Production" \
  --idempotency-key="tenant:environment:<request-id>"
bun run --cwd cli src/main.ts nodes list
password-manager read "a3s-cloud/node-enrollment/worker-1" | \
  bun run --cwd cli src/main.ts nodes bootstrap "worker-1" \
    --enrollment-token-stdin \
    --expires-at="<RFC3339-within-24-hours>" \
    --agent-release-url="https://releases.example.test/a3s-cloud-node-agent-linux-x86_64" \
    --agent-release-sha256="<64-lowercase-hex-sha256>" \
    --node-config="/etc/a3s-cloud/node.acl" \
    --idempotency-key="fleet:bootstrap:<request-id>"
bun run --cwd cli src/main.ts nodes drain "<node-uuid>" \
  --expected-version="<current-aggregate-version>" \
  --idempotency-key="fleet:drain:<request-id>"
bun run --cwd cli src/main.ts operations list
bun run --cwd cli src/main.ts search resources "cloud worker" --limit=20
bun run --cwd cli src/main.ts workloads list
bun run --cwd cli src/main.ts workloads get "<workload-uuid>"
bun run --cwd cli src/main.ts workloads logs "<workload-uuid>" "<revision-uuid>" --limit=100
bun run --cwd cli src/main.ts deployments get "<deployment-uuid>"
bun run --cwd cli src/main.ts domain-claims list
bun run --cwd cli src/main.ts domain-claims create "api.example.com" \
  --idempotency-key="edge:domain:<request-id>"
bun run --cwd cli src/main.ts domain-claims verify "<domain-claim-uuid>" "<dns-proof>" \
  --idempotency-key="edge:verify:<request-id>"
bun run --cwd cli src/main.ts gateway-scopes create "<node-uuid-a>" "<node-uuid-b>" \
  --min-ready=1 --max-unavailable=1 \
  --idempotency-key="edge:scope:<request-id>"
bun run --cwd cli src/main.ts routes list
bun run --cwd cli src/main.ts routes get "<route-uuid>"
bun run --cwd cli src/main.ts routes publish \
  "<gateway-scope-uuid>" "<workload-revision-uuid>" "<domain-claim-uuid>" \
  "api.example.com" "/" "http" \
  --idempotency-key="edge:route:<request-id>"
bun run --cwd cli src/main.ts build-runs list
bun run --cwd cli src/main.ts build-runs evidence "<build-run-uuid>" --output=json
bun run --cwd cli src/main.ts source-connections get
bun run --cwd cli src/main.ts source-connections begin --output=json
bun run --cwd cli src/main.ts source-revisions list
bun run --cwd cli src/main.ts source-revisions resolve \
  "https://github.com/A3S-Lab/Cloud.git" branch main \
  --context-path="." --dockerfile-path="Dockerfile" \
  --platforms="linux/amd64" \
  --idempotency-key="source:resolve:<request-id>"
bun run --cwd cli src/main.ts source-subscriptions create \
  "https://github.com/A3S-Lab/Cloud.git" main \
  --context-path="." --dockerfile-path="Dockerfile" \
  --platforms="linux/amd64" \
  --idempotency-key="source:subscribe:<request-id>"
bun run --cwd cli src/main.ts secrets list
bun run --cwd cli src/main.ts secrets get "<secret-uuid>" --output=json
password-manager read "a3s-cloud/database-url" | \
  bun run --cwd cli src/main.ts secrets create "Database URL" \
    --value-stdin --idempotency-key="secret:create:<request-id>"
password-manager read "a3s-cloud/database-url" | \
  bun run --cwd cli src/main.ts secrets add-version "<secret-uuid>" \
    --value-stdin --idempotency-key="secret:rotate:<request-id>"
bun run --cwd cli src/main.ts secrets revoke-version "<secret-uuid>" 1 \
  --idempotency-key="secret:revoke:<request-id>"
bun run --cwd cli src/main.ts workloads create \
  --file=examples/workload.oci.example.acl \
  --idempotency-key="release:create:<request-id>"
bun run --cwd cli src/main.ts workloads update "<workload-uuid>" \
  --file=examples/workload.oci.example.acl \
  --idempotency-key="release:update:<request-id>"
bun run --cwd cli src/main.ts source-revisions deploy "<source-revision-uuid>" \
  --file=examples/workload.source.example.acl \
  --idempotency-key="release:source-deploy:<request-id>"
bun run --cwd cli src/main.ts workloads stop "<workload-uuid>" \
  --idempotency-key="release:stop:<request-id>"
bun run --cwd cli src/main.ts workloads rollback "<workload-uuid>" "<revision-uuid>" \
  --idempotency-key="release:rollback:<request-id>"
bun run --cwd cli src/main.ts deployments cancel "<deployment-uuid>" \
  --idempotency-key="release:cancel-deployment:<request-id>"
bun run --cwd cli src/main.ts build-runs retry "<build-run-uuid>" \
  --idempotency-key="release:retry-build:<request-id>"
```

Use `bun run --cwd cli build` to produce the standalone `cli/dist/a3s-cloud`
binary. Remote endpoints must use HTTPS and end in `/api/v1`; plain HTTP is
accepted only for literal localhost or loopback addresses. See the
[CLI reference](cli/README.md) for context variables, output contracts, and
exit codes. Operational resource and paged-log reads are implemented;
all mutations require an explicit stable idempotency key. Organization,
Project, and Environment creation call the existing resource commands. Node
ready/drain/revoke additionally require the current aggregate version and use
the existing optimistic-concurrency command. Workload create, update, and
SourceRevision deployment accept only a bounded UTF-8 A3S ACL file. Cloud
parses it with `a3s-acl`, enforces the closed version-1 schema, and then
dispatches the same application commands used by JSON clients. `diagnostics
status` reads the public platform, liveness, and readiness endpoints without
sending a bearer token. A legitimate unhealthy health report remains visible
on stdout and returns exit code `8`, while a real API error remains an error.
DomainClaim create/verify/revoke and Gateway-scope create responses expose the
authoritative `replayed` state; route publication exposes both request and
Gateway-command replay state. Source revision resolution and repository
subscription mutations expose the same durable replay state. GitHub connection
bootstrap deliberately follows the existing short-lived no-store browser flow
and should use `--output=json` to preserve the complete installation URL.
Secret create and add-version read the exact standard-input bytes, reject
empty, invalid UTF-8, or values larger than 1 MiB, and never trim the value.
There is no plaintext argument, environment variable, configuration field, or
result field. Replace the illustrative `password-manager` command above with a
trusted provider that writes the intended bytes to stdout. API-token list/get
return metadata only. API-token create accepts the new credential only through
`--token-stdin`, requires exactly `a3s_` plus 64 lowercase hexadecimal digits,
clears the 68-byte input buffer, and projects both successful and failed
mutations without the credential. This input is separate from the caller's
`A3S_CLOUD_TOKEN`. Node bootstrap similarly accepts exactly `a3sn_` plus 64
lowercase hexadecimal digits only through `--enrollment-token-stdin`, clears
the 69-byte input buffer, and calls the existing `node:write` Fleet command.
It returns credential-free token metadata and a Bash invocation that downloads
one HTTPS Agent release, verifies its caller-supplied SHA-256, installs it, and
prompts for the credential on the target without putting it in argv or the
pre-provisioned absolute `.acl` node configuration. The release URL and digest
must come from trusted A3S release metadata; accepting a digest does not create
a trust root.

Organization-scoped search accepts 1 through 128 safe characters and returns at
most 50 credential-free resource projections, defaulting to 20. The public API
applies the organization tenant guard before querying PostgreSQL through A3S
ORM. The shared client, CLI, and Web console all call that endpoint; none loads
broad resource lists for local filtering. Web search debounces requests,
supports keyboard selection, and validates server-generated contextual links
before navigation. This is organization-level `C0.1` authorization, not the
grant-derived resource filtering planned for `C0.3`.

The REST compatibility slice publishes a public, unwrapped OpenAPI 3.0.3
snapshot with stable operation IDs, explicit security, mutation headers,
request media types, success and error statuses, shared envelope schemas, and
the `/api/v1` server boundary. The TypeScript client and every HTTP response
carry the same `1.0.0` contract version. CI compares `openapi/v1.json` with the
pull request base and rejects removed paths or methods, new required inputs,
removed response statuses or schema fields, and semantic changes without a
version increment. A deprecated operation must name its replacement, record
the deprecation version and date, and retain a minimum 180-day sunset window.

The real [`C0.1` cross-surface gate](tools/c0-conformance/README.md) runs raw
REST, the exact shared `CloudApi` import used by Web, and the compiled CLI
against one control-plane process and PostgreSQL 17 database. It proves
cross-surface idempotency replay, stable conflicts, authorized search,
cross-tenant denial, immediate token revocation, A3S ORM persistence, and zero
plaintext credentials in API/CLI evidence or the PostgreSQL dump. `C0.1` is
verified. The [`C0.2` management MCP](docs/management-mcp.md) now provides the
sessionless `2025-06-18` initialization-based protocol, scoped core-resource
tools, tenant-authorized Node, Operation, Workload, Deployment, Route, and
BuildRun reads, bounded paged Workload and BuildRun logs, signed BuildRun
evidence, and five replay-safe Workload, Deployment, and BuildRun mutation
tools. Its dedicated real PostgreSQL gate proves exact 23-tool administrator
and 16-tool read-only catalogs, strict arguments and annotations, operational
query and command dispatch, hidden mutation denial, Project and Workload
replay, foreign-resource non-disclosure, next-request revocation, A3S ORM
state, and credential-free evidence. `C0.2` is verified; the modern
`2026-07-28` migration remains planned as `C0.2m`.

## Platform Model

### Tenancy

```text
Organization
└── Project
    └── Environment
        ├── sources and BuildRuns
        ├── desired workload revisions
        ├── deployments and operations
        └── routes, Secrets, and observations
```

Authentication is global except for bootstrap and health routes. API tokens are
bound to an organization unless they carry the platform-administrator role.
Commands and queries enforce tenant ownership and scope at the application
boundary.

### Runtime boundary

A3S Runtime exposes two provider-neutral lifecycle classes:

| Class | Purpose |
| --- | --- |
| Task | Finite work such as a build, migration, evaluation, or backup |
| Service | Long-running work such as an application, Agent, MCP server, or model backend |

Runtime owns capability discovery and idempotent `apply`, `inspect`, `stop`, and
`remove` mechanics. Cloud owns resource identity, desired state, placement,
deployment workflows, release provenance, routing, and convergence decisions.

Applications use this path today. Agent, MCP, and Skill publication has an
implemented `A0.1` domain and PostgreSQL foundation plus the first `A0.2` local
bare-repository slice. Authorized Smart HTTP and the remaining repository
safety gates, publication, Agent deployment, Skill binding, and catalog
surfaces remain open `A0` work. Hosted modern MCP contract/compiler,
scope-complete Cloud planning, ordinary-plus-MCP Gateway snapshot composition,
complete version-vector CAS, atomic publication/certificate/scope/Outbox
staging, durable Fleet dispatch/redelivery, exact acknowledgement and expiry
projection, and Gateway request-path foundations are in development under
`MCP0`. Migration 056 retains immutable MCP publication-kind and tenant
evidence so restart scanning and acknowledgements never infer intent from an
ephemeral event. Migration 057 adds a stable logical desired-state digest and
MCP route-count evidence while correcting secondary-member scope binding.
Migration 058 records the canonical desired logical-scope set on every marker
and maintains one mutable latest-MCP head per physical Gateway while preserving
immutable publication history. Migration 059 adds an explicit publication
owner so MCP reconciliation and ordinary Route/rollout workers can share the
same marker/head evidence without competing for dispatch or acknowledgement.
The production single-member, replicated, and deployment-cutover Route
publication paths now use a shared node desired-state planner: they load every
active MCP scope together with the exact ordinary Route/DomainClaim
observation, compile one combined snapshot, and validate the complete MCP
policy/version vector in the same PostgreSQL transaction that stages the
ordinary Route, rollout, or cutover. The
registered desired-state worker rotates
through logical-scope triggers, deduplicates their physical nodes, loads every
active MCP scope for each node, plans them independently, and merges them into
one complete node snapshot. Staging locks and compares the complete active
scope set plus its membership, policies, ordinary routes, Claims, Workloads,
credentials, and physical revision. A node removed from its last scope still
receives one route-less cleanup snapshot; its exact Applied acknowledgement
releases the head so historical markers do not cause permanent scans.
Physical revision, command, certificate identity, and observation time do not
cause digest churn. Focused tests cover two-scope composition, one-publication
node deduplication, membership-removal cleanup, no-op convergence, bounded
cursor fairness, pending deferral, terminal retry, and displaced-state repair.
Focused coverage additionally proves that new ordinary Route and deployment
cutover snapshots keep their MCP router, policy block, joint certificate
names, ordinary pre-write CAS evidence, and ordinary/MCP event identities. The
PostgreSQL fixture also compiles stored scope-set/head evidence and an automatic
post-acknowledgement no-op check, but no database URL is available in the
default local gate.

Exact rollback and certificate-convergence/renewal still need to consume the
same unified node plan. Proactive MCP-only certificate renewal,
revoked-credential cleanup, public lifecycle surfaces, executed real PostgreSQL
evidence for the new path, real Box hosting, and joint product conformance
remain unavailable. Stateful resources remain `S0`;
replicas and multi-node placement remain `H0`; accelerator and inference
capabilities remain `I0`. These profiles do not create separate schedulers.

## Delivery Model

### Durable operations

```text
API command
  -> commit desired state + outbox fact in PostgreSQL
  -> create or locate an idempotent A3S Flow operation
  -> lease work to a reconciler or outbound node
  -> apply through Runtime or Gateway
  -> record exact observations and acknowledgements
  -> rebuild query and SSE projections
```

PostgreSQL is the desired-state authority. A3S Flow owns durable operation
progress. The transactional outbox and A3S Event accelerate coordination but
are never the only recovery path. Reconcilers compare desired and observed
generations until success is proven or a terminal failure is recorded.

### Workload deployment

An accepted workload template becomes an immutable revision. Mutable OCI tags
are resolved before scheduling, and only the digest is persisted as deployment
authority. A v3 deployment reserves current-inventory capacity, prepares the
exact Claim on the assigned Agent, applies one bound Runtime Service, persists
matching allocation evidence, waits for durable health, publishes the required
Gateway state, and activates only after the matching edge acknowledgement.
Replacement cleanup stops the old Runtime before releasing its Claim.

Service `port` and `health` blocks are optional in the A3S ACL workload
manifest. Omitting both defines a headless Service, which Cloud projects as a
Runtime Service with `NetworkMode::None` and no health probe. This explicit
health-neutral profile supplied the verified `BX0.2` lifecycle evidence; it
does not claim the private networking, endpoint, or health capabilities owned
by `BX0.3`.

Update and rollback use the same path. A candidate cannot replace the active
revision until Runtime health and Gateway cutover succeed. Rollback clones a
previously activated template into a new monotonically increasing generation;
history is never rewritten.

### Source-to-workload delivery

The current `G0` path is:

```text
GitHub reference
  -> verified immutable commit and versioned recipe
  -> tenant-owned BuildRun and cloud.build@3 operation
  -> bounded exact checkout and content-addressed Artifact
  -> isolated Runtime Task and typed A3S Box build
  -> complete OCI and trusted retry-cache graph validation
  -> deterministic digest-only registry publication
  -> deterministic SPDX/SLSA evidence and locally verified DSSE signature
  -> explicit cloud.deployment@3 workload handoff
```

Private access uses short-lived GitHub App credentials that are revalidated for
the exact installation, account, and repository. Build cancellation and retry
remain durable operations; retry creates a new attempt while retaining the
source revision and lineage. Retry cache reuse cannot bypass OCI validation,
publication, evidence generation, signing, or local verification. Node-local
Artifact locations, signing private keys, and provider credentials are not part
of the public BuildRun state.

The `G0 external provider conformance` workflow binds its evidence to the exact
Cloud revision. Its private-source job proves credential-free replay and
checkout removal. Its signed-build job proves one remote OCI publication, one
locally verified Vault signature, one persisted evidence document, one
Runtime apply and cleanup, and recovery from both process-death boundaries.
The workflow is implemented but is not external certification until an
operator configures its private providers and the retained run passes.

The detailed request contracts, failure boundaries, and acceptance evidence
remain in the [Development Plan](docs/development-plan.md).

## Gateway Relationship

| Product | Position | Owns |
| --- | --- | --- |
| A3S Cloud | Desired-state control plane | Tenancy, identity, catalogs, Workloads, replicas, placement, rollout, autoscaling, complete traffic policy, operations, usage ledger, and management surfaces |
| A3S Gateway | Traffic and protocol data plane | Transport, TLS, streaming, local policy enforcement, healthy endpoint selection, atomic configuration application, and request-path telemetry |

Cloud never proxies provider bytes or becomes a synchronous authorization
dependency. Gateway never becomes a tenant database, scheduler, production
rollout controller, production autoscaling authority, or long-term usage
ledger.

The Cloud-to-Gateway bridge compiles one complete ACL snapshot, binds it to the
target Gateway identity, revision, expected revision, exact ACL digest, issue
time, and expiry, and delivers it through an outbound node command. The node
agent calls Gateway's native snapshot apply endpoint and then queries exact
readiness. It emits an applied acknowledgement only when Gateway returns the
same snapshot metadata and `ready` state; rejection, expiry, mismatched status,
or unavailable readiness cannot advance Cloud state.

The node agent first selects `a3s.gateway.management-protocol.v1` from the
versioned Gateway descriptor. A legacy Gateway that exposes the exact
pre-descriptor v1 version response remains compatible, while an unknown
descriptor or mismatched request/status schema fails before snapshot apply.
The v4 Gateway acknowledgement and v2 command acknowledgement persist the
selected tuple and discovery mode. New control planes continue to read the old
v3/v1 acknowledgement pair, whose rows retain `NULL` protocol evidence rather
than a fabricated backfill.

Cloud owns a logical Gateway scope inside one organization, project, and
environment. The scope stores ordered desired physical membership, a
membership generation, and readiness policy. Cloud resolves each member to the
same immutable workload revision and declared port through an exact
command-bound healthy Runtime observation, then compiles a separate complete
node-local snapshot for that member. The staged Route projections carry both
logical and physical identities together with the exact workload revision,
deterministic Runtime unit identity, positive generation, port, canonical
node-local origin, and observation time. Gateway receives the resulting
complete policy but does not infer a target, interpret the logical scope, or
store Cloud tenancy. Replicated API publication atomically stages the logical
Route and complete per-member rollout; invalid or conflicting bundles leave no
partial publication, certificate, ownership, idempotency, or outbox state.

Gateway's native journal is the sole source of truth for applied snapshot
state. The node agent does not maintain a second installed-snapshot CAS file,
so command redelivery and process restart converge through Gateway's idempotent
apply and status contract. This is the Gateway-native boundary verified by `H0.2`;
the periodic Edge reconciler renews an unchanged applied ACL before expiry,
reuses the existing certificate files, and keeps the prior revision
authoritative when renewal is rejected or unavailable. Logical-scope ownership,
migration, same-environment/node enforcement, generation-bound target
replacement, mixed-version delivery, PostgreSQL recovery, and real
certificate/target rotation are verified. Cloud persists independent per-member
rollout evidence and computes ready, succeeded, or degraded aggregate outcomes
without assuming a global atomic reload. Its reconcilers recover pending Fleet
dispatch, observe exact physical state after ambiguous loss, retain prior Route
and certificate ownership, and stage one exact higher-revision rollback when a
terminal rollout misses its threshold. The real two-Gateway gate verifies
partial availability, loss, restart, cross-CA isolation, independent cursor
advancement, and apply-before-ack replay. Production multi-node placement and
HA remain separate `H0.3` and `H0.4` work.

Standalone Gateway remains independent with operator-owned ACL desired state.
In `cloud-managed` mode, Gateway rejects local providers and local scaling or
rollout blocks; Cloud is the sole production authority for those decisions.

## Architecture

A3S Cloud is a modular monolith with a separate outbound-only node agent. API,
worker, and event-relay roles can run in one control-plane process or as
independent roles from the same binary.

```text
browser / API client
        |
        v
A3S Boot control-plane API
        |
        +----> DDD application modules ----> PostgreSQL
        |              |                         |
        |              +----> A3S Flow <---------+
        |              +----> outbox -> A3S Event
        |
        v  outbound mTLS command lease
node agent
        +----> A3S Runtime ----> A3S Box
        |                         +----> A3S Power Service (inference)
        +----> A3S Gateway ----> active edge revision
        +----> inventories, observations, and durable acknowledgements
```

| Component | Responsibility |
| --- | --- |
| A3S Boot | Modular API, dependency injection, CQRS, authentication, health, and OpenAPI |
| A3S ORM | Typed PostgreSQL access, transactions, and migrations |
| A3S Flow | Durable operations, retries, timers, and worker leases |
| A3S Event | Integration-fact delivery through local or NATS providers |
| A3S Runtime | Provider-neutral Task and Service lifecycle, endpoints, and health observations |
| A3S Box | Sole node-local execution, image build, network, health-probe, mount, log, snapshot, and cleanup provider |
| A3S Power | Required Box-hosted inference serving and attestation boundary |
| A3S Gateway | HTTPS, routing, health, native snapshot application, and durable applied-state recovery |
| A3S ACL | Closed product configuration and validated manifests |

Business modules follow domain, application, infrastructure, and presentation
layers. Domain code remains independent of A3S Boot, SQL, HTTP, Runtime, Flow,
Event, and provider SDKs; infrastructure adapters implement ports owned by the
inner layers.

See [Technical Architecture](docs/architecture.md) for consistency ownership,
the node protocol, security boundaries, and recovery behavior.

The [A3S Cloud product site](https://a3s-lab.github.io/Cloud/) animates this
control loop and projects the delivery matrix directly from `ROADMAP.md`. Its
[interactive architecture](https://a3s-lab.github.io/Cloud/architecture/)
preserves the deeper 3D and 2D system views under the same Pages deployment.

## Configuration

Cloud and the node agent use closed, validated A3S ACL. Unknown fields and
unsafe timing relationships fail before the corresponding process starts.
Secrets are referenced through environment-variable names or Secret resources;
credential values do not belong in ACL.

| Configuration area | Responsibility |
| --- | --- |
| `server`, `auth`, `postgres` | API role, bootstrap, and durable state |
| `events`, `operations` | Outbox publication and durable operation timing |
| `node_control`, `fleet` | Outbound mTLS protocol, leases, inventories, and observations |
| `deployments`, `builds`, `artifacts` | Workload and Box source-build execution bounds |
| `registry`, `sources` | OCI publication and GitHub delivery policy |
| `edge`, `gateway` | Route compilation, certificates, snapshot validity, and node-local native Gateway application |
| `logs` | Durable log object storage, paging, retention, and compaction |
| `security` | Development or production PKI and encryption providers |
| `box` | Node-local A3S Box provider, isolation, build, and transient Secret materialization policy |

Use [control-plane configuration](config/cloud.acl) and
[node-agent configuration](config/node.example.acl) as the executable
references. The production security profile requires external Vault-backed
identity, Gateway certificate signing, and Secret encryption; production log
storage requires the configured S3-compatible adapter.

## Repository

Cloud is an application-local Rust workspace:

```text
Cloud/
├── architecture-3d/
├── ROADMAP.md
├── cli/
├── config/
│   ├── cloud.acl
│   └── node.example.acl
├── crates/
│   ├── contracts/
│   ├── control-plane/
│   ├── node-agent/
│   └── web-server/
├── deploy/
├── docs/
├── examples/
│   ├── workload.oci.example.acl
│   └── workload.source.example.acl
├── migrations/
├── packages/
│   └── cloud-client/
├── tools/
├── web/                   # authenticated operations console
└── website/               # public product site and versioned documentation
```

## Development

Run Rust checks from the Cloud repository directory:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Run web checks from `web/`:

```bash
bun install --frozen-lockfile
bun run typecheck
bun run format:check
bun run lint:check
bun run test
bun run build
```

Run product-site and documentation checks from `website/`:

```bash
npm ci
npm run format:check
npm run lint
npm run build
npm run check:site
```

Run the shared-client and CLI checks from their package directories:

```bash
bun install --cwd packages/cloud-client --frozen-lockfile
bun run --cwd packages/cloud-client typecheck
bun run --cwd packages/cloud-client format:check
bun run --cwd packages/cloud-client lint:check
bun run --cwd packages/cloud-client test

bun install --cwd cli --frozen-lockfile
bun run --cwd cli typecheck
bun run --cwd cli format:check
bun run --cwd cli lint:check
bun run --cwd cli test
bun run --cwd cli build
```

Real-provider and release certification must run on an isolated Linux host.
Use the repository-owned instructions rather than copying partial commands from
the README:

- [`C0.1` Cross-Surface Conformance](tools/c0-conformance/README.md)
- [Runtime Conformance](tools/runtime-conformance/README.md)
- [A3S Box Provider Conformance](tools/box-conformance/README.md)
- [Production Web Delivery](deploy/web/README.md)

Design and delivery references:

- [Product Roadmap](ROADMAP.md)
- [Development Plan](docs/development-plan.md)
- [Domain Model](docs/domain-model.md)
- [Technical Architecture](docs/architecture.md)
- [Inference Plan](docs/inference-plan.md)

## License

MIT
