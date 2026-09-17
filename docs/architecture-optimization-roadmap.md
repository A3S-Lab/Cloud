# A3S Cloud Architecture Optimization and Execution Roadmap

**Status as of 2026-09-17.**

This document is the **execution view** of Cloud architecture optimization. It
does not replace stable ownership in [architecture.md](architecture.md), public
gate status in [ROADMAP.md](../ROADMAP.md), structural-versus-delivery gap
analysis in [platform-gap-analysis.md](platform-gap-analysis.md), or the DDD
debt baseline in [architecture-audit.md](architecture-audit.md).

| Document | Role relative to this file |
| --- | --- |
| [architecture.md](architecture.md) | Stable target ownership and failure behavior |
| [ROADMAP.md](../ROADMAP.md) | Gate status vocabulary and product outcomes |
| [platform-gap-analysis.md](platform-gap-analysis.md) | Structural vs delivery gaps and P0–P2 priority |
| [architecture-audit.md](architecture-audit.md) | Boundary debt, ratchets, convergence waves |
| [Cloud substrate dependency roadmap](../../../docs/cloud-substrate-dependency-roadmap.md) | Per-crate Wave 1–3 obligations outside Cloud |
| This document | Pragmatic critical path, dual-track I0, near-term Cloud-only backlog |

Capability availability remains gate-driven. Architecture targets and module
folders are not shipped claims.

## 1. Purpose and non-goals

### 1.1 Purpose

- Preserve first-principles **single-authority** design while delivering the
  smallest honest production-usable vertical as early as evidence allows.
- Separate **mission EXIT** (what operators can run) from **portfolio aspiration**
  (AX/K8s-class outcomes, full AI application matrix, self-evolution).
- Sequence work so one Box-hosted path works before proliferating product
  kinds, multi-node HA, or hardware-TEE claims on the same unfinished substrate.
- Split inference into a **control-plane track** Cloud can continue without
  inventing Power workers, and a **data-plane track** blocked on software
  `BX0` + `PW0` (TEE is a stronger profile, not the only door).

### 1.2 First-principles mission (unchanged)

```text
Tenant-authorized intent
  -> PostgreSQL desired state + Outbox
  -> Flow / Operations
  -> Workloads + Fleet
  -> Runtime Task|Service via Box
  -> Edge snapshot -> Gateway applied state
```

Cloud is the self-hosted Agent-first **control plane**. It is not six runtimes,
not a second Gateway, not a Dashboard backend, and not a date-driven feature
factory.

### 1.3 Pragmatic cut (2026-09-17)

The previous wave model treated `BX0` TEE isolation, full Platform P0
(`WI`/`CD0`/`H0.3`–`H0.5`), and every product vertical as one stacked critical
path. That over-coupled **hardware availability**, **day-two HA**, and
**reference-product breadth** into the first Verified exit.

| Keep as architecture law | Stop treating as first EXIT |
| --- | --- |
| One Workloads/Fleet path; Gateway-only ingress; PG+Outbox+Flow truth | “Replace AX + Kubernetes operationally” as a single Definition of Done |
| Product lanes as Task/Service projections | `APP0.6` six-experience parity before one vertical is available |
| Empty ports fail closed; ACL-only config; no Dashboard | `EV0` Agentic RL / self-evolution on the critical path |
| Real-provider evidence for claimed availability | Hardware SEV-SNP TEE as the only way to Verified `BX0` / product EXIT |
| Integrity ratchets (Wave 0) parallel forever | Full `CELL0` RPO=0 Durable Objects before AaaS/WaaS usable |
| I0 Track A without inventing workers | Distributed gang / prefill-decode `I0` before single-node Power |

### 1.4 Non-goals

- Do not add a second scheduler, workflow engine, object client, Gateway
  management plane, omniscient policy database, Cloud request-byte proxy, or
  management Dashboard backend.
- Do not invent `workers` ACL blocks, billing tokenizer authority, or
  `InferenceDeployment` aggregates before Power observation delivery.
- Do not redefine product availability around Cloud-only unit tests or module
  folders.
- Do not treat historical `R0` / `N0` / `D0` / `E0` Docker-era evidence as
  current Box provider certification.
- Do not block **software** Box re-certification or the first usable EXIT on
  absent AMD SEV-SNP hardware. TEE remains a named stronger profile
  (`BX0.tee` / `PW0.tee`), not the sole Verified door.
- Do not mark `EV0`, full `CELL0`, distributed `I0`, or `APP0.6` parity as
  required for GA-0 / GA-1 / GA-2 below.

## 2. What stays (first principles)

| Principle | Keep because |
| --- | --- |
| One Workloads + Fleet placement path | Prevents Agent/MCP/Cell/GPU/inference schedulers |
| Gateway-only public ingress | Cloud never becomes a second data plane |
| PostgreSQL + Outbox + Flow as truth | Redis/Doris remain acceleration or projection |
| Product lanes as projections over Task/Service | No six runtime stacks |
| Empty ports fail closed | Honest snapshots beat fake workers |
| ACL-only configuration via `a3s-acl` | One config authority |
| No Cloud Dashboard | API / CLI / MCP remain the ops surface |
| Aspiration ≠ EXIT | Portfolio horizons may remain; critical path must shrink |

## 3. Optimized authority map (execution view)

```text
clients → Gateway → product owner → Operations/Flow
  → Executions|Workloads → Fleet → Node Agent → Runtime → Box → payload
Edge desired state → Fleet → Gateway applied state

Identity credentials ─┐
Inference routes ─────┼→ Edge snapshot compiler → Fleet → Node Agent → Gateway
(Empty workers until PW0) ┘
```

| Concern | Sole authority | Must not appear elsewhere |
| --- | --- | --- |
| Tenant identity / grants / inference keys | Identity | Edge inventing credentials; Gateway storing bearers |
| Route / model catalog intent | Inference | Edge inventing catalog; Power owning desired state |
| Gateway desired traffic | Edge | Product-local publishers |
| Placement / Claims / rollout | Workloads + Fleet | Per-product schedulers |
| Provider lifecycle | Runtime + Box | Direct process calls from product domains |
| Durable coordination | Operations + Flow | Product retry tables |
| Inference serving process | Power as Box-hosted Service (`PW0`) | Cloud-side fake worker inventories |

## 4. Delivery profiles and critical path

### 4.1 Named delivery profiles

| Profile | Meaning | Blocks marketing claim |
| --- | --- | --- |
| `software` | Clean Linux + Box without Docker/K8s; no SEV-SNP required | Ordinary Task/Service / Agent / Workflow availability |
| `tee` | Hardware-backed MicroVM/TEE (`simulate=false` SEV-SNP) | Isolation-attested Power / confidential workloads |
| `ha` | Multi-node placement, clean install/upgrade/restore, autoscaling | Production HA / multi-replica claims |
| `portfolio` | Full gate family including deferred verticals | Complete platform / website matrix claims |

A gate may be **Verified for `software`** while remaining **Planned/In progress
for `tee`**. Documentation must name the profile. Silent upgrade from software
to tee/ha is forbidden.

### 4.2 Critical-path GAs (mission EXIT)

```text
Wave 0     Architecture integrity (parallel forever)
  -> GA-0  Usable Box service path (software BX0 + N0/D0/E0 re-cert)
  -> GA-1  One Agent vertical available (A0 + A1 Code path)
  -> GA-2  One Workflow / Application vertical available (narrow W0 + one APP0 experience)
  -> GA-3  Platform day-two (WI → CD0 → H0.3–H0.5 → OBS/COMP)  [ha profile]
  -> GA-4+ Deferred verticals (MCP/FN/WEB, single-node I0/PW0, S0/CELL0, …)
  -> P2    EV0, distributed I0, APP0.6 full parity, multi-region
```

### Wave 0 — Architecture integrity (parallel)

**Goal:** Make the modular monolith enforce ownership at compile time.

**Owns:** [architecture-audit.md](architecture-audit.md); ROADMAP §1.1.

| Exit evidence | Forbidden |
| --- | --- |
| Drive cross-context outer-layer imports toward zero | New foreign-repository shortcuts “to ship faster” |
| One ORM mapping authority per physical table | Second mapping for `workloads` / … |
| Zero public Infrastructure/Presentation facades across contexts | New `pub use` of foreign persistence |
| Keep domain technical deps and Shared Kernel back-edges at zero | Domain imports of Nest-style frameworks or infra crates |

Integrity work runs **in parallel** with every GA. It does not by itself move
product availability; affected real-provider gates must re-pass after each
debt-removal wave.

### GA-0 — Usable Box service path (`software`)

**Goal:** One operator can enroll a Linux node, deploy one digest-pinned OCI
image through Box, observe health, activate HTTPS via Gateway, stream ordered
logs, update, roll back, and stop—**without** requiring SEV-SNP hardware.

| Gate / slice | Outcome | Profile |
| --- | --- | --- |
| `F0` | Control plane foundation | Already Verified |
| `C0.1`–`C0.2m` | REST / client / CLI / Management MCP core | Already Verified core |
| `H0.1`–`H0.2` | Replicas + Gateway target projection | Already Verified |
| `BX0.software` | Box-only Task/Service, build, recovery, cleanup; re-cert `N0`/`D0`/`E0` | **Verified (`2026-09-17`)** — [evidence/bx0-software-exit-2026-09-17](evidence/bx0-software-exit-2026-09-17/) |
| `BX0.tee` | Hardware MicroVM/TEE isolation bind | Deferred profile; fail-closed until SEV runner exists |

**Forbidden:** Docker fallback as production provider; claiming historical
`R0`/`D0`/`E0` as Box-current without software re-cert; blocking GA-0 on
`A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED`.

**Physical-capacity rule (revised):** With no `sev-snp` runner, **skip**
`BX0.tee`, `PW0.tee`, and confidential-workload claims. Do **not** skip
`BX0.software` or the GA-0 clean-host loop. A skipped TEE job is not Verified
tee; it must not freeze the software exit.

Until GA-0 exits, treat Box-backed production claims as **provisional**.

### GA-1 — One Agent vertical (`software`)

**Goal:** One immutable Agent release path is **available** end-to-end through
Gateway: Code Harness (or one certified external provider), durable events,
approval where required, and process-death recovery already retained in A1
evidence—without waiting for every A1.x external-provider matrix cell.

| Prefer | Defer |
| --- | --- |
| `A0.4` Agent deployment + `A1.0`/`A1.2` Code path + minimal public surface | Full heterogeneous provider matrix; `AR0` full experience; fork/S3 private checkpoint production claims |

`AR0` remains a projection over existing authorities and stays **after** GA-1
availability, not a second Agent platform.

### GA-2 — One Workflow / Application vertical (`software`)

**Goal:** One ordinary Workflow Goal→Plan→Run path and **one** Application
experience (classic Agent **or** Workflow—not six) publish through Gateway with
session/invocation recovery already retained.

| Prefer | Defer |
| --- | --- |
| Narrow `W0` public Agent + finite Execution path; `APP0` single experience + publication | `APP0.6` six-mode parity; 23-node commercial matrix; full `K0` RAG pipelines; full `AUT0` product availability |

`K0` Files admission already in progress may continue as foundation work; live
MinIO/scanner/SEV ingestion is **not** GA-2 EXIT.

### GA-3 — Platform day-two (`ha`)

**Goal:** Close structural platform gaps that multiply risk across every profile
**after** GA-0 and at least one product vertical exist.

| Slice | Outcome | Notes |
| --- | --- | --- |
| Workload identity (`H0.4-WI*`) | Attestation, short-lived identity, peer policy, discovery, revocation | Identity owns policy; no east-west mesh Gateway |
| `CD0` Delivery Pipelines | One Flow-backed source→build→release→promote→rollback | Reuse Artifacts/Releases/Workloads |
| `H0.3`–`H0.5` | Multi-node placement, HA install/upgrade/restore, autoscaling | Single region before any `H0.6` |
| `H0.5-OBS*` | Correlation, SLO, incident aggregate | Owner evidence stays authoritative |
| `C0.4-COMP*` | Wire/schema/storage deprecation and skew locks | CI rejects undocumented breaks |

Contract-only freezes (`CD0.1`, `WI1`, `OBS1`, `COMP1`) may still land early;
they do not redefine GA-0 EXIT.

**Forbidden:** Omniscient policy DB; second Operator control plane; Dashboard as
the only day-two surface.

### GA-4+ — Deferred verticals (keep architecture; delay EXIT)

Certify Gateway publication and recovery **one lane at a time** after GA-1/GA-2:

| Lane | Gate family | Extra blocker | Priority vs GA-0..2 |
| --- | --- | --- | --- |
| Hosted MCP | `MCP0` | software `BX0` | After GA-1 |
| FaaS | `FN0` | software `BX0` | After GA-1 |
| Static Web | `WEB0` | Gateway static-object target | After GA-0 |
| Inference **single-node** data plane | `PW0.software` + `I0` Track B minimal | software `BX0` | After GA-0; before distributed I0 |
| Inference **distributed** | gang / PD pools / KV-aware | `ha` + `PW0` | P2 |
| Stateful storage | `S0` retained provider | — | Before CELL0 |
| Durable Cell | `CELL0` | `S0` + software `BX0`/`E0` | After GA-2; not critical path |
| Inference control plane | I0 Track A | none (continue now) | Parallel anytime |
| Governed evolution | `EV0` | W0 + A1.6 + safety | **P2 only** |

### P1 / P2

| Priority | Items |
| --- | --- |
| P1 | Usage/cost (`C0.5-UG*`), data governance (`C0.5-DG*`), enterprise tenancy remainder, single-home multi-region (`H0.6-R*`) after `ha` |
| P2 | `EV0`, distributed `I0`, `APP0.6` full parity, optional dynamic feature flags for tenant apps only—never Cloud auth or Delivery bypass |

## 5. Dual-track I0 (unchanged honesty, clearer EXIT)

Inference is a first-class **shared platform service** in the target
architecture, but it must not be marketed as available while workers and Power
are absent.

### Track A — Cloud control plane (continue without inventing Power)

Allowed while `EmptyInferenceWorkerAclProjectionPort` remains wired:

- Identity inference key lifecycle (create / rotate / revoke / create-then-revoke)
- Inference route catalog publish / revise / retire and grant admission
- Edge managed-snapshot ACL succession (credentials + routes; **no `workers`**)
- Usage batch ingest, showback, retention (prompt-free facts)
- Path-scope / missing-environment fail-closed behavior
- First-principles tests that prove fail-closed honesty
- Owner `*Access` projection at REST/MCP entry for catalog and mutation paths

**Hard rule:** snapshots that omit `workers` are correct until Power
observation delivery. Do not invent worker observations in Cloud.

### Track B — Data plane

Requires Verified **software** profile first:

1. `BX0.software` Box re-certification
2. `PW0.software` Power Service profile and observation delivery (non-TEE
   Box-hosted Service is enough for ordinary inference availability)
3. Gateway OpenAI-compatible dispatch, auth denial, fallback, and live worker
   binding for a **single-node** serving path

`PW0.tee` / confidential serving and distributed `I0` remain stronger profiles
and must not block Track B software EXIT.

Only Track B may claim “inference service available.”

## 6. Near-term Cloud-only backlog

Work that advances the real end state **without** inventing BX0/PW0 product
surfaces. Prefer first-principles tests; push to `main` with local `cargo`
evidence (no CI requirement for this backlog unless a gate already demands it).

**Near-term priority order for Cloud-owned edits:**

1. ~~Close remaining `BX0.software` / clean-host GA-0 evidence (not TEE theater).~~
   **Done (`2026-09-17`)** — retained
   [evidence/bx0-software-exit-2026-09-17](evidence/bx0-software-exit-2026-09-17/);
   checklist [ga0-bx0-software-checklist.md](ga0-bx0-software-checklist.md).
2. Finish the one Agent public availability slice for GA-1.
   Checklist: [ga1-agent-availability-checklist.md](ga1-agent-availability-checklist.md)
   (first gap: A0.4 retained Box pin skew vs current `BX0.software` pins).
3. Narrow one Workflow/Application availability slice for GA-2.
4. Continue Wave 0 integrity and I0 Track A fail-closed honesty.
5. Land contract-only `CD0.1` / `WI1` / `OBS1` / `COMP1` without claiming `ha`.

| Theme | Examples |
| --- | --- |
| Architecture integrity | Reduce audit ratchets; replace foreign repository imports with owner Application ports. Identity inference-key handlers use `IIdentityEnvironmentAccess`; Identity `create_resource_grant` uses `IIdentityProjectAccess` / `IIdentityEnvironmentAccess` / `IIdentityNodeAccess`; Inference route/usage handlers use `IInferenceEnvironmentAccess`; Edge `create_gateway_scope` / `create_domain_claim` / `create_mcp_credential` use `IEdgeEnvironmentAccess` (+ `IEdgeNodeAccess` for gateway scopes); Edge MCP route-policy create/revise uses `IEdgeMcpServiceProfileAccess` returning owned `EdgeMcpServiceProfileAdmission`; Edge postgres MCP route-policy hydration restores owned admission facts on `mcp_route_policies` (no Assets `mcp_service_profiles` join); Edge MCP Gateway projection uses owned `EdgeMcpServiceProfileProjectionBinding` and `EdgeMcpWorkloadRevisionProjectionBinding` materialised through Edge Application ports; healthy route-target resolution uses Domain `IRouteTargetReader` with Workloads+Fleet quarantined in `WorkloadsFleetRouteTargetAccessAdapter`; Edge deployment route cutover reads Fleet Runtime observations through `IEdgeRuntimeObservationAccess`; managed Gateway inference ACL staging uses `IEdgeManagedInferenceAclAccess` with Identity/Inference quarantined in `IdentityInferenceEdgeManagedAclAccessAdapter`; Edge MCP credential create/rotate delivery encryption uses `IEdgeMcpCredentialEncryption` with Domain-owned `EdgeEncryptedCredentialValue`; Identity inference-key create/rotate delivery encryption uses `IIdentityInferenceCredentialEncryption` with Domain-owned `IdentityEncryptedCredentialValue`; Data object-namespace recovery OperationRequest composition uses Infrastructure `ObjectNamespaceRecoveryOperationRequest`; Workloads deployment/stop/writer-fence commands emit owned operation intents composed in Infrastructure and persisted through Operations `insert_operation_request_in_transaction` (no Workloads `operation_requests` ORM); WorkflowRun create uses the same Operations participant (no Workflow `operation_requests` ORM); Workloads secret-rotation reconcile locks Secrets through `lock_secret_version_for_rotation` (no Workloads Secrets ORM); Edge staging locks Fleet nodes through `lock_node_organization_for_update` (no Edge `nodes` ORM); Edge MCP Gateway snapshot Workload authority CAS uses Workloads `lock_running_workload_authority_for_update` (no Edge `workloads` ORM); cross-context txn participants are imported from owner module roots (Fleet/Secrets/Workloads/Operations), and Workloads resource-claim placement uses Fleet module-root participants (no outer-layer `fleet/infrastructure` allowlist row); Edge MCP revision projection ACA consumes Workloads `IWorkloadMcpActiveRevisionProjectionQueryPort` / published `ActiveMcpWorkloadRevisionProjection` (no Edge `IWorkloadRepository`); Edge healthy route-target ACA consumes Workloads `IWorkloadHealthyRouteTargetCandidateQueryPort` / published candidate set plus `IEdgeRuntimeObservationAccess` (no Edge `IWorkloadRepository` / `INodeControlRepository`); Edge Gateway install/observe queues consume Fleet `IFleetGatewaySnapshotCommandPort` (no Edge `INodeControlRepository` / `NodeCommandDraft`); Projects `create_project` uses `IProjectOrganizationAccess` with Identity quarantined in `IdentityProjectsOrganizationAccessAdapter` (no Projects Application `IOrganizationRepository`); Operations list queries carry owned `OperationAccess` projected at REST/MCP entry (no Operations Application `ResourceAccessEvaluator`); Executions get/list/cancel carry owned `ExecutionAccess` projected at REST entry (`ListExecutions` fails closed on environment visibility; no Executions Application `ResourceAccessEvaluator`); Operation subject resolver maps Execution subjects through `ExecutionAccess` (no Identity bridge for that arm); Agents queries/commands carry owned `AgentAccess` projected at REST entry (`ListAgentConversations` fails closed on environment visibility; `CreateAgentConversation` carries owned `AgentAccess` and fails closed on environment visibility; no Agents Application `ResourceAccessEvaluator`); Operation subject resolver maps AgentExecution subjects through `AgentAccess` (no Identity bridge for that arm); Workflow queries/commands/authoring carry owned `WorkflowAccess` projected at REST entry (no Workflow Application `ResourceAccessEvaluator`); Operation subject resolver maps WorkflowRun subjects through `WorkflowAccess` (legacy Identity bridge removed); Projects list/attribution/environment list carry owned `ProjectAccess` projected at REST/MCP entry; Projects `CreateEnvironment` carries owned `ProjectAccess` and fails closed without project authority (no Projects Application `ResourceAccessEvaluator` for those paths); Projects `CreateProject` carries owned `ProjectAccess` and fails closed without organization-catalog visibility (no Projects Application `ResourceAccessEvaluator`); Fleet list nodes / node-pool query and manage carry owned `FleetAccess` projected at REST/MCP entry (no Fleet Application `ResourceAccessEvaluator` for those paths); Applications queries/commands/delivery carry owned `ApplicationAccess` projected at REST/MCP entry (no Applications Application `ResourceAccessEvaluator`); Notifications queries/commands carry owned `NotificationAccess` projected at REST/MCP entry (domain no longer imports Identity visibility; outbox projection uses `INotificationOutboxIdentityAccess` with Identity membership/grant repositories quarantined in `IdentityNotificationOutboxIdentityAccessAdapter`, which synthesizes `NotificationAccess` without `ResourceAccessEvaluator`); Durable Cells queries/commands/admission/route publish carry owned `DurableCellAccess` projected at REST/MCP entry (Node grants discarded); Connectors queries/commands/execution carry owned `ConnectorAccess` projected at REST/MCP entry (Notifications outbound synthesizes `ConnectorAccess` instead of Identity grants); Inference route/usage queries carry owned `InferenceAccess` projected at REST entry; Inference `PublishInferenceRoute` / `ReviseInferenceRoute` / `RetireInferenceRoute` carry owned `InferenceAccess` and fail closed on environment visibility (REST); Edge `CreateDomainClaim` / `CreateGatewayScope` / `CreateMcpCredential` carry owned `EdgeAccess` and fail closed on environment visibility (REST); Identity inference-key list/get queries carry owned `IdentityAccess` projected at REST entry (Node grants discarded; Domain `ResourceAccessEvaluator` remains the authority surface for guards/grants); Edge query surface carries owned `EdgeAccess` projected at REST entry for routes, domain claims, gateway scopes, MCP credentials, MCP route policies, and gateway certificates (MCP for routes; Node grants discarded; env-scoped lists fail closed on visibility; certificate inventory requires organization-wide access; Get* resolve indirect IDs through Edge-owned environment facts); writer-fence Domain admits `WorkloadRuntimeRemoveEvidence` instead of Fleet `NodeCommand`; `DeploymentBundle` / `WorkloadStopBundle` / `ReplicaDeploymentMaterialization` return owned intents; Workloads deployment queries enrich Operations through `IWorkloadDeploymentOperationAccess` and Fleet Runtime observations through `IWorkloadRuntimeObservationAccess`; Workload queries/commands carry owned `WorkloadAccess` projected at REST/MCP entry (`ListWorkloads` fails closed on environment visibility; `CreateWorkloadDeployment` / `CreateSourceWorkloadDeployment` / `CreateAgentWorkloadDeployment` carry owned `WorkloadAccess` and fail closed on environment visibility; log queries use `IWorkloadLogAccess` with owned `WorkloadLogRecord` / `WorkloadLogRecordResponse`); Secrets `ListSecrets` carries owned `SecretAccess` and fails closed on environment visibility; Secrets `CreateSecret` carries owned `SecretAccess` and fails closed on environment visibility; Notifications alert-policy creation uses `INotificationsEnvironmentAccess` / `INotificationsNodeAccess`; Connectors `create_connector_profile` uses `IConnectorsEnvironmentAccess`; Connectors create/revise profile Secret admission uses published `IExactSecretVersionAccess`; Agents conversation create + workflow agent port use `IAgentsEnvironmentAccess`; Agents reconciler schedules Operations through `IAgentExecutionOperationScheduler`; Plugins reconciler schedules Operations through `IPluginAssignmentOperationScheduler`; Durable Cells `create_durable_cell_application` uses `IDurableCellsEnvironmentAccess`; Executions create/workflow + template list/create use `IExecutionsEnvironmentAccess` / `IExecutionsProjectAccess`; Executions reconciler schedules Operations through `IExecutionOperationScheduler`; Workloads create/source/agent deployment handlers use `IWorkloadsEnvironmentAccess` and `IWorkloadsNodePoolAccess`; Workloads agent create/update deployment handlers admit releases through `IWorkloadAgentReleaseAdmissionPort`; Workloads Skill bind admits releases through `IWorkloadSkillReleaseAdmissionPort`; Workloads MCP revision Domain binding uses owned `McpReleaseAdmission` / `McpProfileAdmission` with revision-owned `runtime_port` / `health_path` facts (no Workloads `mcp_service_profiles` join); Workloads source create admits builds through `IWorkloadSourceBuildAdmissionPort`; Workloads create/update/bind/unbind/rollback Secret admission uses `IWorkloadsSecretBindingAccess`; Workflow create ontology / definition publication / create goal / node catalog use `IWorkflowProjectAccess` (+ `IWorkflowEnvironmentAccess` for goal env); Applications admit invocation uses `IApplicationsEnvironmentAccess`; Workflow `SubmitHumanTask` uses `IHumanTaskAuthorizationPort` with Identity quarantined in `IdentityHumanTaskAuthorizationAdapter`; Agents `DecideAgentApprovalCheckpoint` uses `IAgentApprovalAuthorizationPort` with Identity quarantined in `IdentityAgentApprovalAuthorizationAdapter`; Notifications outbound subscription/SMTP uses `IOutboundRecipientContactAccess` returning owned `OutboundVerifiedRecipientContact` with Identity quarantined in `IdentityOutboundRecipientContactAccessAdapter` (SMTP prepare takes owned address string; Identity email parsing stays in Infrastructure); Plugins `ListPluginAssignments` carries owned `PluginAccess` and fails closed on environment visibility; Plugins `SetPluginAssignment` carries owned `PluginAccess` and fails closed on environment visibility; Sources list queries carry owned `SourceAccess` and fail closed on environment visibility; Sources create/deactivate GitHub repository subscriptions carry owned `SourceAccess` and fail closed on environment visibility; Executions `CreateExecutionCommand` carries owned `ExecutionAccess` and fails closed on environment visibility; Executions `CreateExecutionTemplateCommand` carries owned `ExecutionAccess` and fails closed on project visibility; Executions `GetExecutionTemplate` carries owned `ExecutionAccess` and fails closed on project visibility; Executions `ListExecutionTemplates` carries owned `ExecutionAccess` and fails closed on project visibility; Workflow project-scoped lists carry owned `WorkflowAccess` and fail closed on project visibility; Workflow `CreateOntology` / `CreateWorkflowDefinition` / `CreateWorkflowGoal` / `StartWorkflowRun` carry owned `WorkflowAccess` and fail closed on project visibility; Forms `ListFormDrafts` carries owned `FormAccess` and fails closed on project visibility; Forms `CreateFormDraft` carries owned `FormAccess` and fails closed on project visibility; Assets `CreateAsset` carries owned `AssetAccess` and fails closed on organization-catalog visibility; Fleet `IssueEnrollmentToken` uses `IFleetOrganizationAccess` with Identity quarantined in `IdentityFleetOrganizationAccessAdapter`. |
| I0 Track A | Further Edge succession / admission / path-scope bricks; usage recovery gaps that stay Cloud-owned without inventing workers |
| C0 remainder | Enterprise slices that do not require Box; day-two CLI/MCP parity for already-owned lifecycles |
| G0 / P0 / U0 components | Provider evidence and handoffs already named in ROADMAP without claiming full lane exit |
| Documentation honesty | README / plans keep Verified vs Planned vocabulary **and** name `software` / `tee` / `ha` profiles |

Do **not** use this backlog to declare GA-0–GA-3 complete.

## 7. Success metrics

| Metric | Pass condition |
| --- | --- |
| Gate status | Named exits in ROADMAP marked `Verified` with retained evidence **and profile** |
| Integrity | Audit debt lists only shrink; no new entries |
| GA-0 | Software clean-host enroll→deploy→HTTPS→logs→update/rollback without TEE |
| Inference claims | Track B software gates Verified before any “OpenAI data plane available” language |
| Vertical claims | Gateway publication + recovery for that lane, not module presence or full matrix |
| Fail-closed | Empty ports and missing env/scope continue to fail closed under test |
| Deferred honesty | `EV0`, distributed `I0`, `APP0.6`, full `CELL0` never implied by GA-0..2 |

## 8. Relationship to prior priority text

This document supersedes the stacked critical path in
[platform-gap-analysis.md](platform-gap-analysis.md) §5 as follows:

```text
Wave 0  Architecture integrity (parallel; audit ratchets → zero debt)
GA-0    BX0.software + N0/D0/E0 Box re-cert (TEE is BX0.tee, not the door)
GA-1    One AaaS vertical (A0 + A1 Code path availability)
GA-2    One WaaS/APP vertical (narrow W0 + one APP0 experience)
GA-3    Platform day-two: WI → CD0 → H0.3–H0.5 → OBS/COMP   [ha]
GA-4+   MCP/FN/WEB, PW0.software + single-node I0, S0, CELL0
P1      Usage/cost, data governance, enterprise remainder, multi-region
P2      EV0, distributed I0, APP0.6 full parity, optional feature flags
```

Legacy labels `P0-A`…`P0-F` map to GA-0 / GA-3 / GA-4+ above; they must not be
read as a single blocking stack that freezes product EXIT on TEE or full HA.

Architecture integrity (Wave 0) runs beside every GA and is a release
condition for claimed surfaces, not a substitute for those gates.
