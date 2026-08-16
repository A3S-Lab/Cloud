# A3S Cloud Durable Cell Service Plan

## 1. Authority and status

**Status as of 2026-08-16: `CELL0.1-C1` through `CELL0.1-C3`, component-only `CELL0.2-C1/C2`, `CELL0.3-C1/C2`, and `CELL0.4-C1/C2/C3/C4/C5` are implemented; the [`CELL0.4` PostgreSQL 17 authority and C6a process-death gate](https://github.com/A3S-Lab/Cloud/actions/runs/31935793202/job/95137381473) has a retained pass, while remaining C6 stop/restart/cleanup evidence, the `CELL0.2-C3` storage gate, and the `CELL0.3-C3` runtime-only provider gate remain open; the product is unavailable.**

This document owns the detailed `CELL0` delivery contract for a managed service
similar in outcome to [Deno celld](https://github.com/denoland/celld). The root
[ROADMAP](../ROADMAP.md) remains authoritative for portfolio ordering and public
status, while [architecture.md](architecture.md) remains authoritative for
cross-component ownership.

celld is a reference implementation, not an API promise. Its documented model
combines a Worker runtime with named Durable Objects, one SQLite database per
object, object-store coordination, single-writer fencing, replication before
write acknowledgement, hibernatable WebSockets, alarms, and inactive objects
that consume almost no compute. See its official
[README](https://github.com/denoland/celld/blob/main/README.md),
[ownership and fencing](https://github.com/denoland/celld/blob/main/docs/fencing.md),
[security boundary](https://github.com/denoland/celld/blob/main/docs/security.md),
and [current limitations](https://github.com/denoland/celld/blob/main/docs/limitations.md).

A3S Cloud adopts those product outcomes through existing A3S authorities. It
does not copy celld's control topology, provider-native configuration,
deployment authority, or unauthenticated operator surface.

## 2. Product outcome

`CELL0` provides a tenant-scoped **Durable Cell Application** whose code can
address named, long-lived state entities. Each Cell:

- has one stable application-local name and one private SQLite state lineage;
- admits one writer at a time and fences every previous ownership epoch;
- handles bounded request, alarm, and WebSocket events serially within its
  execution turn;
- can leave memory when idle and later reactivate from durable state;
- preserves acknowledged writes across process or node loss; and
- moves between provider replicas without exposing placement to callers.

The first production profile is a dedicated Cell fleet per application. A
shared process may not host mutually untrusted applications until a later
provider proves hostile multi-tenant isolation. Individual Cells may become
inactive, but `CELL0` does not reinterpret that as a new Workload autoscaler or
as one Runtime Service per Cell.

Cloudflare Workers and Durable Objects behavior is the initial compatibility
target. Compatibility is capability-by-capability and test-backed; the product
does not claim full Cloudflare platform, project-tooling, or celld
compatibility merely because one provider runs a compatible bundle.

## 3. One concern, one authority

| Concern | Sole authority | Durable Cell rule |
| --- | --- | --- |
| Tenant application intent | Durable Cells context in PostgreSQL through A3S ORM | Own application identity, immutable revisions, desired release, retention policy, and exact projections only |
| Long-running control operation | A3S Flow plus Operations | Deploy, replace, stop, restore, and delete use the existing operation rail |
| Source, build, and provenance | Sources, Artifacts, `G0`, and `P0` | Worker bundles enter through one immutable build path; imported provider project metadata is only a proposal and never product truth |
| Process desired state and rollout | Workloads | One Cell application revision projects to one managed ordinary Service fleet |
| Node placement and capacity | Fleet, Node Agent journal, and Claims | Cell providers receive no scheduler, node channel, or capacity ledger |
| Provider lifecycle | A3S Runtime `Service` | No `Cell`, `Actor`, `Worker`, or `DurableObject` Runtime unit class is added |
| Local isolation and execution | A3S Box | The Cell provider is a digest-pinned service artifact hosted by Box |
| Mutable Cell bytes and per-Cell ownership | Selected Cell data-plane provider inside an S0 namespace | Cloud never mirrors SQLite, ownership leases, epochs, wake records, or peer membership in PostgreSQL |
| Object-store capability and lifecycle | `S0` immutable-object/provider contracts | One tested client and credential path supplies conditional create, conditional overwrite, and read-after-write consistency |
| Secrets and credentials | Secrets | A dedicated application/fleet binding materializes narrowly scoped provider credentials just in time |
| Public request policy and TLS | Edge intent and A3S Gateway | Gateway routes only to healthy public Service endpoints; it does not resolve Cell owners or implement sticky routing |
| Peer and operator traffic | Cell provider plus Node Agent on the private node network | The internal endpoint is never a public Route and is not directly exposed to tenants |
| Alarms | Cell provider state machine | An alarm wakes an existing Cell; it does not create an Automation, Task, WorkflowRun, queue, or Cloud timer row |
| Metrics, logs, and audit | Existing telemetry/log owners plus Cloud audit | Per-Cell names and state content are redacted or hashed; observations are projections, never authority |

These boundaries deliberately keep the specialized data-plane mechanism where
it is required while preventing it from becoming a second Cloud platform.

## 4. Topology and request flow

```mermaid
flowchart LR
    User[Management caller] --> API[Cloud API]
    API --> Cells[Durable Cells context]
    Cells --> Flow[A3S Flow / Operations]
    Cells --> Workloads[Workloads]
    Workloads --> Fleet[Fleet / Claims]
    Fleet --> Agent[Node Agent journal]
    Agent --> Runtime[A3S Runtime Service]
    Runtime --> Box[A3S Box]
    Box --> Provider[Cell provider replicas]
    Provider <--> Store[S0 object namespace]
    Provider -. private peer/operator .-> Provider

    Client[HTTP / WebSocket client] --> Gateway[A3S Gateway]
    Gateway -->|public endpoint only| Provider
    Provider -->|local or signed peer forward| Owner[Current Cell owner]
    Owner --> Store
```

Cloud applies an immutable application revision, waits for exact Runtime health
and provider storage-probe evidence, and then publishes the public endpoints
through the existing complete Gateway snapshot path. A request may reach any
healthy replica. The Cell provider resolves or forwards to the current owner;
Gateway and Cloud remain unaware of per-Cell placement.

The provider's deployment pointer, node lease, ownership record, and local
SQLite copy are applied state. The immutable Cloud revision remains desired
application authority. Out-of-band provider deployment is drift and cannot
silently replace the active Cloud revision.

## 5. Durable state and fencing contract

Every admitted provider and object-store pair must prove all of the following:

1. Conditional create admits only one initial owner.
2. Conditional overwrite rejects a stale ownership token.
3. Read-after-write returns the accepted ownership record.
4. Every activation advances a monotonic fencing epoch.
5. State written by a stale owner cannot enter the active lineage.
6. A response that acknowledges a mutation is withheld until the corresponding
   state is durably replicated and current ownership is revalidated.
7. Restore selects one sealed, immutable cut of the previous epoch.
8. Loss of object-store reachability self-fences writes instead of serving an
   uncertain owner.

`CELL0` exposes no switch that disables items 4 through 8. A provider may use
epoch-prefixed segments, snapshots, write-ahead logs, or another verified
implementation, but the observable guarantees and crash matrix are fixed.

Stopping a Cell application stops compute and preserves its state according to
retention policy. Deleting state is a separate, authorized, auditable Operation
that proves the exact application namespace and backup policy before cleanup.
Workload removal alone never implies data deletion.

## 6. Security boundary

- The first profile uses one provider fleet and one object namespace per
  Durable Cell application. Credentials are scoped to that namespace.
- Public and internal Runtime ports are distinct. Edge may publish only the
  public port; the internal port is reachable only by trusted provider peers
  and the Node Agent's typed operator adapter.
- A provider's native operator API is not a tenant API. If the provider has no
  authentication, its adapter binds it to loopback or an isolated private
  interface and authorizes every Cloud operation before local dispatch.
- Public application authentication, domain policy, TLS, request limits, and
  denial behavior remain Gateway and Identity concerns.
- Worker variables and credentials bind exact Secret versions. Plaintext does
  not enter the revision, command receipt, logs, metrics, or audit payload.
- Cell names are application data. Management and telemetry surfaces return
  bounded identifiers only when explicitly authorized and otherwise use a
  stable redacted digest.
- Dynamic code loading, unrestricted outbound networking, and cross-application
  bindings remain disabled until their owning Box, egress, Secret, and grant
  contracts pass independent conformance.

## 7. Domain and projection model

The Durable Cells context owns these semantic resources:

| Resource | Purpose | Explicitly does not own |
| --- | --- | --- |
| `DurableCellApplication` | Tenant/project/environment identity, name, desired state, active revision, aggregate version | Runtime unit, Route, bucket credentials, or Cell inventory |
| `DurableCellApplicationRevision` | Immutable bundle/provenance reference, compatibility policy, declared Cell classes/bindings, exact Service-profile digest, state schema/migration contract, retention policy | Mutable deployment pointer, per-Cell state, provider tuning, or plaintext Secret |
| `DurableCellProjectionIdentity` | Deterministic application/revision correlation to reserved S0 namespace identity plus existing Workload, Workload revision, Deployment, and Operation identities | Persistence, lifecycle, Gateway-scope selection, a second rollout controller, or provider lease |
| `DurableCellDeployment` (implemented component-only `CELL0.4-C3`) | Immutable correlation intent from one exact revision to its existing managed Workload deployment, S0 credential/retention binding, placement digest, and Operation; C4 selects an environment-owned Gateway scope without mutating this record | A separate deployment ID, status/lifecycle, scheduler, rollout controller, namespace lifecycle, Fleet receipt, Gateway binding, or provider lease |
| `DurableCellDeploymentBinding` (implemented `CELL0.4-C5`) | Canonical plaintext-free `cloud.durable-cell.deployment.v1` ACL carrying exact Secret versions, credential generation, provider-profile digest, and S0 retention policy for public admission | Caller-selected tenant or namespace scope, Secret material, provider configuration, namespace lifecycle, or another deployment record |
| `DurableCellServiceProfile` | Canonical ACL for non-negotiable provider semantics and bounded public/internal surface | Application code, placement, credentials, ownership rows, or state bytes |

There is intentionally no authoritative `Cell` aggregate or `cells` table.
Application code creates a Cell by addressing a name through the data plane.
Operator actions such as diagnose or evict carry an application, class, and
bounded name reference, are audited, and are dispatched through the existing
Fleet command journal without persisting a second ownership record.

`CELL0.1-C1` implements the canonical
`cloud.durable-cell.service.v1` profile. It requires:

- provider protocol `a3s.durable-cell-provider.v1`;
- dedicated application fleet and distinct public/internal Runtime ports;
- SQLite-per-Cell, single-threaded event turns, idle eviction, hibernatable
  WebSockets, one writer, epoch fencing, and replication before acknowledgement;
- exact `fetch`, `alarm`, and `websocket` handler support;
- conditional create, conditional overwrite, and read-after-write storage; and
- bounded Cell names, HTTP bodies, and WebSocket messages.

The profile is generated, parsed, canonicalized, and digested only through
`a3s-acl`. Provider selection and application compatibility remain separate
immutable bindings so a semantic profile cannot smuggle provider configuration.

`CELL0.1-C2` implements the canonical
`cloud.durable-cell.application.v1` definition plus the application/revision
aggregate. The ACL binds one existing `BuildRun`, bounded immutable bundle
digest, main ESM module, compatibility date and ordered flags, exact Service
profile digest, and an ordered set of Cell classes. Every class declares the
state versions it can read and the one it writes. A successor must read the
parent's written state, may not regress its write version or remove a class,
and may claim compatible rollback only when the parent can read the target's
written state; otherwise the rollout is explicitly forward-only. The
application aggregate owns only tenant identity, desired running/stopped state,
exact revision lineage, and optimistic version. It does not own the BuildRun,
bundle bytes, Workload, deployment pointer, provider state, or Cell inventory.

`CELL0.1-C3` checks in canonical producer fixtures for both ACL schemas and
locks their digests through the same `a3s-acl` parse/generate path. It also
implements `DurableCellProjectionIdentity`: `StorageNamespaceId` and
`WorkloadId` remain stable for the application, while `WorkloadRevisionId`,
the existing Workloads `DeploymentId`, and `OperationId` remain stable for one
application revision. The existing Workloads managed-owner fence carries kind
`durable-cell.application`, application ID, revision number, and exact
application-definition digest. Gateway scope stays an environment-selected
`CELL0.4` input. C3 creates no `DurableCellDeploymentId`, persistence,
lifecycle, scheduler, object client, or Gateway owner lookup; S0 still owns
namespace lifecycle and Workloads/Operations retain their existing authority.

Component-only `CELL0.2-C1` introduces the first shared S0 conditional-object
foundation without a second client. The existing object client now exposes
atomic create-only, exact-version overwrite, and body-plus-version reads
through a typed `IObjectNamespace` port. A destructive unique-key startup probe
requires competing-create rejection, read-after-write, version advancement,
stale-version rejection, and verified cleanup. An uncertified local backend
fails closed. `ObjectNamespaceCredentialBinding` uses the shared exact
`SecretVersionReference`, locks tenant/namespace scope and credential lineage
by digest, and contains no plaintext. `DurableCellStorageBinding` correlates
the exact current application revision to that S0 credential generation plus
provider/retention policy digests while leaving namespace and Secret lifecycle
with their owners.

Component-only `CELL0.2-C2` reuses the Secrets-owned exact-version access and
decryption services for binding admission and just-in-time, zeroizing provider
credential materialization. S0 now owns one digest-locked retention policy,
monotonic sealed recovery-point lineage, an exact isolated restore plan and
post-restore evidence, plus a writer-fenced, retention-receipted,
grace-delayed deletion plan and terminal cleanup evidence. Recovery evidence
re-observes the sealed source manifest and exact restored state digest; deletion
evidence binds the deleted namespace separately from the retained isolated
restore namespace. `DurableCellStorageBinding` only verifies that these S0
contracts match its exact namespace, provider, and policy digests. It adds no
backup engine, deletion worker, Secret store, object client, Operation, or Flow.
Real provider execution, retained fault evidence, and certification remain
open.

Component-only `CELL0.2-C3` checks in the retained HTTPS S3-compatible
conditional-write gate without another client. One test-only fixture constructs
the production `ImmutableObjectClient::s3`; both the existing immutable-log
provider test and S0's typed namespace probe consume it. The old log test no
longer constructs an extra raw S3 client. The S0 gate emits a machine-checked
marker only after all seven CAS/read/cleanup checks pass over HTTPS, scans the
retained log for every supplied credential, records the Cloud revision and
evidence hashes, and is exposed by one manual workflow. A retained
operator-owned pass is still required, and no provider is certified by this
checked-in gate alone. Operator inputs and evidence rules are documented in the
[S0 object namespace conformance guide](../tools/s0-conformance/README.md).

Component-only `CELL0.3-C1` adds `DurableCellProviderBinding` without another
Service definition or lifecycle. It correlates the exact current application
revision and deterministic existing Workload revision with the canonical
Service-profile digest, resolved Service-template digest, and digest-pinned OCI
provider artifact. Admission requires exactly the profile's public and internal
TCP ports on distinct container sockets and an HTTP readiness check on the
public port. The Runtime projection calls the shared Workloads Service projector and
sets only Runtime's existing opaque semantics-profile digest. Readiness consumes
the existing Fleet `RuntimeApply` command acknowledgement, validates it through
the shared command/receipt contract, and returns the two existing typed Runtime
endpoints. C1 adds no Runtime class, provider configuration, endpoint registry,
command table, deployment state, or product-specific receipt store.

Component-only `CELL0.3-C2` adds one `DurableCellOperatorObserve` payload to
Fleet's existing node-command journal. Node Agent resolves the exact healthy
internal Runtime endpoint, performs a bounded non-redirecting `GET /state`, and
keeps only six bounded anonymous counters and erases the bounded raw buffer. Provider-native ownership names,
dynamic phases, Cell names, resident/published sets, memory values, and raw
bytes never enter the command receipt. Adoption requires both the exact C1
healthy `RuntimeApply` receipt and this digest-bound observation. Graceful drain
still uses the ordinary Runtime `SIGTERM` path and exact `RuntimeStop` receipt;
process cleanup still uses `RuntimeRemove`. C2 therefore adds no provider
shutdown command, rollout/adoption state machine, cleanup worker, journal, or
receipt store and makes no real-provider certification claim by itself.

Component-only `CELL0.3-C3` pins celld v0.2.1 by immutable release tag, exact
upstream revision, OCI index digest, Linux manifest/config digests, revision
labels, and GitHub Actions provenance. It adds no runner: the existing Box
provider workflow pulls that exact image and runs one ignored Node Agent gate
through the production Box Runtime client and Fleet journal. The gate proves
healthy public readiness, distinct node-local public/internal endpoints, the
sanitized internal operator observation and exact replay, ordinary graceful
Runtime stop, exact removal, and restart-safe absence. Its retained JSON and
marker require `storage=not-certified`; no S0 durability, Worker application,
Cell behavior, Gateway publication, or fault claim follows. A successful
retained run is still required.

## 7.1 Cloud orchestration delivery (`CELL0.4`)

`CELL0.4` is split so persistence and each existing-owner composition can be
verified without creating a second control plane:

| Sub-gate | Outcome | Explicit non-authority |
| --- | --- | --- |
| `C1` | Migration `116` persists the application head and immutable canonical-ACL revisions through the sole A3S ORM Migrator. One repository transaction reuses shared idempotency, Outbox, and audit records; exact tenant BuildRun, linear revision, optimistic aggregate, and desired-state/revision fences are enforced | No deployment row, per-Cell table, migration runner, event log, scheduler, queue, Runtime state, or provider receipt |
| `C2` | Implemented authorization-before-replay create/revise/start/stop commands plus tenant-bounded get/list/revision queries on the shared buses; new revisions resolve the exact scoped BuildRun through Artifacts | No interface-specific repository, client-side authorization, duplicated BuildRun state, or permission store |
| `C3` | Implemented: migration `117` persists only immutable `DurableCellDeployment` correlation intent. After exact S0 credential/retention and Secret admission, the internal handler stores intent first and idempotently invokes the existing managed Workload revision/Deployment, Operation request, Outbox, and Fleet flow; focused recovery resumes after process death at that boundary | No deployment identity/status, rollout controller, namespace lifecycle, object client, Fleet channel/receipt, retry loop, or Operation engine |
| `C4` | Implemented: an authorization-before-replay internal command loads the exact C3 correlation, derives only the public Runtime port from canonical A3S ACL, and delegates initial publication to the existing Edge route handler. Edge retains verified-claim, healthy-target, complete-snapshot, idempotency, and Fleet-dispatch authority; the existing Workloads route updater retains later revision cutover | No table, Route copy, Cell-owner lookup, internal endpoint publication, route controller, retry loop, or Gateway request replay after dispatch |
| `C5` | Implemented: bounded REST/OpenAPI `1.38.0`, the maintained TypeScript client, CLI, and ten Management MCP tools reuse C2-C4 and admit deployment through three canonical A3S ACL strings; Web stays deferred by the active freeze | No presentation-owned state, repository, authorization mechanism, configuration parser, OCI/DNS validator, or configuration format |
| `C6` | In progress: C6a retains PostgreSQL replay and an actual child-process death after immutable correlation commit while the Workloads insert is blocked, then reconstructs the exact Workload/revision/Deployment/Operation/Outbox/managed-replica projection once. Stop/restart and cleanup evidence remain | No process hook, second transaction coordinator, lifecycle, retry loop, or product availability claim; `CELL0.5` still owns real storage-backed application behavior |

Component-only `C1/C2/C3/C4` and the complete non-Web `C5` interfaces are
implemented. Their shared PostgreSQL fixture verifies
migration registration, exact historic idempotency replay, immutable
revision/state fences, authorization before replay, exact tenant BuildRun
resolution, bounded current/history queries, atomic Outbox/audit evidence,
compact references rather than duplicated ACL bodies, immutable C3 correlation,
and absence of per-Cell, lifecycle, and queue tables. The C3 application gate
also proves intent-first recovery through the existing Workload bundle without
creating another event or control loop. C4 focused tests prove that an Edge
commit followed by failed Fleet dispatch resumes from the same persisted Route
without re-resolving the target or creating another Route, authorization is
checked before Edge replay, and a changed Service-profile digest fails before
target resolution or dispatch. Focused `C5` tests prove bounded ACL transport,
strict argument handling, idempotent lifecycle reuse, permission equivalence,
and response redaction across REST, client, CLI, and Management MCP. The
[retained PostgreSQL 17 H0 gate](https://github.com/A3S-Lab/Cloud/actions/runs/31935793202/job/95137381473)
passes this C1-C3 fixture and C6a. C6a holds the existing Workloads table lock,
waits until PostgreSQL proves the child is blocked on the Workload insert after
the immutable correlation committed, sends SIGKILL, and reconstructs every
deterministic existing-owner projection exactly once through fresh production
repositories. Retained provider runs and remaining C6 stop/restart/cleanup
fault evidence remain.

## 8. Rollout and recovery

A rollout follows the existing Workload generation lifecycle:

1. admit one immutable application revision and exact bundle digest;
2. validate its state migration and rollback declaration;
3. project one managed Workload revision using a reviewed Cell provider image;
4. reserve Claims and apply new Runtime Service replicas through Fleet;
5. require each replica to pass provider protocol, object-store, peer, restore,
   and health probes;
6. publish the complete Gateway target set only after exact acknowledgements;
7. drain the previous generation, hand off resident Cells, and fence it before
   Claims or Secrets are released; and
8. retain the previous immutable revision for an explicitly compatible
   rollback, otherwise require a forward repair revision.

Provider binary upgrades and application code rollouts are distinct revisions.
A provider generation that changes on-disk or replication formats must declare
mixed-version compatibility. If it cannot, Cloud performs a bounded full-fleet
drain and rejects rolling coexistence.

## 9. Ordered delivery gates

| Gate | State | Outcome | Required evidence/dependencies |
| --- | --- | --- | --- |
| `CELL0.1` | Implemented | Freeze ownership, ACL, identities, revision/projection boundaries, errors, bounds, and compatibility vocabulary | `C1` canonical Service profile, `C2` canonical application definition/revision aggregate, and `C3` digest-locked shared ACL fixtures plus deterministic existing-owner projection identities are implemented; this is a contract gate, not service availability |
| `CELL0.2` | In progress | Add S0 object-namespace and credential bindings plus a destructive conditional-write/startup probe and sealed backup/restore contract | `C1` implements the sole-client CAS port/probe and exact credential/storage bindings. `C2` implements exact active Secret/JIT materialization and digest-locked recovery, retention, restore, and deletion contracts. `C3` checks in the shared HTTPS S3-compatible gate, secret-safe retained-evidence script, and manual workflow while removing a duplicate raw test client. A retained provider pass, recovery/deletion execution, and fault evidence remain |
| `CELL0.3` | In progress | Certify one digest-pinned Cell provider as an ordinary Box-hosted Runtime Service with public/internal endpoints, typed health/operator receipts, graceful drain, adoption, and cleanup | Component-only `C1` binds and projects the exact provider through the shared Workloads Service path and admits only an exact healthy Fleet `RuntimeApply` receipt. `C2` adds one bounded, Cell-name-free operator observation through Fleet's existing journal; adoption combines it with C1 evidence, and drain/cleanup validate the existing `RuntimeStop`/`RuntimeRemove` receipts. `C3` pins celld v0.2.1 provenance and composes a real Box runtime-only gate into the existing provider workflow. Its retained pass and every storage/application/fault gate remain; no new Runtime class, lifecycle, journal, runner, or receipt store |
| `CELL0.4` | In progress | Persist the frozen aggregates through A3S ORM, then add idempotent commands/queries, managed Workload projection, Gateway publication, Operations, audit, REST/client/CLI/Management MCP; Web stays deferred | Component-only `C1` implements migration `116`, application/revision repositories, compact exact replay, immutable fences, and shared Outbox/audit writes. `C2` registers authorization-before-replay mutation and bounded current/history CQRS. `C3` implements migration `117`, immutable intent-first Workload/S0/Operation correlation, the sole Workloads-owned managed-owner handoff, and process-death recovery through the existing Workload/Operation/Fleet path without another lifecycle. `C4` composes the exact correlation and ACL-derived public port into Edge's sole healthy-target/snapshot publication path; later revisions use the existing Workloads route updater. `C5` exposes this authority through bounded REST/OpenAPI `1.38.0`, the maintained TypeScript client, CLI, and ten Management MCP tools plus the canonical `cloud.durable-cell.deployment.v1` ACL without another state or authorization mechanism. The retained C1-C3 PostgreSQL 17 and C6a actual-process-death gates pass; real S0 application evidence and remaining C6 stop/restart/cleanup evidence remain. Dependencies stay `CELL0.1`-`CELL0.3`, `E0`, `C0.3`, and `H0.2` |
| `CELL0.5` | Planned | Pass one real single-node application gate covering named SQLite state, alarms, hibernatable WebSockets, idle eviction/reactivation, RPO=0 process death, rollout, rollback, stop, restore, and deletion | Exact Cloud/Runtime/Box/Gateway/S0/provider revisions and retained fault evidence |
| `CELL0.6` | Planned | Pass multi-node ownership, forwarding, takeover, node loss, partition, pressure shedding, graceful handoff, rolling provider upgrade, and stale-node return without split brain | `CELL0.5`, `H0.3`, production S0 provider and private networking |
| `CELL0.7` | Planned | Publish a capability-tested Workers/Durable Objects compatibility matrix, bounded import/deploy workflow, quotas, observability, disaster recovery, and hostile-tenant isolation posture | `P0`, `H0.4`/`H0.5`, relevant `C0.5`; no blanket compatibility claim |

The initial provider may be a pinned celld build behind the Cloud-owned provider
adapter if provenance, licensing, protocol, security, recovery, and cleanup
gates pass. `CELL0` does not require that choice and does not vendor or fork
celld inside the Cloud control plane.

## 10. Mandatory fault and conformance matrix

At minimum, retained real-provider tests kill or partition the system after:

- object ownership read but before conditional acquire;
- local SQLite commit but before durable replication;
- durable replication but before current-epoch revalidation;
- revalidation but before client response;
- takeover but before the previous epoch is sealed;
- Runtime apply but before Fleet acknowledgement;
- provider health but before Gateway publication;
- Gateway apply but before Cloud acknowledgement projection;
- drain start with active requests and hibernatable WebSockets;
- last Cell handoff but before old Runtime removal; and
- namespace deletion intent but before state/backup cleanup evidence.

The gates prove no acknowledged write disappears, no two current writers
exist, no stale generation becomes routable, no internal endpoint becomes
public, no Secret leaks, no Cell name crosses tenants, and cleanup never removes
another application namespace.

## 11. Explicit exclusions

`CELL0` does not add:

- another Cloud scheduler, queue, timer table, workflow engine, or autoscaler;
- another Runtime lifecycle class, Box provider, node endpoint, or command
  journal;
- PostgreSQL rows for individual Cell state, leases, ownership, alarms, or
  WebSocket sessions;
- Gateway-side Cell lookup, owner caching, sticky routing, state proxying, or
  post-dispatch replay;
- a second object-store client, credential store, backup engine, or mutable
  deployment authority;
- provider-native product configuration as Cloud truth; importers emit
  reviewed A3S ACL and exact typed revisions only;
- automatic D1, KV, R2, Queues, Workflows, AI, Vectorize, Browser, Email, cron,
  or arbitrary Cloudflare platform claims; or
- a public celld operator API, celld internal-protocol compatibility promise,
  or shared hostile multi-tenant provider process before its gate passes.

The public service remains unavailable until `CELL0.5`. Multi-node and broad
compatibility claims remain unavailable until their own `CELL0.6` and
`CELL0.7` evidence passes.
