# A3S Cloud Project Roadmap

## A3S Cloud

**Mission:** provide a self-hosted, multi-tenant, Agent-first developer platform
for AaaS, WaaS, FaaS, Durable Cell, inference, static Web delivery, and their
complete supply, CI/CD, traffic, security, and operational lifecycle.

A3S Cloud owns product intent and durable desired state. It composes A3S
libraries through ports, projects admitted execution to A3S Runtime over A3S
Box, and publishes every external surface through A3S Gateway. It provides no
Cloud management Dashboard.

## 1. Bounded-context portfolio

| Context | Sole authority | Important collaborators |
| --- | --- | --- |
| Identity and Access | Principals, credentials, federation links, memberships, tenant roles, system-admin roles, resource grants, support grants, break-glass evidence | Gateway enforcement, Sentry runtime policy |
| Organizations and Projects | Installation, Organization, Project, Environment, ownership, lifecycle, attribution, quotas | Every tenant-scoped context |
| Sources | Git connections, repositories, subscriptions, webhooks, exact source revisions | Git provider, Developer Workflows, Delivery Pipelines |
| Developer Workflows | BuildPlan, WorkloadProfile, pull-request Preview policy and projection | Sources, Artifacts, Workloads, Edge |
| Artifacts | BuildCandidate, BuildRun, build evidence, immutable artifact manifests, provenance, retention | Runtime Task, Box build, OCI Registry, object storage |
| Delivery Pipelines | Pipeline definition/revision, trigger, PipelineRun, stage graph, approvals, promotion policy, and linked owner receipts | A3S Flow, Lane, all release/deployment owners |
| Assets and Registries | Agent, Function, MCP, Skill, Connector, Application, and related release metadata; tenant registry projections | Use Registry, OCI Registry, Git, model supply |
| Model Supply | Logical Model, ModelRevision, external-hub resolution, immutable WeightManifest, license/trust, cache intent | Object storage, ModelScope adapter, Power |
| Workloads | Workload, immutable WorkloadRevision, Deployment, replica intent, rollout, writer fence | Runtime, Fleet, Edge |
| Fleet | Nodes, CPU/GPU/topology inventory, placement claims, capacity reservations, drain, maintenance | Node Agent, Box, Power, Lane |
| Agents | Agent execution, semantic events, approvals, checkpoint/fork lineage, session placement binding | Code, Runtime, Functions, Cells, Inference |
| Workflows | Tenant Workflow assets, revisions, goals/plans, WorkflowRun, HumanTask, product node bindings | Flow, Agents, Functions, Cells, Inference |
| Functions | Function assets/releases, hosted Task/Service or external-connector mode, Invocation, concurrency and result policy | Runtime, external FaaS adapters, Gateway |
| Durable Cells | Cell application/revision/deployment, namespace, compatibility, retention, collaboration access | Runtime, S3 provider, Gateway |
| Inference | Endpoint, provider binding, model release, serving group intent, routing/usage policy | Power, Fleet, Gateway, external model providers |
| Files and Knowledge | User files, immutable objects, Knowledge bases/documents/chunks, ingestion and retrieval semantics | Object storage, Parser, OCR, Search, Flow |
| Applications and Web | Application composition, immutable static Web release, API/Agent/Workflow bindings | Object storage, Gateway, tenant UI |
| Edge | Domain, certificate, Route intent, complete Gateway snapshot publication | Gateway only |
| Operations | Idempotency, long-running Operation, audit, Outbox, usage, notifications, evidence retention, system health | PostgreSQL, Event, Redis, observability pipeline |

Each row owns its aggregate state. Delivery Pipelines coordinate owner commands
but never copy their lifecycle tables; Workloads remain the only deployment
authority; Flow remains the only workflow-history engine; Runtime remains the
only Unit lifecycle contract.

## 2. Ordered delivery plan

| Order | Planned outcome | Exit evidence |
| --- | --- | --- |
| `CLOUD-R0` | Remove architecture debt and enforce DDD/hexagonal boundaries, one repository mapping per table, one composition root, and explicit AOP pipelines | Architecture fitness gates reach zero allowlisted private cross-context imports and presentation-to-infrastructure shortcuts |
| `CLOUD-R1` | Complete installation/Organization/Project/Environment tenancy, tenant IAM, separate system-admin RBAC, quotas, support/break-glass, and lifecycle isolation | Cross-tenant denial, last-owner, admin-without-tenant-access, quota contention, deletion, export, and restore tests pass |
| `CLOUD-R2` | Harden every API for multi-replica execution with durable idempotency, request digests, CAS, transactions, Operations, Outbox, rate shaping, cache epochs, distributed locks, and saga recovery | Duplicate, concurrent, stale, lost-response, lock-expiry, relay-replay, and cross-provider failure matrices pass |
| `CLOUD-R3` | Complete Git, OCI, Use, model/weight, object, and artifact supply with immutable revisions, signatures, provenance, licenses, scanning, and retention | Source-to-release provenance verifies; compromised, revoked, mutable, cross-tenant, and missing-byte paths fail closed |
| `CLOUD-R4` | Deliver one Flow-backed Runtime CI/CD module for Agent, Workflow, Function, Durable Cell, Inference, static Web, and Cloud system services | Exact source builds once; test/promotion/deployment never rebuilds; every stage is replay-safe and binds owner receipts |
| `CLOUD-R5` | Deliver AaaS, WaaS, FaaS, Durable Cell, inference, MCP, and tenant Web application product APIs over the unified Runtime/Flow/Gateway substrate | Each product passes its semantic, runtime-profile, traffic, recovery, security, and cleanup conformance |
| `CLOUD-R6` | Complete CPU/GPU Fleet placement, reservations, hierarchical fairness, node drain, state-aware rollouts, scale-to-zero, warm capacity, distributed inference topology, and autoscaling | Placement remains single-winner under contention; stale generations cannot execute, write, or receive traffic |
| `CLOUD-R7` | Complete Gateway-only external access, protocol adapters, TLS, target snapshots, rate shaping, cache safety, rollout, regional failover, and static delivery | No internal provider is publicly reachable; stale or unauthorized targets receive zero requests |
| `CLOUD-R8` | Complete logs, metrics, traces, profiles, security telemetry, usage, SLOs, alerting, replayable analytics projection, and optional Doris OLAP | One correlation chain spans request to kernel; telemetry loss is explicit; Doris loss cannot affect product truth |
| `CLOUD-R9` | Qualify self-hosted installation, control-plane upgrades, backups, restores, disaster recovery, certificate/key rotation, mixed versions, and signed distribution | Clean install, rolling upgrade, interrupted migration, rollback boundary, node/region loss, and full restore drills pass |

The workload-identity sub-lane of `CLOUD-R1/R2` has a verified WI1 trust-policy
foundation and verified `WI2-C1/C2` evidence/owner-port composition. The
[Cloud main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33310808529)
and [Box provider conformance](https://github.com/A3S-Lab/Cloud/actions/runs/33310808538)
pass that exact gate. Component-only `WI2-C3a` is verified by the complete
[C3a main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33319781762) and
[same-revision Box provider conformance](https://github.com/A3S-Lab/Cloud/actions/runs/33319781830):
Identity publishes one generic current-policy authorization fact, and Workloads
migration `180` persists an immutable pre-scheduling bound or explicit no-policy
outcome used by ordinary, placement-group and reconciliation projection. Legacy
Units are not relabelled or backfilled. NodePool lineage is checked before
reservation and in the final placement transaction; concurrent Flow workers
adopt the first valid committed outcome. Next come C3b's one Identity evidence
history and C4's Fleet hardware-attestation fact/full versioned decision; V1 cannot
authorize issuance. Redis, Lane or a copied lifecycle cannot replace those
gates.

## 3. Runtime CI/CD as a core module

The detailed contract lives in
[Runtime CI/CD architecture](../runtime-cicd-architecture.md). The uniform
pipeline is:

```text
Trigger -> exact SourceRevision -> BuildPlan -> BuildRun -> immutable Artifact
        -> verification attestations -> product-owned Release
        -> product-owned Deployment/Publication -> observation gate
        -> promotion or selection of a prior immutable release
```

The stage graph is one A3S Flow history. Lane controls worker pressure after a
stage is durably ready. Runtime Tasks run bounded builds/tests/migrations;
Runtime Services run preview or soak environments. Box supplies node-local
build and execution. Workloads/Fleet own placement and rollout. Gateway owns
traffic shifting. The target product owns its Release. Delivery Pipelines own
only orchestration intent, policy, approvals, and receipt correlation.

| Runtime target | CI must prove | CD must prove |
| --- | --- | --- |
| Agent Service | Harness/profile compatibility, tool/skill grants, model contract, hostile output, checkpoint export/import, restart cleanup | Session fencing, drain, checkpoint handoff, canary health, old-session policy, rollback compatibility |
| Workflow Service | Definition/compiler digest, node contracts, deterministic replay, child/compensation behavior | Build-ID compatibility, worker routing, in-flight history policy, staged node-provider availability |
| Function Task/Service | Input/output schema, idempotency class, timeout/cancel, concurrency, cold/warm behavior | scale-to-zero/wake, overload, drain, target snapshot, external-FaaS connector health when selected |
| Durable Cell | Application bundle, state schema/revision compatibility, storage profile, migration and backup/restore | single-writer fencing, rolling compatibility, hibernation/reactivation, RPO/RTO, rollback restrictions |
| Inference | Model/weight/runtime digests, license, tokenizer, accuracy/safety, memory and performance profile | topology formation, weight warmup, KV compatibility, canary usage/quality, GPU cleanup |
| Static Web | reproducible build, asset integrity, CSP/base-path, SPA routing, dependency and secret scan | immutable object publication, cache policy, route snapshot, atomic release switch, instant prior-release selection |
| Cloud system service | API/schema compatibility, migrations, authorization, concurrency, failure and upgrade tests | quorum/role sequencing, migration fence, mixed-version window, canary SLO, backup and bounded rollback |

## 4. Deployment form and elasticity

| Work form | Cloud behavior |
| --- | --- |
| Stateless Service | Many interchangeable Runtime Service units; request concurrency and Gateway targets scale independently from durable hard quota |
| Stateful Agent Service | Session owner is lease/fence-bound; scale by sharding Sessions, not concurrent writers to one Session |
| Durable Cell Service | Provider namespace and writer generation are fenced; replicas follow provider-supported compatibility and recovery rules |
| Workflow | Flow history is durable; workers are stateless and replaceable; nodes invoke owner ports |
| Task/FaaS | Each admitted invocation has durable identity before Lane dispatch; execution may scale to zero |
| Distributed inference | Serving-group topology, model/weight revision, GPU claims, and component health are one admitted placement generation |
| Static Web | No Runtime unit after build; immutable objects are served through Gateway |
| Cloud system service | Explicit process roles, leader/fence only where required, safe rolling version window, and system-scope policy |

## 5. Platform interfaces

A3S Cloud delivers:

- versioned REST APIs and OpenAPI;
- maintained SDKs and the A3S CLI;
- a sessionless Management MCP surface;
- signed webhooks and provider protocols through Gateway; and
- tenant Agent/Application static Web hosting.

It deliberately does not deliver an A3S Cloud management Dashboard. System
administration and tenant management must remain complete through the public
contracts above.

## 6. Non-goals

A3S Cloud must not:

- reimplement Flow history, Lane scheduling mechanics, Runtime Unit lifecycle,
  Box node execution, OCI isolation, Gateway request proxying, Power model
  execution, Use package activation, Observer probes, or Sentry enforcement;
- treat Redis, NATS, object storage, Gateway snapshots, search indexes, model
  caches, or Doris as the source of product truth;
- expose provider credentials or internal services directly to tenants;
- create one runtime class per AI product; or
- add a second build, release, deployment, rollout, retry, registry, scheduler,
  authorization, or audit mechanism for a particular product.

## 7. Project exit

Cloud reaches a production-complete release only when the local product gates
and shared `ECO-G1` through `ECO-G8` evidence pass at exact dependency
revisions. Documentation completeness, mocks, local-only tests, or target
architecture alone never changes a capability from `Planned` to `Verified`.
