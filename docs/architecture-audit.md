# A3S Cloud Architecture Audit

## 1. Purpose and authority

This document records the implementation-facing architecture audit begun on
2026-08-24 and updated on 2026-08-28. It does not replace the stable target in
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

1. Public outer-layer exposure remains broad: 46 context/layer facade entries
   still expose Infrastructure or Presentation when public module declarations
   and equivalent public re-exports or aliases are treated as the same
   mechanism. This makes the intended facade only partially compiler-enforced.
   The expanded fitness ratchet now detects both spellings, so visibility
   cannot be hidden behind a private module followed by `pub use`.
2. Product application services frequently import another context's repository
   trait and aggregate directly. The dependency is abstract in Rust, but it
   still bypasses the owning application boundary.
3. Production code contains direct cross-context Infrastructure and
   Presentation dependencies. The Artifacts-to-Assets persistence edge and
   Workflow-to-Forms persistence edge have been removed; the most important
   remaining examples are Durable Cells to Workloads/Edge implementation
   types and shared tenant guards defined under Identity presentation.
4. Multiple modules independently map the same physical tables. The source
   scan found duplicate mappings for operation_requests, workloads, nodes,
   mcp_service_profiles, and workflow_runs. The workflow_runs duplication is
   internal to Workflow but still creates two schema authorities. Forms still
   contains raw SQL for its own release records; HumanTask submission evidence
   now has one Workflow-owned mapper only.
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

Visibility is semantic, not syntactic. `pub mod infrastructure`,
`pub use infrastructure::Adapter`, and a public alias of that adapter all
expose the same forbidden outer-layer surface. Tests and process assembly use
crate-private composition exports; an external conformance fixture must move
behind a bounded test contract instead of making a concrete adapter part of
the product API.

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
| shared_kernel | Stable cross-context IDs, exact scope identity/narrowing, canonical digests/timestamps, repository error, and idempotency request/result shapes | Small and mostly disciplined. `C0.5-MT1-C1/C3` admit the closed resolved `ScopeContext` and public uncommitted `CloudScopeRef` because Identity, Projects, Workloads, Audit/Outbox and other owner ports require identical lineage semantics. The single PostgreSQL boundary resolves every tenant reference against canonical rows before persisting the full Installation lineage. Architecture tests forbid ambient request, Workspace, node, membership or Runtime authority from entering the kernel and forbid parallel platform/tenant Audit or Outbox mechanisms. Identity- and Secrets-flavoured references still require continued admission scrutiny. | Retain the admission rule: a type enters only when at least three independent contexts need identical semantics and no business lifecycle moves with it. Keep both scope values identity-only; do not add repositories, policy services, headers or convenience adapters. |
| Identity | Organizations, Principals, Memberships, credentials, grants, authorization decisions, verified recipient contacts, platform role policy/bindings, Trust Domains, and Workload Identity Policy revisions | Existing tenant aggregates and repository ports are clear. `C0.5-MT1-C1/C2` add one canonical platform-role-policy ACL, immutable permission ceilings, deterministic accepted revisions, binding/grant lifecycles, a canonical bounded tenant-support ACL, and one digest-bound privileged decision evidence model. `MT1-C3` persists one immutable Installation identity and reuses the existing Audit/Outbox rail for exact installation and tenant scopes without a sentinel Organization. Verified `MT2-C1/C2` add one platform-RBAC repository, one support-approval repository, migrations `177`-`178`, immutable policy and approval histories, versioned bindings, terminal grants, Installation-row serialization, shared idempotency/Audit/Outbox, and database-enforced owner/approval invariants. The `MT2-C3` core adds one registered Application command and sole Identity/PostgreSQL decision port that share-locks the current Principal, exact API token, policy/binding and optional grant, persists the full digest-bound allow through shared Audit, and conflicts with each revocation path; it adds no decision table, Outbox, Redis/Lane lock, or cache truth. Component-only `H0.4-WI1-C1` adds two canonical workload-trust ACLs, deterministic immutable revisions, predecessor-fenced repository ports, and one capability-only provider port through the Contracts published language; it adds no certificate, Secret, network, Runtime, or provider lifecycle. The existing boolean platform-administrator bypass, direct Resource Grant access to Projects/Fleet repositories and widely imported Identity presentation guards remain architectural debt. | Complete `MT2-C3` by composing maintained concrete REST/client/CLI/Management MCP use cases from verified request context without exposing a generic caller-authored evaluator; then finish MT3 cross-surface enforcement before treating RBAC as general authority. Replace boolean administrator bypasses, publish the broader authorization context, move shared inbound adapters to root presentation composition, and replace direct Project/Node repository access with owner ports. WI1 may build on canonical installation-scoped persistence, but WI2 evidence must still bind through owner ports. |
| Projects | Project, Environment, tenant attribution lineage | Compact aggregates and repositories. Creation directly checks Identity ownership, while queries carry Identity's concrete evaluator. | Depend on an Identity organization-scope port and a published authorization contract, not Identity repositories or presentation types. |
| Audit | Append-only security-relevant records, signed export, and retention policy | Authority is distinct. A periodic retention runner currently lives in application code. | Keep the deterministic retention pass in application; move ticker/shutdown policy behind the shared worker lifecycle. Audit remains observation, never domain state. |
| Security | Authorized investigation projections over owner evidence | Correctly projection-only, but its domain imports Edge event types directly. | Consume versioned Edge published facts through a projection port. Keep evidence ownership and enforcement in Edge/Identity. |
| Search | Rebuildable authorized read projections | Correctly non-authoritative, but repository and result domain shapes depend on Identity's concrete evaluator. | Accept a bounded published authorization scope at the application boundary; keep indexing and filtering behind Search ports. |
| Integration Events | Transactional Outbox publication and consumer coordination | Clear shared mechanism with dedicated ports. | Treat it as platform infrastructure rather than a business context in diagrams. Keep one relay, one event envelope, and explicit projectors. |

### 5.2 Source and software supply chain

| Module | Unique authority | Current assessment | Required optimization |
| --- | --- | --- | --- |
| Sources | External connection, subscription, exact source revision, and authenticated webhook delivery | Strong provider ports and immutable revision model. The transient `GithubSourceDiscoveryQueryService` restores current connection authority, applies `SourceRepositoryPolicy`, and calls one revalidating provider port; OpenAPI `1.76.0`, client, CLI, and two MCP reads share its closed repository/reference projection without persisting mutable refs or credentials. It owns the versioned `a3s.cloud.source-build-input.v1` published snapshot and `ISourceBuildInputQueryPort`; the owner-side service loads and validates the aggregate, enforces the complete organization/project/environment/revision binding, and returns only that snapshot. Its existing `source.revision.accepted@1` Outbox contract is an explicit Published Language fact consumed by the Artifacts candidate projector. `P0.3-C3` adds the distinct closed `source.pull-request-change.committed@1` Published Language fact: migration `156` extends the single provider Inbox, exact active Subscription fanout and Outbox publication share one transaction, and delivery ID/signature/raw body/digest remain private. C4's Developer Workflows anti-corruption projector consumes only this Published Language through the shared Relay and imports no Sources aggregate, repository, verifier, or Inbox. `P0.3-C5b` adds one separate Relay projector over the Developer Workflows lifecycle fact and a Sources-owned projection port; migration `159` persists an append-only version receipt and the same transaction creates or adopts the ordinary Preview SourceRevision and emits one bounded specialized fact. Push and Preview paths share the sole SourceRevision authority and neither introduces another Inbox or lifecycle. The Sources-owned `ExternalSourceBuildArchiveAdapter` implements Artifacts' consumer port and keeps installation credentials, exact checkout receipts, deterministic bounded tar policy, local paths, and temporary-file cleanup inside Sources. Artifacts Domain enters Sources only through published language, while its resolver and projector import neither the Sources aggregate nor repository. Other Application handlers still directly query Projects/Identity repositories for scope. | Migrate remaining consumers to owner Published Language/scope ports. External Git/GitHub clients remain infrastructure; do not add another Inbox, queue, relay, or retry mechanism. |
| Developer Workflows | Reviewable BuildPlan and workload-profile proposal/acceptance, Preview Policy revisions and lifecycle projection, owner-facing handoff intent, and later monorepo/import decisions | Domain owns canonical proposal, acceptance, profile, policy, lifecycle, event, and receipt values; Application owns consumer ports and the shared action-scoped authorization boundary; `published` owns only aggregate-free lifecycle facts. Sources, Workloads, Executions, Artifacts, and Projects cross through one consumer-owned anti-corruption adapter per owner. `P0.1-C5` reuses the sole Sources checkout/credential/inventory/digest/replay/cleanup mechanism. `P0.1-C6` exposes BuildPlan through one REST/OpenAPI `1.72.0`, client/CLI, four-MCP boundary and one Application read authority. `P0.2-C6` applies the same shape to WorkloadProfile acceptance/current/history/exact reads through OpenAPI `1.74.0` and four additional tools while retaining migration `147` as the sole write/revision authority. `P0.3-C7` uses two narrow Application query services for policy lineage and current behavioral Preview state, exposing the existing command and four queries through OpenAPI `1.75.0`, maintained client/CLI, and five tools without merging the two aggregates. Architecture tests reject repository/auth/parser bypass, foreign owner models, optional handoffs, duplicate checkout/credential traversal, or another Outbox/Relay/queue/worker/retry rail. | Retain Sources-owned live-GitHub discovery evidence; implement Edge/Operations adapters, production Workloads/Executions lifecycle and scheduling handoff, expiry/cleanup, retained PostgreSQL Preview cross-surface evidence, and retained WorkloadProfile certification. Never add a second parser, repository, authorization evaluator, build/deployment/route/scheduler lifecycle, webhook verifier, provider delivery path, or retry mechanism. |
| Assets | Hosted product identity, immutable Agent/MCP/Skill releases, hosted Git, and release bindings | Domain consumes the canonical BuildRecipe only through Sources Published Language and the versioned Artifacts-owned `HostedBuildOutcome` language for hosted publication. Assets owns the idempotent outcome projector and release transaction; Artifacts no longer imports Assets infrastructure or mutates `asset_releases`. Creating an active Agent/MCP draft now atomically commits both the ordinary release event and the explicit `asset.hosted-build.requested@1` Published Language fact; Skill releases cannot emit it. For build-source admission, Assets publishes `a3s.cloud.hosted-asset-build-input.v1`; `IHostedAssetBuildInputQueryPort` keeps aggregate, release, kind, pinned-manifest, and hosted-Git validation inside Assets. The deployable Agent query returns a bounded Assets read model and obtains the mutable OCI registry location through `IHostedArtifactQueryPort`, never a BuildRun aggregate. | Narrow the root facade and replace remaining direct consumer repository access with Assets-owned admission/query ports. Keep release lifecycle and projection exclusively in Assets. |
| Artifacts | BuildRun, admitted immutable outputs, evidence, provenance, retention, and node artifact transport | The async node-artifact byte port and `IBuildInputPreparer` belong to Application, so Domain imports no Tokio, provider, checkout, path, or object-store transport type. Domain translates only owner-published immutable input and recipe language into local `BuildSource`. `CloudBuildSourceResolver` composes the Sources and Assets owner query ports and revalidates the exact subject; it imports no foreign aggregate, repository, or hosted-Git authority. External staging uses the consumer-owned `IExternalSourceArchivePort`; Artifacts receives only source provenance digest, exact archive digest, size, and a temporary byte stream, then alone admits those bytes to the node-artifact store. Application owns `IBuildLogQueryPort`, the minimal `IHostedArtifactQueryPort`, `IExternalSourceBuildOutcomeQueryPort`, `IBuildCandidateProjectionPort`, and the narrower `IPreviewBuildLifecycleProjectionPort`; Published Language owns the versioned location-free `a3s.cloud.hosted-build-outcome.v1` fact and aggregate-free `a3s.cloud.external-source-build-outcome.v1` value. `P0.2-C3c` keeps BuildRun loading and terminal-success interpretation in the owner query, exposes no BuildPlan, command, credential, retry, or cleanup state, and adds no event or lifecycle. The existing generic Outbox Relay maps only Sources/Assets Published Language facts into immutable Artifacts-owned projections. `P0.3-C5c` extends the existing projector rather than adding another one: migration `162` adds immutable optional Preview provenance plus an append-only local version/retirement receipt; reservation locks only a candidate matching the latest applied active head, while retirement atomically requests cancellation on the existing BuildRun and one exact receipt can authorize only one same-revision retry. Candidate rows retain no processed, lease, retry, or lifecycle state, so `BuildRun` remains the sole executable build state machine. Successful hosted finalization commits BuildRun plus one Outbox fact in the owner transaction; Assets projects it independently. Migration 150 classifies retained cross-context foreign keys as physical identity guards, not behavioral authority. Presentation is crate-private. Infrastructure remains public migration debt while shared Flow composition imports its runtime registry; only the hosted Asset input path still reaches owner internals. | Extract hosted Asset input staging behind its owner port and privatize Infrastructure after shared Flow composition consumes the root facade. Implement a Box-owned durable build-log adapter when that published contract exists. Never restore foreign Asset writes or add another publication queue, candidate lifecycle, retry rail, or worker. |

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
| Agents | Conversation, AgentExecution, semantic event sequence, approvals/checkpoints/forks trajectory | Semantic state is separate from Runtime logs and generic Executions. Start, fork, and Workflow dispatch now consume one Agents-owned `IAgentReleaseAdmissionPort`; only the consumer-owned Infrastructure adapter may compose the Assets deployable-release and Artifacts OCI-location owner interfaces, and it returns one immutable `AgentReleaseBinding`. Application imports neither owner repository/query interface nor the foreign deployable read model. An architecture fitness test rejects a second adapter, concrete persistence, Outbox/projector, command handler, or worker in this boundary. | Keep provider-neutral Harness execution behind Agents-owned ports and the common Workloads/Fleet/Runtime path. Apply the same owner-port rule to remaining model/Tool producers; never add an Agent release repository, build query, scheduler, or retry mechanism. |
| Applications | Application/release, session, invocation, message, variable, and delivery semantics | The Application layer now has no direct Workflow module dependency. Both invocation boundaries enter timeout admission through `IApplicationWorkflowRunPort`; its sole production adapter delegates to Workflow's owning rule, the copied 30-day Domain constant is removed, public schemas reference owner constants, and migration `171` removes migration `127`'s historical database copy. An architecture fitness test rejects a direct Workflow import, a second timeout-rule entry point, or another copied bound. | Preserve the anti-corruption layer: all Workflow compilation/run/effect interaction travels through Applications-owned ports and published snapshots. Keep Workflow repositories, timeout helpers, defaults, and maxima outside Applications Application and Domain code. |
| Workflow | Ontology, WorkflowDefinition, Goal, Plan, WorkflowRun, HumanTask, decision, and semantic step projections | Rich invariants and strong Flow authority tests. Workflow now owns immutable `HumanTaskSubmission` evidence and the sole historical `form_submissions` mapper. Its Application and Domain layers import no Forms internals; task coordination and submission evaluation share one consumer-owned `IHumanTaskFormPort`, implemented by one Infrastructure adapter. Migration `173` corrects the physical-table ownership description without rewriting historical bytes, IDs, URNs, or replay state. | Define internal subdomains for Definition/Planning, Run, and Human Interaction. Preserve the single Forms adapter and access Executions, Connectors, and Applications only through equivalent application ports. Flow remains the only durable execution history. |
| Forms | Form draft/release schema and form semantic validation | Domain is isolated and guarded. Forms owns definitions, immutable releases, and the version-pinned semantic evaluator only; the former standalone submission entity/repository/mapper have been removed. | Keep general standalone submissions out of the HumanTask transaction. If later required, introduce a distinct Forms command and aggregate rather than reviving the removed repository or sharing Workflow evidence. |
| Connectors | Reusable outbound profile/revision, egress policy, exact attempt fencing/evidence, and response-object reference | Provider-neutral attempt model and Workflow port are strong. Application services directly depend on Secrets application/domain types. | Publish a SecretVersionAccess port result and keep exact materialization in Secrets. Connector attempts remain the sole HTTP retry/evidence authority for consumers such as Notifications. |
| Notifications | Personal inbox, subscriptions, alert policy, delivery facts, and terminal delivery receipts | Correctly reuses Connector and SMTP mechanisms, but domain types embed Connector outcomes and Identity email/evaluator types. | Consume versioned Connector and Identity published contracts. Alert sources arrive only as closed owner facts. Do not add provider retries, timers, or authorization stores. |
| Plugins | Tenant registry enrollment and exact A3S Use package assignment intent | Small, port-oriented, and correctly excludes installation/reconciliation authority. | Narrow the module facade and preserve A3S Use as the only package lifecycle owner. |
| Files | Upload session/metadata intent and immutable-object references | `K0.1-C1/C2` isolate one aggregate and ACL, one streaming object port, and one metadata/quota repository port. Migration `170` atomically persists lifecycle, quota, shared audit/Outbox/idempotency; authorization-first CQRS is exposed through REST/OpenAPI `1.77.0`, client, CLI, and five Management MCP tools. Presentation, module assembly, and every concrete adapter are crate-private. The external persistence fixture compiles only a non-default conformance surface that returns the existing owner ports. HTTP authorization enters through the root Presentation adapter instead of Identity Presentation, and the Domain persistence-write bundle is the one mapping authority for the canonical `file.user-file.*` audit vocabulary used by PostgreSQL and failure probes. Architecture tests reject a second uploader, provider, scanner store, cleanup queue, presentation bypass, or public outer-layer exposure written as a declaration, re-export, or alias. The retained [PostgreSQL 17 H0 persistence step](https://github.com/A3S-Lab/Cloud/actions/runs/33159659047/job/98810769471) verifies rollback, concurrent quota serialization, exact lifecycle replay, quota release, and atomic side effects through those owner ports. | Live public byte upload/download plus scan/cleanup execution remain before Files availability. Bytes use the shared immutable-object authority and never become Build Artifacts. |

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

- Durable Cells Domain no longer imports Workloads owner models. It admits only
  a consumer-owned immutable provider projection containing the exact revision
  identity, digests, ports, and health contract;
- the sole Workloads-to-Runtime compiler now belongs to Workloads Application,
  and Durable Cells Runtime receipt policy belongs to Durable Cells
  Application. Architecture tests reject either policy returning to
  Infrastructure or an Application-to-Infrastructure dependency;
- remaining Durable Cells domain and application paths still import
  Executions, Data, Artifacts, Fleet, Operations, Workloads, and Edge internal
  owner types or repositories;
- application code still calls several Workloads/Edge handlers directly;
- presentation DTOs reuse Workloads and Edge presentation DTOs;
- the Cell provider profile is correctly frozen in Cloud and bound into
  Runtime only through its opaque digest.

Target split:

| Layer | Owns |
| --- | --- |
| Durable Cells in Cloud | Application identity, immutable revision, Cell-class/state-schema compatibility, retention intent, and exact deployment/route correlation |
| a3s-runtime | Ordinary Service lifecycle, generic capabilities/evidence, typed endpoints, and opaque product-semantics digest binding |
| Box and selected Cell provider | Provider process, activation, per-key serial turns, SQLite lineage, alarm/WebSocket behavior, idle eviction, recovery, and epoch fencing |
| Data / S0 | Namespace lifecycle, credentials, conditional object semantics, backup, restore, retention, and deletion evidence |
| Workloads/Fleet/Edge | Placement, claims, node commands, rollout, healthy target selection, Route intent, and Gateway publication |

Named-state behavior is a Cloud/Box/provider consumer-conformance profile
outside the Runtime wire, not a Runtime capability type or Unit class. Cloud's
A3S ACL compiles only generic process, network, health, resource, mount,
Secret, port, and opaque-digest requirements. The joint gate must prove the
product behavior black-box before availability; Runtime proves only its generic
lifecycle and evidence contracts. Product policy and tenant state do not move
down.

The required Cloud refactor is a set of consumer-owned ports:

- DurableCellBuildArtifactPort;
- DurableCellStoragePort;
- DurableCellExecutionPort;
- DurableCellWorkloadPort;
- DurableCellRoutePublicationPort.

Concrete owner repositories, handlers, and presentation DTOs must disappear
from Durable Cells imports. Pure owner compilers may be consumed through their
Application facade; I/O, mutable owner reads, and side effects require a
consumer-owned port. Do not add a trait around deterministic local policy only
to rename a function call. The pure Durable Cells Runtime profile service is
therefore Application policy, not a sixth I/O port.

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
| Source discovery and acceptance | Sources transient provider query plus the existing immutable `ExternalSourceRevision` authority | Discovery may return mutable repository/ref choices but persists nothing; only the existing acceptance command may pin a full commit and create durable source state |

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

Baseline status updated on 2026-08-28: the source-level ratchets live in
[`architecture_tests.rs`](../crates/control-plane/src/modules/architecture_tests.rs).
They freeze exact cross-context outer-layer sites, duplicate physical ORM
mappings, domain technical-dependency debt, named Runtime/Flow published
contract entry points, Shared Kernel direction, and public Infrastructure /
Presentation facades. Public facade detection now treats a public module,
re-export, or alias of a private outer layer as the same exposure. The current
allowlists contain 60 cross-context outer-layer import sites, 11 duplicate
mapping sites across five tables, and 46 public outer-layer context/layer
surfaces; Domain technical dependency and Shared Kernel back-edge allowlists
remain empty. Files Presentation and every Files adapter, the Data recovery
runtime, and Developer Workflows module assembly are now crate-private. The
baseline passes. This proves that these debt classes cannot silently expand;
it does not certify the allowlisted sites as correct or replace the refactors
in Waves 1-6.

### Wave 1: governance ports and module facades

1. Move shared tenant/resource HTTP guards to root presentation composition.
2. Publish the immutable authorization context from Identity.
3. Introduce Organization, Project/Environment, and Node scope lookup ports.
4. Make Infrastructure and Presentation crate-private; keep public contracts
   and application ports deliberate.

The first facade hardening slice closes the Data recovery runtime, Developer
Workflows module assembly, and all Files outer-layer leaks without changing
behavior. Files module assembly and every adapter are crate-private. Its
retained external PostgreSQL/object-store fixture compiles only with the
non-default `persistence-conformance` feature and receives the same repository
and object-store owner ports used by production. The feature is not a
second persistence mechanism or a product facade.

Artifacts now satisfies item 4 for Presentation. Its node-artifact stream,
descriptor, error, and store contract form one consumer-owned Application
port; Domain receives admitted artifact values and receipts only. The public
root names exact presentation DTOs without exposing that namespace.
Infrastructure remains temporarily public because shared Flow composition
still names its runtime registry directly; that exact site stays frozen. The
Artifacts-to-Assets publication write, cross-context candidate scans, and
external Sources input-preparation internals are now removed. Hosted Assets
input preparation and Fleet runtime relationships stay explicit Wave 2 debt.

### Wave 2: supply-chain handoffs

1. Publish Sources input snapshots.
2. Remove Sources/Artifacts/Assets aggregates from Developer Workflows domain.
3. Replace the Artifacts-to-Assets table write with an idempotent durable
   publication handoff.
4. Replace Fleet log DTO reuse with an owner log-query port.
5. Replace cross-context build-candidate scans with a fact-fed Artifacts
   projection without adding another queue or lifecycle.

Item 1's owner contract is implemented as the immutable
`a3s.cloud.source-build-input.v1` published language. Sources alone validates
`ExternalSourceRevision`, and its Application service performs the sole pure
projection through the Sources root facade; Artifacts Domain consumes only the
minimal typed snapshot and the Sources-published recipe vocabulary. The
recipe, platform, provider, and canonical repository types physically belong
to the published layer rather than being aliases for Domain internals.
Executable architecture tests reject both a future Artifacts Domain import of
a Sources-internal path and any published layer that imports its owner's Domain.
The production resolver now consumes `ISourceBuildInputQueryPort`. The
Sources-owned service loads and validates the aggregate, verifies the exact
tenant/project/environment/revision request, and returns only the published
snapshot. The consumer-owned `IBuildSourceResolver` revalidates that binding
before creating its local `BuildSource`; executable architecture tests reject
any return to a Sources repository or aggregate read at that boundary.

External byte materialization follows the same ownership rule. Artifacts
Application owns `IBuildInputPreparer` and the narrower
`IExternalSourceArchivePort`; neither is a Domain service. The Sources-owned
adapter implements that consumer contract and alone performs public-first
checkout, authoritative GitHub credential fallback, exact receipt validation,
deterministic bounded tar creation, post-package credential-free replay, and
temporary-file cleanup. Artifacts receives only the source-content digest,
archive digest, byte count, and stream, then admits the stream through its own
node-artifact port. This is one checkout mechanism and one artifact store, not
a second source aggregate, storage authority, queue, or build lifecycle.

Item 2's core boundary is implemented. Developer Workflows Domain owns local
review proposal values and exact opaque owner references, and can import a
foreign context only through its Published Language. Pull-request Preview
reconciliation consumes a minimal local semantic observation rather than the
Sources webhook-verifier DTO. Sources now commits the closed
`source.pull-request-change.committed@1` fact per exact active Subscription
through its single Inbox/shared Outbox transaction; private delivery evidence
does not cross. One Developer Workflows anti-corruption projector now consumes
only that Published Language through the shared Outbox Relay; migration `157`
persists the event-time-policy-bound Preview and immutable local receipts in
one CAS transaction. C5a commits one exact Preview lifecycle fact in that same
transaction and routes it through the same Relay/projector into the
consumer-owned `IPreviewEnvironmentPort`. Only one Infrastructure adapter may
import Projects internals; it reuses Projects' ordinary Environment aggregate,
repository, idempotency, and Outbox, so no second Environment or delivery
mechanism exists. C5b routes that same lifecycle through a separate
Sources-owned projection port in the shared Relay; Sources alone commits the
ordinary SourceRevision, migration `159` version receipt, and bounded
specialized fact. C5c extends the existing Artifacts projector through an
Artifacts-owned port; migration `162` supplies the only local Preview head,
candidate admission and atomic retirement target the existing candidate and
BuildRun authorities, and exact retirement evidence permits only one later
same-revision attempt. Thus neither handoff introduces a foreign table read,
second SourceRevision/BuildRun lifecycle, Inbox, Relay, queue, worker,
scheduler, saga, or retry rail. BuildPlan, workload-profile, and Preview Policy
acceptance ask the same consumer-owned, closed-action
`IDeveloperWorkflowAuthorizationPort` before parsing ACL, replaying
idempotency, or reading owner state; Identity grant evaluators and policy
vocabulary no longer enter the commands. C6 confines synchronous Sources owner
models to one additional Infrastructure query adapter beside the existing
SourceRevision adapter. It delegates aggregate validation to Sources, enforces
exact requested identity, and returns only the minimal Preview policy binding;
Application imports no Sources owner model. Management and Relay select
separate instances of the existing Preview Policy repository through one
constructor rule. Application obtains a successful attested build only through
`IWorkloadBuildOutcomePort` and
validates the versioned `a3s.cloud.developer-workflow-build-outcome.v1`
snapshot, including exact BuildPlan ID/digest, against the accepted BuildPlan.
An Assets ACL detector remains an Infrastructure
anti-corruption adapter over the single Assets parser, with an executable path
compatibility test. Application submits the accepted local profile through
consumer-owned `IServiceProfileAdmissionPort` and
`IScheduledTaskProfileAdmissionPort` contracts and receives only an immutable
receipt bound to the target, complete request context, Artifact digest, and
opaque owner-contract digest. Test-only owner adapters prove that Workloads and
Executions still perform final template validation. The concrete owner-side
Workloads/Executions adapters and exact accepted-revision compilation query are
production-composed without creating an owner record. Workload/Execution
lifecycle, scheduling, route, Operation, and cleanup remain named follow-up
work; the separate C5a Projects Environment adapter is
production-composed but does not imply those capabilities.

`P0.2-C6` now exposes accepted WorkloadProfile intent without widening those
owners. One Application query service revalidates exact scope, canonical ACL,
and bounded continuous revision history through the existing repository and
shared authorization interfaces. REST and Management MCP dispatch only those
CQRS handlers and share one typed projection; OpenAPI `1.74.0`, client, and CLI
mirror it. Static architecture fitness tests reject parser, repository,
authorization, owner-model, compiler, lifecycle, or delivery bypass from both
public adapters.

`P0.3-C7` applies the same fitness rule without merging the policy and Preview
aggregates. `PreviewPolicyQueryService` owns current/history/exact policy reads;
`PullRequestPreviewQueryService` owns one exact behavioral Preview read. Both
share the existing authorization port but depend on their own bounded-context
repository interfaces and revalidate restored scope and state. REST and five
Management MCP tools dispatch the existing command and four queries through
shared DTOs; OpenAPI `1.75.0`, client, and CLI mirror that ACL-only contract.
Static gates require exactly one production composition and reject parser,
repository, concrete authorization, persistence, or owner-model access from
both adapters. No new storage, event rail, worker, lifecycle, or cleanup
mechanism was introduced.

BuildPlan detection now enters that same authorization boundary before it can
load an accepted SourceRevision or acquire provider bytes. The query accepts
only exact tenant, Project, Environment, revision, and Principal identities;
Developer Workflows owns the narrow `IBuildPlanSourceLayoutPort`, while its
single Sources-owned adapter resolves the existing source-build-input published
value and maps an owner checkout receipt to the bounded layout. Sources has one
`AuthorizedSourceCheckoutService` for public-first/private-credential fallback,
shared by detection and the existing external-build archive adapter. It and
SourceRevision resolution consume one `SourceRepositoryCredentialService`, the
sole connection-restoration, installation-validation, token-issuance, and
provider-error-redaction authority. The sole
`GitSourceCheckout` source-inventory traversal emits the whole-tree and
per-file digests in one scan; the layout adapter reads only fixed Asset ACL
evidence, uses the distinct credential-free replay operation, and removes its
transient checkout. Missing replay bytes fail closed rather than reacquiring a
provider. No
second path walker, credential resolver, SourceRevision reader, checkout cache,
event rail, lifecycle, or persistence authority was introduced. The existing
archive packaging walk only serializes that validated checkout and does not
establish source identity.

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
after replay. Migration 150 changes no table, constraint, or row; it makes the
retained relational identity guards explicit so they cannot be mistaken for
cross-context lifecycle authority.

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

Item 5 is implemented through two owner facts and one existing delivery
mechanism. Sources exposes its existing `source.revision.accepted@1` payload as
Published Language. Assets atomically emits `asset.hosted-build.requested@1`
with every admitted active Agent/MCP draft and rejects that fact for Skill
releases. `BuildCandidateProjector` consumes only those published values through
the generic Outbox Relay and writes migration 152's immutable
`artifact_build_candidates` projection through `IBuildCandidateProjectionPort`.
Exact replay is a no-op and conflicting replay fails closed. The projection
retains the complete owner-published commit plus recipe/manifest identity so
same-subject semantic drift cannot masquerade as a replay. The bounded
reconciler locks only candidate rows with `FOR UPDATE SKIP LOCKED`, then creates
the deterministic initial `BuildRun`; it contains no query for Sources or
Assets tables. The projection stores no processing flag, claim, lease, retry,
or terminal state. Historical rows are seeded once by the migration, while the
Outbox and BuildRun remain the only delivery-retry and executable lifecycle
authorities. Executable architecture tests freeze both the local-only
reservation query and Published-Language-only projector imports.

### Wave 3: execution and traffic boundaries

1. Centralize Operation request participation and eliminate duplicate schema
   mappings.
2. Replace Workloads foreign repositories with admission/observation ports.
3. Feed Edge through owner snapshots or committed projection facts.
4. Remove duplicate nodes, workloads, mcp_service_profiles, and workflow_runs
   table mappings.

### Wave 4: AI product boundaries

1. Publish Connector attempt outcomes and Identity contact references.

HumanTask submission ownership is complete for this wave: Forms owns
definitions, releases, and semantic evaluation; Workflow owns immutable
`HumanTaskSubmission` evidence and the sole historical `form_submissions`
mapper. Task coordination and submission share one Workflow-owned Forms port
and one Infrastructure adapter. Migration `173` changes only the ownership
comment, preserving all historical IDs, JSON bytes, URNs, and replay behavior.

Agent release admission is complete for this wave: Start, Fork, and Workflow
dispatch share one Agents-owned port and one consumer-side adapter, with a
source-level ratchet that forbids direct Assets/Artifacts access from Agents
Application.

Applications timeout admission is also complete for this wave: both admission
handlers share one Applications-owned port, its single Infrastructure adapter
delegates to Workflow, and Applications persists the admitted value without
copying Workflow's default or maximum. Migration `171` removes the historical
database copy as well.

### Wave 5: Durable Cells and Runtime boundary

1. Preserve a3s-runtime's generic Task/Service wire and opaque
   `semantics_profile_digest`; keep named-state behavior in the joint consumer
   conformance harness.
2. Compile Cloud's Durable Cell A3S ACL only into the existing generic Runtime
   Service specification plus its exact opaque digest.
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
- no context root publicly declares, re-exports, or aliases Infrastructure or
  Presentation;
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
