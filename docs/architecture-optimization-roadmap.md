# A3S Cloud Architecture Optimization and Execution Roadmap

**Status as of 2026-09-10.**

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
| This document | Optimized execution order, dual-track I0, near-term Cloud-only backlog |

Capability availability remains gate-driven. Architecture targets and module
folders are not shipped claims.

## 1. Purpose and non-goals

### 1.1 Purpose

- Preserve first-principles single-authority design while closing the gaps that
  make production self-hosted Agent platforms unsafe or unoperable.
- Sequence work so structural foundations (trust, delivery, HA, observability,
  compatibility) land before proliferating more product verticals on the same
  weak substrate.
- Split inference into a **control-plane track** Cloud can continue without
  inventing Power workers, and a **data-plane track** blocked on `BX0` + `PW0`.

### 1.2 Non-goals

- Do not add a second scheduler, workflow engine, object client, Gateway
  management plane, omniscient policy database, Cloud request-byte proxy, or
  management Dashboard backend.
- Do not invent `workers` ACL blocks, billing tokenizer authority, or
  `InferenceDeployment` aggregates before Power observation delivery.
- Do not redefine Cloud EXIT around Cloud-only unit tests while `BX0` / `PW0`
  remain open.
- Do not treat historical `R0` / `N0` / `D0` / `E0` evidence as current Box
  provider certification.

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

## 4. Execution waves

Dependency order is deliberate. Shipping more product kinds before trust,
delivery, operations, and compatibility foundations multiplies the same risk
across every Runtime profile.

```text
Wave 0  Architecture integrity (parallel forever)
  -> Wave 1  Substrate: BX0 Box re-cert, then PW0 Power
  -> Wave 2  Platform P0: WI → CD0 → H0.3–H0.5 → OBS/COMP
  -> Wave 3  Verticals: AaaS/WaaS/FaaS/Cell/I0-data-plane/WEB0
  -> P1      Usage/cost, data governance, AI assurance, single-home multi-region
  -> P2      Optional dynamic feature delivery (never on critical path)
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

Integrity work runs **in parallel** with Waves 1–3. It does not by itself move
product availability; affected real-provider gates must re-pass after each
debt-removal wave.

### Wave 1 — Substrate (`BX0`, `PW0`)

**Goal:** Re-certify the sole execution/build provider, then land Power as an
ordinary Box-hosted Runtime Service.

| Gate | Outcome | Depends on |
| --- | --- | --- |
| `BX0` | Box-only Task/Service, build, recovery, cleanup re-cert | — |
| `PW0` | Immutable Power Service profile, health, inference, recovery | `BX0` baseline |

**Forbidden:** Docker fallback providers; Power as a second scheduler or node
channel; claiming historical `R0`/`D0`/`E0` as Box-current without re-cert.

Until Wave 1 exits, treat Runtime/deployment/edge production claims as
**provisional**.

### Wave 2 — Platform P0 (`WI`, `CD0`, `H0.3`–`H0.5`, `OBS`, `COMP`)

**Goal:** Close structural platform gaps before vertical proliferation.

| Slice | Outcome | Notes |
| --- | --- | --- |
| Workload identity (`H0.4-WI*`) | Attestation, short-lived identity, peer policy, discovery, revocation | Identity owns policy; Fleet/Runtime/Box attest; no east-west mesh Gateway |
| `CD0` Delivery Pipelines | One Flow-backed source→build→release→promote→rollback | Reuse Artifacts/Releases/Workloads; no parallel updater |
| `H0.3`–`H0.5` | Multi-node placement, HA install/upgrade/restore, autoscaling | Single region before any `H0.6` multi-region |
| `H0.5-OBS*` | Correlation, SLO, incident aggregate | Owner evidence stays authoritative; Notifications does not own incidents |
| `C0.4-COMP*` | Wire/schema/storage deprecation and skew locks | CI rejects undocumented breaks |

**Forbidden:** Omniscient policy DB; second Operator control plane; Dashboard as
the only way to operate day-two.

### Wave 3 — Product verticals

Certify end-to-end Gateway publication and recovery for:

| Lane | Gate family | Extra blocker |
| --- | --- | --- |
| AaaS | `A0` / `A1` (then `AR0` projection) | Wave 1–2 foundations |
| WaaS | `W0` | Wave 1–2 foundations |
| FaaS / MCP | `FN0` / `MCP0` | Wave 1–2 foundations |
| Durable Cell | `CELL0` | `S0` provider evidence |
| Inference **data plane** | `I0.2b`+ through `I0.6` | **`BX0` + `PW0`** + Gateway dispatch |
| Static Web | `WEB0` | Gateway static-object target |

### P1 / P2 (after single-region platform)

| Priority | Items |
| --- | --- |
| P1 | Platform-wide usage/cost (`C0.5-UG*`), data governance/residency (`C0.5-DG*`), AI assurance (`EV0` + CD0 gates), multi-region (`H0.6-R*`) after `H0.4`/`H0.5` |
| P2 | Optional dynamic feature flags for tenant apps only—never Cloud auth or Delivery bypass |

## 5. Dual-track I0 (architecture optimization)

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

**Hard rule:** snapshots that omit `workers` are correct until Power
observation delivery. Do not invent worker observations in Cloud.

### Track B — Data plane (blocked)

Requires Verified:

1. `BX0` Box re-certification
2. `PW0` Power Service profile and observation delivery
3. Gateway OpenAI-compatible dispatch, auth denial, fallback, and live worker
   binding (`I0.2b`+ product exits)

Only Track B may claim “inference service available.”

## 6. Near-term Cloud-only backlog

Work that advances the real end state **without** inventing BX0/PW0 product
surfaces. Prefer first-principles tests; push to `main` with local `cargo`
evidence (no CI requirement for this backlog unless a gate already demands it).

| Theme | Examples |
| --- | --- |
| Architecture integrity | Reduce audit ratchets; replace foreign repository imports with owner Application ports. Identity inference-key handlers use `IIdentityEnvironmentAccess`; Identity `create_resource_grant` uses `IIdentityProjectAccess` / `IIdentityEnvironmentAccess` / `IIdentityNodeAccess`; Inference route/usage handlers use `IInferenceEnvironmentAccess`; Edge `create_gateway_scope` / `create_domain_claim` / `create_mcp_credential` use `IEdgeEnvironmentAccess` (+ `IEdgeNodeAccess` for gateway scopes); Edge MCP route-policy create/revise uses `IEdgeMcpServiceProfileAccess` returning owned `EdgeMcpServiceProfileAdmission`; Edge postgres MCP route-policy hydration restores owned admission facts on `mcp_route_policies` (no Assets `mcp_service_profiles` join); Edge MCP Gateway projection uses owned `EdgeMcpServiceProfileProjectionBinding` and `EdgeMcpWorkloadRevisionProjectionBinding` materialised through Edge Application ports; healthy route-target resolution uses Domain `IRouteTargetReader` with Workloads+Fleet quarantined in `WorkloadsFleetRouteTargetAccessAdapter`; Edge deployment route cutover reads Fleet Runtime observations through `IEdgeRuntimeObservationAccess`; managed Gateway inference ACL staging uses `IEdgeManagedInferenceAclAccess` with Identity/Inference quarantined in `IdentityInferenceEdgeManagedAclAccessAdapter`; Edge MCP credential create/rotate delivery encryption uses `IEdgeMcpCredentialEncryption` with Domain-owned `EdgeEncryptedCredentialValue`; Identity inference-key create/rotate delivery encryption uses `IIdentityInferenceCredentialEncryption` with Domain-owned `IdentityEncryptedCredentialValue`; Data object-namespace recovery OperationRequest composition uses Infrastructure `ObjectNamespaceRecoveryOperationRequest`; Workloads deployment/stop/writer-fence commands emit owned operation intents composed in Infrastructure and persisted through Operations `insert_operation_request_in_transaction` (no Workloads `operation_requests` ORM); WorkflowRun create uses the same Operations participant (no Workflow `operation_requests` ORM); Workloads secret-rotation reconcile locks Secrets through `lock_secret_version_for_rotation` (no Workloads Secrets ORM); Edge staging locks Fleet nodes through `lock_node_organization_for_update` (no Edge `nodes` ORM); Edge MCP Gateway snapshot Workload authority CAS uses Workloads `lock_running_workload_authority_for_update` (no Edge `workloads` ORM); cross-context txn participants are imported from owner module roots (Fleet/Secrets/Workloads/Operations), and Workloads resource-claim placement uses Fleet module-root participants (no outer-layer `fleet/infrastructure` allowlist row); Edge MCP revision projection ACA consumes Workloads `IWorkloadMcpActiveRevisionProjectionQueryPort` / published `ActiveMcpWorkloadRevisionProjection` (no Edge `IWorkloadRepository`); Edge healthy route-target ACA consumes Workloads `IWorkloadHealthyRouteTargetCandidateQueryPort` / published candidate set plus `IEdgeRuntimeObservationAccess` (no Edge `IWorkloadRepository` / `INodeControlRepository`); Edge Gateway install/observe queues consume Fleet `IFleetGatewaySnapshotCommandPort` (no Edge `INodeControlRepository` / `NodeCommandDraft`); Projects `create_project` uses `IProjectOrganizationAccess` with Identity quarantined in `IdentityProjectsOrganizationAccessAdapter` (no Projects Application `IOrganizationRepository`); Operations list queries carry owned `OperationAccess` projected at REST/MCP entry (no Operations Application `ResourceAccessEvaluator`); Executions get/cancel carry owned `ExecutionAccess` projected at REST entry (no Executions Application `ResourceAccessEvaluator`); Operation subject resolver maps Execution subjects through `ExecutionAccess` (no Identity bridge for that arm); Agents queries/commands carry owned `AgentAccess` projected at REST entry (no Agents Application `ResourceAccessEvaluator`); Operation subject resolver maps AgentExecution subjects through `AgentAccess` (no Identity bridge for that arm); Workflow queries/commands/authoring carry owned `WorkflowAccess` projected at REST entry (no Workflow Application `ResourceAccessEvaluator`); Operation subject resolver maps WorkflowRun subjects through `WorkflowAccess` (legacy Identity bridge removed); Projects list/attribution/environment list carry owned `ProjectAccess` projected at REST/MCP entry (no Projects Application `ResourceAccessEvaluator` for those paths); Fleet list nodes / node-pool query and manage carry owned `FleetAccess` projected at REST/MCP entry (no Fleet Application `ResourceAccessEvaluator` for those paths); Applications queries/commands/delivery carry owned `ApplicationAccess` projected at REST/MCP entry (no Applications Application `ResourceAccessEvaluator`); Notifications queries/commands carry owned `NotificationAccess` projected at REST/MCP entry (domain no longer imports Identity visibility; outbox alert authorization synthesizes `NotificationAccess` from membership role + grants without `ResourceAccessEvaluator`); Durable Cells queries/commands/admission/route publish carry owned `DurableCellAccess` projected at REST/MCP entry (Node grants discarded); Connectors queries/commands/execution carry owned `ConnectorAccess` projected at REST/MCP entry (Notifications outbound synthesizes `ConnectorAccess` instead of Identity grants); Inference route/usage queries carry owned `InferenceAccess` projected at REST entry; Identity inference-key list/get queries carry owned `IdentityAccess` projected at REST entry (Node grants discarded; Domain `ResourceAccessEvaluator` remains the authority surface for guards/grants); Edge `GetRoute` / `ListRoutes` / `GetDomainClaim` / `ListDomainClaims` carry owned `EdgeAccess` projected at REST entry (MCP for routes; Node grants discarded; list queries fail closed on environment visibility; GetDomainClaim resolves claim→environment through `EdgeResourceAccess`); writer-fence Domain admits `WorkloadRuntimeRemoveEvidence` instead of Fleet `NodeCommand`; `DeploymentBundle` / `WorkloadStopBundle` / `ReplicaDeploymentMaterialization` return owned intents; Workloads deployment queries enrich Operations through `IWorkloadDeploymentOperationAccess` and Fleet Runtime observations through `IWorkloadRuntimeObservationAccess`; Workload log queries use `IWorkloadLogAccess` with owned `WorkloadLogRecord` / `WorkloadLogRecordResponse`; Notifications alert-policy creation uses `INotificationsEnvironmentAccess` / `INotificationsNodeAccess`; Connectors `create_connector_profile` uses `IConnectorsEnvironmentAccess`; Connectors create/revise profile Secret admission uses published `IExactSecretVersionAccess`; Agents conversation create + workflow agent port use `IAgentsEnvironmentAccess`; Agents reconciler schedules Operations through `IAgentExecutionOperationScheduler`; Plugins reconciler schedules Operations through `IPluginAssignmentOperationScheduler`; Durable Cells `create_durable_cell_application` uses `IDurableCellsEnvironmentAccess`; Executions create/workflow + template list/create use `IExecutionsEnvironmentAccess` / `IExecutionsProjectAccess`; Executions reconciler schedules Operations through `IExecutionOperationScheduler`; Workloads create/source/agent deployment handlers use `IWorkloadsEnvironmentAccess` and `IWorkloadsNodePoolAccess`; Workloads agent create/update deployment handlers admit releases through `IWorkloadAgentReleaseAdmissionPort`; Workloads Skill bind admits releases through `IWorkloadSkillReleaseAdmissionPort`; Workloads MCP revision Domain binding uses owned `McpReleaseAdmission` / `McpProfileAdmission` with revision-owned `runtime_port` / `health_path` facts (no Workloads `mcp_service_profiles` join); Workloads source create admits builds through `IWorkloadSourceBuildAdmissionPort`; Workloads create/update/bind/unbind/rollback Secret admission uses `IWorkloadsSecretBindingAccess`; Workflow create ontology / definition publication / create goal / node catalog use `IWorkflowProjectAccess` (+ `IWorkflowEnvironmentAccess` for goal env); Applications admit invocation uses `IApplicationsEnvironmentAccess`; Workflow `SubmitHumanTask` uses `IHumanTaskAuthorizationPort` with Identity quarantined in `IdentityHumanTaskAuthorizationAdapter`; Agents `DecideAgentApprovalCheckpoint` uses `IAgentApprovalAuthorizationPort` with Identity quarantined in `IdentityAgentApprovalAuthorizationAdapter`; Notifications outbound subscription/SMTP uses `IOutboundRecipientContactAccess` returning owned `OutboundVerifiedRecipientContact` with Identity quarantined in `IdentityOutboundRecipientContactAccessAdapter` (SMTP prepare takes owned address string; Identity email parsing stays in Infrastructure); Fleet `IssueEnrollmentToken` uses `IFleetOrganizationAccess` with Identity quarantined in `IdentityFleetOrganizationAccessAdapter`. |
| I0 Track A | Further Edge succession / admission / path-scope bricks (route + domain-claim list/get now share `EdgeAccess`); usage recovery gaps that stay Cloud-owned (node-control mTLS now proves empty-stream hole advertise + fill/redelivery without inventing workers; managed certificate-convergence live path now stages inference credential+route ACL without inventing workers; managed route-rollout planner path now stages inference credential+route ACL without inventing workers; managed route-cutover staging path now stages inference credential+route ACL without inventing workers; managed rollout-rollback compile path embeds inference credential+route ACL without inventing workers; live managed rollout-rollback reconciler path now stages inference credential+route ACL without inventing workers) |
| C0 remainder | Enterprise slices that do not require Box; day-two CLI/MCP parity for already-owned lifecycles |
| G0 / P0 / U0 components | Provider evidence and handoffs already named in ROADMAP without claiming full lane exit |
| Documentation honesty | README / plans keep Verified vs Planned vocabulary |

Do **not** use this backlog to declare Wave 1–3 complete.

## 7. Success metrics

| Metric | Pass condition |
| --- | --- |
| Gate status | Named exits in ROADMAP marked `Verified` with retained evidence |
| Integrity | Audit debt lists only shrink; no new entries |
| Inference claims | Track B gates Verified before any “OpenAI data plane available” language |
| Vertical claims | Gateway publication + recovery evidence for that lane, not module presence |
| Fail-closed | Empty ports and missing env/scope continue to fail closed under test |

## 8. Relationship to prior priority text

This document expands [platform-gap-analysis.md](platform-gap-analysis.md) §5:

```text
P0-A  Box certification (BX0) then Power (PW0)
P0-B  Workload identity and east-west trust
P0-C  CD0 source-to-release-to-rollout
P0-D  H0.3/H0.4/H0.5 cluster, upgrade, recovery
P0-E  Observability/SLO/incident and compatibility
P0-F  AaaS/WaaS/FaaS/Cell/Inference-data-plane/Web verticals
P1    Usage/cost, data governance, AI assurance, multi-region
P2    Optional dynamic feature delivery
```

Architecture integrity (Wave 0) runs beside P0-A through P0-F and is a release
condition, not a substitute for those gates.
