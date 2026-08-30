# A3S Cloud Platform Completeness Review

## 1. Executive conclusion

**Review baseline: 2026-08-28.**

A3S Cloud no longer lacks a coherent product thesis. The target already covers
AaaS, WaaS, FaaS, first-class Durable Cell collaboration, local/distributed
inference, static Web delivery, object/model/package/code supply, tenancy,
system administration, CPU/GPU scheduling, autoscaling, and Gateway-only
external access.

The remaining work falls into two materially different categories:

1. **delivery gaps:** a target and owner exist, but complete real-provider,
   multi-replica, failure, recovery, cleanup, upgrade, and release evidence
   does not; and
2. **structural gaps:** a necessary platform outcome is still fragmented or
   lacks a canonical aggregate, owner, contract, or gate.

Creating a new subsystem for a delivery gap would be harmful. Structural gaps
require a design decision before implementation.

## 2. What is not missing

The following capabilities have a target owner and architecture. They remain
unavailable wherever their roadmap gate is not `Verified`, but they should be
completed through the existing path rather than redesigned as new mechanisms.

| Capability | Existing canonical path | Remaining class of work |
| --- | --- | --- |
| Agent as a Service | Agents -> Code -> Runtime Service -> Box | Provider, checkpoint, session-fence, rollout and recovery certification |
| Workflow as a Service | Workflow -> Flow -> owner node ports | Heterogeneous-node, replay, failure and operational evidence |
| Function as a Service and sessionless MCP | Functions -> Runtime Task/Service or external connector -> Gateway | Hosted/external conformance, activation, overload and scale-to-zero evidence |
| Durable Cell | Durable Cells -> ordinary Workload Service -> Runtime/Box -> S3 provider | Named-state, writer-fence, managed Gateway and full fault evidence |
| Local/distributed inference | Inference/Fleet -> Power -> Gateway | Model/weight supply, GPU topology, KV transport, routing, usage and chaos evidence |
| Static React/Vue delivery | Developer Workflows/Artifacts -> immutable object release -> Gateway | Build, CSP/cache, route, rollback and framework matrix |
| Git, OCI, Use and model supply | Four separate typed authorities | Production providers, signing, recovery and lifecycle operations |
| Multi-tenancy and administrator RBAC | One explicit Installation/Organization/Project/Environment scope plus Identity-owned platform roles and bounded support grants | `C0.5-MT1-C1/C2` provide canonical role/grant intent and replayable privileged-decision evidence; `MT1-C3` provides the canonical persisted Installation and shared scoped Audit/Outbox rail; verified `MT2-C1/C2` add the sole policy/binding and actual support-approval/grant repositories with database-backed recovery and evidence invariants. Fresh bootstrap atomically creates the tenant identity and matching baseline `PlatformOwner` through one dedicated Identity port and the shared platform writer, with main PostgreSQL recertification complete. Verified `MT2-C3` atomically share-locks current Principal/credential/policy/binding/grant state and commits the exact allow through shared Audit; all seven protected RBAC/support mutations, maintained REST/OpenAPI/client/CLI/Management MCP interfaces, and installation-wide organization catalog reads use that authority without an ambient platform-role claim. A valid exact `cloud:read` credential without `TenantLifecycleRead` sees only its own Organization. Controlled recovery for older rootless installations, the broader system/organization-role matrix, owner-port cleanup, complete scope enforcement, and hostile-tenant evidence remain |
| Distributed API consistency | Idempotency/CAS/PostgreSQL transaction/Operation/Outbox plus bounded Redis/Lane acceleration | Repository-wide adoption and multi-replica fault certification |
| CPU/GPU placement and elasticity | Workloads desired state plus Fleet Claims and one scaling authority | H0.3/H0.5 real-cluster scale, drain, state and failure gates |

## 3. Structural gaps

### 3.1 P0: unified Runtime CI/CD

**Gap:** source, BuildPlan, BuildRun, release, Workload and rollout foundations
exist, but there was no first-class pipeline authority spanning all Runtime
profiles and Cloud's own system services.

**Decision:** add the Delivery Pipelines bounded context and `CD0`, backed by
A3S Flow and owner receipts. Reuse Artifacts, product Releases, Workloads,
Fleet, Runtime, Box, Edge, Gateway, Operations, Outbox and Audit. Never rebuild
between Environments.

**Design:** [Runtime CI/CD architecture](runtime-cicd-architecture.md).

### 3.2 P0: workload identity and east-west service connectivity

**Gap:** node control has outbound mTLS and product deployments carry workload
identity fields, but there is no complete workload-attestation, short-lived
service identity, trust-domain, peer authorization, internal discovery, mTLS
rotation, or federation contract.

**Required design:**

- Identity owns `WorkloadIdentityPolicy` and trust-domain/federation intent;
- Fleet attests nodes; Runtime/Box attests an exact Unit generation;
- Secrets/PKI issues short-lived identity documents through a provider port;
- Workloads/Runtime observations are the internal endpoint source;
- the consuming product owns peer authorization and egress intent;
- a connectivity compiler produces generation-bound network/mTLS attachments;
  and
- Gateway remains external ingress and does not become an east-west mesh
  management plane.

Use a standards-compatible workload-identity provider where practical. The
architecture should consume short-lived identities through a bounded port,
not make a particular provider's database Cloud truth.

**Proposed gates:** `H0.4-WI1` through `WI7`: identity/trust contract, node and
Unit attestation, issuance/rotation, peer policy, service discovery, failure
and revocation, federation/upgrade conformance.

**Implementation progress:** `H0.4-WI1-C1` plus the local `WI1-C2` persistence
and management slices establish Identity-owned strong IDs, two canonical ACL
contracts, deterministic immutable revisions, predecessor-fenced PostgreSQL
heads, direct reuse of Runtime's `Task`/`Service` and isolation types, atomic
Installation-scoped authorization/idempotency/Audit/Outbox, and one
inspection-only replaceable provider port with explicit declared-versus-observed
evidence provenance. The first
`cloud.identity.workload-provider.v1` profile and API-only
`spiffe_https_web` adapter add exact-digest configuration plus strict, bounded,
real-TLS SPIFFE JWK-bundle observation without a provider registry or issuance
path. REST/OpenAPI `1.80.0`, TypeScript client, CLI, and ten Installation-bound
Management MCP tools share the same closed CQRS. The retained H0 PostgreSQL job
and local 7/7 provider protocol gate pass; complete main certification remains
pending. This closes the ambiguous contract, management, and provider-observation
boundaries but does not close the P0 gap: Fleet/Runtime attestation, issuance,
private discovery, enforcement, failure/revocation evidence, and federation
certification remain absent.

### 3.3 P0: platform observability, SLO and incident lifecycle

**Gap:** many contexts emit excellent local evidence, but the platform lacks
one complete operational contract for correlation, telemetry loss, service
level objectives, burn-rate alerts, incident declaration, ownership, timeline,
mitigation, resolution, and post-incident evidence.

**Required design:**

- owner logs and histories remain authoritative;
- OpenTelemetry carries correlation; Observer supplies kernel evidence;
- Prometheus-class storage serves bounded real-time metrics;
- object storage retains immutable evidence/log chunks;
- optional Apache Doris serves high-cardinality log/trace/usage analytics as a
  rebuildable projection with watermarks;
- Notifications delivers alerts but does not own incidents; and
- a small Operations-owned `Incident` aggregate links exact owner evidence and
  operator actions without copying every signal.

**Proposed gates:** `H0.5-OBS1` through `OBS8` for signal contracts,
correlation, loss accounting, SLOs, alerts/incidents, retention/redaction,
Doris projection, and chaos/operations evidence.

### 3.4 P0: platform installation, upgrade and disaster recovery closure

**Gap:** `H0.4` defines process roles, the terminating migrator, middleware,
bootstrap and some access gates, but HA placement, dependency recovery,
credential rotation, retained upgrade/rollback, and whole-platform restore are
not verified.

**Required outcome:** a signed system release graph with expand/migrate/contract
database rules, role-aware rolling order, migration fences, compatibility
window, backup set, restore order, RPO/RTO, automated halt, rollback boundary,
and disconnected supply. Cloud system services must pass through `CD0` rather
than a privileged parallel updater.

**Owner/gate:** existing `H0.4`, `S0`, `CD0`, and distribution gates; no second
Operator control plane.

### 3.5 P0: API and ecosystem compatibility governance

**Gap:** OpenAPI versions and several local compatibility checks exist, but the
whole ecosystem lacks one enforced policy for wire/schema/storage deprecation,
Runtime/Box/Gateway/Flow/Use version skew, client support windows, migration
decoding, and exact compatibility locks.

**Required design:**

- every public API group and event/ACL schema declares stability and supported
  revisions;
- removal requires a major/versioned boundary and telemetry-backed notice;
- persisted data remains decodable or explicitly migrated before removal;
- the A3S root distribution publishes an exact compatible component graph;
- provider conformance runs against minimum/current/next compatible versions;
  and
- CI rejects undocumented breaking changes.

**Proposed gates:** `C0.4-COMP1` through `COMP6` for policy, schema registry,
contract diff, mixed-version suites, deprecation evidence and distribution
lock.

### 3.6 P0: policy admission and signed policy distribution

**Gap:** policy exists in several rightful bounded contexts, but there is no
uniform mechanism for compiling, signing, distributing, acknowledging,
revoking, and diagnosing complete policy snapshots at Gateway, Runtime/Box,
Observer/Sentry, and build workers.

**Decision:** do not create one omniscient policy database or general policy
language. Each bounded context continues to own its canonical ACL and decision
semantics. A shared projection mechanism may distribute typed, signed,
generation-bound bundles and receipts.

**Proposed gates:** `H0.5-POL1` through `POL5`: envelope, compiler ports,
atomic application/acknowledgement, expiry/revocation, and cross-data-plane
failure evidence.

### 3.7 P1: platform-wide usage, cost and budget governance

**Gap:** inference has a usage-ledger design and Projects can store attribution
references, but there is no normalized platform usage contract spanning CPU,
GPU, memory, object bytes/requests, egress, builds, Functions, Agents,
Workflow activities, Cells, models and retained evidence.

**Required design:** product owners emit immutable usage facts; one Usage
context validates, deduplicates and allocates them to immutable attribution
profiles; Doris may provide analytical projections; quotas and budget alerts
consume closed summaries. Commercial price books, balance, checkout, invoice,
tax, settlement and entitlement stay outside Cloud core.

**Proposed gates:** `C0.5-UG1` through `UG7`: fact profiles, ledger, late data,
allocation, showback, budgets/alerts, export and reconciliation.

### 3.8 P1: data governance, residency and customer-held keys

**Gap:** retention, audit, Secrets, object namespaces and later BYOK/residency
references exist, but classification and residency are not uniformly carried
through source, artifact, model, checkpoint, log, backup, cache and analytics
projections.

**Required design:** immutable `DataPolicyRevision` values define
classification, allowed regions/providers, encryption/key binding, retention,
legal hold, export and deletion. Owners enforce the policy at admission and
carry its digest into every derived manifest. Node/cache placement must not
move bytes outside the policy. Deletion uses owner tombstones and evidence,
not direct multi-store best effort.

**Proposed gates:** `C0.5-DG1` through `DG7` composed with `S0`, Secrets,
Model Supply, Artifacts, Files, Audit and observability.

### 3.9 P1: AI assurance and abuse-resistance lifecycle

**Gap:** Sentry, inference guardrails, Bench and `EV0` provide pieces, but no
complete release gate combines Agent/tool prompt-injection tests, model safety,
policy evaluation, red-team corpora, regression thresholds, human approval,
production feedback and emergency halt.

**Required design:** Bench owns reproducible evaluation mechanics; the target
product owns acceptance thresholds; Evolution owns authorized evidence
datasets and candidate/promotion semantics; Delivery Pipelines enforce the
exact gate; Sentry handles runtime-security judgment; Gateway applies admitted
request guardrails. No generic safety score may silently authorize deployment.

**Owner/gate:** `EV0`, product release gates, `CD0`, Sentry and Inference.

### 3.10 P1: multi-region and sovereignty model

**Gap:** failure-domain and independent-Gateway requirements exist, but region
identity, control-plane/data-plane placement, write-home policy, regional
failover, object/database replication, trust federation, data residency and
conflict semantics are not closed.

**Required sequence:** first certify one HA region. Then define a Cell-based
regional architecture in which each product aggregate has one write home or an
explicit merge model. Gateway may route regionally only from complete admitted
snapshots. Region loss must never create two writers for Agent Sessions,
Durable Cells, Workflow histories, deployment claims or usage ledgers.

**Proposed gates:** `H0.6-R1` through `R7`; `H0.6` remains deferred until
single-region `H0.4/H0.5` is verified.

### 3.11 P1: day-two operations without a Dashboard

**Gap:** the absence of a Cloud Dashboard is intentional, but it increases the
contract burden on APIs, CLI, SDKs and Management MCP. Some lifecycle commands
and diagnostic timelines remain product-specific or incomplete.

**Required outcome:** install/upgrade status, capacity, topology, health, logs,
traces, usage, policy, pipeline, deployment, backup/restore, incident,
credential/key rotation, certificate, registry and garbage-collection
operations must all be discoverable and actionable through the same public
Application services with system-admin and tenant scopes kept separate.

**Owner/gate:** `C0`, `H0.4`, product gates and maintained-client parity.

### 3.12 P2: dynamic feature delivery

**Gap:** rollouts and immutable releases exist, but there is no generic dynamic
feature-flag authority.

**Decision:** do not add one to the critical path. Platform features should
prefer immutable release and rollout policy. If tenant applications later need
dynamic flags, add a separately owned capability with immutable revisions,
targeting, audit, expiry and SDK contracts; it must not control Cloud
authorization or bypass Delivery Pipelines.

## 4. Delivery gaps on the current critical path

Even after the structural decisions above, the product remains blocked by
existing unverified gates:

1. Runtime/Box/OCI Runtime exact-provider certification for all advertised
   Task/Service features;
2. `H0.3` real CPU/GPU multi-node placement, drain, network and placement-group
   behavior;
3. `H0.4` clean installation, HA middleware, upgrade, rollback and restore;
4. `H0.5` stateless/stateful/Agent/Cell/Task/inference autoscaling and
   multi-tenant overload safety;
5. AaaS/WaaS/FaaS/Cell/inference/Web end-to-end Gateway publication and
   recovery;
6. real Git/OCI/Use/model/object provider lifecycle and supply-chain evidence;
7. public API/client/CLI/Management MCP parity for every completed product; and
8. retained load, chaos, security, data-loss, cleanup and exact-revision
   release bundles.

These are not reasons to introduce replacement controllers. They are reasons
to finish the named conformance gates.

## 5. Priority recommendation

```text
P0-A  Unified Runtime consumer contract landed; finish Box certification
  -> P0-B  Establish workload identity and east-west trust
  -> P0-C  Deliver CD0 source-to-release-to-rollout
  -> P0-D  Close H0.3/H0.4/H0.5 cluster, upgrade and recovery
  -> P0-E  Close observability/SLO/incident and compatibility gates
  -> P0-F  Certify AaaS/WaaS/FaaS/Cell/Inference/Web vertical slices
  -> P1    Usage/cost, data governance, AI assurance and single-home multi-region
  -> P2    Optional dynamic feature delivery and ecosystem breadth
```

The dependency order is deliberate. Shipping more product kinds before the
trust, delivery, operations and compatibility foundations would multiply the
same risk across every Runtime profile.

## 6. External design references

- [SPIFFE workload identity and Workload API](https://spiffe.io/docs/latest/spiffe-specs/spiffe_workload_api/)
- [SPIFFE federation](https://spiffe.io/docs/latest/spiffe-specs/spiffe_federation/)
- [SLSA specification 1.2](https://slsa.dev/spec/v1.2/)
- [Kubernetes API deprecation policy](https://kubernetes.io/docs/reference/using-api/deprecation-policy/)
- [OpenShift 4.20 architecture](https://docs.redhat.com/en/documentation/openshift_container_platform/4.20/html-single/architecture/index)

These are outcome and contract references, not compatibility requirements.
A3S retains its own APIs, ACL configuration, domains, scheduler, Runtime, and
deployment model.
