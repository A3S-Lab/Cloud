# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud exposes four stable interfaces over one durable control-plane authority" />
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.42.0" src="https://img.shields.io/badge/REST_contract-1.42.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#current-delivery">Delivery</a> &middot;
  <a href="#architecture">Architecture</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#interfaces">Interfaces</a> &middot;
  <a href="#product-map">Product map</a> &middot;
  <a href="#documentation">Documentation</a>
</p>

**A3S Cloud is a self-hosted, interface-only control plane for operating AI
applications, Agents, MCP services, Workflows, model workloads, and durable
state on infrastructure you own.** Authorized tenant intent enters through
REST/OpenAPI, the maintained TypeScript client, CLI, or Management MCP and
converges through one PostgreSQL authority and one durable execution path.

> [!IMPORTANT]
> This repository publishes implemented backend foundations and explicit
> availability boundaries. A component is not a production capability until
> its real-provider, failure, recovery, cleanup, and release gates pass. The
> authoritative status is always [ROADMAP.md](ROADMAP.md).

> [!NOTE]
> A3S Cloud deliberately contains no product Web UI, static SPA,
> documentation website, or interactive architecture application. Supported
> behavior is exposed only through the backend interfaces described below;
> project documentation remains repository-native Markdown and README assets.

## Current delivery

The code on `main` separates implemented mechanics from released capability:

- **Implemented / durable foundation update** — `main` pins A3S Flow `1.0.0`
  at exact latest-main revision `006e988b`, including its bounded child
  Workflow batch primitive, so Workflow ACL graphs reuse Flow's portable DAG
  compiler, while Boot
  `0.2.0`, ORM `0.3.1`, the PostgreSQL queue, Operations, Outbox, audit, and
  replay remain the only durable path. One process-level supervisor observes
  every mandatory worker and fails serving on an unexpected exit or panic. A
  startup-validated exact registry owns every workflow name/version and step
  name; unknown identities fail closed and no product runtime is a fallback.
  New Operations pin replay generation `a3s-cloud-workflows@9`; the former
  `@1` through `@8` generations are admitted only through the explicit Flow compatibility
  set, which readiness exposes with the remaining unpinned migration switch.
  The stable release converges Cloud and Code on one Flow revision. The
  [2026-08-19 `main` PostgreSQL 17 plus local/NATS gate](https://github.com/A3S-Lab/Cloud/actions/runs/32266327719/job/96111906175)
  passes the complete foundation suite against that exact lock, so `F0` is
  `Verified` again.
- **Implemented / stable management contract** — committed
  [OpenAPI `1.42.0`](openapi/v1.json), maintained
  [TypeScript client](packages/cloud-client), [CLI](cli), and
  [Management MCP](docs/management-mcp.md) share the same commands and
  queries. Broader enterprise `C0` gates remain.
- **Implemented / split-process capability boundary** — dedicated Worker and
  Relay processes expose only process status. Relay constructs only
  PostgreSQL, NATS, Outbox, and its notification projection. Worker omits the
  complete management capability bundle, including bootstrap, OIDC, webhook,
  node-CA, plugin-catalog, and management route adapters. API owns management
  routes, node control, and a query-only A3S Flow history adapter; it does not
  connect NATS or construct Boot Flow queues, workflow runtimes, reconcilers,
  checkout, or build staging. Real PostgreSQL 17 API plus PostgreSQL/NATS
  Worker and Relay gates retain all three boundaries. One I/O-free
  `PostgresAdapterFactory` now owns every production repository constructor;
  bounded-context families project each multi-port concrete repository from
  one `Arc`, while dedicated Relay selects only Memberships, Notifications,
  and Outbox. A source gate rejects direct constructors, duplicate constructor
  rules, SQL, migration, or async behavior in that factory.
- **Implemented / one-shot PostgreSQL schema authority** — the
  `a3s-cloud-migrate` executable is the only Cloud process root
  that invokes the A3S ORM migrator. API, Worker, Relay, and `all` only connect
  through a read-only schema-admission path: every migration required by that
  Cloud build must exist with its exact checksum, while later expansion
  records remain admissible during a rolling upgrade. Empty, behind, or
  altered schemas fail before any product capability is constructed. A real
  PostgreSQL 17 gate concurrently starts two migrator processes and proves one
  atomic apply plus one idempotent replay; the development launcher follows
  the same migrate-then-serve order. The sole Cloud ACL names distinct serving
  and migration credential references plus the non-secret `serving_role`.
  Serving processes resolve only `serving_url_env`; the migrator resolves only
  `migration_url_env`, then reconciles CONNECT, schema, table, sequence, and
  function access after all three owner manifests. Migration ledgers remain
  read-only to the serving role. Before applying any migration it also proves
  that the named role exists, differs from PostgreSQL `current_user`, has no
  migration-role membership, and has no administrative attributes. Legacy
  global/schema default grants are revoked before current-object replay. The
  former shared `url_env` field is rejected rather than retained as an alias.
- **Implemented foundation / ACL-native Box installation** — one checked-in
  Box Compose ACL uses the exact transient Secret boundary, one shared Cloud
  ACL, a non-widening role selector, PostgreSQL/NATS health, an idempotent
  migration job, and API/Worker/Relay startup ordering. New PostgreSQL volumes
  receive distinct migration-owner and non-DDL serving roles; the same
  migration job replays serving access for new, existing, and externally
  managed databases. HA placement, operator credential-rotation evidence,
  Gateway packaging, and retained upgrade/failover/restore evidence remain
  open.
- **Implemented / one deployment storage topology** — API and Worker construct
  one filesystem or S3-compatible immutable-object root and derive the
  `logs`, `artifacts`, `asset-git-backups`, and `plugin-trust-roots`
  namespaces from that exact client. Production requires one shared HTTPS
  S3-compatible root. Migration `121` records only secret-free create-once
  topology digests in PostgreSQL, so a replica with another bucket, prefix,
  local root, or Hosted Git filesystem fails before serving or advancing
  work. PostgreSQL does not mirror object bytes, Git refs, or Git objects.
- **Implemented / one compute path** — Sources, assets, builds, finite
  Executions, Workloads, Fleet, outbound Node Agent control, Edge snapshots,
  Runtime, and Box already compose. Current Box/Gateway real-provider
  recertification remains open.
- **Implemented component / storage recovery** — exact
  `cloud.object-namespace.seal@1`, `restore@1`, and `delete@1` Operations/Flow
  workflows use an isolated recovery scope, durable grace wait, and
  just-in-time Secret materialization. Workloads writer-fence enqueue and a
  retained real-S3 pass remain.
- **Implemented backend / Durable Cell interfaces** — application and revision
  authority, build/deployment composition, storage-profile binding, and all
  four management adapters exist. Storage, Box `Outbound`, joint
  behavior/Gateway, and lifecycle gates remain; the service is unavailable.
- **In-progress / future platform families** — `APP0.1` now implements one
  project-authorized Application and immutable release authority over migration
  `124`, exact Workflow revision evidence, atomic idempotency/audit/Outbox,
  REST/OpenAPI `1.42.0`, the maintained client, CLI, and six Management MCP
  tools. Component-only `APP0.2-C1/C2/C3/C4/C5/C6` freezes and persists
  release-pinned end users, sessions, invocation correlation, ordered messages,
  optimistic conversation variables, exactly-once Workflow semantic effects,
  and immutable invocation execution authority through migrations `125`-`127`
  and one production A3S ORM repository. It also compiles deterministic
  Model/Agent preset wrappers through Workflow's sole publication port,
  composes each exact invocation into one ordinary Workflow Goal, Plan, and
  Run, and recovers cancellation from persisted authority through the existing
  Workflow state machine. Project-authorized component commands now open and
  close exact sessions, request and cancel invocations, and replay bounded
  contiguous message cursors with ambiguous-commit recovery. Public delivery,
  monitoring, and the `APP0.6` parity
  gate remain open.
  `K0.1-C1` now has a component-only Files admission
  foundation: one canonical UserFile ACL, bounded upload/scan/retention
  lifecycle, typed immutable reference, and streaming adapter over the shared
  immutable-object client's verified multipart path. Quota, persistence,
  interfaces, and all Knowledge/KnowledgePipeline lifecycle remain open.
  Automations, Inference, and Evolution retain their gate-driven plans;
  `APP0`, `K0`, `AUT0`, `I0`, and `EV0` remain unavailable.

Gate-by-gate evidence, dependencies, and remaining work live in the
[product roadmap](ROADMAP.md) and detailed plans.

## Architecture

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud architecture showing one interface layer and PostgreSQL authority, Flow-coordinated storage and workload paths, outbound node execution, and Gateway live traffic outside Cloud" />
</p>

The system keeps three paths explicit:

1. **Control and recovery.** An authorized command commits desired state,
   idempotency, an Operation, audit, and bounded Outbox facts. A3S Flow replays
   durable work; Data and Secrets perform exact storage steps without a second
   queue, worker, credential cache, or provider client.
2. **Node execution.** Workloads owns placement and rollout. Fleet delivers one
   versioned command over the outbound-only Node Agent channel. A3S Runtime
   owns Task and Service lifecycle; A3S Box is the sole local execution and
   build provider.
3. **Live requests.** Edge compiles complete target snapshots for A3S Gateway.
   Gateway sends opaque bytes to the exact applied target; Cloud remains off
   the request byte path and advances only from matching evidence.

Agent Runtime, hosted MCP, Durable Cells, inference, and later application
profiles are sibling product projections over this substrate. Each compiles to
an existing Execution or Workload and ultimately to A3S Runtime `Task` or
`Service`; no product adds a Runtime class or scheduler. A Cell provider
replica is a Runtime Service, while individual named Cells remain entirely
provider-owned inside S0.

Explore the [interactive architecture](https://a3s-lab.github.io/Cloud/architecture/)
or read the [technical architecture](docs/architecture.md) for bounded
contexts, consistency rules, failure behavior, and the full capability
preservation register.

### One concern, one authority

| Concern | Sole authority | Duplicate mechanism deliberately absent |
| --- | --- | --- |
| Desired state and projections | PostgreSQL through A3S ORM | Redis, streams, node journals, or local files as product truth |
| PostgreSQL schema execution | The terminating `a3s-cloud-migrate` process through one A3S ORM mechanism, with owner manifests and ledgers scoped to Cloud `public`, Flow `a3s_flow`, and Boot `a3s_boot` | Serving-process DDL, copied component SQL/admission logic, a second runner, or one shared credential reference |
| PostgreSQL adapter composition | One role-selected, I/O-free `PostgresAdapterFactory`; each bounded-context family projects one concrete repository instance to all of its ports | Direct constructors in the process root, per-role repository factories, duplicate concrete instances inside one family, or SQL/migrations in composition |
| Long-running coordination | A3S Flow plus Cloud Operations, driven by one `FlowOperationCoordinator` | Product-specific workflow engines, retry tables, schedulers, or an Operations-local timer |
| Flow runtime dispatch | One startup-validated registry of exact workflow name/version and exact step name | Prefix routing, a default product runtime, duplicate ownership, or collision discovery after serving starts |
| Flow replay-code identity | A3S Flow `RuntimeBuildCompatibility` configured by one Cloud build manifest | Reusing one build ID across runtime generations, caller-selected build IDs, or a second build router |
| Portable DAG structure | A3S Flow `WorkflowDag`; Cloud constructs it programmatically from canonical ACL | A Cloud compatibility parser, topology sorter, or editor-owned execution schema |
| Placement and rollout | Workloads plus Fleet | Agent-, MCP-, inference-, Cell-, or Gateway-specific schedulers |
| Provider lifecycle | A3S Runtime Task/Service plus A3S Box | Direct provider calls from business contexts or a Cloud executor |
| Storage and credentials | Data S0 port plus Secrets exact-version materialization | Raw S3 clients, credential stores, or recovery workers per product |
| Shared immutable bytes | One deployment-level object client with typed child namespaces | Log-, Artifact-, Asset-, or Plugin-local filesystem/S3 authorities |
| Deployment storage identity | Create-once PostgreSQL `infrastructure_bindings` digests | A data plane attesting itself, mutable topology overrides, or byte/ref mirrors |
| Traffic application | Edge planner/compiler, Fleet command, A3S Gateway applied state | Cloud proxying, competing publishers, or inferred success |
| Gateway runtime settings | One target-neutral `GatewaySnapshotRuntimeSettings` validator shared by ACL admission and snapshot compilation | Host-OS path interpretation or a second compiler validator |
| Local metadata durability | One platform-aware directory-sync primitive shared by immutable objects and hosted Git | Store-specific directory handles or Windows no-op flushing |
| Git subprocess execution | One hardened `GitCommandRunner`, including canonical host-path normalization at the process boundary | Asset- or Source-specific command environments and Windows path workarounds |
| Identity and authorization | Principals, Memberships, grants, tokens, and revocation | Adapter-local users, roles, or authorization rules |
| Management behavior | One command/query application layer | REST-, client-, CLI-, MCP-, or UI-specific business lifecycles |
| A3S dependency identity | One exact source for each package name/version in the root lock | The same release resolved from both crates.io and Git, or an undocumented version fork |

## Quick start

### Requirements

- Rust 1.88 or later
- PostgreSQL 17 or a compatible supported release
- A3S Box for node-local workload and build execution
- The pinned A3S Gateway revision for routed services
- Bun only when developing the TypeScript client or CLI
- NATS JetStream for every event-owning production `all`, worker, or relay
  process; a dedicated API process owns no event transport;
  the in-process A3S Event provider is development all-in-one only

Redis is not required and is never a durable business, workflow, queue,
session, lock, or replay authority.

### Start the control-plane API

```bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
# Local development may point the distinct migration reference at the same database user.
export A3S_CLOUD_POSTGRES_MIGRATION_URL="$A3S_CLOUD_POSTGRES_URL"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

Serving processes never run migrations. Run the one-shot migrator after
PostgreSQL becomes reachable and before starting or upgrading any API, Worker,
Relay, or `all` process. The migrator applies Cloud's manifest and delegates
Flow and Boot manifests to their owner APIs. A serving process fails closed if
any required component manifest is absent or altered. Production must provide distinct migration and
serving principals through the two ACL references; the development shortcut
above is not a production least-privilege setup. See the
[PostgreSQL schema-management contract](docs/postgres-schema-management.md)
for rolling-upgrade and failure rules. The development all-in-one profile
listens on `127.0.0.1:8080` and may use the in-memory A3S Event provider.
Production and split-process topologies fail configuration validation unless
they use NATS JetStream, because an in-process bus cannot cross an
API/worker/relay boundary.

The [Box-hosted production baseline](deploy/production/README.md) uses one
shared closed Cloud ACL, narrows its `all` capability envelope into dedicated
API/Worker/Relay units, runs the one-shot migrator first, and projects Secret
values through Box's private tmpfs boundary. It is a single-host installation
foundation; it does not claim the remaining HA, failover, backup/restore, or
Gateway placement gates.

```bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
```

<details>
<summary><strong>Bootstrap the first organization</strong></summary>

Cloud stores only the API-token digest. The caller creates and retains the
first credential.

```bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: ${A3S_CLOUD_BOOTSTRAP_TOKEN}" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"${A3S_CLOUD_ADMIN_TOKEN}\",\"expiresAt\":null}"
```

Subsequent requests use `Authorization: Bearer ${A3S_CLOUD_ADMIN_TOKEN}`.
Every mutation requires a stable `idempotency-key`.

</details>

### Call it from the CLI

```bash
bun install --cwd cli --frozen-lockfile

export A3S_CLOUD_TOKEN="${A3S_CLOUD_ADMIN_TOKEN}"
export A3S_CLOUD_URL="http://127.0.0.1:8080/api/v1"

bun run --cwd cli src/main.ts diagnostics status --output=json
bun run --cwd cli src/main.ts organizations list --output=json
bun run --cwd cli src/main.ts operations list --output=json
```

Credentials come from environment variables or standard input and are never
written to a CLI context file.

## Interfaces

| Surface | Contract | Start here |
| --- | --- | --- |
| REST/OpenAPI | Versioned `/api/v1`, common envelopes, request IDs, idempotency, and committed contract snapshot | [`openapi/v1.json`](openapi/v1.json) |
| TypeScript client | Maintained adapter over the same REST contract; no presentation-owned lifecycle | [`packages/cloud-client`](packages/cloud-client) |
| CLI | Scriptable automation with structured JSON output and no token argument | [`cli/README.md`](cli/README.md) |
| Management MCP | Sessionless, tenant-authorized tools over the same application commands and queries | [`docs/management-mcp.md`](docs/management-mcp.md) |

Controllers and adapters stay thin. They cannot call providers directly,
invent surface-owned state, or weaken tenant authorization.

## Product map

Five product directions compose the same Cloud authorities rather than
creating their own control planes:

1. **Unified Gateway** governs Workflow, Agent, MCP, model, and application
   traffic through complete Edge snapshots and one Gateway apply path.
   Foundations exist; current Box/Gateway recertification remains.
2. **Workflow Orchestration** compiles ontology-defined goals and typed graphs
   into recoverable execution. `W0.1` is implemented, `W0.2` is verified, and
   `W0.3` includes deterministic composite frame/export and ordinal reducers
   plus Flow-backed sequential Iteration/Loop child WorkflowRun dispatch,
   linkage, cancellation, and recovery. It also pins finite Execution error
   ports in Plan v3 and routes typed dispatch/terminal failures through the
   same DAG and Flow history. Component-only WorkflowRun v5 interprets exact
   Connector attempts, observations, durable waits, and bounded retries through
   the sole C6 execution/evidence authority. Version 6 composes accepted responses
   through the Connectors-owned typed child of the shared immutable-object
   client and retains only an exact opaque reference, digest, and byte count in
   Flow. The internal Connector read port now requires exact environment
   authorization and accepted terminal C6 evidence before returning transient,
   integrity-checked, Debug-redacted bytes to a typed owner. New v8 consumes
   that port in one no-retry Flow step, accepts exactly one duplicate-key-free
   JSON value, enforces the immutable output schema and Workflow output bound,
   and records only the validated typed node output. Plan v5/Run v9 additionally
   maps closed Connector terminal and response-validation failures to bounded
   `cloud.workflow.step-failure.v2` values only when the exact
   descriptor-bound `error` edge exists; the source projection remains failed
   while the ordinary reachable failure branch may complete the parent. The
   same DAG and Flow history remain the sole control path. Historic v8 keeps
   its fail-closed behavior without this edge, v5 remains digest-only, and v6
   remains reference-only, while Plan v4/Run v7 folds finite
   Execution failure observations into one exact policy-owned default output
   with typed projection evidence. This is a component execution path, not
   public HTTP Request availability; Answer, remaining non-Execution error semantics,
   remaining providers, recovery evidence, and later `W0` gates remain open.
3. **Agent Factory** turns heterogeneous Harness implementations into
   immutable, evaluated, deployable Agent products. `A1.0` is verified and
   `A1.1` is implemented; native Code integration verification remains.
4. **AI Application Platform** composes Applications, Knowledge, plugins,
   automations, and governed delivery from exact Workflow/Agent revisions.
   `APP0.1` freezes one canonical immutable release across all six
   experiences, including distinct classic/New Agent identities, pins the
   exact Workflow revision evidence without copying its graph or runtime, and
   persists sequence-fenced releases with atomic idempotency, audit, and
   Outbox facts through PostgreSQL/A3S ORM. Project authorization, CQRS,
   REST/OpenAPI `1.42.0`, the maintained client, CLI, and six Management MCP
   tools all reuse that authority. Component-only `APP0.2-C1/C2/C3/C4/C5/C6` adds and
   persists the single release-pinned session/message/variable contract,
   deterministic Workflow-effect replay boundary, and immutable invocation
   execution authority through migrations `125`-`127`. A typed internal port
   creates or adopts the exact ordinary Workflow Goal, Plan, and Run and
   recovers cancellation, while deterministic Model/Agent preset wrappers use
   Workflow's shared publication authority. Neither adds another Flow history
   or dispatch path. Authorization-first internal session, invocation,
   cancellation, and bounded cursor CQRS recover exact persisted state; no
   public delivery protocol is claimed.
   `K0.1-C1` now freezes the Files-owned canonical upload reference and
   admission state machine while reusing the shared immutable-object client;
   quota transactions, persistence, maintained interfaces, and the Knowledge
   aggregates remain open. `AUT0` has component-only Connector foundations in
   progress. All three product surfaces remain unavailable.
5. **Durable Cell Service** targets named SQLite-backed state entities with
   alarms, WebSockets, idle eviction, and fenced recovery. Backend contracts,
   composition, and interfaces exist; provider and lifecycle gates remain, so
   the service is unavailable.

A3S Code is one first-party Harness provider, not a privileged execution path.
The [roadmap](ROADMAP.md) owns exact public status and the dependency order
across A3S Flow, Runtime, Box, Gateway, Use, Event, ORM, and Boot.

## Configuration

Cloud and the Node Agent use closed, validated A3S ACL. Unknown fields and
unsafe timing relationships fail before startup; Secret values never belong in
ACL. Configuration is parsed only through `a3s-acl`.

| Area | Responsibility |
| --- | --- |
| `server`, `auth`, `postgres` | Process roles, bootstrap, identity, and durable state |
| `events`, `operations` | Outbox publication and durable operation timing |
| `node_control`, `fleet` | Outbound mTLS, leases, inventory, observations, and Claims |
| `deployments`, `executions`, `builds`, `artifacts` | Workload, Task, Box build, and immutable-content admission bounds |
| `objects` | The one deployment-level local or S3-compatible immutable-object root; production requires shared HTTPS S3 |
| `registry`, `sources`, `edge`, `gateway` | Source policy, OCI publication, routes, certificates, and exact Gateway apply |
| `logs`, `security`, `box` | Log retention/compaction, production trust, isolation, and transient Secret materialization |

The storage schema is intentionally singular: move the former
`artifacts.store_dir` and `logs.s3_*`/`logs.storage_provider` settings into
`objects`. Those former fields are rejected as unknown instead of remaining
as aliases that could create a second provider authority.

`server.role = "all"` or `"api"` owns the management REST/OpenAPI/MCP
surface. Dedicated `"worker"` and `"relay"` processes expose only liveness,
readiness, and their `/platform` identity; they cannot become accidental
management API replicas. The relay composition initializes only PostgreSQL,
NATS, the existing Outbox/notification projection, and those status routes;
it does not require API, Flow, Runtime, Box, Vault, Gateway, or object-storage
providers. Worker readiness is exactly PostgreSQL, NATS, Flow, Gateway
certificate authority, key encryption, and shared object storage. Its composition does
not resolve the bootstrap or webhook credentials and does not create the node
CA, node-control server identity, or plugin-catalog state. API readiness is
exactly PostgreSQL, query-only Flow history, the node and Gateway certificate
authorities, key encryption, and shared object storage. API does not own event transport:
it neither resolves NATS nor constructs the Outbox relay or notification
consumer. Its Flow adapter reuses the sole A3S Flow PostgreSQL event store and
projection engine but cannot execute workflows or steps. Worker construction
owns checkout, build staging, runtime registration, the Boot task queue, and
the complete reconciler set. `all` composes those same typed capabilities; it
does not introduce a third path.

Schema migration is not a `server.role`. `a3s-cloud-migrate` is a terminating
deployment step with no HTTP routes, worker, event transport, object client,
or domain adapter. It delegates locking, transactional application, checksum
recording, and concurrent replay to the sole A3S ORM migration mechanism.
Cloud, Flow, and Boot retain separate owner manifests and component-scoped
ledgers; Cloud does not copy their SQL or verification. Every serving role
performs only manifest admission and ordinary PostgreSQL health checks.
The closed ACL gives these process roots different credential references, and
repository launchers remove the migration variable before starting a serving
process. The checked-in Box baseline provisions separate migration-owner and
serving roles on a new PostgreSQL volume, removes bootstrap superuser authority
from the migration path by disabling bootstrap login, and exposes only the
applicable URL to each unit. After every Cloud/Flow/Boot owner migration, that
same terminating process reconciles current cross-schema DML access and
read-only migration-ledger admission for the ACL-named serving role. No
bootstrap-only default grants or second grant runner exist. Operator credential
rotation and retained failure/restore evidence remain open.

Use [`config/cloud.acl`](config/cloud.acl) and
[`config/node.example.acl`](config/node.example.acl) as development references,
and [`deploy/production/compose.acl`](deploy/production/compose.acl) plus its
single shared [`cloud.acl`](deploy/production/cloud.acl) as the Box-hosted
installation baseline.

## Repository

```text
Cloud/
|-- crates/
|   |-- contracts/       # versioned cross-process contracts
|   |-- control-plane/   # API, domain modules, workers, and persistence
|   `-- node-agent/      # outbound node protocol and execution adapters
|-- migrations/          # PostgreSQL schema evolution
|-- config/              # closed A3S ACL configuration
|-- openapi/             # committed REST contract
|-- packages/cloud-client/
|-- cli/
|-- tools/               # provider, recovery, and release gates
`-- docs/                # architecture, plans, decisions, and runbooks
```

This directory is its own Rust workspace inside the wider A3S monorepo.

## Development

Run Rust validation from the Cloud repository root:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Real-provider and release certification runs on isolated Linux hosts. The
repository-owned gates include:

- [`C0.1` cross-surface conformance](tools/c0-conformance/README.md)
- [Runtime conformance](tools/runtime-conformance/README.md)
- [A3S Box provider conformance](tools/box-conformance/README.md)
- [Pinned Gateway conformance revision](tools/gateway-conformance/gateway-revision)

Client, CLI, documentation, contract, compatibility, and policy checks run in
CI.

## Documentation

| Document | Authority |
| --- | --- |
| [Product roadmap](ROADMAP.md) | Gate status, dependencies, and delivery order |
| [Technical architecture](docs/architecture.md) | Ownership, topology, consistency, and failure behavior |
| [PostgreSQL schema management](docs/postgres-schema-management.md) | One-shot migration authority, rolling order, admission, and failure rules |
| [Development plan](docs/development-plan.md) | Implementation slices and exit evidence |
| [Domain model](docs/domain-model.md) | Aggregates, state machines, and invariants |
| [Workflow and evolution](docs/workflow-evolution-plan.md) | `W0`, heterogeneous `A1`, and governed `EV0` contracts |
| [AI application platform](docs/ai-application-platform-plan.md) | `APP0`, `K0`, `AUT0`, node coverage, and parity evidence |
| [Durable Cell Service](docs/durable-cell-platform-plan.md) | `CELL0` ownership, fencing, provider boundary, and fault evidence |
| [Architecture decisions](docs/decisions/app-platform/README.md) | Normative application-platform authority boundaries |
| [Inference plan](docs/inference-plan.md) | Model, provider, routing, usage, and conformance design |
| [Management MCP](docs/management-mcp.md) | Protocol, authorization, and tool contract |

## License

[MIT](LICENSE) &copy; 2026 A3S Lab
