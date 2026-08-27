# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud exposes four stable interfaces over one durable control-plane authority" />
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.74.0" src="https://img.shields.io/badge/REST_contract-1.74.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#architecture">Architecture</a> &middot;
  <a href="#domain-model">Domain model</a> &middot;
  <a href="#delivery-status">Delivery</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#interfaces">Interfaces</a> &middot;
  <a href="#documentation">Documentation</a>
</p>

**A3S Cloud is a self-hosted, interface-only control plane for operating AI
applications, Agents, MCP services, Workflows, model workloads, and durable
state on infrastructure you own.** Authorized tenant intent enters through
REST/OpenAPI, the maintained TypeScript client, CLI, or Management MCP and
converges through one PostgreSQL authority and one durable execution path.

> [!IMPORTANT]
> This repository contains production foundations and partially delivered
> product lanes. A capability is not production-ready until its real-provider,
> failure, recovery, cleanup, and release gates pass. [ROADMAP.md](ROADMAP.md)
> is the sole authority for availability; diagrams below include clearly
> labelled target boundaries.

> [!NOTE]
> A3S Cloud deliberately contains no product Web UI or static SPA. Management
> behavior is exposed through the four supported interfaces, all backed by the
> same application commands and queries.

## Why this architecture

A3S Cloud starts from six invariants:

1. Accepted intent is durable before work starts.
2. Every concern has exactly one decision and data authority.
3. Dependencies point inward; side effects stay behind owner ports.
4. Desired state advances only from exact generation-bound evidence.
5. Every durable side effect has one retry, recovery, cleanup, and fencing
   owner.
6. Product meanings stay in Cloud domains; lower layers expose reusable
   mechanisms.

The result is a modular monolith for control-plane consistency, an outbound-only
node channel, A3S Flow for durable coordination, A3S Runtime and Box for
provider-neutral Task/Service execution, and A3S Gateway for applied request
traffic. Redis, Kubernetes controllers, product-specific schedulers, and
surface-specific business state are not parallel authorities.

## Architecture

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud control, execution, storage, and live request paths" />
</p>

Three paths remain intentionally separate:

- **Control and recovery.** An authorized command commits desired state,
  idempotency, an Operation, audit evidence, and transactional Outbox facts.
  A3S Flow owns durable waits, retry, cancellation, and replay.
- **Node execution.** Workloads owns placement and rollout. Fleet delivers one
  versioned command over the outbound-only Node Agent channel. A3S Runtime owns
  Task/Service lifecycle; A3S Box is the sole local execution/build provider.
- **Live requests.** Edge compiles complete target snapshots for A3S Gateway.
  Gateway applies and serves those snapshots; Cloud stays off the request byte
  path.

Product contexts compile intent into these paths. They do not acquire their
own queue, scheduler, node journal, Runtime class, Gateway publisher, Secret
store, or object client.

## Domain model

### One bounded context, one authority

<p align="center">
  <img src="assets/readme/ddd-boundary.svg" width="100%" alt="DDD dependency rule with presentation, application, domain, infrastructure, owner ports, providers, and committed facts" />
</p>

Within a context, Presentation calls Application, Application coordinates its
Domain and consumer-owned ports, and Infrastructure implements those inward
ports. Across contexts there are only two behavioral relationships:

- a synchronous consumer-owned port implemented by the owning context's
  public application contract; or
- a versioned fact published from the owner's committed Outbox transaction.

Foreign repositories, infrastructure helpers, presentation DTOs, and physical
table mappings are not interfaces. Concrete adapters are wired only at the
crate composition root.

### Context map

<p align="center">
  <img src="assets/readme/domain-context-map.svg" width="100%" alt="Detailed A3S Cloud bounded-context map grouped into governance, supply chain, product semantics, execution, storage, traffic, and projections" />
</p>

The map distinguishes domain authorities (green), planned contexts (dashed
green), shared execution/storage authorities (amber), external mechanisms
(blue), and rebuildable projections (gray). The following catalog covers the
27 modules currently present in the control plane; dashed target contexts do
not imply availability.

| Area | Context | Aggregate and decision authority |
| --- | --- | --- |
| Governance | Identity | Organization, Principal, Membership, credential, grant, authorization decision, verified recipient contact |
| Governance | Projects | Project, Environment, tenant attribution lineage |
| Governance | Audit | Append-only audit record, signed export, retention decision |
| Governance | Security | Authorized investigation projection over owner facts; never enforcement truth |
| Governance | Search | Rebuildable tenant-authorized search projection |
| Platform | Integration Events | Transactional Outbox publication and consumer coordination |
| Platform | Shared Kernel | Stable typed IDs, digest, timestamp, idempotency shapes; no business lifecycle or repository |
| Supply | Sources | External connection, subscription, authenticated webhook Inbox, exact SourceRevision, committed pull-request and Preview-SourceRevision Published Language |
| Supply | Developer Workflows | Public authorization-first BuildPlan detection/acceptance/reads and WorkloadProfile acceptance/immutable revision reads over one CQRS/application authority; trusted SourceLayout acquisition, Preview Policy acceptance, exact accepted-profile compilation, durable PR projection, and owner-facing handoff intent |
| Supply | Assets | Hosted Agent/MCP/Skill identity, immutable release, hosted Git binding |
| Supply | Artifacts | BuildCandidate, sole BuildRun lifecycle, Preview build-admission/retirement fence, admitted output, successful external-source outcome Published Language, provenance, evidence, retention, node artifact transport |
| Execution | Operations | User-visible long-running operation identity and progress projection |
| Execution | Executions | Finite Task intent and immutable ExecutionTemplate revision |
| Execution | Workloads | Service desired state, WorkloadRevision, Deployment, replica, rollout, placement, writer fence |
| Execution | Fleet | Node, pool, enrollment, inventory, Claim, command journal, observation, fencing |
| Traffic | Edge | DomainClaim, GatewayScope, Route, certificate, rollout, complete applied snapshot intent |
| Security | Secrets | Secret and immutable version lifecycle, binding, authorization, exact materialization |
| Storage | Data / S0 | Namespace, mutable storage policy, backup, restore, retention, deletion, writer fencing |
| Product | Agents | Conversation, AgentExecution, semantic event sequence, approval/checkpoint/fork trajectory |
| Product | Applications | Application, immutable release, session, invocation, message, variable, delivery semantics |
| Product | Workflow | Ontology, WorkflowDefinition, Goal, Plan, WorkflowRun, HumanTask, decision, semantic projection |
| Product | Forms | Form draft/release schema and deterministic semantic validation |
| Product | Connectors | Outbound profile/revision, exact attempt, egress policy, response evidence |
| Product | Notifications | Personal inbox, subscription, alert policy, delivery fact and terminal receipt |
| Product | Plugins | Tenant registry enrollment and exact A3S Use package-assignment intent |
| Product | Files | Upload/admission metadata and immutable-object reference; never Artifact authority |
| Product | Durable Cells | Cell application, immutable revision, compatibility/retention intent, deployment correlation; never individual Cell state |

### Aggregate collaboration

<p align="center">
  <img src="assets/readme/domain-collaboration.svg" width="100%" alt="A3S Cloud aggregate collaboration from tenant scope and immutable product intent through owner ports, Flow, Workloads, Fleet, Runtime, providers, Gateway, S0, and Outbox facts" />
</p>

An aggregate crosses a boundary as a typed ID, immutable revision, digest,
bounded snapshot, command result, or committed fact—never as another context's
mutable aggregate. The typical convergence path is:

```text
Product intent
  -> owner application port
  -> Operation + Flow correlation
  -> Execution Task or Workload Service
  -> Fleet Claim and versioned node command
  -> Runtime / Box apply
  -> exact generation evidence
  -> owner projection
  -> Edge snapshot or committed integration fact
```

### One concern, one mechanism

| Concern | Sole authority | Mechanism that must not be duplicated |
| --- | --- | --- |
| Product desired state | Owning bounded context in PostgreSQL through A3S ORM | Foreign table writes, local files, provider state as product truth |
| Long-running coordination | Operations + A3S Flow | Product workflow engine, retry table, sleep loop, second scheduler |
| Placement and rollout | Workloads + Fleet | Agent-, MCP-, Cell-, model-, or Gateway-specific scheduler |
| Provider lifecycle | A3S Runtime Task/Service + A3S Box | Direct process/provider calls from product domains |
| Traffic application | Edge desired state + A3S Gateway applied snapshot | Cloud proxy, competing publisher, Cell-owner lookup in Gateway |
| Mutable storage | Data / S0 | Product-local backup, retention, volume, or fencing engine |
| Immutable bytes | One deployment object client with typed namespaces | Per-product filesystem/S3 clients |
| Credentials | Secrets | Plaintext in ACL, events, DTOs, logs, or product-owned stores |
| Integration facts | Transactional Outbox + A3S Event | Publish-before-commit or product-local event bus |
| Authorization | Identity policy + owner admission | Adapter-local roles, token parser, or foreign presentation guard |
| Audit | Shared append-only audit path | Reconstructing domain truth from audit or a second audit store |
| Product configuration | A3S ACL parsed by `a3s-acl` | Compatibility parsers or another product configuration language |

### Durable Cells and a3s-runtime

Durable Cells is a stateful product, but an individual named Cell is not a
Cloud aggregate, Workload replica, or Runtime Unit.

| Boundary | Owns now / target responsibility |
| --- | --- |
| Durable Cells in Cloud | Application identity, immutable revision, state-schema compatibility, retention intent, exact Workload/S0/Operation/Route correlation |
| a3s-runtime | Generic Task and Service lifecycle, typed endpoints, recovery evidence, and an opaque `semantics_profile_digest`; no Durable Cell fields or product-specific capability type |
| Box + selected Cell provider | Provider process, activation, per-key serial turns, SQLite lineage, alarm/WebSocket behavior, idle eviction, recovery, epoch fencing |
| Data / S0 | Namespace lifecycle, credentials, conditional object semantics, backup, restore, retention, deletion evidence |
| Workloads / Fleet / Edge | Placement, Claims, node commands, rollout, healthy target selection, Route intent, Gateway publication |

Named-state semantics remain outside the Runtime wire. Cloud binds the exact
immutable Service-profile digest to an ordinary Runtime Service, while the
joint Cloud/Box/provider consumer gate must black-box test serial turns,
activation, idle eviction, alarms, hibernatable connections, durable
acknowledgement, and fencing before product availability. This follows Runtime
ADR 0005: Runtime certifies only its generic lifecycle, identity, endpoint,
recovery, resource, cleanup, and opaque-digest contracts. An individual Cell is
never a Runtime Unit.

### Architecture audit and current debt

The [first-principles architecture audit](docs/architecture-audit.md) reviews
all 27 control-plane modules by authority, consistency boundary, legal payload,
excluded concerns, recovery owner, and proof. Source-level architecture tests
already enforce that current debt can shrink but cannot spread:

- direct cross-context `infrastructure` / `presentation` imports are frozen by
  exact source location;
- duplicate ORM mappings of `mcp_service_profiles`, `nodes`,
  `operation_requests`, `workloads`, and `workflow_runs` are frozen;
- Runtime/transport/persistence/provider dependencies in domains are rejected;
  Artifacts now keeps byte streaming and input I/O in Application ports;
  external checkout, provider credentials, and deterministic packaging stay
  behind a Sources-owned adapter; committed pull-request facts cross only the
  Sources Published Language and are reduced by one Developer Workflows
  projector in the shared Outbox Relay; committed Preview mutations use that
  same transaction and Relay to reach Projects through one consumer-owned
  Environment port; Sources then projects the exact version to one ordinary
  SourceRevision and publishes one bounded specialized fact, while the existing
  Artifacts projector consumes that fact through an Artifacts-owned version
  fence, admits only the current active BuildCandidate, and retires only the
  existing BuildRun lifecycle. The internal accepted-profile query loads exact
  Developer Workflows repository authority and traverses the sole Artifacts,
  Workloads, and Executions anti-corruption adapters without introducing a
  lifecycle, worker, relay, or scheduler. The BuildPlan detection
  query now authorizes before acquiring the exact accepted-revision layout
  through Sources' sole checkout coordinator. SourceRevision resolution and
  checkout share Sources' one repository-credential authority; detection then
  composes the authoritative Asset ACL and heuristic Dockerfile adapters
  through the sole bounded detector service after a strict credential-free
  replay, without persistence. That query, the public BuildPlan and
  WorkloadProfile acceptance commands, and the internal Preview Policy command
  share one action-scoped authorization port backed by Identity's sole grant
  evaluator and exact Projects Environment lookup. BuildPlan acceptance
  additionally uses the Sources evidence port; each acceptance command
  delegates to its existing immutable repository transaction. REST, the
  maintained client and CLI, and Management MCP dispatch those exact CQRS
  handlers; accepted-plan and accepted-profile reads each go through one
  Application query service that performs the same authorization and rejects
  repository scope/order drift. They add no authorization, persistence,
  delivery, or lifecycle
  mechanism. Hosted-Asset
  staging and public Infrastructure remain frozen debt;
- Runtime and Flow may enter domains only through named pure published
  contracts; and
- Shared Kernel dependencies and public outer-layer facades cannot expand.

The allowlists are a migration ratchet, not an architectural exemption. The
optimization is complete only when they are empty and affected PostgreSQL,
Flow replay, Runtime/Box, Gateway, S0, and cross-interface gates pass again.

## Delivery status

The portfolio is gate-driven rather than percentage-driven. This table is a
short orientation only; consult [ROADMAP.md](ROADMAP.md) before depending on a
capability.

| Lane | Current public status |
| --- | --- |
| `F0` foundation | Verified PostgreSQL tenancy, identity, ORM-backed Flow operations, Outbox/projections, API, and migration authority |
| Box/Runtime/node/deployment baseline | Historical evidence; current Box re-certification remains in progress |
| Sources, builds, artifacts, developer workflows | In progress; BuildPlan and WorkloadProfile REST/OpenAPI, client, CLI, and Management MCP surfaces are production-composed over authorization-first canonical acceptance and their sole immutable read authorities. Preview public interfaces, pre-acceptance source discovery, Workload/Execution/route/operation/schedule and Environment cleanup handoffs, monorepos, and import completion remain unavailable; remote WorkloadProfile certification is pending |
| Control surfaces, collaboration, notifications, security | In progress; enterprise gates remain |
| Agent/MCP releases and heterogeneous Agent execution | In progress; several component and provider gates remain |
| Ontology-driven Workflow | In progress and unavailable as a complete product; W0.1 is implemented, W0.2 verified, and the component runtime now includes Plan v11/Run v19 composite failure routing, Run v20 Variable Aggregation, Run v21 List Operator execution, Run v23 Connector compensation, Run v24 exact AgentRelease lifecycle, and Plan v12/Run v25 descriptor-bound Agent failure routing |
| AI Applications, Files/Knowledge, Automations | Component foundations in progress; complete products unavailable |
| Data/S0 and Durable Cells | Component foundations in progress; retained provider/lifecycle/fault evidence remains, service unavailable |
| Inference, governed self-evolution, simplified Agent Runtime experience | Planned |
| Production scale / HA | In progress; release claims remain gate-bound |

## Quick start

### Requirements

- Rust 1.88 or later
- Git CLI for pinned external-source acquisition and replay validation
- PostgreSQL 17 or a compatible supported release
- A3S Box for node-local workload and build execution
- The pinned A3S Gateway revision for routed services
- Bun only for TypeScript client or CLI development
- NATS JetStream for production `all`, worker, or relay processes that own
  event delivery; the in-process provider is development-only

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
PostgreSQL becomes reachable and before starting or upgrading API, Worker,
Relay, or `all`. Production uses distinct migration and serving principals;
see the [schema-management contract](docs/postgres-schema-management.md).

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

The [Box-hosted production baseline](deploy/production/README.md) narrows one
closed Cloud ACL into dedicated API/Worker/Relay units and runs the migrator
first. It is a single-host installation foundation, not an HA certification.

## Interfaces

| Surface | Contract | Start here |
| --- | --- | --- |
| REST/OpenAPI | Versioned `/api/v1`, common envelopes, request IDs, idempotency, committed contract snapshot | [Guide](docs/openapi.md) · [`openapi/v1.json`](openapi/v1.json) |
| TypeScript client | Maintained adapter over the same REST contract; no surface-owned lifecycle | [`packages/cloud-client`](packages/cloud-client) |
| CLI | Scriptable structured output with no token argument | [`cli/README.md`](cli/README.md) |
| Management MCP | Sessionless tenant-authorized tools over the same commands and queries | [`docs/management-mcp.md`](docs/management-mcp.md) |

Controllers and adapters stay thin. They cannot call providers directly,
invent interface-owned state, or weaken tenant authorization.

<details>
<summary><strong>Detailed product map and component evidence</strong></summary>

### Product map

Five product directions compose the same Cloud authorities rather than
creating their own control planes:

1. **Unified Gateway** governs Workflow, Agent, MCP, model, and application
   traffic through complete Edge snapshots and one Gateway apply path.
   Foundations exist; current Box/Gateway recertification remains.
2. **Workflow Orchestration** compiles ontology-defined goals and typed graphs
   into recoverable execution. `W0.1` is implemented, `W0.2` is verified, and
   `W0.3` includes deterministic composite frame/export and ordinal reducers,
   Flow-backed bounded-parallel Iteration waves, and sequential Loop child
   WorkflowRun dispatch, linkage, cancellation, process-death adoption, and
   recovery. New parallel policies use WorkflowRun/Flow v22 while v3-v21
   histories retain serial replay. Run v23 composes those semantics with
   Flow-owned, reverse-order cancellation compensation for accepted exact
   Connector effects, including a distinct cleanup response step when
   cancellation preempts ordinary typed-response materialization. Agent-bearing
   Run admission emits Run v24 for exact `agent.classic` or `agent.release`
   steps bound to one immutable `AgentRelease` and `agent.execute`. The
   Agents-owned application port creates or adopts one dedicated conversation
   and Agent execution, verifies the published artifact digest, links the child
   Agent Flow identity, resumes only from a matching terminal semantic event,
   and waits for child cancellation before parent termination. Completed and
   cancelled projections retain the exact Agent conversation, execution, and
   Operation URNs; successful output also pins the immutable provider profile
   and provider-run identity. Migration `161` admits the Agent projection kind,
   Plan v12 and Run v25 additionally admit one exact descriptor-owned `error`
   output. Dispatch rejection, terminal execution failure, and terminal child
   cancellation become redacted `cloud.workflow.step-failure.v9` values on that
   ordinary DAG edge; the Agent source projection remains failed and retains
   its exact child evidence. Migration `163` admits only this failed Agent
   selected-handle shape. Runtime build `a3s-cloud-workflows@27` keeps `@1`
   through `@26` replayable.
   This is a component-only backend capability; public Agent node availability
   remains closed. Composite Run admission binds each
   `workflow.iteration` or `workflow.loop` descriptor to the matching immutable
   region policy, and linked child evidence advances on its exact Flow
   child-operation event. It also pins finite Execution error
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
    with typed projection evidence. Plan v6/Run v14 applies the same ordinary
    DAG mechanism to the exact Application conversation-variable descriptor:
    deterministic `Invalid`, `NotFound`, `Conflict`, and `Forbidden` writes
    become redacted failure v3 data, while transient errors keep the Hook active.
    Plan v7/Run v15 extends that exact rule to the Application Answer Output:
    deterministic terminal writes become redacted failure v4 data for root and
    composite-frame routes, while transient errors keep the Hook active.
    Migration `143` admits only failed Output selected-handle evidence. This is
    a component execution path with no OpenAPI schema change. Plan v8/Run v16
    now applies the same descriptor-bound DAG rule to Workflow-local Transform
    evaluation: one non-retryable failure becomes fixed redacted failure v5
    data, and migration `145` admits only its failed Transform selected-handle
    evidence. Plan v9/Run v17 now applies the same exact rule to ordinary
    Workflow-local Output evaluation: template or output-schema failure runs
    once, becomes fixed redacted failure v6 data, and reuses migration `143`'s
    failed Output selected-handle shape. Plan v10/Run v18 routes the exact
    Workflow-local Branch error edge, and Plan v11/Run v19 routes deterministic
    composite-region failure while keeping resume-authority drift outside DAG
    data. Run v20 adds the exact Workflow-local Variable Aggregator over
    authoritative typed reads. Its versioned ACL freezes bounded groups,
    concrete types, and ordered optional candidates; publication requires the
    exact `workflow.variable-aggregate` descriptor plus matching schemas and
    variable reads. Constraint-only migration `149` widens the existing closed
    Workflow payload-schema registry for this configuration and the already
    supported policy v2/v3 payloads. The runtime selects the first available
    non-null candidate without another store or scheduler. Run v21 adds the
    exact Workflow-local List Operator over authoritative typed reads. Its
    versioned ACL freezes the array item type, ordered filters, optional
    one-based extraction, typed ordering, and limit, including the closed
    file-compatible field matrix for object arrays; runtime applies them in
    that fixed order and returns `result`, optional `first_record`, and
    optional `last_record`. Constraint-only migration `151` widens only the
    existing payload-schema registry. The maintained client enumerates Plan
    v5-v12, failure v2-v9, and both exact local-transform configuration schemas.
    Verified terminal finite Execution projections
    now retain exact child Execution and Operation URNs, while received
    Connector observations retain exact attempt URNs in the existing bounded,
    sorted `evidenceReferences` field. Received HumanDecision resumes retain
    exact HumanTask and WorkflowDecision URNs, plus the accepted FormSubmission
    URN for interactive outcomes. Automatic expiry and cancellation add no
    synthetic submission. Linked Subworkflow frames retain exact child
    WorkflowRun and Operation URNs; Iteration and Loop steps retain the latest
    16 linked frames within the existing 32-reference bound. These are
    authorization-neutral correlations reconstructed from Flow history, not
    copied evidence bodies. REST/OpenAPI `1.74.0` is the current contract. It
    adds closed Developer Workflows WorkloadProfile acceptance plus current,
    bounded-history, and exact immutable revision reads with canonical A3S ACL
    and typed Secret-material-free intent. The maintained client, CLI, and four
    additional Management MCP tools dispatch the same CQRS/application
    authority without adding a parser, repository, evaluator, or downstream
    lifecycle. It retains `1.73.0`'s authorization-first logical Agent
    execution checkpoint capture, list/read/snapshot, paged trajectory, and
    immutable fork APIs. Migration `168` stores only bounded object projections,
    exact execution/provider/invocation bindings, telemetry correlation, and
    parent lineage; canonical snapshots remain in the shared
    `agent-checkpoints` immutable-object namespace and materialize verified
    inherited trajectories for nested forks. Every new fork revalidates the
    published Agent artifact and selected provider profile, creates a new
    execution, verifies its parent object again before provider dispatch, and
    fails closed on missing or drifted evidence. The API does not claim
    provider-private or Box suspend/resume support. It also retains `1.72.0`'s
    closed BuildPlan detection, idempotent acceptance, and exact accepted-plan
    reads plus `1.71.0`'s bounded, authorization-first Agent
    approval-checkpoint list, read, and
    optimistic decision APIs; the closed `awaiting_approval` execution state;
    and digest-only approval-resolution evidence. Exact replay, expiry, denial,
    cancellation, and resume state never expose Secret or Tool payload
    material. It also retains `1.70.0`'s immutable Harness invocation profile,
    typed digest-only Tool request/result event content, and same-transaction
    shared audit correlation, plus `1.69.0`'s closed
    Agent provider selection and immutable provider evidence, plus `1.68.0`'s
    Plan v12/compiler v12, `cloud.workflow.step-failure.v9`, and the
    three closed Agent failure classifications through OpenAPI and the maintained
    TypeScript client without adding a route or response field. It retains
    `1.67.0`'s exact `cloud.workflow.policy.v4` cancellation-compensation policy,
    `1.66.0`'s bounded unresolved Connector-attempt reads and exact idempotent
    operator conclusion that closes an expired dispatch only as body-free
    `indeterminate` evidence, plus `1.65.0`'s exact Connector revision
    revocation and closed operation-specific Connector success schemas, plus
    `1.64.0`'s complete Workflow-tagged schemas, without changing prior response
    bytes. The
    maintained client, CLI, and one read-only Management MCP tool expose an authorized bounded
    `cloud.workflow-run.diagnostics.v1` projection. It compares the persisted
    Workflow sequence with one consistent A3S Flow snapshot/history read,
    reports step and event counts, durable waits, retries, host-shutdown
    recovery boundaries, child correlations, and at most 256 exact evidence
    references, and explicitly reports truncation. It stores no diagnostic,
    metric, evidence body, or second history. User-authored publication also
    checks both legacy graphs and every admitted descriptor against the closed
    set of Cloud runtime dispatch paths. Descriptor-bearing Execution steps
    must use the exact `executions.finite` identity and semantic profile;
    `execution.code` and caller-named aliases remain fenced until their owning
    contracts land. Semantic-free Agent, MCP, model, Tool, Memory, and
    Subworkflow steps are rejected. Descriptor-bearing Agent steps admit only
    the exact `agent.classic` or `agent.release` identity, Agents ownership, one
    non-nil immutable `AgentRelease`, and `agent.execute`; caller-named aliases
    remain fenced. Exact Applications-generated Agent presets use this internal
    runtime path, while unwired model presets remain deferred composition
    evidence. Historic revisions, Plans, Goals, and persisted Run histories
    remain readable, but every new Goal/Plan or Run compilation rechecks the
    same closed dispatch set. Unwired internal provider presets therefore cannot
    launch a new execution before their owning ports land. After authorization,
    an exact pre-upgrade idempotency replay is resolved before this availability
    check; changed input under the same key still conflicts, and a new key still
    reaches the fence. The public API shape is unchanged. The exact Connectors-
    owned HTTP Request foundation is now internally discoverable, and one exact
    ordinary-step Connector domain-result compensation path is certified at
    component scope. Deferred Connector cancellation and immutable-deadline
    projection are also fenced against provider redispatch across coordinator
    replacement. Migration `154` adds immutable exact Connector revision
    revocation, serialized with C6 dispatch admission; REST/OpenAPI `1.65.0`
    and the maintained client expose its authorized idempotent write and exact
    read without changing historic attempt replay. Migration `155` adds the
    separate immutable attempt-resolution authority; REST/OpenAPI `1.66.0`
    exposes only safe attempt metadata and an audited `indeterminate`
    conclusion after the exact outcome deadline. Terminal replay remains
    indeterminate and cannot redispatch, retry, cancel, or fabricate a provider
    outcome. Migration `158`, Workflow policy v4, Connector Hook v4, and Run
    v23 additionally compensate accepted exact Connector effects in reverse
    Plan order during Flow-owned cancellation and close the accepted-effect /
    typed-response race with a distinct stable cleanup step. Run v24 adds exact
    Agent child dispatch, restart adoption, terminal semantic output, provider
    evidence, and cancellation cleanup. Plan v12/Run v25 add only the exact
    descriptor-bound Agent error branch and redacted failure v9 interpretation,
    without adding a public route. Public
    business-service and Agent node availability, MCP/model/Tool dispatch,
    broader provider conformance and revocation, general or multi-provider
    compensation, and later `W0` gates remain open.
3. **Agent Factory** turns heterogeneous Harness implementations into
   immutable, evaluated, deployable Agent products. `A1.0` is verified and
   `A1.1` is implemented. Native Code `A1.2` is verified and carries start,
   run-scoped cancellation, deterministic recovery, event pages, retention
   gaps, and same-generation provider-process restart recovery through the
   existing Flow/Fleet/Runtime/node-journal path. The
   [retained clean Linux PostgreSQL 17 and real Box Runtime recovery gate](https://github.com/A3S-Lab/Cloud/actions/runs/32875814179/job/97893488672)
    proves durable command, receipt, event, process-incarnation, and cleanup
    behavior while Cloud consumes the exact crates.io releases
    `a3s-code-core 8.0.1` and `a3s-flow 1.1.0`. The component-level `A1.3`
    foundation now freezes one canonical ACL provider profile, capability
    negotiation, provider-neutral start/cancel/recover commands and event-page
    evidence, and admits profile-bound event batches through the authenticated
    Node Control API with exact Runtime/run identity checks and idempotent
    in-memory/PostgreSQL receipts. It migrates new Code runs onto the common
    contract while retaining the native event endpoint and durable legacy
    history replay, and exercises Code plus a deterministic non-Code reference
    Harness through one restart/cancellation/cleanup conformance suite. The
    current backend adds closed REST/client/CLI selection, migration `164`'s
    creation-time immutable profile persistence, exact Flow registry recovery,
    fail-closed Node protocol routing, and durable provider-neutral event-page
    delivery through the shared outbound-batch primitive. Component-level
    `A1.4` adds one fail-closed immutable invocation profile before dispatch,
    binds its digest into provider run identity, and records digest-only Tool
    request/result semantics with shared audit correlation. Component-level
    `A1.5` adds durable approval checkpoints and exact provider-neutral resume.
    Component-level `A1.6` adds bounded immutable logical execution
    checkpoints, explicit fork lineage, trajectory reads, exact Runtime
    telemetry correlation, migration `168`, REST/OpenAPI `1.73.0`, and the
    maintained TypeScript client. Production model and Tool binding producers,
    any additional independent MCP binding, retained PostgreSQL/real-provider
    recovery and audit evidence, and provider/Box private checkpoint
    certification remain open.
4. **AI Application Platform** composes Applications, Knowledge, plugins,
   automations, and governed delivery from exact Workflow/Agent revisions.
   `APP0.1` freezes one canonical immutable release across all six
   experiences, including distinct classic/New Agent identities, pins the
   exact Workflow revision evidence without copying its graph or runtime, and
   persists sequence-fenced releases with atomic idempotency, audit, and
   Outbox facts through PostgreSQL/A3S ORM. Project authorization, CQRS,
   REST/OpenAPI `1.42.0`, the maintained client, CLI, and six Management MCP
   tools all reuse that authority. Component-only
    `APP0.2-C1/C2/C3/C4/C5/C6/C7/C9/C10/C11/C13/C14/C15` adds and
   persists the single release-pinned session/message/variable contract,
   deterministic Workflow-effect replay boundary, and immutable invocation
   execution authority through migrations `125`-`127`. A typed internal port
   creates or adopts the exact ordinary Workflow Goal, Plan, and Run and
   recovers cancellation, while deterministic Model/Agent preset wrappers use
   Workflow's shared publication authority. Neither adds another Flow history
   or dispatch path. Authorization-first internal session, invocation,
   cancellation, and bounded cursor CQRS recover exact persisted state; no
   application-scoped public delivery protocol is claimed. The C7 internal
    Workflow consumer port resolves Applications authority from the bound Run
    and applies exact Answer/final-output/variable/terminal effects. C9 uses
    application-only Run v10 to project aggregate final output and terminal
    state before WorkflowRun persistence. C10 adds descriptor-bound Answer
    dispatch through Run v11; C11 adds Run v12 snapshot/CAS dispatch and
    Flow-derived inspection for exact Application-variable ports; C13 adds v13
    root-bound repeated Answer frames; C14 adds Plan v6/Run v14 redacted
    deterministic Application-variable failure branches; and C15 adds Plan
    v7/Run v15 redacted root and frame-bound Answer failure branches through
    migration `143`. C8 adds
    project-member management delivery through REST
   contract `1.43.0`, the client, CLI, and five Management MCP tools. C12 adds
   close/cancel/full replay through contract `1.44.0` and three more Management
   MCP tools, while application credentials, answer streaming, and Gateway
   delivery remain unavailable.
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

</details>

## Configuration

Cloud and the Node Agent use closed, validated A3S ACL. Unknown fields and
unsafe timing relationships fail before startup; Secret values never belong in
ACL. Configuration is parsed only through `a3s-acl`.

| Area | Responsibility |
| --- | --- |
| `server`, `auth`, `postgres` | Process roles, bootstrap, identity, durable state |
| `events`, `operations` | Outbox publication and durable operation timing |
| `node_control`, `fleet` | Outbound mTLS, leases, inventory, observations, Claims |
| `deployments`, `executions`, `builds`, `artifacts` | Workload, Task, Box build, immutable-content admission |
| `objects` | One deployment-level local or S3-compatible immutable-object root |
| `registry`, `sources`, `edge`, `gateway` | Source policy, OCI publication, routes, certificates, Gateway apply |
| `logs`, `security`, `box` | Retention, production trust, isolation, transient Secret materialization |

Use [`config/cloud.acl`](config/cloud.acl) and
[`config/node.example.acl`](config/node.example.acl) for development, and
[`deploy/production/compose.acl`](deploy/production/compose.acl) with its
shared [`cloud.acl`](deploy/production/cloud.acl) for the Box-hosted baseline.

## Repository

```text
Cloud/
|-- crates/
|   |-- contracts/       # versioned cross-process contracts
|   |-- control-plane/   # bounded contexts, API, workers, persistence
|   `-- node-agent/      # outbound node protocol and execution adapters
|-- migrations/          # PostgreSQL schema evolution
|-- config/              # closed A3S ACL configuration
|-- openapi/             # committed REST contract
|-- packages/cloud-client/
|-- cli/
|-- tools/               # provider, recovery, architecture, release gates
`-- docs/                # architecture, plans, decisions, runbooks
```

## Development

Run validation from the Cloud repository root:

```bash
cargo fmt --all -- --check
cargo test -p a3s-cloud-control-plane architecture_tests --lib
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Real-provider and release certification runs on isolated Linux hosts. Important
repository-owned gates include:

- [`C0.1` cross-surface conformance](tools/c0-conformance/README.md)
- [Runtime conformance](tools/runtime-conformance/README.md)
- [A3S Box provider conformance](tools/box-conformance/README.md)
- [Pinned Gateway conformance revision](tools/gateway-conformance/gateway-revision)

## Documentation

| Document | Authority |
| --- | --- |
| [Product roadmap](ROADMAP.md) | Gate status, dependencies, delivery order |
| [Technical architecture](docs/architecture.md) | Ownership, topology, consistency, failure behavior |
| [Architecture audit](docs/architecture-audit.md) | Per-module boundary findings, duplicate mechanisms, optimization waves |
| [Domain model](docs/domain-model.md) | Aggregates, state machines, invariants |
| [Development plan](docs/development-plan.md) | Implementation slices and exit evidence |
| [PostgreSQL schema management](docs/postgres-schema-management.md) | Migration authority, rolling order, admission, failure rules |
| [Workflow and evolution](docs/workflow-evolution-plan.md) | Workflow, heterogeneous Agent, governed evolution contracts |
| [AI application platform](docs/ai-application-platform-plan.md) | Applications, Knowledge, Automations, node coverage |
| [Durable Cell Service](docs/durable-cell-platform-plan.md) | Cell authority, Runtime/provider/S0 split, fencing, fault evidence |
| [Inference plan](docs/inference-plan.md) | Model, provider, routing, usage, conformance design |
| [Architecture decisions](docs/decisions/app-platform/README.md) | Normative application-platform decisions |
| [Management MCP](docs/management-mcp.md) | Protocol, authorization, tool contract |

## License

[MIT](LICENSE)
