# A3S Cloud Architecture Audit

## 1. Purpose and authority

This document records the implementation-facing architecture audit begun on
2026-08-24. It does not replace the stable target in
[architecture.md](architecture.md), the aggregate definitions in
[domain-model.md](domain-model.md), or delivery status in
[ROADMAP.md](../ROADMAP.md).

Its purpose is narrower:

- compare the current modular-monolith code with the declared DDD boundaries;
- identify duplicated mechanisms, foreign authority access, and missing ports;
- define an ordered refactoring path whose intermediate states remain testable;
- provide architecture fitness criteria that can eventually become mandatory
  CI checks.

An item in this document is not a shipped capability claim. Product
availability remains gate-driven.

## 2. First-principles review model

Every module is reviewed against six questions.

| Question | Required answer |
| --- | --- |
| What decision does the module uniquely own? | One bounded context owns each mutable business invariant. A facade or product composition is not a second owner. |
| What is the consistency boundary? | One aggregate transaction is local to its owner. A cross-context invariant must become an explicit port, committed fact, or a deliberate boundary merge. |
| What crosses the boundary? | Typed IDs, immutable revisions, digests, published-language snapshots, commands through owner ports, and committed integration facts. |
| What must remain outside? | Foreign repositories, foreign tables, provider clients, presentation DTOs, scheduler state, and copied lifecycle state machines. |
| How does work survive failure? | Intent is committed before work; Flow and Operations coordinate durable work; exact generation evidence drives recovery. |
| How is the claim proved? | Layer tests, contract tests, process-death replay, provider conformance, and gate status must all agree. |

These yield the dependency rule:

~~~text
presentation -> application -> domain
                     ^           |
                     |           | owns ports and published language
               infrastructure ---+

consumer application -> owner application port
owner commit -> Outbox -> integration fact -> consumer projection
~~~

A technical dependency is acceptable only when it preserves that ownership
direction. Reusing a foreign repository or table is not interface reuse.

## 3. Evidence from the current tree

The codebase has strong foundations:

- nearly every implemented business module has explicit application, domain,
  infrastructure, and presentation layers;
- domain repositories and provider services are generally expressed as Rust
  traits and implemented by infrastructure adapters;
- PostgreSQL business state goes through A3S ORM and one migrator;
- idempotency, audit, Outbox, A3S Event, Flow, Operations, Fleet, Runtime, Box,
  Gateway, Secret materialization, and immutable-object access already have
  named authorities;
- Workflow and Forms already contain source-level authority tests.

The audit also found structural gaps that prevent the stronger statement that
all boundaries are interface-only:

1. Most context roots publicly expose their application, domain,
   infrastructure, and presentation modules. This makes the intended facade a
   convention instead of a compiler-enforced boundary.
2. Product application services frequently import another context's repository
   trait and aggregate directly. The dependency is abstract in Rust, but it
   still bypasses the owning application boundary.
3. Production code contains direct cross-context Infrastructure and
   Presentation dependencies. The Artifacts-to-Assets persistence edge has
   been removed; the most important remaining examples are Workflow to Forms
   persistence, Durable Cells to Workloads/Edge implementation types, and
   shared tenant guards defined under Identity presentation.
4. Multiple modules independently map the same physical tables. The source
   scan found duplicate mappings for operation_requests, workloads, nodes,
   mcp_service_profiles, and workflow_runs. The workflow_runs duplication is
   internal to Workflow but still creates two schema authorities. Workflow also
   maps Forms-owned release state, while
   Forms still contains raw SQL for its own release records.
5. The architecture text says domain code has no Runtime or Flow imports, while
   several execution-plane domains use pure a3s-runtime contract types and the
   Workflow domain deliberately uses the pure Flow DAG compiler. The target
   must distinguish an admitted published contract from a provider adapter;
   otherwise the rule is both too broad and unenforceable.
6. Periodic reconciliation loops are repeated across modules. Their durable
   decisions generally remain in repositories and Flow, which is correct, but
   scheduling, shutdown, and error policy are not yet expressed through one
   small worker lifecycle abstraction.

The first four findings are boundary defects. The fifth is a specification
defect. The sixth is a maintainability issue and must not be solved by creating
a second durable scheduler.

## 4. Target module boundary

Each bounded context will expose three deliberately different surfaces.

| Surface | Visibility | Contents |
| --- | --- | --- |
| Published language | Cross-context | Stable IDs, immutable references, bounded snapshots, closed outcome enums, and committed fact schemas |
| Owner ports | Cross-context, application-facing | Commands and queries that preserve the owner's authorization, invariants, idempotency, and replay rules |
| Composition surface | Crate-only | Concrete repositories, provider adapters, controllers, workers, and module wiring used only by the process composition root |

The target Rust shape is:

~~~text
context/
  contracts/        # published language only
  application/      # owner use cases and inbound/outbound ports
  domain/           # private aggregates and invariants
  infrastructure/   # private adapter implementations
  presentation/     # private inbound adapter implementation
  mod.rs             # narrow facade
~~~

Domain types are not made public merely to simplify a consumer. When a
consumer needs part of an aggregate, the owner publishes a bounded immutable
snapshot or an application port result.

## 5. Module-by-module review

### 5.1 Governance and shared mechanisms

| Module | Unique authority | Current assessment | Required optimization |
| --- | --- | --- | --- |
| shared_kernel | Stable cross-context IDs, canonical digests/timestamps, repository error, and idempotency request/result shapes | Small and mostly disciplined. It also contains Identity- and Secrets-flavoured references that require continued admission scrutiny. | Freeze an explicit admission test: a type enters only when at least three independent contexts need identical semantics and no business lifecycle moves with it. Do not add repositories, policy services, or convenience adapters. |
| Identity | Organizations, Principals, Memberships, credentials, grants, authorization decisions, and verified recipient contacts | Aggregates and repository ports are clear. Resource Grant creation directly consumes Projects and Fleet repositories; shared HTTP guards live under Identity presentation and are imported by nearly every presentation module. | Publish an authorization context and owner application ports. Move shared inbound authorization adapters to the root presentation composition layer. Replace direct Project/Node repository access with resource-scope owner ports. |
| Projects | Project, Environment, tenant attribution lineage | Compact aggregates and repositories. Creation directly checks Identity ownership, while queries carry Identity's concrete evaluator. | Depend on an Identity organization-scope port and a published authorization contract, not Identity repositories or presentation types. |
| Audit | Append-only security-relevant records, signed export, and retention policy | Authority is distinct. A periodic retention runner currently lives in application code. | Keep the deterministic retention pass in application; move ticker/shutdown policy behind the shared worker lifecycle. Audit remains observation, never domain state. |
| Security | Authorized investigation projections over owner evidence | Correctly projection-only, but its domain imports Edge event types directly. | Consume versioned Edge published facts through a projection port. Keep evidence ownership and enforcement in Edge/Identity. |
| Search | Rebuildable authorized read projections | Correctly non-authoritative, but repository and result domain shapes depend on Identity's concrete evaluator. | Accept a bounded published authorization scope at the application boundary; keep indexing and filtering behind Search ports. |
| Integration Events | Transactional Outbox publication and consumer coordination | Clear shared mechanism with dedicated ports. | Treat it as platform infrastructure rather than a business context in diagrams. Keep one relay, one event envelope, and explicit projectors. |

### 5.2 Source and software supply chain

| Module | Unique authority | Current assessment | Required optimization |
| --- | --- | --- | --- |
| Sources | External connection, subscription, exact source revision, and webhook delivery | Strong provider ports and immutable revision model. It now owns the versioned `a3s.cloud.source-build-input.v1` published snapshot and the sole Application projection exposed by its root facade; Artifacts Domain enters Sources only through that language. Other Application handlers still directly query Projects/Identity repositories for scope. | Migrate remaining consumers to the published BuildRecipe/input language, replace foreign aggregate readers with owner adapters over consumer ports, and introduce organization/environment scope ports. External Git/GitHub clients remain infrastructure. |
| Developer Workflows | Reviewable BuildPlan, workload-profile proposal/acceptance, preview intent, and later monorepo/import decisions | Domain owns its process, Secret-binding, resource, port, health, branch, installation-ref, and pull-request-change proposal values. It enters Sources only through Published Language and imports no foreign owner internals. Application owns action-scoped `IDeveloperWorkflowAuthorizationPort`, `IWorkloadBuildOutcomePort`, `IServiceProfileAdmissionPort`, and `IScheduledTaskProfileAdmissionPort`; Identity policy, Artifacts aggregate state, and Workloads/Executions templates stay private. Build outcomes bind the exact BuildPlan ID/digest, while target admission returns one correlation-bound receipt carrying the target, exact request context, Artifact digest, and opaque owner-contract digest. Architecture tests enforce both layers against every foreign internal model. | Implement the Identity/Artifacts/Workloads/Executions owner adapters and production composition, then add exact Projects/Edge handoffs. No build, scheduler, deployment, route, webhook-verification, or provider lifecycle moves here. |
| Assets | Hosted product identity, immutable Agent/MCP/Skill releases, hosted Git, and release bindings | Domain consumes only the versioned Artifacts-owned `HostedBuildOutcome` published language for hosted publication. Assets owns the idempotent Outbox projector and release transaction; Artifacts no longer imports Assets infrastructure or mutates `asset_releases`. The deployable Agent query returns a bounded Assets read model and obtains the mutable OCI registry location through `IHostedArtifactQueryPort`, never a BuildRun aggregate. Assets Domain still embeds Sources BuildRecipe types. | Move BuildRecipe usage to Sources Published Language, narrow the root facade, and eventually replace direct consumer repository access with an Assets-owned release-admission port. Keep release lifecycle and projection exclusively in Assets. |
| Artifacts | BuildRun, admitted immutable outputs, evidence, provenance, retention, and node artifact transport | The async node-artifact byte port belongs to Application, so Domain imports no Tokio or object-store transport error. Domain no longer imports a Sources aggregate or `sources::domain`; it translates only the owner-published immutable input and recipe language into local `BuildSource`. Application owns `IBuildLogQueryPort`, the minimal `IHostedArtifactQueryPort`, and the versioned location-free `a3s.cloud.hosted-build-outcome.v1` fact. Successful hosted finalization commits BuildRun plus one Outbox fact in the owner transaction; Assets projects it independently. Presentation is crate-private. Infrastructure remains public migration debt while shared Flow composition imports its runtime registry, and its resolver still loads Sources and Assets repositories for input preparation. | Implement a Box-owned durable build-log adapter when that published contract exists, replace transitional source/input repository dependencies with Artifacts-owned reader ports implemented by their owners, and privatize Infrastructure after shared Flow composition consumes the root facade. Never restore foreign Asset writes or a second publication queue. |

### 5.3 Execution and traffic plane

| Module | Unique authority | Current assessment | Required optimization |
| --- | --- | --- | --- |
| Operations | User-visible long-running operation identity and progress | Correct single projection authority, but several owner repositories map operation_requests themselves to obtain atomic creation. | Define one transactional Operation request participant or an intent Outbox contract. No context may independently declare the Operations table schema. The operation engine remains Flow-backed. |
| Executions | Generic finite Task product and immutable ExecutionTemplate | Clear Task semantics and one Workflow port. Application services directly use Operations and Projects repositories; domain policy contains a3s-runtime protocol types. | Keep product requirements in local value objects, translate to Runtime contracts at the adapter edge, and use scope/operation owner ports. Runtime contract use is allowed only in the execution adapter published language. |
| Workloads | Service desired state, Deployment, rollout, replicas, placement groups, resource claims, and writer fencing | The correct scheduling authority, but also the largest coupling hub. Agent binding now receives a Workloads-owned `AgentReleaseAdmission`; its Domain no longer imports Artifacts or a BuildRun, and one Application anti-corruption adapter translates the Assets deployable read model. Other domain/application paths still import Assets, Fleet, Operations, Projects, Secrets, and Sources internals, while Infrastructure maps foreign Fleet, Asset, Secret, and Operation state. | Continue splitting inbound admission ports from internal aggregates. Publish WorkloadDeploymentIntent, RuntimeTargetObservation, and writer-fence receipts. Fleet, Secrets, Assets, and Operations must be accessed through owner ports or facts. Remove foreign table mappings. |
| Fleet | Node, pool, enrollment, inventory, Claim, command journal, observation, and fencing | Strong single node-control authority. Its domain legitimately carries the versioned node command published language but should not expose provider implementations. | Make NodeCommand and observation schemas an explicit contract surface. Keep a3s-runtime translation at the Node/Runtime adapter boundary. Publish node-scope queries for Identity/Edge instead of sharing repositories. |
| Edge | DomainClaim, certificate, Gateway scope, Route, rollout, and complete applied snapshot | Correct live-traffic intent owner, but domain types embed Assets, Workloads, and Secret shapes; infrastructure reads foreign nodes/workloads/MCP profiles through local table mappings. | Introduce RouteTarget, GatewayMember, and MCPProfile owner ports or fact-fed local projections. Edge compiles only owner-published snapshots. Gateway remains the sole applied request path. |
| Secrets | Secret/version lifecycle, authorization, encryption and exact materialization | Clear authority. Deployment material resolution directly loads Workloads state to authorize a consumer. | Replace Workload repository access with an exact SecretConsumerAuthorization port implemented by Workloads. Material never crosses presentation or domain events. |
| Data / S0 | Object namespace contract, storage provider policy, backup/restore/retention, and writer fencing | The object namespace is correctly separate from business desired state. Current scope is incomplete for general databases/volumes. | Keep one immutable-object client and define owner ports for exact Secret materialization. Add database/volume aggregates only under S0 gates; never absorb Durable Cell application semantics. |

### 5.4 AI and interaction products

| Module | Unique authority | Current assessment | Required optimization |
| --- | --- | --- | --- |
| Agents | Conversation, AgentExecution, semantic event sequence, approvals/checkpoints/forks trajectory | Semantic state is separate from Runtime logs and generic Executions. Start no longer reads a BuildRun aggregate: it consumes the bounded Assets deployable read model and an Artifacts OCI-location query interface. It still orchestrates those two owner interfaces directly. | Introduce an Agents-owned AgentReleaseAdmission port whose adapter composes the two owner interfaces and returns one immutable execution binding. Keep provider-neutral Harness execution behind an Agents-owned port and the common Workloads/Fleet/Runtime path. |
| Applications | Application/release, session, invocation, message, variable, and delivery semantics | Best current example of consumer-owned application ports to Workflow. A few Workflow constants/types still leak directly into application code. | Complete the anti-corruption layer: all Workflow compilation/run/effect interaction travels through Applications-owned ports and published snapshots. No Workflow repositories or timeout helpers cross directly. |
| Workflow | Ontology, WorkflowDefinition, Goal, Plan, WorkflowRun, HumanTask, decision, and semantic step projections | Rich invariants and strong Flow authority tests. Domain imports Forms submission types; persistence reads/writes Forms state directly. The module is also large enough that internal packages need explicit ownership. | Define internal subdomains for Definition/Planning, Run, and Human Interaction. Access Executions, Connectors, Applications, and Forms only through application ports. Flow remains the only durable execution history. |
| Forms | Form draft/release schema and form semantic validation | Domain is isolated and already guarded. FormSubmission is declared Forms-owned while Workflow persists it inside the HumanTask transaction, so authority and storage disagree. | Make the ownership decision explicit. Recommended: Forms owns definitions/releases and validation; Workflow owns an immutable HumanTaskSubmission evidence value. If general standalone submissions are later required, they use a separate Forms command and aggregate. |
| Connectors | Reusable outbound profile/revision, egress policy, exact attempt fencing/evidence, and response-object reference | Provider-neutral attempt model and Workflow port are strong. Application services directly depend on Secrets application/domain types. | Publish a SecretVersionAccess port result and keep exact materialization in Secrets. Connector attempts remain the sole HTTP retry/evidence authority for consumers such as Notifications. |
| Notifications | Personal inbox, subscriptions, alert policy, delivery facts, and terminal delivery receipts | Correctly reuses Connector and SMTP mechanisms, but domain types embed Connector outcomes and Identity email/evaluator types. | Consume versioned Connector and Identity published contracts. Alert sources arrive only as closed owner facts. Do not add provider retries, timers, or authorization stores. |
| Plugins | Tenant registry enrollment and exact A3S Use package assignment intent | Small, port-oriented, and correctly excludes installation/reconciliation authority. | Narrow the module facade and preserve A3S Use as the only package lifecycle owner. |
| Files | Upload session/metadata intent and immutable-object references | The aggregate and ACL contract are isolated. The byte-stream and object-store port now live in Application, while Domain receives only an exact durable-write receipt; no command/query or presentation boundary exists yet. | Complete the application use cases and persistence boundary before exposure. File bytes use the shared immutable-object authority and never become Build Artifacts. |

### 5.5 Durable Cells

Durable Cells is a product semantic context, not a generic storage module and
not a second runtime.

Current strengths:

- application identity, immutable revisions, service profile, schema
  compatibility, storage binding, and deployment correlation are explicit;
- individual Cell state, lease, epoch, alarm, and residency are not copied into
  Cloud PostgreSQL;
- Workloads, Fleet, Runtime Service, Box, Edge/Gateway, Secrets, Operations,
  Artifacts, and S0 remain recognizable authorities.

Current boundary debt:

- Durable Cells domain and application code import Workloads, Executions, Data,
  Artifacts, Fleet, Operations, and Edge internal types;
- application code calls Workloads/Edge implementation helpers and handlers;
- presentation DTOs reuse Workloads and Edge presentation DTOs;
- the Cell provider profile is frozen in Cloud, while a3s-runtime currently
  exposes only generic Task/Service capabilities and conformance.

Target split:

| Layer | Owns |
| --- | --- |
| Durable Cells in Cloud | Application identity, immutable revision, Cell-class/state-schema compatibility, retention intent, and exact deployment/route correlation |
| a3s-runtime | Ordinary Service lifecycle plus a composable, provider-neutral NamedStatefulService capability profile and conformance evidence |
| Box and selected Cell provider | Provider process, activation, per-key serial turns, SQLite lineage, alarm/WebSocket behavior, idle eviction, recovery, and epoch fencing |
| Data / S0 | Namespace lifecycle, credentials, conditional object semantics, backup, restore, retention, and deletion evidence |
| Workloads/Fleet/Edge | Placement, claims, node commands, rollout, healthy target selection, Route intent, and Gateway publication |

NamedStatefulService is a Runtime Service capability profile, not a new
Runtime Unit class. An individual Cell is never a Runtime Unit. Cloud's A3S ACL
profile compiles to versioned Runtime capability requirements; a provider
advertises and proves those requirements through a3s-runtime conformance.
Product policy and tenant state do not move down.

The required Cloud refactor is a set of consumer-owned ports:

- DurableCellBuildArtifactPort;
- DurableCellStoragePort;
- DurableCellExecutionPort;
- DurableCellWorkloadPort;
- DurableCellRoutePublicationPort;
- DurableCellRuntimeProfilePort.

Concrete owner repositories, handlers, Runtime projection helpers, and
presentation DTOs must disappear from Durable Cells imports.

## 6. Duplicate-mechanism decisions

| Concern | Single authority | Refactoring rule |
| --- | --- | --- |
| Authorization | Identity policy plus owner application admission | One published authorization context; no context-specific token parser or foreign presentation guard |
| Idempotency | Shared idempotency record and owner transaction | No local idempotency table or in-memory production replay cache |
| Long-running work | Operations plus A3S Flow | A local ticker may discover durable intent, but it cannot own retry history, workflow state, or success |
| Placement and rollout | Workloads plus Fleet | Product modules compile intent; none owns Claims, nodes, rollout controllers, or queues |
| Runtime lifecycle | a3s-runtime Task/Service plus Box | Product capabilities are profiles; no product-specific Runtime class or direct process runner |
| Traffic | Edge desired state plus Gateway applied snapshot | No direct Gateway configuration, Cell owner lookup, or product-local route store |
| Secrets | Secrets | Exact opaque references cross ports; plaintext exists only in bounded materialization adapters |
| Immutable bytes | Shared immutable-object client, with semantic metadata owned by its context | No second S3 client, bucket lifecycle, or byte authority |
| Mutable storage | Data / S0 | No product-local backup, retention, volume, or writer-fence engine |
| Integration facts | Transactional Outbox plus A3S Event | No product-local event bus or publish-before-commit path |
| Audit | Shared append-only audit path | No domain state reconstructed from audit and no second audit store |
| Retry | Flow for workflow steps; owning provider attempt model for external delivery | Retry budget, clock, and terminal classification must have one owner per side effect |

## 7. Ordered optimization waves

### Wave 0: architecture fitness functions

1. Add a source-level module dependency audit that understands production code
   versus test-only blocks.
2. Freeze the current cross-context outer-layer edges as explicit debt; reject
   every new edge.
3. Reject duplicate physical-table mappings outside a temporary reviewed debt
   list.
4. Reject provider/framework imports from product semantic domains, while
   explicitly admitting pure published contracts at named execution boundaries.
5. Add a narrow-facade test for every context.

Baseline status on 2026-08-24: the source-level ratchets live in
[`architecture_tests.rs`](../crates/control-plane/src/modules/architecture_tests.rs).
They freeze exact cross-context outer-layer sites, duplicate physical ORM
mappings, domain technical-dependency debt, named Runtime/Flow published
contract entry points, Shared Kernel direction, and public Infrastructure /
Presentation facades. The baseline passes. This proves that these debt classes
cannot silently expand; it does not certify the allowlisted sites as correct or
replace the refactors in Waves 1-6.

### Wave 1: governance ports and module facades

1. Move shared tenant/resource HTTP guards to root presentation composition.
2. Publish the immutable authorization context from Identity.
3. Introduce Organization, Project/Environment, and Node scope lookup ports.
4. Make Infrastructure and Presentation crate-private; keep public contracts
   and application ports deliberate.

Artifacts now satisfies item 4 for Presentation. Its node-artifact stream,
descriptor, error, and store contract form one consumer-owned Application
port; Domain receives admitted artifact values and receipts only. The public
root names exact presentation DTOs without exposing that namespace.
Infrastructure remains temporarily public because shared Flow composition
still names its runtime registry directly; that exact site stays frozen. The
Artifacts-to-Assets publication write is now removed. Remaining Sources/Assets
input preparation and Fleet runtime relationships stay explicit Wave 2 debt.

### Wave 2: supply-chain handoffs

1. Publish Sources input snapshots.
2. Remove Sources/Artifacts/Assets aggregates from Developer Workflows domain.
3. Replace the Artifacts-to-Assets table write with an idempotent durable
   publication handoff.
4. Replace Fleet log DTO reuse with an owner log-query port.

Item 1's owner contract is implemented as the immutable
`a3s.cloud.source-build-input.v1` published language. Sources alone validates
`ExternalSourceRevision`, and its Application service performs the sole pure
projection through the Sources root facade; Artifacts Domain consumes only the
minimal typed snapshot and the Sources-published recipe vocabulary. The
recipe, platform, provider, and canonical repository types physically belong
to the published layer rather than being aliases for Domain internals.
Executable architecture tests reject both a future Artifacts Domain import of
a Sources-internal path and any published layer that imports its owner's Domain.
The current Artifacts Infrastructure resolver still loads the Sources
repository at the composition edge; replacing that adapter with a
consumer-owned input-reader port remains an explicit follow-up and does not
weaken the Domain boundary.

Item 2's core boundary is implemented. Developer Workflows Domain owns local
review proposal values and exact opaque owner references, and can import a
foreign context only through its Published Language. Pull-request Preview
reconciliation consumes a minimal local semantic observation rather than the
Sources webhook-verifier DTO. BuildPlan and workload-profile acceptance ask
the consumer-owned, closed-action `IDeveloperWorkflowAuthorizationPort` before
parsing ACL, replaying idempotency, or reading owner state; Identity grant
evaluators and policy vocabulary no longer enter the commands. Application obtains a
successful attested build only through `IWorkloadBuildOutcomePort` and
validates the versioned `a3s.cloud.developer-workflow-build-outcome.v1`
snapshot, including exact BuildPlan ID/digest, against the accepted BuildPlan.
An Assets ACL detector remains an Infrastructure
anti-corruption adapter over the single Assets parser, with an executable path
compatibility test. Application submits the accepted local profile through
consumer-owned `IServiceProfileAdmissionPort` and
`IScheduledTaskProfileAdmissionPort` contracts and receives only an immutable
receipt bound to the target, complete request context, Artifact digest, and
opaque owner-contract digest. Test-only owner adapters prove that Workloads and
Executions still perform final template validation. Concrete owner-side
adapters and production composition remain named follow-up work; their absence
does not reopen either model boundary.

Item 3 is implemented as one owner-commit/fact/projection pipeline. Artifacts
publishes the closed, location-free `a3s.cloud.hosted-build-outcome.v1` fact
with event key `artifact.hosted-build.succeeded`; BuildRun finalization and the
Outbox insert share the Artifacts transaction. Assets consumes that fact with
`HostedBuildOutcomeProjector`, revalidates envelope and payload identity, and
performs its own idempotent release transition and `asset.release.published`
Outbox insert. A Draft release under an archived Asset is an acknowledged
terminal no-op, so archival cannot turn a successful build into a foreign
failure or introduce another state machine. The obsolete consumer-driven
`BuildRunFinalization::Rejected` result and its compensating completion branch
are removed; Artifacts finalization now returns its terminal aggregate
directly. The generic Outbox Relay is the
only worker and retry mechanism in both all-in-one and dedicated Relay roles.
Executable fitness tests reject any future Artifacts Application import of
Assets, any Artifacts PostgreSQL mutation of Asset storage, an Assets Domain
import outside Artifacts Published Language, or a Workloads Domain import of
Artifacts. PostgreSQL replay coverage proves one hosted outcome, Draft before
projection, Published after projection, and exactly one publication event
after replay.

Item 4's consumer boundary is implemented. Artifacts Application owns
`IBuildLogQueryPort`, `BuildLogReadRequest`, `BuildLogPage`, and typed data,
chunk-gap, compacted-range, and source-discontinuity records. The handler
derives public BuildRun identity and generation from its own aggregate, rejects
pages that violate cursor, ordering, limit, or stream-filter invariants, and
never exposes node or Runtime unit placement. Artifacts Presentation preserves
the existing JSON/SSE record schema with a local DTO. An executable fitness
function rejects any future Fleet import from Artifacts Application or
Presentation. The default port remains deliberately unavailable until Box
publishes a stable durable build-log contract; inventing a Fleet unit mapping
would create a false capability and the wrong ownership boundary.

### Wave 3: execution and traffic boundaries

1. Centralize Operation request participation and eliminate duplicate schema
   mappings.
2. Replace Workloads foreign repositories with admission/observation ports.
3. Feed Edge through owner snapshots or committed projection facts.
4. Remove duplicate nodes, workloads, mcp_service_profiles, and workflow_runs
   table mappings.

### Wave 4: AI product boundaries

1. Finish Applications-to-Workflow anti-corruption ports.
2. Resolve FormSubmission ownership and migrate Workflow HumanTask persistence.
3. Publish Connector attempt outcomes and Identity contact references.
4. Isolate Agent release admission from Assets/Artifacts repositories.

### Wave 5: Durable Cells and Runtime profile

1. Define the provider-neutral NamedStatefulService profile and conformance in
   a3s-runtime without a new Runtime Unit class.
2. Compile Cloud's Durable Cell A3S ACL into that Runtime requirement.
3. Replace every Durable Cells foreign implementation import with the six
   consumer-owned ports above.
4. Retain real provider, S0, process-death, Gateway, and fencing evidence before
   advancing CELL0.

### Wave 6: seal the architecture

1. Remove every temporary boundary-debt allowlist entry.
2. Make the architecture fitness suite mandatory in CI.
3. Re-run PostgreSQL, Flow replay, Box/Runtime, Gateway, S0, and cross-surface
   certification gates affected by the refactors.
4. Update the README diagrams from proved final boundaries and keep ROADMAP as
   the only availability authority.

## 8. Definition of done

The architecture optimization is complete only when all of the following are
true:

- every module has one documented decision authority and a narrow facade;
- product domains import no foreign aggregate, repository, infrastructure, or
  presentation implementation;
- every synchronous cross-context action uses an owner application port;
- every asynchronous cross-context reaction consumes a committed versioned
  fact;
- no physical table is independently mapped by multiple owning contexts;
- no context writes another context's state;
- each durable side effect has exactly one retry, recovery, and fencing owner;
- Runtime capabilities remain provider-neutral and product semantics remain in
  Cloud bounded contexts;
- A3S ACL remains the only product configuration language;
- all temporary debt entries are gone and the complete affected certification
  matrix passes.
