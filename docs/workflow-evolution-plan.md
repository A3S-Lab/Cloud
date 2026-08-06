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

### 4.2 Ordered sub-gates

| Sub-gate | Outcome | Dependencies |
| --- | --- | --- |
| `W0.1` | Freeze the Workflow/Ontology domain contract, closed ACL schemas, authority tests, quotas, and one federated capability-reference shape | `F0`, `C0.1` |
| `W0.2` | Persist versioned ontologies, validate object/relation/rule migrations, publish authorized query and diff surfaces, and rebuild Search projections | `W0.1`, PostgreSQL, A3S ORM, Search |
| `W0.3` | Persist Workflow definitions and goals, compile one deterministic immutable plan, and run human/service/finite-task steps through one Operation and A3S Flow | `W0.2`, `C0.3`, existing Executions |
| `W0.4` | Add immutable Agent, MCP, model, Tool, and business-service step bindings with typed inputs/outputs, compensation, approval, and bounded evidence references | `W0.3`, provider-neutral `A1.3`, `MCP0.5`, `I0.2`, `U0.4` where a Use surface is selected |
| `W0.5` | Certify pause/resume, migration, replay, cancellation, compensation, tenant isolation, quotas, multi-day recovery, and operator runbooks | `W0.4`, `H0.3`, applicable `A1`/`MCP0`/`I0` recovery gates |

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
| Ontology revision bytes written before revision commit | Adopt the exact digest or remove the orphan; no partial ontology becomes current |
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

- during the active backend-first phase, every aggregate, command, query,
  migration, A3S ORM repository, adapter, REST/OpenAPI contract, maintained
  client, CLI, and Management MCP surface lands under its owning backend slice;
  new Web projections are deferred, and a broader gate that promises Web stays
  in progress until that retained projection is later delivered;
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
