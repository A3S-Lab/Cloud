# A3S Cloud Platform Capability Architecture

## 1. Decision and status

A3S Cloud preserves the valuable **outcomes** of OpenShift and TokenHub while
keeping A3S-native domain ownership, protocols, and execution paths. It does
not embed either product, emulate Kubernetes, or add a TokenHub-shaped control
plane.

This is a target architecture. Rows are available only when every named
roadmap gate has passed its real-provider, failure, recovery, cleanup, and
release evidence. The matrix prevents a useful outcome from being forgotten;
it is not an availability claim.

Two first-principles user needs drive the design:

1. a platform team must be able to supply, deploy, expose, scale, secure,
   observe, update, and recover applications on CPU/GPU infrastructure; and
2. an AI team must be able to discover governed models, obtain scoped access,
   route across local and external capacity, enforce limits, and attribute
   every request without exposing credentials or payloads.

Those needs are already covered by A3S bounded contexts. Adding another API,
controller, scheduler, gateway, identity store, usage ledger, catalog, or
console-owned authority would duplicate a mechanism rather than add a
capability.

## 2. One abstraction stack

Every capability in this document compiles through the same stack:

```text
tenant intent in one owning bounded context
  -> immutable revision + authorization + idempotency
  -> Operation and A3S Flow for durable coordination
  -> Execution Task or managed Workload Service
  -> Workloads placement/scaling + Fleet Claims and node commands
  -> A3S Runtime Task/Service contract
  -> A3S Box execution
  -> exact generation-bound observations
  -> Edge complete snapshot
  -> A3S Gateway request path
```

PostgreSQL is the desired-state and durable-operation authority. The shared
S3-compatible service is the immutable-byte authority. Hosted Git, OCI, A3S
Use, and model supply retain different identities and formats. A3S Gateway is
the only public ingress. A3S Cloud ships no management Dashboard. Tenant
Agent/Application UIs hosted through `WEB0` use public Application contracts;
browser state is never business authority.

## 3. OpenShift outcome preservation

OpenShift is used as a completeness reference for an enterprise application
platform. A3S implements the following outcomes through its existing domain
model instead of reproducing Pods, CRDs, Operators, image streams, Routes, or
the Kubernetes reconciliation API.

| Core outcome | A3S-native authority and mechanism | Required gates | Duplicate mechanism forbidden |
| --- | --- | --- | --- |
| Tenant developer spaces, membership, RBAC and quotas | Organizations, Projects, Environments and Identity Grants; quota is enforced by the owning resource context and Workloads admission | `F0`, `C0.3`, `H0.5-C1` | Kubernetes namespace/project mirror, role store or console-only authorization |
| Source-to-build-to-release delivery | Assets and Sources pin source; Developer Workflows admits plans; Artifacts owns BuildRun, provenance and publication; Runtime Task/Box performs the build | `G0`, `P0`, `BX0`, `R0` | Build controller, build queue, source mirror or product-specific builder |
| Git, container images, software packages and model supply | Hosted Git, external OCI Registry, A3S Use Registry and Model/Weight Supply remain four typed supply authorities | `G0`, `U0`, `I0.2a-MS1`-`MS6` | Universal registry or treating a tag as immutable identity |
| Declarative application deployment and self-healing | Product contexts compile immutable intent to the one Workloads/Deployment model; Operations/Flow coordinates; Fleet/Runtime/Box converges | `D0`, `E0`, `H0.1`-`H0.5`, `BX0` | CRD/Operator control plane, profile reconciler or second desired-state store |
| Stateless services, jobs and serverless Functions | Managed Runtime Service, finite Execution Task and `FN0` Function profiles share the same workload substrate and autoscaling authority | `R0`, `FN0`, `H0.5` | Pod/Job/Function scheduler or FaaS-specific node channel |
| Stateful services and persistent volumes | Data/S0 owns storage intent, backup, restore and writer fences; stateful Workloads consume it | `S0`, `H0` | Product-local volume registry, backup controller or unfenced failover |
| Named collaborative durable state | Durable Cells project one ordinary shared provider Service; the provider owns per-Cell state, alarms, connections and writer epochs in one S0 namespace | `CELL0.1`-`CELL0.7` | Per-Cell Cloud rows, Cell scheduler, Cell Runtime class or Cell-specific Gateway lookup |
| Services, public routes, TLS and one-origin composition | Edge owns DomainClaim/Route/complete snapshots; Gateway terminates and routes traffic, including Web, Agent, Workflow, Function, MCP, Cell and inference targets | `E0`, `WEB0`, product route gates | Kubernetes Service/Route mirror, product proxy or direct public Runtime/S3 endpoint |
| CPU/GPU scheduling, replicas, topology, rollout and autoscaling | Workloads is the sole placement, desired-replica and autoscaling writer; Fleet owns inventory, Claims and fences; placement groups cover gang workloads | `H0.1`-`H0.5`, `I0.1`, `I0.3`, `I0.4` | GPU scheduler, inference scheduler, HPA clone or telemetry-owned mutation |
| Cataloged lifecycle automation | Product profiles and signed A3S Use packages compile through owner ports to Flow/Workloads; exact revisions support install, update, rollback and removal | `U0`, the consuming product gate, `H0.4` | Operator Lifecycle Manager, untyped extension hooks or package-owned platform authority |
| Secrets, admission and workload isolation | Identity authorizes; Secrets owns versions and scoped materialization; policy admission precedes Claims; Runtime/Box enforces process, filesystem, network and device isolation | `C0.3`, `E0`, `BX0`, `H0.5-C5` | Plaintext configuration, alternate admission webhook authority or label-only isolation |
| Logs, metrics, traces, audit and operational diagnosis | Owners emit bounded facts; Operations, Audit and authorized projections correlate them; Gateway/Fleet/Runtime observations remain typed evidence | `C0`, `E0`, `H0.5-C6`, product evidence gates | Second audit store, payload telemetry by default or metrics that directly mutate desired state |
| API, CLI and automation parity | One set of Application commands/queries is projected to REST/OpenAPI, the maintained client, CLI and Management MCP. `WEB0` hosts tenant Agent/Application UIs, not a Cloud management Dashboard | `C0`; `WEB0` only for tenant Web delivery | Cloud Dashboard, UI-owned lifecycle, private console API or browser-side filtering as authorization |
| Installation, upgrades, HA, backup/restore and disconnected operation | Digest-pinned system bootstrap, singleton migrations, split Cloud roles, replicated Gateway, exact offline supply, PostgreSQL/S3 recovery and tested rollback | `H0.4`, `S0`, `G0`, `U0`, `I0.2a` | Self-scheduling bootstrap cycle, mutable installer state or unverified offline cache |

The A3S equivalent of an Operator is therefore not a new generic controller.
It is a versioned product profile owned by its bounded context, compiled to the
common Operations/Flow/Workloads contracts, and reconciled by the existing
workers. A3S Use distributes signed cognitive capabilities; it does not gain
deployment, scheduling, identity, or storage authority.

OpenShift API, `oc`, Kubernetes YAML, Helm, CRDs, Operators, OLM, image-stream,
Route, and cluster wire compatibility are non-goals. They may later appear as
bounded import/export or infrastructure adapters only when A3S remains the
sole desired-state authority.

## 4. TokenHub outcome preservation

TokenHub is used as a completeness reference for a private enterprise model
gateway. Its outcomes are split by domain so a model gateway cannot become a
second platform control plane.

| Core outcome | A3S-native authority and mechanism | Required gates | Duplicate mechanism forbidden |
| --- | --- | --- | --- |
| External model directory separate from upstream inventory | Inference owns `Model`, immutable `ModelRevision`, `ExternalModelProvider`, provider inventory bindings and route revisions; Model Supply owns weight/source resolution for hosted models | `I0.2a`, `I0.2d`, `I0.2e` | Provider model names as public truth or a second model catalog |
| Local and external Provider resources | Inference owns typed provider/resource revisions; Secrets owns credentials; local Power capacity is an ordinary managed Workload | `PW0`, `I0.2a`, `I0.2d`, `I0.5` | Provider-native desired state, credentials in routes or an inference scheduler |
| Unified model APIs and capability discovery | Gateway implements closed, versioned `InferenceProtocolProfile` contracts and filters model discovery from effective access plus healthy eligible routes | `I0.2b`, optional `I0.6` | Generic untyped byte proxy or advertised capability without conformance |
| Project/environment API keys and one-time reveal | Identity owns credential identity, verifier, scope, expiry, rotation and revocation; Gateway receives only compiled verification policy | `C0.3`, `I0.2b`, `I0.2e` | Inference-local user store, recoverable plaintext key or browser-held provider key |
| Enterprise sign-in, team roles and least-privilege model access | Identity owns OIDC subjects, Principals, Memberships and Grants; Inference owns model-access revisions. Effective access is the intersection of organization, project, environment and credential restrictions; restricted-empty denies all | `C0.3`, `I0.2b`, `I0.2e` | Role encoded in a key, UI-mode authorization or lower-scope privilege expansion |
| Scoped routing policy | Inference resolves exactly one immutable effective policy by credential, environment/project, then organization/global precedence. An invalid higher scope fails closed and never falls through to a broader policy | `I0.2b`, `I0.2d`, `I0.5` | Gateway-owned management state, mutable provider routing or implicit fallback to broader access |
| Priority, weighted allocation, sequential fallback and route constraints | Inference compiles eligible candidates after access, provider/resource/tag/region/environment constraints; Gateway executes the frozen selection/fallback plan | `I0.2b`, `I0.2d`, `I0.3`, `I0.5` | A second proxy, request-time database policy discovery or fallback that re-adds an excluded route |
| Session/cache affinity and provider recovery | Gateway applies bounded revision-scoped affinity; typed health observations drive cooldown and half-open probes, but never desired replicas or policy writes | `I0.3`, `I0.5` | Sticky state without expiry, health-owned configuration mutation or retry after response bytes |
| Quota, rate and concurrency control | Inference owns typed policy; Gateway enforces local bounds and the separately gated exact distributed limiter; request leases are always released or visibly recoverable | `I0.2b`, `I0.5`, `H0.5` | Key-local counters as truth, silent fail-open or quota logic in each provider adapter |
| Request logs, usage analytics and attribution | Gateway emits prompt-free request/attempt facts; Inference owns one durable usage ledger; Projects supplies immutable attribution/cost-center references | `I0.2c`, `I0.2e` | Provider invoices as usage truth, prompts/responses in management telemetry or second ledger |
| Diagnostics, API documentation and test traffic | Authorized queries expose route health, applied revisions, sanitized failures, usage completeness and contract examples through API/client/CLI/Management MCP. Independently developed tenant UIs may call the same public Gateway through `WEB0` | `I0.2e`; `WEB0` only for tenant Web delivery | Cloud Dashboard, private console backend, persistent browser prompt store or diagnostics that reveal Secrets/other tenants |
| Content-security and guardrail policy | Inference owns immutable inspection policy and approved guardrail-model binding; Gateway applies bounded pre/post checks and records redacted decisions | `I0.5` | Provider-specific hidden moderation, payload retention by default or policy mutation from telemetry |
| Protocol/provider breadth and approved subscription channels | Each Responses, Anthropic Messages, media, custom-upstream or subscription channel is a separate credential-isolated conformance profile | optional `I0.6` | Template presence as certification or one adapter with unrestricted protocol forwarding |
| HA, backup, observability and rollout | The common Gateway, PostgreSQL, Secrets, Operations, Workloads and H0 lifecycle gates apply; inference adds only protocol/usage/cache-specific evidence | `I0.5`, `H0.4`, `H0.5`, `S0` | TokenHub-specific deployment database, updater, monitor or backup mechanism |

Commercial prices may be recorded as immutable reference metadata for
showback and quota calculation. Balance, checkout, invoice, tax, settlement,
payment and commercial entitlement remain an external bounded service. They
must consume authorized exports rather than mutate the Inference usage ledger.

## 5. DDD ownership and policy flow

The model request path preserves aggregate boundaries:

```text
Identity credential + effective Grants
  -> Inference model-access and scoped routing revision
  -> Edge route binding and complete Gateway snapshot
  -> Gateway authentication / limits / protocol validation
  -> frozen candidate filter and attempt plan
  -> local Power target or credential-isolated external Provider
  -> prompt-free request and attempt facts
  -> Inference usage ledger + authorized C0 projections
```

Gateway owns request-path execution but no management aggregate. Inference owns
model, Provider, policy and usage decisions but no identity, Secret, workload,
node, object, or route aggregate. Workloads owns desired capacity but never
chooses an inference request target. This separation is what prevents the
OpenShift-style platform outcomes and TokenHub-style gateway outcomes from
creating competing controllers.

## 6. Capability closure gate

OpenShift- and TokenHub-inspired product claims are allowed only when:

1. the relevant matrix row maps to exactly one decision/data authority;
2. accepted ACL and immutable revisions exist for every cross-process contract;
3. all work uses the common Flow, Workloads, Fleet, Runtime and Box path;
4. all public traffic uses an acknowledged complete Gateway snapshot;
5. authorization, quota, audit, failure, recovery and cleanup tests use real
   providers at exact revisions;
6. REST/OpenAPI, maintained client, CLI and Management MCP project the same
   Application behavior, while tenant UIs use only those public contracts; and
7. ROADMAP reports the capability as unavailable until all named gates pass.

Deleting the reference product name does not delete these outcomes. Changing
an owner or replacing a shared mechanism requires an explicit architecture
decision, data migration, compatibility window and recovery evidence.
