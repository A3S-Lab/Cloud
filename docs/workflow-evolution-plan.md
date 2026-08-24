# A3S Cloud Workflow and Evolution Plan

## 1. Authority and scope

This plan owns the detailed delivery design for the website-level Workflow,
ontology, heterogeneous Agent hosting, and governed self-evolution outcomes.
It refines the `W0`, `A1`, and `EV0` gates without creating another control
plane.

The [technical architecture](architecture.md) remains authoritative for
component ownership. The [product roadmap](../ROADMAP.md) remains
authoritative for availability and delivery order. The
[development plan](development-plan.md) remains authoritative for shared
platform evidence. This document owns the additional domain contracts,
ordered slices, crash points, and exit evidence for these intelligence
capabilities.

The [AI application platform plan](ai-application-platform-plan.md) owns the
`APP0`, `K0`, and `AUT0` consumption of these contracts, including the 23-node
coverage matrix. It may require an existing `W0` sub-gate but cannot redefine
Workflow or Flow authority.

Website names describe product capabilities, not deployable process names:

| Website capability | Canonical implementation |
| --- | --- |
| Workflow Service | `Workflow` application port and bounded context, with A3S Flow as the only durable orchestration engine |
| Intelligent orchestration | Deterministic goal-to-plan compilation owned by Workflow; execution is an ordinary Flow run |
| Ontology Knowledge Graph | Versioned Workflow-owned ontology state in PostgreSQL, with disposable Search projections |
| Agent Service | `Agents` context plus one provider-neutral Harness contract and the existing execution path |
| Model Service | `Inference` context and A3S Power through `I0`; no model scheduler in Workflow or Evolution |
| MCP Service | Existing `A0`/`MCP0` release, hosting, discovery, and Gateway contracts |
| Self-Evolution | `Evolution` experiment, evaluation, candidate, and promotion policy; compute still uses Flow, Workloads, Fleet, Runtime, and Box |
| AnySentry feedback | Evidence ingestion only; it cannot mutate desired state or promote a candidate directly |
| Distributed file storage | Existing immutable-object infrastructure plus `S0` volume/data provider contracts and `H0` replication evidence |

The website is not an exhaustive Cloud inventory. This plan is additive: it
does not remove or narrow tenancy, projects, sources, builds, ordinary Tasks
and Services, Assets, Artifacts, A3S Use assignments, Workloads, Fleet, Edge,
Secrets, Operations, Search, stateful data, production scale, management
surfaces, audit, observability, update, rollback, or disaster recovery. Those
capabilities keep their existing owners and gates even when the public diagram
does not name them.

## 2. Consolidation invariants

Every design and pull request under `W0`, `A1`, or `EV0` must preserve this
ledger:

| Concern | One reusable mechanism | Forbidden duplicate |
| --- | --- | --- |
| Durable orchestration | A3S Flow plus Operations | Workflow runner, Agent job queue, evaluation scheduler, or retry daemon |
| Portable DAG structure | A3S Flow `WorkflowDag`, constructed programmatically from Cloud ACL | Workflow compatibility parser, topology sorter, or authoring-tool runtime contract inside Cloud |
| Placement and scaling | Workloads | Workflow, Harness, training, model, or MCP scheduler/autoscaler |
| Node delivery | Fleet commands, leases, and the Node Agent journal | Provider-specific queue or direct Cloud-to-process channel |
| Provider lifecycle | A3S Runtime Task/Service over A3S Box | WaaS/AaaS/FaaS runtime implementations or a training executor daemon |
| Integration facts | Transactional Outbox plus A3S Event | Workflow, Agent, evolution, or telemetry event bus |
| Semantic histories | One history per owning aggregate | Flow history, logs, traces, and domain events copied into a second transcript |
| Immutable bytes | Shared content-addressed object client with typed adapters | Context-local filesystem, S3, dataset, checkpoint, or model clients |
| Mutable data | `S0` Data provider contracts and fenced volumes | Workflow filesystem, Agent volume manager, or evolution dataset filesystem |
| Release identity | Existing owning release/revision plus one federated catalog projection | Copied Workflow, model, A3S Use, or Harness registries |
| Authentication and grants | Identity and `C0.3` | Workflow roles, Agent ACLs, or evolution approval stores |
| Audit | Shared append-only audit chain | Product-specific audit databases |
| Telemetry | OpenTelemetry-compatible adapters and AnySentry integration | Telemetry used as desired state, receipt, or promotion authority |

`WaaS`, `AaaS`, and `FaaS` are product profiles, not new A3S Runtime unit
types:

- `WaaS` is a `WorkflowRun` coordinated by A3S Flow. Only executable steps
  create ordinary Runtime Tasks or Services.
- `AaaS` is an `AgentExecution` bound to one immutable Harness provider
  profile and projected through the same Workload/Runtime path.
- `FaaS` is an `Execution` projected to one finite Runtime Task.

## 3. One execution lifecycle

Workflow steps, Agent runs, evaluations, training jobs, deployments, builds,
and repairs use the same control sequence:

```text
authenticated command
  -> owning aggregate and desired generation in PostgreSQL
  -> one Operation and A3S Flow run
  -> Workloads placement and Fleet Claims when compute is required
  -> Fleet command and outbound Node Agent journal
  -> A3S Runtime Task or Service
  -> A3S Box execution
  -> generation-bound receipt and observation
  -> owning aggregate projection and transactional Outbox fact
```

The owning context defines semantic state, but it never implements the lower
execution stages. A missing or ambiguous receipt remains pending or unknown;
telemetry, process exit, or event delivery never implies success.

## 4. `W0`: ontology-driven Workflow Service

### 4.1 Authority boundary

The `Workflow` bounded context owns:

- `Ontology`, immutable `OntologyRevision`, and validation policy;
- `WorkflowDefinition` and immutable `WorkflowRevision`;
- `WorkflowGoal`, deterministic `PlanRevision`, and plan-input digest;
- `WorkflowRun`, its current plan revision, semantic step projection, and
  correlated Operation identity; and
- human task and decision records that are part of Workflow semantics.

It does not own Flow history, task retries, node placement, Agent
conversations, MCP server state, inference routes, Secrets, telemetry, or
provider processes.

Ontology definitions use closed A3S ACL parsed only through `a3s-acl`.
Authoritative objects, relation types, constraints, rules, and revision
lineage live in PostgreSQL through A3S ORM. Search and vector indexes are
rebuildable projections. `W0` does not introduce a graph database or make a
Search index authoritative.

Workflow definitions are also canonical ACL. Cloud maps their typed steps and
edges directly into Flow's `WorkflowDag`; it does not add a compatibility
parser. Flow owns generic graph identity, endpoint, scope, cycle, and
deterministic-order compilation. Workflow owns the single-input/reachable-
output contract, branch handles, capability bindings, ontology/dataflow
semantics, and immutable `PlanRevision`.

### 4.2 Ordered sub-gates

| Sub-gate | Outcome | Dependencies |
| --- | --- | --- |
| `W0.1` | Freeze the Workflow/Ontology domain contract, closed ACL schemas, authority tests, quotas, and one federated capability-reference shape | `F0`, `C0.1` |
| `W0.2` | Persist versioned ontologies, validate object/relation/rule migrations, publish authorized query and diff surfaces, and rebuild Search projections | `W0.1`, PostgreSQL, A3S ORM, Search |
| `W0.3` | Persist Workflow definitions and goals; freeze immutable step descriptors and typed variable scopes; compile one deterministic plan including bounded Iteration/Loop regions, typed error branches/fallback, Answer frames, and reachable-sink Output aggregation; run human/service/finite-task steps through one Operation and A3S Flow | `W0.2`, `C0.3`, existing Executions |
| `W0.4` | Add immutable Agent, MCP, model, Tool, and business-service step bindings with typed inputs/outputs, compensation, approval, and bounded evidence references | `W0.3`, provider-neutral `A1.3`, `MCP0.5`, `I0.2`, `U0.4` where a Use surface is selected |
| `W0.5` | Certify pause/resume, migration, replay, cancellation, compensation, tenant isolation, quotas, multi-day recovery, and operator runbooks | `W0.4`, `H0.3`, applicable `A1`/`MCP0`/`I0` recovery gates |

`W0.1`, the backend implementation of `W0.2`, and the planning/persistence,
immutable descriptor and typed-variable domain contracts, read-only built-in
discovery, internal Workflow-local, reachable-Output, HumanTask, and finite
Execution portions of `W0.3`, including its descriptor-bound typed failure
edge and exact default-output fallback, plus Flow-derived authorized variable
inspection and descriptor-bound Connector failure routing, are now present. The
descriptor registry uses
canonical ACL, exact SemVer identity, typed ports, existing coarse
step/capability types, owner/execution class, semantic/configuration/default-
policy digests, required bindings, typed failure behavior, compiler ranges,
fail-closed admission, and presentation isolation. Its representative fixture
is execution-conformance evidence, not a global catalog or availability claim.
User-authored publication additionally proves that every bound descriptor has a
current Cloud runtime dispatch path. Caller-provided admission metadata cannot
enable unwired Agent, MCP, model, Tool, Memory, or other owning-context ports;
finite Execution, Connector Service, exact Application variable/Answer ports,
Workflow-local steps, and composite regions remain admitted. Exact
Applications-generated presets retain their deferred internal composition
contract without claiming runtime availability. Historic restored snapshots
retain structural compatibility, and this check changes no persisted or public
protocol shape.
Migration `103` snapshots the exact admitted registry under WorkflowRevision,
and Plan v2 pins exact descriptor semantic digests while existing Plan v1
histories remain byte-stable. A graph that opts into the exact finite-Execution
error port emits Plan v3 and immutable WorkflowRun input/runtime/Flow v4; every
Plan step pins its descriptor failure contract while Run inputs v1-v3 retain
their bytes and replay behavior. The mutually exclusive default fallback emits
Plan v4 and immutable WorkflowRun input/runtime/Flow v7; policy v3 binds one
canonical value and completed projection evidence retains the terminal reason.
An exact Connector descriptor error edge emits Plan v5 and immutable
WorkflowRun input/runtime/Flow v9; a closed terminal provider classification
becomes `cloud.workflow.step-failure.v2` data on the ordinary DAG while the
Service source projection remains failed. Plans v1-v4 and Run inputs v1-v8
retain their exact bytes and replay behavior.
An exact Application conversation-variable assignment descriptor error edge
emits Plan v6 and immutable WorkflowRun input/runtime/Flow v14. Applications
`Invalid`, `NotFound`, `Conflict`, and `Forbidden` write results become
redacted `cloud.workflow.step-failure.v3` data on the ordinary DAG while the
Service source projection remains failed. `Unavailable` and `Internal` leave
the write Hook unresolved for the existing idempotent retry path. Plans v1-v5
and Run inputs v1-v13 retain their exact bytes and replay behavior.
An exact Application Answer descriptor error edge emits Plan v7 and immutable
WorkflowRun input/runtime/Flow v15. The same closed deterministic terminal
classifications become redacted `cloud.workflow.step-failure.v4` data on the
ordinary DAG for root and composite-frame Answers while the source Output
projection remains failed. `Unavailable` and `Internal` leave the Answer Hook
unresolved. Plans v1-v6 and Run inputs v1-v14 retain their exact bytes and
replay behavior.
An exact Workflow-local Transform descriptor error edge emits Plan v8 and
immutable WorkflowRun input/runtime/Flow v16. Deterministic local evaluation
failure runs once without retry, becomes fixed redacted
`cloud.workflow.step-failure.v5` data on the ordinary DAG, and leaves the
source Transform projection failed with the exact selected handle. Plans v1-v7
and Run inputs v1-v15 retain their exact bytes and replay behavior; runtime
build `a3s-cloud-workflows@18` explicitly retains `@1` through `@17`.
An exact Workflow-local Output descriptor error edge emits Plan v9 and immutable
WorkflowRun input/runtime/Flow v17. Template or output-schema evaluation failure
runs once without retry, becomes fixed redacted
`cloud.workflow.step-failure.v6` data on the ordinary DAG, and leaves the source
Output projection failed with the exact selected handle. Plans v1-v8 and Run
inputs v1-v16 retain their exact bytes and replay behavior; runtime build
`a3s-cloud-workflows@19` explicitly retains `@1` through `@18`.
An exact Workflow-local Branch descriptor error edge emits Plan v10 and immutable
WorkflowRun input/runtime/Flow v18. Missing or invalid selector evaluation runs
once without retry, becomes fixed redacted
`cloud.workflow.step-failure.v7` data on the ordinary DAG, and leaves the source
Branch projection failed with the exact selected handle. Descriptor error
handles are disjoint from ordinary If / Else handles. Plans v1-v9 and Run inputs
v1-v17 retain their exact bytes and replay behavior; runtime build
`a3s-cloud-workflows@20` explicitly retains `@1` through `@19`.

Current finite Execution, Connector, HumanDecision, and Subworkflow steps also
populate the existing bounded `WorkflowStepProjection.evidenceReferences`
field from verified Flow history. Execution terminal observations retain exact
child Execution and Operation URNs; received Connector observations retain
exact attempt URNs. Received HumanDecision resumes retain exact HumanTask and
WorkflowDecision URNs and, for interactive outcomes, the accepted
FormSubmission URN. Automatic expiry and cancellation retain no synthetic
submission. Each linked Subworkflow frame retains its exact child WorkflowRun
and Operation URNs. Iteration and Loop projections select the latest 16 linked
frames by ordinal before canonical sorting, preserving the existing
32-reference bound while complete frame history remains with Flow. References
are closed, canonical, sorted, and duplicate-free. They provide correlation
only: provider and interaction bodies remain with the owning context, and a
dispatch rejection without owning-context evidence retains no reference. No
historical terminal projection is mutated or backfilled.

The same user-authored publication boundary now rejects structurally valid
descriptors whose owning Cloud runtime port is not implemented. Provider nodes
therefore fail at revision publication instead of entering a run that can only
terminate at the local-executor fallback. This is an availability fence, not
implementation of the remaining provider ports.

The separate catalog projection composes the frozen parity manifest's exact
23-node owner/gate/dependency/evidence/availability inventory with its exact
digest-bound kind/execution-class/semantic-profile ACL. REST `1.31.0`, the
maintained client, CLI, and Management MCP call one project-authorized Workflow
query. Five entries are internal, eighteen remain unavailable, none are public,
and the projection cannot admit descriptors. It has no persistent catalog
management, table, migration, writer, worker, or Flow state.

The typed-variable contract uses canonical ACL, exact SemVer and compiler-schema
identity, typed declaration/read/assignment/export records, root and leaf schema
digests, reachability and dominance, deterministic mutation order, opaque
Secret/object references, and optimistic Applications-port evidence. Its
fixture is also conformance-only; Plan v1 rejects Applications state access
because it cannot prove the owning descriptor. Migration
`075` stores one project-scoped Ontology aggregate head and immutable canonical
ACL revisions through A3S ORM. Create, list, get, revise, revision list/get,
and deterministic diff are exposed through REST `1.15.0`, the maintained
client, CLI, and seven Management MCP tools; Search has one rebuildable current
Ontology projection. Compatible migration policy is derived from the diff. A
breaking change is valid only when the caller names an exact rule in the
target ACL whose kind is `migration`; there is no separate policy document or
migration registry. Migration `076` stores project-scoped WorkflowDefinition
heads, immutable Workflow revisions, every exact referenced closed ACL
payload, immutable Goals, and deterministic Plan revisions through the same
A3S ORM transaction boundary. REST `1.15.0`, the maintained client, CLI, and
ten additional Management MCP tools reuse the same CQRS handlers. Historical
idempotency replay reconstructs the aggregate
as it existed at the referenced revision instead of pairing an old revision
with the current head. Focused tests pass, while clean real-PostgreSQL and
expanded cross-surface conformance still block Workflow planning verification.
Migration `079` additionally stores project-scoped canonical native Form draft
heads and immutable owner-compiled releases through A3S ORM. REST `1.15.0`, the
maintained client, CLI, and seven Management MCP tools share create/list/get/
revise/publish commands and queries, tenant and role boundaries, optimistic
versions, and historical idempotency replay. Focused PostgreSQL 17 plus domain,
REST, OpenAPI, client, CLI, and MCP lifecycle tests pass without copying the
Form compiler or validator. Migration `080` atomically stores the exact
Goal/Plan-bound WorkflowRun, correlated Operation, WorkflowStepProjection
records, idempotency, audit, and Outbox through A3S ORM. One A3S Flow run
executes Workflow-local `input`, `transform`, `branch`, `human_decision`, and
`output` steps; the reconciler verifies immutable plan/input/payload authority,
rejects replay drift, and projects cancellation, deadlines, waiting, terminal
output, and bounded redacted history. Migration `081` stores immutable accepted
FormSubmission records, optimistic HumanTasks, immutable WorkflowDecisions,
hook-event Inbox evidence, and leased resume Outbox/receipt records through
typed A3S ORM queries. Worker-role coordination validates the exact published
interaction-mode FormRelease and Flow hook authority, creates and activates the
task, and resumes the same hook from the immutable decision with retry,
lease-takeover, conflict, and commit-before-ack recovery. A real PostgreSQL plus
A3S Flow test covers concurrent coordinators, tenant scope, atomic
submission/decision storage, replay, and receipt evidence. REST `1.16.0`, the
maintained client, CLI, and seven additional Management MCP tools continue to
expose start, cancel, list, get, wait, output, and history through the same CQRS
handlers. REST `1.24.0`, the client, `human-tasks` CLI commands, and five
Management MCP tools now expose bounded protected task reads plus versioned
claim/release/submission through the same Workflow repository, domain state machine,
transaction-bound idempotency/Outbox/audit path, and shared Identity Resource
Grant evaluator. Lists omit interaction payloads and only the current claimant
receives the exact request-bound A3S Form interaction. Migration `096` reuses
the same coordinator, immutable WorkflowDecision, and resume Outbox for
automatic expiry; it recomputes the exact Run/Plan deadline authority and
settles only from matching `HookReceived` or parent `RunTimedOut` evidence.
Migration `097` records the exact cancelling Principal and uses that same
decision/Outbox path for cancellation-over-expiry precedence and exact parent
`RunCancellationRequested`/`RunCancelled` evidence. Migrations `098` through
`100` add Executions-owned immutable, project-scoped, ACL-native ExecutionTemplate
revisions, the exact Run/Plan/step/attempt/template/digest columns and
composite foreign keys on the existing Execution aggregate, and `execution`
admission in the existing WorkflowStepProjection kind constraint. An
`execution` plan step accepts only owner `executions`, type
`execution_template`, exact UUID revision, exact digest, and capability
`execution.run`; its plan also requires
one exact environment. The Workflow coordinator uses one typed Executions
application port to create or adopt the child, links the existing Execution
Operation as the A3S Flow child, resumes the hook only from a digest-bound
terminal result, and waits for cleanup-first child cancellation before parent
cancellation or timeout. REST/OpenAPI `1.24.0`, the maintained client,
`execution-templates` CLI commands, and three Management MCP tools reuse the
same CQRS and persistence path. Focused tests and a local real PostgreSQL
seven-boundary run pass. The clean Linux H0 gate now passes finite persistence
and the same seven process-death boundaries, while the clean C0.2 Management
MCP/A3S Box/PostgreSQL gate passes the exact `77/47` catalog and an `8/8`
ExecutionTemplate persistence, replay, rollback, immutability, and tenant
non-disclosure result. This verifies the finite Execution sub-gate, not all of
W0.3. Revision-owned descriptor bindings, the recoverable registry snapshot,
the variable contract, and exact Plan v2 pinning now persist through migration
`103`. Migration `107` adds optional immutable default material. Migration
`108` adds optional immutable `cloud.workflow.composite-regions.v1` material
without a new table. New publication requires it to exactly cover admitted
Iteration/Loop descriptors, their bounded scheduling/failure/termination
policy, and one exact non-nil child WorkflowRevision binding. The semantic-set,
Plan v2 `compositeRegionsDigest`, and immutable Run v3 input pin its exact ACL
and digest for composite execution while historical and non-composite v2 runs
remain readable.

WorkflowRun input/runtime/Flow v2 freezes exact non-composite variable/default
material. Version 3 adds exact composite material and projects invocation,
node-output, defaults, deterministic
run-assignment, typed-read, and opaque-reference values from immutable input
plus existing Flow history; migrations `105`, `107`, and `108` only widen
that immutable input. Explicit reads are authoritative for their step and are
consumed only through `current`; steps without reads retain legacy dependency
input. REST/OpenAPI `1.35.0`, the maintained client, CLI, and Management MCP
accept optional `variableDefaultsAcl` and `compositeRegionsAcl`.
REST/OpenAPI `1.33.0` expose
`cloud.workflow-run.variable-inspection.v1` through one authorized CQRS query
and the same materializer used by execution. The bounded, declaration-ordered
response identifies the exact contract and observed Flow sequence,
distinguishes materialized from unavailable values, and redacts Secret
references. Pre-Flow immutable inputs may appear at sequence zero; Plan v1
conflicts.

Composite policy, exact child identity, deterministic frame/export and ordinal
region reducers, and Flow-backed child lifecycle are implemented. Runtime v3
uses one exact hook per ordinal and deterministic ordinary child WorkflowRun,
links its Flow identity, resumes digest-bound results, and propagates parent
cancellation/timeout before termination. Iteration is initially sequential
under its declared concurrency ceiling. Descriptor-bound Application Answer,
conversation-variable snapshot/CAS, and repeated-frame Answer semantics are
implemented through Run v11-v15. Workflow-local Transform, Output, and Branch
failure routes are implemented through Run v16-v18. Current finite Execution
and Connector projections retain bounded owning-context evidence URNs. The
remaining business-service and Agent/MCP/model/Tool capability dispatch,
compensation, full provider conformance, and public Workflow availability
remain open. Runtime v4 converts authority-bound Execution dispatch rejection,
failure, or cancellation into `cloud.workflow.step-failure.v1`, selects the
exact descriptor handle, and lets the ordinary DAG and Flow history activate
the error path. The Execution projection remains failed even if its reachable
failure branch completes the parent. These foundations add no table, cache,
event log, worker, scheduler, queue, retry engine, or second Flow mechanism.
Runtime v7 interprets the same terminal observation through the exact Plan v4
default-output contract and retains `cloud.workflow.step-default-output.v1`
evidence beside the completed value; migration `122` only extends the existing
step projection with nullable fallback evidence. Runtime v9 preserves v8's
strict Connector JSON success projection and selects an exact Plan-v5 error
edge only for a closed terminal provider classification. Migration `123`
admits the already wired Service projection kind and its failed selected-handle
shape; immutable WorkflowRun plan validation still proves the exact
ConnectorRevision binding and declared handle. No new table, log, queue,
scheduler, retry engine, or Flow mechanism is introduced.
Runtime v14 admits only an Application-composed Plan v6 with the exact
`application.conversation-variable-assign` descriptor and error edge. It maps
only deterministic terminal owner rejections to failure v3 and the declared
handle; transient or internal owner errors stay unresolved. The existing
migration `123` already admits the failed Service projection shape, so this
slice adds no migration or public OpenAPI schema.
Runtime v15 admits only an Application-composed Plan v7 with the exact
`application.answer` descriptor and error edge. It maps only deterministic
terminal owner rejections to failure v4 and the declared root or frame-local
handle; transient or internal owner errors stay unresolved. Migration `143`
widens only the existing projection constraint for failed Output selected-
handle evidence and still rejects completed Output aliases. This slice adds no
public OpenAPI schema or second message, retry, or orchestration authority.
Runtime v16 admits Plan v8 only when an exact Workflow-local Transform selects
its descriptor-bound error edge. The failed local step is scheduled once with
no retry, and Flow reconstructs fixed redacted failure v5 data instead of
copying evaluator diagnostics. Migration `145` widens only the existing
projection constraint for failed Transform selected-handle evidence. This
slice adds no table, column, public OpenAPI schema, or second retry or
orchestration authority.
Runtime v17 admits Plan v9 only when the exact Workflow-local Output selects its
descriptor-bound error edge. Template or output-schema evaluation is scheduled
once with no retry, and Flow reconstructs fixed redacted failure v6 data instead
of copying evaluator diagnostics. The existing migration `143` already admits
failed Output selected-handle evidence. This slice adds no table, column, public
OpenAPI schema, or second retry or orchestration authority.

Reachable-sink Output aggregation is now implemented in the Workflow
compiler/runtime adapter without changing Flow. A graph admits one or more
terminal Output sinks and every step must reach at least one. Runtime completion
waits until every declared sink is active or inactive, excludes inactive branch
sinks, preserves the historical value shape for one declared sink, and emits a
stable step-ID-keyed object for multiple declared sinks under the existing
output bound. Focused Workflow tests verify this behavior together with legacy
replay and HumanTask compatibility. The descriptor registry and typed-variable
domain contracts are implemented. Migration `103` atomically binds all three
mandatory contracts to WorkflowRevision compiler schema 2, migration `107`
permits the optional exact default-material child, and migration `108` permits
the optional exact composite-region child. `cloud.workflow.plan.v2` pins every
exact descriptor plus the semantic, variable, and optional composite-region
digests. Legacy Plan v1 remains byte-stable and executable. Plan v2 executes
the first typed-variable subset plus digest-bound defaults. The pure composite
frame component materializes local inputs, assignments, Run updates, and
explicit exports; the region reducer restores observations to contiguous
ordinal order, applies Iteration failure policy, requires Loop termination,
and folds updates and exports in ordinal order. Authorized inspection reads
parent and reduced composite materialization from immutable input and Flow
history. Descriptor-bound Application reads/writes are implemented through Run
v12 and their deterministic write-failure branches through Plan v6/Run v14;
descriptor-bound root/frame Answer failures are implemented through Plan
v7/Run v15. Unsupported or transient owner observations still fail closed. Typed
finite-Execution failure branches are implemented through Plan v3/Run v4 and
exact default-output fallback through Plan v4/Run v7. Descriptor-bound
Connector failure branches are implemented through Plan v5/Run v9. Application
composite Answer frames use Run v13 root/child authority and stable zero-based
ordinals; their deterministic Answer failures retain that root authority in
Run v15. Workflow-local Transform failure branches are implemented through
Plan v8/Run v16, Workflow-local Output failure branches through Plan v9/Run
v17, and Workflow-local Branch failure branches through Plan v10/Run v18.
Current finite Execution, Connector, HumanDecision, and Subworkflow projections
retain closed, bounded child/Operation/attempt/task/decision/submission evidence
URNs from verified Flow history.
Business-service and remaining Agent/MCP/model/Tool failure semantics,
compensation, full provider conformance, and public availability remain
unimplemented parts of `W0.3`.

### 4.3 Compiler rules

The planner produces a closed `PlanRevision` containing exact ontology,
Workflow, capability, policy, and input digests. The same inputs and compiler
revision must produce the same plan digest. Dynamic choice is represented as
an explicit policy step whose inputs, candidate set, decision, and evidence
are recorded; it is not hidden non-determinism inside Flow replay.

Plan steps call typed application ports:

- Agent work calls the Agents command bus;
- MCP work binds one admitted `MCP0` service profile;
- model work calls one authorized Inference route;
- Tool/OKF work binds an exact A3S Use package capability;
- human work creates a Workflow-owned decision record guarded by Identity;
- ordinary compute creates an existing finite `Execution`; and
- external business services use a versioned typed connector with Secrets
  referenced by identity only.

No connector may write another context's tables or start a Runtime unit
directly.

### 4.4 Standalone A3S Workflow consolidation register

The former standalone `A3S-Lab/Workflow` repository is a migration source,
not a second product authority. Its useful outcomes are retained by the Cloud
contract below. Its standalone server, database bootstrap, Flow queue,
development Runtime provider, node-execution store, Memory API, CLI binary,
deployment stack, and Studio are not embedded because Cloud already owns those
mechanisms or surfaces.

| Standalone outcome | Cloud-owned form | Gate | Consolidation rule |
| --- | --- | --- | --- |
| Optimistically versioned graph definitions | `WorkflowDefinition` plus immutable `WorkflowRevision` in closed ACL | `W0.1`-`W0.3` | Preserve immutable publication and conflict detection; do not retain a mutable legacy representation as product configuration |
| DAG validation, one input, one output, reachability, deterministic ordering, and named branch handles | Workflow contract validator and deterministic compiler | `W0.1`, `W0.3` | Preserve the graph invariants and make compiler order independent of authoring/layout order |
| Start, template, router, and output nodes | `input`, `transform`, `branch`, and `output` Workflow-local semantic steps | `W0.1`, `W0.3` | These steps are part of the immutable plan; they do not create Runtime Tasks merely to copy, transform, branch, or return data |
| LLM node | Typed `model` step bound to one exact Inference model/route revision | `W0.4`, `I0.2` | No direct OpenAI-compatible client, model key, or model scheduler enters Workflow |
| Bounded Agent node | Typed `agent` step bound to one exact Agent release and provider profile | `W0.4`, `A1.3`-`A1.6` | Retire the node-local model/tool loop; Agents owns conversations, semantic events, approvals, checkpoints, and Harness lifecycle |
| Tool node | Typed `tool` step bound to one exact A3S Use package capability | `W0.4`, `U0.4` | No endpoint-backed Tool registry or direct Tool process launch is copied into Workflow |
| Memory node | Typed `memory` step bound to an admitted A3S Use/Agent memory capability | `W0.4`, selected `U0`/`A1` gate | Preserve store/search/retrieve/delete outcomes without a Workflow-owned Memory database or HTTP API |
| HTTP node | Typed `service` step bound to one immutable connector revision | `W0.4` | Connector policy owns method, schema, destination, egress, and Secret references; arbitrary URLs and header environment injection are not durable Workflow state |
| Approval execute/resume | `WorkflowDecision` guarded by Identity/Resource Grants and coordinated by the same Operation/Flow run | `W0.3`-`W0.5`, `C0.3` | Preserve explicit allow/deny/expiry/cancel and replay; do not launch a Runtime Task only to create or consume a hook |
| Per-node provider, pool, resources, isolation, network, timeout, and Secret references | Exact capability and policy digests compiled through the owning context, Workloads, Fleet, Runtime, Box, and Secrets | `W0.4`, applicable provider gates | Preserve placement and isolation intent; provider names, pool selectors, plaintext values, and another provider registry do not enter Workflow |
| Invocation/result schemas, generation fencing, artifact digests, and per-attempt evidence | Exact child identity, request/receipt digest, Operation correlation, and bounded evidence reference on `WorkflowStepProjection`; finite Execution, Connector, HumanDecision, and Subworkflow correlations are implemented | `W0.3`-`W0.5` | Preserve evidence and stale-attempt rejection; do not copy Runtime observations or create a Workflow node-execution evidence store |
| Run lifecycle, event history, per-step tracing, statistics, and diagnostics | Authorized WorkflowRun and WorkflowStepProjection reads correlated with the one Operation/Flow history and owning-context evidence | `W0.3`-`W0.5` | Preserve list/get/start/wait/cancel/history/evidence/diagnostic outcomes without a second event log, metrics authority, or mutable run-history store |
| PostgreSQL durability, Flow recovery, approval hooks, and worker scaling | Cloud PostgreSQL through A3S ORM plus the existing Operations/A3S Flow workers | `W0.2`-`W0.5` | No Workflow database bootstrap, queue table, lease worker, retry daemon, or local audit file is introduced |
| Machine-readable CLI and coding-agent Skill | Existing Cloud client, CLI, and Management MCP expose the same list/get/author/apply/start/wait/cancel/history/evidence/decide outcomes | `W0.2`-`W0.5` | One Cloud authentication, response envelope, idempotency model, and management catalog; no `a3s-workflow` control-plane URL or token namespace |
| Graph authoring, node catalog, diagnostics, patch review, and run projection | Versioned REST/OpenAPI contracts plus the maintained client, CLI, and Management MCP | `W0.2`-`W0.5` | Preserve controlled editing and inspection without copying a standalone React application or making presentation state execution authority |

The ten standalone node names have one explicit migration map:

```text
start -> input                 template -> transform
llm -> model                  agent -> agent
tool -> tool                  router -> branch
memory -> memory              http -> service
approval -> human_decision    output -> output
```

This map preserves user intent, not the old standalone wire contract. A3S ACL
remains the only admitted product configuration. A future authoring surface
may use an internal view model, but only canonical closed ACL and its semantic
digest can become a Workflow revision.

### 4.5 Step descriptor and Designer boundary

One versioned descriptor defines exact executable semantics. The separate
read-only catalog now drives REST, client, CLI, and Management MCP discovery,
and the deferred Designer must consume that same projection without becoming
execution authority:

```text
WorkflowStepDescriptor
├── identity           stable descriptor ID and immutable descriptor revision
├── dispatch           owning context, coarse semantic kind, semantic profile, execution class
├── ports              typed static or declarative dynamic inputs and outputs
├── configuration      closed ACL schema digest, defaults, and semantic digest rules
├── failure             typed error output, retry classification, fallback, and branch policy
├── binding            allowed CapabilityReference kinds and compatibility rules
├── requirements       capability/policy constraints, never a provider or pool registry
├── compatibility      admitted compiler/protocol range and unavailable reason
└── presentation       label/icon/summary digest, separate from execution semantics
```

Authoring position, grouping, comments, and viewport metadata belong to a
separate presentation digest. Changing only presentation data does not change
the semantic Workflow revision or PlanRevision digest. Structured AI edits
produce a revision-bound patch with diagnostics and an explicit review result;
they cannot mutate a published graph or running plan in place.

Every step configuration, input schema, output schema, capability reference,
policy, ontology, Workflow, and compiler revision is digest-bound before plan
compilation. Workflow-local `input`, `transform`, `branch`, `output`, and
decision coordination remain deterministic semantic work. Other steps call
the named owning application port; only that owner may decide whether an
ordinary Runtime Task or Service is required.

The implemented variable contract validates immutable invocation inputs and
node outputs, region-confined composite values, deterministically ordered run
assignments, and Applications-owned values. Required reads must be backed by a
default or a dominating source/write; optional node outputs may cross a branch
only in forward graph order. Composite locals leave only through an explicit
typed export. Secrets and immutable objects remain opaque references, and an
Applications mutation must carry exact optimistic-revision and idempotency
variables. These rules are compilation semantics, not a second runtime state
store.

The implemented `W0.3` publication path persists each admitted closed
configuration and schema payload atomically with its WorkflowRevision, then
verifies every stored digest before publication, compilation, and minimal run
replay. A digest without retrievable canonical content is not a publishable or
executable Workflow input, and mutable external content cannot fill the gap
during replay.

The complete 23-node application-platform mapping is additive to the ten-node
standalone migration register and is authoritative in the
[AI application platform plan](ai-application-platform-plan.md). Built-ins and
A3S Use capabilities register immutable descriptors over the same coarse kinds;
neither adds product-specific commands, graph semantics, or a node catalog to
A3S Flow.

### 4.6 Flow preservation contract

A3S Flow retains durable history, replay, scheduled steps, retries, waits,
hooks, cancellation, timeout, progress, batch scheduling, and child-operation
linkage. Workflow owns the graph, descriptors, values, composite regions,
termination policy, and owning-context port calls. Cloud does not reimplement
any Flow ability while adding application-platform nodes.

An upstream Flow change is permitted only after a conformance test proves a
missing domain-neutral primitive. It must be additive, preserve existing public
commands and serialized histories, keep legacy Cloud runs replayable, update the
exact compatibility lock, and pass Build, Deployment, Executions, and Workflow
recovery suites. Tenant, application, model, Knowledge, Tool, graph, ACL, and UI
semantics remain prohibited in Flow.

## 5. `A1`: heterogeneous Agent hosting through one contract

### 5.1 Provider model

`A1` owns one `AgentExecutionProvider` port and one versioned
`cloud.agent-provider` command/event/receipt contract. Each `AgentExecution`
binds an immutable provider kind, provider revision, capability digest,
Workload/Runtime identity, and protocol version before dispatch. `A1.4` then
freezes one closed `HarnessInvocationProfile` containing the exact instructions
digest, environment/security policy, Agent, Skill, MCP, model, workspace,
Secret references, Tools, and capability expectations. Mutable provider JSON
or process environment never becomes recovery authority.

A3S Code is the first-party native provider and preserves its existing Core
run/session implementation. The current Code-specific `A1.2` integration is
retained as the first adapter; it is not the only admissible Harness. Other
languages and frameworks enter through the same provider contract and
conformance suite.

Providers may own private in-process state and source events. They may not own
Cloud conversation identity, scheduling, grants, approval authority,
placement, deployment, the canonical semantic event sequence, or another
Cloud-visible run lifecycle. An adapter normalizes only bounded semantic
events and receipt/checkpoint capabilities required by the contract; Cloud
does not copy a provider's complete private event log.

### 5.2 Revised ordered sub-gates

| Sub-gate | Outcome |
| --- | --- |
| `A1.0` | Verified shared sequence, polling, immutable-object, and outbound-batch primitives |
| `A1.1` | Durable conversations, logical executions, and one semantic event sequence |
| `A1.2` | Native A3S Code provider using the existing Workload, Fleet, Runtime, Box, command, receipt, event-page, cancellation, and recovery path |
| `A1.3` | Provider-neutral contract, immutable provider profile, Code adapter migration, capability negotiation, conformance suite, and one non-Code reference Harness |
| `A1.4` | One closed immutable Harness invocation profile with exact Agent, instructions, environment/security policy, Skill, MCP, model, workspace, Secret-reference, Tool, and provider bindings plus auditable Tool request/result events |
| `A1.5` | Grant-checked approvals, logical pause/resume, denial, expiry, and cancellation across conforming providers |
| `A1.6` | Immutable checkpoints, fork lineage, trajectory export, telemetry correlation, provider capability fallback rules, and full crash/recovery certification |

Provider conformance must prove exact replay, duplicate delivery, malformed
events, sequence gaps, cancellation, process death, node loss, unsupported
checkpoint capability, Secret redaction, cleanup, and mixed-version behavior.
An unsupported provider capability fails closed; Cloud does not emulate it
with another lifecycle.

## 6. `EV0`: governed self-evolution

### 6.1 Authority boundary

The `Evolution` bounded context owns:

- `EvidenceDataset`, an immutable manifest of authorized evidence references;
- `EvaluationSuite` and immutable evaluator/reward-policy revisions;
- `EvolutionExperiment` and its correlated Operation;
- `EvaluationRun` and normalized score/evidence records;
- `CandidateRevision`, referencing immutable model, Agent, Harness-policy, or
  Workflow artifacts; and
- `PromotionDecision`, including required human/policy approvals and rollback
  target.

Evolution does not own raw logs, traces, Agent transcripts, model deployment,
Agent release state, Workflow desired state, training placement, GPU claims,
Gateway routes, or production rollout. It submits commands to the owning
context after a promotion decision; it never writes those tables directly.

### 6.2 Evidence flow

```text
AnySentry / audit / Agent trajectories / Workflow outcomes
  -> explicit tenant-authorized export
  -> redaction, retention, consent, and provenance checks
  -> immutable EvidenceDataset manifest
  -> EvaluationSuite and candidate job through Flow/Workloads/Runtime/Box
  -> immutable results and PromotionDecision
  -> owning context creates a new revision
  -> existing rollout and rollback path
```

Observability is input evidence only. A metric, trace, anomaly, reward, or
model output cannot promote a candidate. Promotion requires an exact dataset,
suite, candidate, policy, approval, and evidence digest, followed by the
normal owning-context command and deployment acknowledgement.

### 6.3 Ordered sub-gates

| Sub-gate | Outcome | Dependencies |
| --- | --- | --- |
| `EV0.1` | Evidence export contract, consent, tenant scope, redaction, retention, provenance, immutable dataset manifests, and deletion/tombstone semantics | `C0.3`, `A1.6` trajectories, Workflow evidence where present, shared object infrastructure |
| `EV0.2` | Versioned evaluation suites, reward policies, deterministic offline replay, baseline comparison, integrity checks, and reproducible score evidence | `EV0.1`, existing Executions/Flow |
| `EV0.3` | Candidate generation and Agentic RL jobs as ordinary accelerator-aware Workloads/Runtime Tasks, with quotas, Claims, interruption recovery, and no production mutation | `EV0.2`, `I0.1`, `H0.3`, `S0` dataset storage where required |
| `EV0.4` | Immutable candidate registration, risk policy, human approval, canary request to the owning context, rollout observation, automatic halt, and exact rollback | `EV0.3`, `A1.4`, `W0.4`, `I0.3`, `C0.3` |
| `EV0.5` | Multi-tenant isolation, adversarial reward/data tests, drift monitoring, disaster recovery, cost/compute limits, mixed versions, and production runbooks | `EV0.4`, `H0.5`, `I0.5` |

`EV0` allows governed candidate generation and Agentic RL. It does not permit
unreviewed online learning, self-modifying production binaries, direct
telemetry-to-deployment loops, or an Evolution-owned model/Agent registry.

## 7. Unified catalog and storage projections

The website's Asset Hosting capability is a federated, read-only catalog over
existing immutable authorities:

```text
CapabilityReference =
    AssetRelease(Agent | MCP | Skill)
  | WorkflowRevision
  | ModelRevision
  | UsePackageRecord(Tool | OKF | other Use surfaces)
```

Search may project those references into one catalog, and a Workflow or Agent
release may pin them in one closed manifest. The projection never republishes
or changes the source identity. A3S Use remains the only package lifecycle
authority for Tool and OKF surfaces.

The website's Distributed File Storage capability is implemented by two
semantically distinct contracts behind one storage plane:

- immutable, content-addressed objects for source bundles, artifacts, model
  bytes, checkpoints, datasets, and evidence; and
- fenced mutable volumes for databases and workloads under `S0`.

They share provider configuration, identity, encryption, quota, health, and
operator surfaces where semantics permit. They do not share mutable-write
APIs, and neither becomes PostgreSQL business truth. Distributed production
claims require `S0` provider conformance plus `H0` replication, failure, and
restore evidence.

## 8. Required failure evidence

| Crash or ambiguity point | Required convergence |
| --- | --- |
| Ontology revision transaction is interrupted before its aggregate-head update commits | The deferred current-revision foreign key and one A3S ORM transaction roll back the head, immutable revision, idempotency record, audit, and Outbox fact together; no partial Ontology becomes current |
| Plan compiled before WorkflowRun/Operation commit | Replay selects the same plan digest and creates one run |
| Child Agent/MCP/model command accepted before parent step receipt | Reconciliation adopts the exact child identity; it never starts a second child |
| Harness provider emits a batch before receipt acknowledgement | The Node Agent replays one batch and Agents advances one contiguous sequence |
| Evidence objects written before dataset manifest commit | Verify and adopt only referenced digests or clean unreferenced objects |
| Evaluation completes before result projection | Replay imports the exact result digest once |
| Candidate registered before promotion decision commit | Candidate remains inactive and cannot receive traffic |
| Owning-context rollout starts before Evolution observes it | Evolution adopts the exact Operation and cannot submit a second promotion |
| Canary fails during control-plane loss | Existing rollout authority halts or rolls back from durable policy without Evolution becoming a scheduler |
| AnySentry delivery is duplicated, reordered, or missing | Dataset provenance records exact gaps; missing telemetry never becomes a positive result |

## 9. Definition of done

`W0`, heterogeneous `A1`, or `EV0` is complete only when:

- every aggregate, command, query, migration, A3S ORM repository, adapter,
  REST/OpenAPI contract, maintained client, CLI, and Management MCP surface
  lands under its owning backend slice; product UI is outside Cloud scope and
  never blocks the gate;
- closed A3S ACL is the only product configuration and every exact revision is
  digest-bound before use;
- architecture tests reject new schedulers, Flow engines, queues, node
  channels, event buses, audit stores, low-level object clients, model
  registries, and direct cross-context table writes;
- tenant denial, revocation, consent withdrawal, redaction, poisoned evidence,
  reward tampering, stale policy, sequence gaps, provider incompatibility, and
  cross-tenant references fail closed;
- all named crash points pass with real PostgreSQL, the selected object and
  volume providers, exact Runtime/Box revisions, and applicable Gateway,
  Harness, MCP, Power, and AnySentry integrations;
- unsupported provider capabilities and incomplete evidence remain explicitly
  unavailable; and
- the website, README, architecture, roadmap, domain model, API documentation,
  runbooks, and generated gate data describe the same verified behavior.
