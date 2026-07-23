# A3S Cloud and A3S Gateway Product Roadmap

## 1. Purpose and authority

This document is the product-positioning and ownership source of truth for the
A3S Gateway and A3S Cloud relationship.

- A3S Cloud is the self-hosted control plane for applications, agents, and
  model-serving workloads.
- A3S Gateway is the AI traffic and protocol data plane for standalone and
  A3S Cloud-managed deployments.

The plan is gate-driven, not date-driven. It reuses the existing `E0`, `H0`,
`I0`, `C0`, and `A0` gates; it does not introduce a parallel milestone system.
The Cloud [development plan](docs/development-plan.md) owns platform delivery
details, the [inference plan](docs/inference-plan.md) owns model-serving contracts,
and the Gateway
[roadmap](https://github.com/A3S-Lab/Gateway/blob/main/ROADMAP.md)
owns Gateway-local execution details. If those documents disagree about which
product owns a decision, this document controls the product boundary.

Capability statements use three meanings:

| State | Meaning |
| --- | --- |
| Verified | The owning real-provider, recovery, and release gates pass |
| Experimental | An implementation exists, but its production exit evidence is incomplete |
| Planned | The capability is unavailable until its named gate passes |

## 2. Product positioning

### 2.1 A3S Gateway

**Positioning:** AI traffic and protocol data plane for standalone and A3S
Cloud-managed deployments.

Gateway is for operators who need one small, ACL-configured binary at the
traffic boundary. It accepts connections, enforces a complete local policy
snapshot, selects only eligible healthy endpoints, and relays long-lived AI
traffic without placing the Cloud API in the request path.

Gateway owns:

- HTTP/1.1, HTTP/2, SSE, WebSocket, gRPC, TCP, UDP, and TLS transport;
- host, path, header, method, and SNI routing;
- streaming, timeout, retry-before-response, drain, and connection behavior;
- active and passive endpoint health used for immediate local suppression;
- load balancing among endpoints allowed by the applied snapshot;
- OpenAI request parsing and model dispatch after `I0.2b`;
- cached authorization and rate-policy enforcement after `I0.2b`;
- a durable local request and attempt usage spool after `I0.2c`;
- telemetry emission and exact applied-revision reporting; and
- validation and atomic application of complete versioned ACL snapshots.

Gateway does not own:

- organizations, projects, environments, memberships, or durable grants;
- model, provider, deployment, or credential catalogs;
- plaintext inference or provider credential storage;
- production desired replica count, placement, rollout, or autoscaling;
- the long-term usage ledger, aggregation, showback, or billing; or
- a second Cloud management UI, API, scheduler, or reconciliation database.

### 2.2 A3S Cloud

**Positioning:** self-hosted control plane for applications, agents, and
model-serving workloads.

Cloud is for platform operators who need durable desired state, self-service
management, workload convergence, release safety, and governance on
operator-owned infrastructure. PostgreSQL is authoritative for desired state;
workers and node agents reconcile Runtime resources and Gateway snapshots until
the exact requested state is observed.

Cloud owns:

- organizations, projects, environments, identity, membership, and grants;
- applications, Agents, MCP assets, models, immutable revisions, providers,
  and Secret references;
- managed Workloads, inference deployments, desired replica count, placement,
  rollout, and the sole production autoscaling evaluator;
- model aliases, weighted and fallback policy, endpoint grants, and limits;
- domain claims, TLS intent, logical Gateway scopes, complete snapshots, and
  exact acknowledgement projection;
- usage ingestion, deduplication, gaps, retention, ledger, rollups, and
  showback;
- durable operations, reconciliation, audit, API, CLI, management MCP, and web
  surfaces; and
- production installation, upgrades, high availability, and recovery.

Cloud does not own:

- per-request proxying or provider-byte forwarding;
- SSE framing, WebSocket relay, or live protocol translation;
- synchronous authorization callbacks from the Gateway hot path;
- prompt, completion, or model bytes in PostgreSQL; or
- commercial prices, balances, invoices, settlement, or managed-service plans.

## 3. Two Gateway operating modes

The Gateway product model has two deliberately separate operating modes.

| Concern | Standalone Gateway | Cloud-managed Gateway |
| --- | --- | --- |
| Desired-state authority | Operator-owned local ACL configuration | A3S Cloud PostgreSQL domain state |
| Configuration delivery | Startup file and optional local providers or file watch | Complete versioned ACL snapshot delivered through the node agent |
| Apply authority | Local operator | Cloud Edge command; Gateway validates and applies |
| Target discovery | Explicit local configuration or supported local provider | Cloud-compiled complete target set only |
| Replica count and placement | External operator or orchestrator | Cloud Workloads |
| Rollout | Static operator-supplied weights; local automation remains experimental until separately proven | Cloud Inference, Workloads, and Edge |
| Autoscaling | Optional local experiment, never implied to be production-ready | Cloud H0 Workloads autoscaler only |
| Durable business state | None | Cloud |
| Failure posture | Preserve last valid local snapshot | Preserve last acknowledged snapshot; fail closed when an expiring security snapshot is no longer valid |

Cloud-managed mode must be explicit and enforceable. It must reject
configuration that enables local discovery, local rollout, or a Gateway-owned
autoscaling controller. A managed Gateway may make temporary local health and
circuit-breaker decisions, but it may never add an endpoint, change desired
weight, create a replica, or promote a revision outside the applied snapshot.

A minimal node-local bootstrap ACL may bind process, management-listener,
identity, and Cloud-delivery settings. It cannot define or mutate managed
traffic routes, target sets, rollout, or scaling policy.

Manual mutation of a managed Gateway is not an ordinary workflow. A
break-glass replacement must produce visible divergence, preserve an audit
record outside Gateway, and be reconciled or superseded by Cloud before the
instance returns to ready service.

Standalone mode remains valuable and independent. It does not require Cloud,
but its local features must not be described as Cloud-equivalent orchestration.

## 4. Single-authority matrix

| Decision or fact | Desired-state authority | Data-plane executor or observer | Durable observed truth |
| --- | --- | --- | --- |
| Tenant, project, environment, principal, and grant | Cloud Identity and Projects | Gateway enforces a compiled subset | Cloud PostgreSQL |
| Workload revision and desired replica count | Cloud Workloads | Runtime providers create and run units | Cloud Workloads/Fleet projections |
| Placement and resource claims | Cloud Workloads | Node agent and Runtime enforce exact bindings | Cloud PostgreSQL plus fenced observations |
| Model alias, rewrite, target order, and fallback | Cloud Inference | Gateway inference dispatch | Cloud Inference route revision |
| Eligible replica endpoints and desired weights | Cloud Edge from Workloads/Inference facts | Gateway selects within the target set | Cloud Edge applied-state projection |
| Immediate endpoint suppression | Applied Gateway policy | Gateway active/passive health | Gateway telemetry; never a new desired target |
| Production rollout and promotion | Cloud Inference/Workloads/Edge | Gateway applies published weights | Cloud operation and rollout generation |
| Production autoscaling | Cloud H0 Workloads autoscaler | Gateway emits bounded signals and buffers only when policy allows | Cloud autoscaling decision and desired count |
| Inference-key lifecycle | Cloud Identity and Inference | Gateway validates a projected verifier/grant | Cloud credential and revocation generation |
| Domain and certificate intent | Cloud Edge | Gateway keeps the node-local private key and terminates TLS | Cloud claim/certificate state plus exact Gateway acknowledgement |
| Request and attempt usage | Cloud defines the contract | Gateway appends the durable local spool | Cloud immutable usage ledger after ingestion |
| Long-term metrics, audit, and showback | Cloud | Gateway emits bounded telemetry | Cloud-selected telemetry and PostgreSQL stores |
| Active traffic snapshot | Cloud Edge in managed mode | Gateway validates and atomically applies | Exact Gateway ID, revision, digest, and acknowledgement |

## 5. Cross-product runtime contract

### 5.1 Desired-state flow

```text
Cloud command
  -> commit versioned desired state
  -> compile one complete Gateway-scope ACL snapshot
  -> deliver a command through the outbound node agent
  -> Gateway validates the complete snapshot
  -> Gateway atomically applies it or preserves the previous revision
  -> node agent records the exact Gateway ID, revision, digest, and result
  -> Cloud advances only after the matching acknowledgement
```

Partial patches are not authoritative configuration. Snapshot composition is a
Cloud compiler concern; transactionally applying the resulting bytes is a
Gateway concern.

### 5.2 Request flow

```text
client
  -> Gateway TLS and protocol handling
  -> cached authorization and route evaluation
  -> healthy endpoint selection
  -> local Runtime backend or credential-isolated provider egress Workload
  -> streaming response
```

The Cloud API, PostgreSQL, and workers are never synchronous request
dependencies. A Gateway must receive complete, bounded, and expiring security
state before serving. It fails closed when policy requires an unavailable or
expired authorization snapshot.

Retry and fallback are permitted only before the first response byte. Each
attempt has a stable identity because an upstream may consume resources even
when a later fallback succeeds. An established stream follows its bounded
timeout and explicit emergency-abort policy.

### 5.3 Feedback flow

Gateway emits:

- exact snapshot application results;
- endpoint and protocol health;
- bounded, low-cardinality operational metrics;
- request and attempt usage batches with contiguous sequence
  acknowledgements; and
- version and readiness information.

Cloud may use those signals to make later desired-state decisions. Gateway
telemetry never mutates desired state directly.

## 6. Coordinated delivery plan

The existing Cloud roadmap is the only milestone vocabulary. Detailed
requirements remain in the owning development and inference plans.

The verified `R0` through `E0` chain remains the shared foundation. `G0` is in
progress; later gates remain planned unless their owning evidence table says
otherwise.

Cloud continues to deliver its broader post-`E0` portfolio without making
Gateway own those control-plane concerns:

| Cloud lane | Product outcome | Gateway involvement |
| --- | --- | --- |
| `G0` and `P0` | Source delivery, builds, previews, monorepos, and project import | Route the resulting ordinary Workloads through the verified `E0` contract |
| `C0` | REST, CLI, management MCP, grants, search, audit, notifications, and exec | Report bounded operational state; do not add a parallel business API |
| `A0` | Agent, MCP, and Skill release catalog over the common deployment path | Add native traffic protocols only when a closed data-plane contract requires them |
| `S0` | Databases, volumes, fencing, backup, and restore | No storage orchestration; proxy only explicitly published service endpoints |
| `H0` | Replicas, placement, private networking, Gateway HA, and autoscaling | Apply complete targets, expose exact readiness, drain safely, and emit scaling signals |
| `I0` | Model serving, authorization, routing, usage, providers, and self-service | Implement inference dispatch, local enforcement, streaming, fallback, and the durable spool |

The joint delivery path is:

| Gate | Cloud delivery | Gateway delivery | Joint exit evidence |
| --- | --- | --- | --- |
| `E0` | Edge desired state, managed TLS, complete snapshot publication, and exact acknowledgement | ACL validation, atomic reload, HTTPS termination, routing, health, and prior-revision preservation | Verified clean-host A-to-B-to-cloned-A route flow through the real Gateway; preserve as a regression gate |
| `I0.0` | Versioned accelerator and node contracts plus mixed-version negotiation | Preserve existing traffic snapshot behavior while Cloud node contracts evolve | An old node agent continues CPU service and Gateway delivery while supported versions negotiate safely |
| `H0.1` | Managed-owner, replica identity, generic claims, and fencing foundation | Maintain snapshot compatibility; do not infer replica ownership | Replay cannot create a duplicate Runtime unit or make Gateway invent a target |
| `I0.1` | Single-node accelerator inventory, claims, Runtime enforcement, and recovery | No public inference route yet | Real device enforcement and recovery pass without exposing a model endpoint |
| `I0.2a` | Model catalog, cache, backend compiler, private healthy inference Workload | No public inference route yet | A real backend passes health and failure recovery while remaining unreachable from a public Gateway |
| `H0.2` | Logical Gateway scopes, cardinality-one complete target sets, private endpoints, and exact acknowledgement | Explicit managed mode, mode-specific validation, exact applied status, and rejection of local control loops | Restart and rejected reload preserve the previous target set; no stale or cross-environment endpoint becomes eligible |
| `I0.2b` | Inference routes, environment keys, grants, limits, TLS binding, and complete dispatch snapshots | Native OpenAI dispatch, body/model parsing, cached verifier/grant enforcement, streaming, and pre-first-byte fallback | Real SDK tests prove the closed endpoint matrix, denial non-enumeration, revocation, framing, fallback, disconnect, and exact route acknowledgement |
| `I0.2c` | Usage ingestion, deduplication, gap recovery, immutable ledger, rollups, rollout authority, and operations | Durable request/attempt spool, ordered upload, backpressure, and applied weight execution | Crash and replay leave every started request terminal or visibly unknown; a failed candidate never replaces the prior healthy revision |
| `I0.2d` | Same-environment external-provider egress Workload, Secret binding, model rewrite, and provider policy | Route only to the credential-isolated internal egress target | Client and provider credentials never cross or enter snapshots, logs, traces, or usage facts |
| `C0.3` + `I0.2e` | Principal grants, authorized search, key lifecycle, role-focused console, diagnostics, playground, and usage showback | Expose only bounded operational status needed by Cloud; do not add business-state management | Consumer, steward, and operator fixtures cannot discover or mutate ungranted resources through any surface |
| `A0` + `C0` | Agent and MCP release catalog plus common deployment and management surfaces | Add native MCP or agent-protocol data-plane behavior only against a closed protocol contract; keep management MCP in Cloud | Real session, authorization, drain, and recovery evidence passes without creating a second asset or identity model |
| `H0.3` | Multi-node replicas, drain, cluster-private endpoints, and independently placed Gateway scopes | Identity-bound private upstream connections and bounded drain behavior | Serving-node loss removes the target before replacement activation; Gateway is outside the serving-node failure domain |
| `H0.4` | Production packaging and HA for API, workers, dependencies, and replicated Gateways | Per-instance exact-revision readiness, mixed-version compatibility, graceful replacement, and recovery | Only exact-revision-ready instances receive external traffic; loss and rolling upgrade preserve configured readiness |
| `H0.5` | Sole production autoscaling controller, quotas, stabilization, load limits, and disaster recovery | Complete and age-stamped load signals plus bounded cold-start buffering; no managed autoscaler | Stale, missing, duplicate, and bursty metrics remain safe without a competing scaling path |
| `I0.5` | Inference HA, quota, cache-pressure, provider breadth, and disaster gates | Gateway loss, revision skew, backlog, protocol load, and fail-closed security hardening | Mixed versions, Gateway loss, usage backlog, and restore pass the published production limits |

No gate is complete because one repository passes unit tests in isolation. A
joint gate pins compatible Cloud and Gateway revisions and exercises the real
cross-repository protocol.

## 7. Immediate implementation order

### 7.1 Gateway baseline correction

Before claiming `I0.2b`, Gateway should:

1. publish the standalone and managed mode contract in its public API and ACL
   validation behavior;
2. reject local providers, rollout controllers, and autoscaling controllers in
   managed mode;
3. complete structured per-request access-log emission on every terminal
   request path or stop advertising it as available;
4. label the current local autoscaler experimental until real in-flight
   measurement, typed executor selection, provider conformance, and recovery
   pass;
5. keep the parsed but inert local rollout configuration explicitly
   unavailable, or implement and certify it only for standalone mode;
6. preserve the optional wire firewall as a separate single-upstream profile,
   not describe it as native MCP or Cloud inference dispatch; and
7. add mode-isolation, rejected-snapshot, prior-revision, drain, and
   cross-version contract tests.

These corrections do not create a new roadmap gate. They make current product
claims truthful and prepare the `H0.2` and `I0.2b` contracts.

### 7.2 Cloud control-plane sequence

Cloud should follow the existing dependency order:

1. preserve the verified `E0` snapshot and acknowledgement path;
2. land `I0.0` versioned contracts with `H0.1` managed replica and claim
   foundations;
3. complete `I0.1` accelerator enforcement and `I0.2a` private single-node
   backend serving;
4. land `H0.2` logical Gateway scopes, private targets, and managed-mode
   snapshot constraints;
5. deliver `I0.2b`, then `I0.2c`, then `I0.2d`;
6. combine `C0.3` with `I0.2e` for governed self-service;
7. reuse `A0` and `C0` identity, catalog, and deployment contracts for Agent
   and MCP products;
8. advance through `H0.3`, `H0.4`, and `H0.5`; and
9. close inference production evidence in `I0.5`.

Cloud must not implement a temporary request proxy, Gateway-side business
database, or second autoscaler to shorten that sequence.

## 8. Cross-repository definition of done

A coordinated slice is done only when:

- one product is named as authority for every new decision and durable fact;
- managed mode has no second config, rollout, placement, or autoscaling writer;
- complete snapshots are canonical, digest-addressed, bounded, validated, and
  acknowledged by exact Gateway identity and revision;
- rejected, stale, partial, and mixed-version snapshots preserve the last
  proven state or fail closed according to explicit policy;
- Cloud process loss and unavailability do not interrupt an already authorized
  request path;
- Gateway process loss, restart, and replacement do not produce an untracked
  active revision;
- streaming, timeout, disconnect, drain, retry, and fallback behavior pass real
  protocol conformance;
- secrets, prompts, and responses are absent from snapshots, logs, traces,
  operations, audit, and durable Cloud state unless an explicitly owned future
  feature says otherwise;
- usage replay is idempotent and every gap remains visible;
- standalone behavior remains usable and is tested independently from managed
  behavior;
- README, examples, API documentation, and roadmap state describe only the
  evidence that passed; and
- compatible Cloud and Gateway revisions are recorded by the release gate.

## 9. Product non-goals

The coordinated roadmap does not include:

- putting Cloud on the live request or token-stream path;
- turning Gateway into a tenant database, scheduler, deployment engine, or
  billing service;
- allowing Gateway to create production replicas in Cloud-managed mode;
- storing provider credentials or TLS private keys in Gateway ACL snapshots;
- treating Kubernetes as a second Cloud workload scheduler;
- claiming every OpenAI, Anthropic, MCP, or A2A endpoint without a closed
  protocol and real conformance gate; or
- implementing commercial billing inside Cloud.

New capabilities enter the plan only after they have one owning product, one
existing roadmap dependency, a closed contract, and real failure and recovery
evidence.
