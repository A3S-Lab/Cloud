# A3S Cloud Product Roadmap

## 1. Scope and document hierarchy

**Status as of 2026-08-03.**

This is the product-level roadmap for A3S Cloud. It summarizes the complete
Cloud portfolio, current gate status, dependencies, delivery order, and the
boundary with A3S Gateway. It does not replace the detailed implementation
plans.

| Document | Authority |
| --- | --- |
| This `ROADMAP.md` | Product outcomes, portfolio ordering, public gate status, and cross-product ownership |
| [Technical architecture](docs/architecture.md) | Stable component ownership, control paths, consistency boundaries, deployment profiles, and failure behavior |
| [Cloud development plan](docs/development-plan.md) | Detailed implementation sequence, exit criteria, provider evidence, recovery gates, and definition of done |
| [Inference plan](docs/inference-plan.md) | Detailed `I0` domain, protocol, scheduling, Gateway, usage, and conformance contracts |
| [A3S Use plugin roadmap](https://github.com/A3S-Lab/Use/blob/main/ROADMAP.md) | Canonical plugin package, catalog, plan/apply, grant, Runtime-binding, capability-generation, and shared Plugin Manager delivery |
| [Runtime roadmap](https://github.com/A3S-Lab/Runtime/blob/main/ROADMAP.md) | Runtime-local Unit lifecycle, provider certification, and `MCP0.2` substrate work |
| [Gateway roadmap](https://github.com/A3S-Lab/Gateway/blob/main/ROADMAP.md) | Gateway-local current capability truth and implementation backlog |

The documents must change together when a product gate changes state. The
owning detailed plan decides whether its exit evidence is sufficient to mark a
gate verified; this roadmap then publishes that state without weakening or
reinterpreting the gate.

The roadmap is gate-driven, not date-driven:

| State | Meaning |
| --- | --- |
| Verified | The complete real-provider, failure, recovery, cleanup, and release evidence passes |
| In progress | A usable implementation slice exists, but named exit evidence remains |
| Planned | The capability is unavailable until its owning gate passes |
| Historical | Prior implementation evidence retained for regression coverage; it does not certify the current provider contract |

## 2. Product position

**A3S Cloud is the self-hosted control plane for applications, Agents, MCP
services, and model-serving workloads on operator-owned infrastructure.**

The cumulative product target is an A3S-native platform that replaces the
operational responsibilities commonly split between Google AX and Kubernetes.
It requires neither system and does not preserve their APIs or controllers.
The outcome is delivered across the existing `A0`, `A1`, `C0`, `H0`, and Box
certification gates rather than through a parallel replacement milestone.

Cloud turns tenant-owned intent into durable, observable infrastructure state.
PostgreSQL is authoritative for desired state, A3S Flow coordinates long-lived
operations, node agents converge A3S Runtime resources, and A3S Gateway applies
the complete traffic policy produced by Cloud.

Cloud owns:

- organizations, projects, environments, identity, membership, and grants;
- immutable application, Agent, MCP, Skill, model, and provider revisions;
- tenant-scoped Agent conversations, executions, approvals, checkpoints,
  forks, and replayable trajectories after `A1`;
- Workloads, desired replica count, placement, rollout, and the sole
  production autoscaling evaluator;
- source resolution, isolated builds, artifact publication, and release
  provenance;
- domains, TLS intent, logical Gateway scopes, complete traffic snapshots, and
  exact applied-state projection;
- databases, volumes, fencing, backup, restore, and retention after `S0`;
- durable operations, audit, logs, usage ledgers, API, CLI, management MCP, and
  web surfaces;
- tenant-scoped A3S Use registry enrollment, exact package assignments,
  reviewed-plan projections, and applied-host observations after `U0`; and
- installation, upgrades, high availability, disaster recovery, and
  operational policy after `H0`.

Cloud does not own:

- per-request proxying, protocol framing, or provider-byte forwarding;
- a second workload engine outside the common Workloads and Runtime path;
- an A3S Use package installer, TUF/catalog implementation, Workspace Grant or
  Runtime Binding store, capability registry, surface reconciler, or plugin
  execution RPC;
- Kubernetes, Helm, CRDs, or Operators as a required installation or an
  alternative Cloud control plane;
- raw provider configuration formats at the product boundary;
- a built-in mail server or a separate native-desktop feature set; or
- commercial prices, balances, invoices, settlement, and managed-service
  plans.

All Cloud product configuration uses closed, validated A3S ACL and is parsed
and generated through `a3s-acl`.

A3S Box is the sole node-local execution and image-build provider. A3S Power is
the required inference serving boundary and runs as an ordinary Box-hosted
Runtime Service. Neither product adds a scheduler, node channel, queue, desired
state store, routing authority, or usage authority to Cloud.

Cloud now owns the generic finite-Task product surface as tenant-scoped
Executions. The initial vertical slice persists replay-safe intent and
Operations, schedules capability-matched Runtime Tasks through Fleet, supports
cancellation, and withholds terminal state until authoritative cleanup. This is
platform execution infrastructure; it does not implement Agent conversations,
training, trajectories, or any Agentic RL policy.

## 3. Current roadmap

| Gate | Product outcome | State |
| --- | --- | --- |
| `BX0` — Box-only platform | Sole A3S Box execution/build path and Box re-certification of the complete Runtime, deployment, source-delivery, recovery, and cleanup baseline | In progress |
| `PW0` — Power inference boundary | ACL-native immutable Power Service profile, Box MicroVM/TEE evidence, health, inference, recovery, and cleanup | Planned |
| `R0` — Universal Runtime | General Task and Service contracts, durable identity, capability matching, and real provider conformance | Historical; Box re-certification pending |
| `F0` — Foundation | Boot control plane, PostgreSQL, tenancy, identity, Flow operations, outbox, projections, API, and web shell | Verified |
| `N0` — Node control | Enrollment, outbound mTLS, command leases, observations, durable command journal, and sole Box driver | Historical; Box re-certification pending |
| `D0` — OCI deployment | Immutable digest-pinned Workload revisions, scheduling, apply, health, activation, stop, cancellation, and recovery | Historical; Box re-certification pending |
| `E0` — Reachable service | Managed TLS, complete Gateway snapshots, encrypted Secrets, durable ordered logs, immutable update, cloned rollback, web operations, and a clean-host release loop | Historical; Box re-certification pending |
| `G0` — External source delivery | Pinned Git sources, isolated builds, OCI validation/publication, provenance, and deployment through the common Workload path | In progress |
| `P0` — Developer workflows | Build detection, web/worker/scheduled profiles, previews, monorepos, and closed Compose import | Planned |
| `C0` — Control surfaces | REST/CLI/management MCP parity, grants, search, collaboration, notifications, audit, and bounded exec/terminal | In progress |
| `A0` — Release catalog | Agent and MCP release publication, Agent deployment, and Skill binding through the common source and artifact paths | In progress |
| `U0` — A3S Use plugin assignments | Trusted registry enrollment, exact workspace package assignments, reviewed plan/apply, enablement, observations, and recovery through the shared A3S Use Plugin Manager | Planned |
| `MCP0` — Hosted MCP services | Modern stateless MCP release admission, Runtime Service hosting, Cloud orchestration, Gateway protocol enforcement, and joint recovery evidence | In progress; unavailable |
| `A1` — Agent execution | Durable conversations, Harness execution, approvals, checkpoints, forks, and trajectories over existing Cloud control paths | Planned |
| `S0` — Stateful platform | Databases, volumes, fencing, backup, restore, retention, and stateful import mappings | Planned |
| `H0` — Production scale | Durable replicas, multi-node placement, private networking, Gateway replication, control-plane HA, and measured autoscaling | In progress |
| `I0` — Inference profile | Accelerator-backed model serving, OpenAI-compatible traffic, scoped keys, routing/fallback, Providers, durable usage, and governed self-service | Planned |

### 3.1 Baseline requiring Box re-certification

`R0` through `E0` define one cumulative behavioral baseline:

```text
general Runtime
  -> durable Cloud desired state
  -> outbound node control
  -> digest-pinned deployment
  -> managed HTTPS, logs, update, rollback, and clean-host recovery
```

The retired Docker implementation proved these behaviors, so its records remain
historical regression evidence. They do not certify the Box-only release.
`BX0` must reproduce the complete baseline on exact Cloud, Runtime, Box, and
Gateway revisions. Later work must reuse this path. A new interface, asset
type, import format, accelerator, replica policy, or provider never creates a
second deployment or reconciliation engine.

### 3.2 Current in-progress gates

`BX0` is the release-blocking provider migration:

1. `BX0.1` pins one certified Box/Runtime pair, adds closed `box` ACL
   configuration, and removes provider fallback. It is verified by
   [Cloud PR #86](https://github.com/A3S-Lab/Cloud/pull/86) and the
   [exact Linux provider gate](https://github.com/A3S-Lab/Cloud/actions/runs/30416879476).
2. `BX0.2` migrates digest-pinned Task/Service lifecycle, recovery, logs,
   resources, stop/remove, cancellation, and cleanup. It is verified by
   [Cloud PRs #87 through #93](https://github.com/A3S-Lab/Cloud/pull/93) and the
   [final interruption gate](https://github.com/A3S-Lab/Cloud/actions/runs/30456965598).
3. `BX0.3` migrates networking, endpoints, health, Secrets, Artifact/Volume/
   tmpfs mounts, outputs, and registry credentials through typed Box ports. The
   typed Service TCP endpoint, shared Runtime health consumer, and explicit
   isolation-selection slices plus Secret and registry-credential
   materialization are implemented. Artifact mounts, persistent Volumes, tmpfs,
   and Task-output publication now use the same Box driver and existing Cloud
   Artifact boundary. The composite provider/Cloud Claim gate closes allocation
   evidence; complete Sandbox/MicroVM/TEE isolation certification keeps the
   gate in progress.
4. `BX0.4` now implements the sole `cloud.build@5` path through typed Box
   commands and canonical ACL build plans. Box owns its operation journal,
   content-addressed cache, and images; Cloud retains Artifact transport, OCI
   admission, publication, and SPDX/SLSA evidence. Exact Linux provider and
   process-death certification remains open.
5. `BX0.5` has removed the retired build executors and adds an architecture
   guard against BuildKit, Bollard, Runtime build adapters, and duplicate build
   caches, journals, schedulers, or services. Complete Box-only conformance and
   the clean-host release gate remain open.

Cloud now delegates provider certification to the exact A3S Box revision and
uses Box-hosted fixtures for local development and the C0 PostgreSQL gates. The
retired provider workflows, release harness, and source-build certification
script have been removed instead of retained as fallbacks. This does not mark
`BX0.3` through `BX0.5` complete: the Box-owned capability work and a new
clean-host release gate must restore the named behavioral evidence.

`BX0.2` is verified. Cloud consumer recovery and hard-resource Claims pass the
[dedicated Linux gate](https://github.com/A3S-Lab/Cloud/actions/runs/30425852930).
It persists the command before dispatch, applies through the shared Box driver,
reconstructs both the Runtime client and Agent executor across the
apply-before-journal-completion boundary, and requires the same durable Runtime
receipt and physical Task or Service identity. The same gate replaces a running
Service generation and proves logs, inspection, stop, removal, and empty
provider state. It also prepares one inventory-bound CPU/memory Claim, binds the
exact Box observation across restarts, rejects release before durable stop
evidence, and releases only after the Runtime is fenced. Deployment cancellation
passes through the existing Flow, Fleet
command journal, Runtime driver, and Claim repository in the
[exact Linux gate](https://github.com/A3S-Lab/Cloud/actions/runs/30429412890).
It uses an explicitly headless Service, requires `RuntimeRemove` evidence
before `ResourceClaimRelease`, records `Cancelled` only after both complete,
and finishes with empty provider state. The
[final interruption gate](https://github.com/A3S-Lab/Cloud/actions/runs/30456965598)
sends `SIGKILL` after Box has durably removed the Service but before the Agent
records command completion. A reconstructed Agent and Flow adopt the exact
removal receipt, keep the prepared Claim capacity held until acknowledgement,
release it once, reach terminal cancellation, and leave no provider residue.

The first `BX0.3` slice is implemented across
[Runtime PR #8](https://github.com/A3S-Lab/Runtime/pull/8),
[Box PR #185](https://github.com/A3S-Lab/Box/pull/185), and
[Cloud PR #95](https://github.com/A3S-Lab/Cloud/pull/95). A3S Runtime owns one
provider-neutral typed Service endpoint observation. The shared Box driver owns
generation-fenced loopback listeners and relays through its existing
`ExecutionPortConnector`. Cloud removed its product-specific endpoint contract
and consumes the Runtime type directly, using one stateless Edge adapter to
compile a TCP socket into Gateway's canonical HTTP origin. The real Box gate
proves live HTTP traffic through that origin, stable observation replay, exact
removal, and listener closure. No separate Box CLI forwarder, namespace
connector, forwarding daemon, lifecycle store, or endpoint registry was added.

The second `BX0.3` slice pins
[Box PR #186](https://github.com/A3S-Lab/Box/pull/186), whose single Runtime
driver provider-certifies HTTP, TCP, and command probes over the same
generation-fenced port and exec boundaries. Cloud does not interpret probe
kinds or run probes. Its existing A3S ACL Workload compiler emits the HTTP
Runtime policy, and the Node Agent carries the kind-neutral current health
observation through the same durable command result. The real Box consumer
gate proves threshold convergence to `Healthy`, exact journal replay after
Runtime and executor reconstruction, a fresh healthy inspection with unchanged
provider identity and endpoint, live traffic through the stateless Gateway
origin adapter, removal, `NotFound`, and listener closure. No health worker,
registry, scheduler, queue, endpoint authority, or lifecycle store was added.

The third `BX0.3` slice pins A3S Box
`9fb9bf528f6c648bbecf203de991106fc39bccdb` and makes isolation selection an
explicit closed Node Agent contract. The required ACL `box.isolation` field
accepts exactly `microvm` or `sandbox`; missing, `automatic`, and unknown values
fail before the Runtime starts. Cloud maps the selected value directly into the
same shared `BoxRuntimeDriver`, ships MicroVM in the product profile, and makes
hosted Cloud consumer tests request Sandbox explicitly. There is no automatic
downgrade, fallback provider, or parallel Runtime driver. Full provider
certification for Sandbox, MicroVM, and TEE remains open.

The fourth `BX0.3` slice pins A3S Box
`211b6bdaa572ba0ad5d55c7988a5b4a72ca36251`, merged through
[Box PR #187](https://github.com/A3S-Lab/Box/pull/187) after the
[provider certification](https://github.com/A3S-Lab/Box/actions/runs/30506005198).
Cloud contributes one
adapter from the existing authenticated node Secret channel to Box's typed
materialization port. Box owns process-create environment and read-only file
projection, restart rematerialization, log redaction, transient registry
authentication, and cleanup. The real consumer gate proves exact Secret
authorization, `0400` file projection, driver reconstruction, restart refresh,
redacted stdout/stderr, one uncached authenticated private-registry pull,
credential-free cache reuse, plaintext exclusion, and empty tmpfs/provider
state after removal. No second Secret channel, credential store, Runtime
driver, scheduler, queue, or lifecycle store is introduced.

The fifth `BX0.3` slice pins A3S Box
`7f29f6314827b1f572401cdda189bae9f34b7f9f`, merged through
[Box PR #190](https://github.com/A3S-Lab/Box/pull/190), and is integrated by
[Cloud PR #100](https://github.com/A3S-Lab/Cloud/pull/100). One
`CloudBoxArtifactPort` delegates to the existing authenticated node Artifact
manager for read-only materialization, deterministic bounded output capture,
durable local receipts, and command-bound publication. Box's existing
VolumeStore remains the sole authority for persistent Volumes, Task-output
staging, live attachment fencing, recovery validation, and cleanup. The real
consumer gate combines Artifact, Volume, tmpfs, output, journal-replay, driver
reconstruction, and zero-residue evidence. It adds no parallel Artifact store,
output database, VolumeStore, Runtime driver, scheduler, or lifecycle path.

The sixth `BX0.3` slice closes allocation evidence through one composite gate.
The exact Box phase executes every advertised Runtime profile, including the
Resources behavior derived from CPU, memory, PID, and execution-timeout
controls. The Cloud phase requires those controls and proves the existing
inventory-bound Claim across prepare, Runtime apply, exact binding-digest
inspection, pre-fence release rejection, durable stop, release, removal, and
cleanup. Both machine-checkable results are retained in one revision-bound
artifact; Cloud adds no provider resource model or second Claim mechanism.

`BX0.3` remains in progress only for complete Sandbox/MicroVM/TEE isolation
certification.

`PW0.1` follows the required `BX0.3` isolation and evidence capabilities. It
makes the immutable ACL-native A3S Power profile the first local I0 backend and
proves Box-hosted health, bounded streaming and non-streaming inference,
attestation, process/VM recovery, update, rollback, and cleanup.

The exit gate installs Cloud, Box, Gateway, and Power on a clean supported
Linux host without Docker or a compatible daemon; deploys, reaches, observes,
updates, rolls back, and removes a Service; builds and publishes one OCI
Artifact; serves one bounded Power inference request; recovers the named
process/VM failures; and leaves no execution, Secret, credential, mount,
network, volume, VM, or build residue.

The current `G0` implementation includes:

- canonical GitHub identities, repository policy, immutable source revisions,
  and versioned build recipes;
- signed replay-safe GitHub ingress, tenant-owned App connections,
  subscriptions, lifecycle reconciliation, and short-lived private access;
- exact-commit checkout, deterministic initial BuildRuns, retry-as-new-attempt
  lineage, cancellation, explicit build-log unavailability, and web controls;
- the sole `cloud.build@5` Flow, Fleet command queue, Node Agent replay journal,
  and typed Box start, inspect, cancel, and remove commands;
- Box-owned `BuildOperationJournal`, `BuildCache`, and `ImageStore` authority,
  with immediate-parent cache receipt binding and no Cloud cache fallback;
- complete OCI graph validation, deterministic registry targets,
  authenticated digest-only publication, remote verification, replay adoption,
  cleanup, and explicit deployment handoff to `cloud.deployment@3`; and
- deterministic SPDX 2.3 and SLSA provenance, locally verified Ed25519 DSSE
  signing through persistent local or Vault Transit providers, durable
  evidence restoration, and tenant-scoped API/web inspection and download; and
- migration of every pre-Box BuildRun to an explicit rebuild-required outcome,
  plus A3S Flow cancellation of known retired build histories on startup.

The retained external-provider workflow certifies private GitHub resolution
only. `G0` remains in progress until a revision-bound Linux gate certifies the
exact Box build provider, external Registry and signing boundaries, command and
Box-journal process-death replay, parent-cache reuse, and zero-residue removal.
Durable BuildRun logs also remain unavailable until Box exposes the
authoritative contract Cloud can transport.

`C0` now includes the initial `C0.1` automation slices:

- one maintained TypeScript client is shared by the web console and CLI;
- the client validates success and error envelopes, preserves bounded error
  metadata, applies request timeouts, and maps malformed or failed transport to
  stable non-secret errors;
- the CLI accepts authentication only through `A3S_CLOUD_TOKEN`, resolves URL
  and tenant context from flags or environment without a credential file, and
  emits bounded table or stable JSON output;
- organization, project, environment, node, and operation queries use the same
  public REST paths and tenant guards as the web console; and
- workload, deployment, route, BuildRun, signed-evidence, and bounded Workload
  log queries extend that same transport without reading PostgreSQL or
  contacting a node directly; BuildRun log requests fail explicitly while the
  Box log contract is unavailable; and
- workload stop/rollback plus deployment and BuildRun cancel/retry commands
  require a caller-owned validated `Idempotency-Key`, surface replay state, and
  call the existing application commands without a hidden confirmation path;
  and
- Workload create/update and SourceRevision deployment accept bounded A3S ACL
  through the same public REST paths. Cloud parses the exact document with
  `a3s-acl`, rejects unknown version-1 fields, and preserves JSON-client
  idempotency semantics; and
- Organization, Project, and Environment creation plus node ready/drain/revoke
  use the existing scoped REST and application commands. Every call has a
  caller-owned idempotency key, and node transitions also require the current
  aggregate version; and
- public administrative diagnostics read platform, liveness, and readiness
  through the shared client without sending a bearer token. A health endpoint's
  wrapped `503` down report remains diagnostic data, while an error envelope
  remains a failure; the CLI preserves the report and returns exit code `8`;
  and
- Edge automation lists and mutates DomainClaims, lists and creates logical
  Gateway scopes with one through 100 unique members and explicit rollout
  thresholds, and publishes routes through the existing tenant-guarded
  commands. DomainClaim and Gateway-scope mutations expose durable replay
  state, while route publication preserves request and Gateway-command replay;
  and
- Source automation lists immutable source revisions, inspects GitHub
  connection authority, starts the existing short-lived no-store installation
  flow, resolves branch/tag/commit inputs into pinned revisions, and
  lists/creates/deactivates GitHub repository subscriptions. Replayable
  mutations require caller-owned idempotency keys and expose durable replay
  state; and
- Secret automation lists metadata, inspects version state, creates Secrets,
  adds versions, and revokes versions through the existing public controllers.
  Plaintext enters the CLI only through bounded fatal-UTF-8 standard input,
  never appears in arguments, environment, configuration, output, or errors,
  and never bypasses Cloud encryption or A3S ORM persistence.
- Identity automation lists and reads tenant-scoped API-token metadata, creates
  scoped credentials, and revokes them through the existing public Identity
  controllers. New credentials enter the CLI only through exact 68-byte
  `--token-stdin` input, are cleared from the input buffer, never appear in
  arguments, environment, configuration, output, or errors, and are persisted
  only as digests through the A3S ORM repository; and
- Node bootstrap issues an idempotent, short-lived one-time enrollment
  credential through the existing tenant-guarded Fleet command. The CLI accepts
  the exact credential only through bounded standard input, clears its input
  bytes, projects no credential, and prints a Bash invocation that installs an
  HTTPS release only after an exact SHA-256 check. The target prompts for the
  credential and keeps it out of argv and the pre-provisioned A3S ACL config;
  Cloud persists only its digest through the Fleet A3S ORM repository.
- Organization-scoped authorized search registers credential-free Project,
  Environment, Node, Workload, Deployment, Route, DomainClaim, Gateway-scope,
  BuildRun, SourceRevision, Secret-metadata, and Operation projections. The API
  applies the tenant guard before a bounded A3S ORM query, while the shared
  client, CLI, and Web console use the same endpoint without broad local reads.
  Web adds debounced keyboard search and validated contextual navigation; and
- REST major version 1 publishes one unauthenticated raw OpenAPI 3.0.3 snapshot
  at `/api/v1/openapi.json`. The shared client and response headers pin contract
  `1.2.0`; route-snapshot tests and a PR-base semantic checker reject removed
  operations, new required inputs, removed responses or schema fields, missing
  version increments, and deprecations without a replacement and a 180-day
  minimum sunset window; and
- the real `C0.1` conformance gate runs raw REST, the exact shared client import
  used by Web, and the compiled CLI against one control-plane process and
  PostgreSQL 17. It proves cross-surface idempotency replay, stable conflicts,
  authorized-search parity, tenant denial, immediate token revocation, expected
  token-digest persistence through A3S ORM, and zero plaintext credentials in
  API/CLI evidence or the PostgreSQL dump.

`C0.1` and `C0.2` are verified. `C0` remains in progress. `C0.2` provides
stateless scoped management MCP for core Project, Environment, and authorized
search commands and queries plus Node, Operation, Workload, Deployment, Route,
and BuildRun reads, bounded cursor-paginated Workload logs, explicit BuildRun-log
unavailability, and signed BuildRun evidence. Five replay-safe Workload stop/rollback, Deployment
cancel, and BuildRun cancel/retry commands reuse the existing mutation scopes
and application handlers. A dedicated real PostgreSQL gate proves scope-derived
catalogs, strict arguments and annotations, operational query and command
dispatch, hidden-mutation zero-write, Project and Workload idempotency replay,
foreign-resource non-disclosure, immediate revocation, and digest-only A3S ORM
persistence. Grant-derived search is a separate `C0.3` authorization outcome;
the current search boundary is the organization tenant guard.

`A0.1` now provides the hosted-asset identity and persistence foundation:

- exact `agent`, `mcp`, and `skill` Asset kinds and closed lifecycle states;
- canonical SemVer, Git commit, profile ACL digest, and typed artifact
  identities;
- organization-scoped Asset-name and per-Asset release-version uniqueness;
- optimistic aggregate transitions, strict typed domain-event validation,
  shared idempotency records, and the existing transactional Outbox; and
- migration 051 plus one A3S ORM PostgreSQL repository, with real-database
  evidence for replay, stale-write rejection, tenant isolation, archival,
  publication immutability, yanking, and atomic event persistence.

Hosted Git is now public through a tenant-authorized Smart HTTP boundary, but
no release API is public and no Agent, MCP, or Skill is deployable from this
foundation alone. `A0` therefore remains in progress.

`A0.2` is verified. One local durable bare-Git adapter under
`{root}/{organization_id}/{asset_id}.git` initializes `main`, binds and
revalidates immutable tenant, Asset, and repository-schema metadata, enables
receive and transfer object checks, publishes through atomic staging and parent
directory sync, converges concurrent provisioning, and rejects symlinked paths
or identity tampering. Smart HTTP uses the existing tenant guard and scoped API
tokens. Source checkout and hosted repositories share the same hardened Git
runner; no second Git subprocess mechanism exists.

One `asset_git_repository_controls` row accessed only through A3S ORM owns the
durable quota, single-writer lease, applied usage, audit commit, and latest
backup receipt. Its lease ID also names one checksummed local rollback journal.
Recovery rolls back refs and newly introduced objects only while the database
lease is uncommitted; after the database commits, the same recovery path only
removes the journal. An uncertain commit result retains evidence for replay.
Backup and restore use the shared immutable-object client, and admission parses
the exact commit's `.a3s/asset.acl` only through `a3s-acl`. Real PostgreSQL and
Git integration covers concurrency, quota, tenant denial, audit atomicity,
process death, exact refs/object rollback, subsequent push, backup/restore, and
manifest rejection without Redis or another coordinator.

## 4. Delivery horizons and dependencies

| Horizon | Required gates | Product outcome |
| --- | --- | --- |
| Usable service platform | `BX0` plus `R0` through `E0` | One operator can deploy, reach, observe, update, roll back, and stop one Box-hosted stateless Service on one Linux node |
| Developer platform | `G0`, `P0`, `C0`, and `A0` | Source-to-release workflows, previews, stable automation, team operations, and A3S assets reuse the verified deployment path |
| Plugin-managed cognitive platform | `U0`, `C0.3`, the required A3S Use gates, and named `BX0`/`H0` host foundations | Tenants assign signed multi-surface A3S Use packages to authorized workspaces without another package manager, scheduler, or node channel |
| Hosted MCP platform | `A0.3`, `MCP0.1` through `MCP0.5`, and their named `BX0`/`H0` foundations | One immutable modern MCP release runs as a Box-hosted Runtime Service through an authorized conforming Gateway |
| Agent execution platform | `A0`, `A1`, and the relevant `C0` grants and audit gates | Immutable Agent releases become durable, resumable, approval-governed executions with replayable trajectories |
| Stateful production platform | `S0` and `H0` | Stateful resources, multi-node placement, HA, measured scaling, backup, and disaster recovery are production-operable |

Inference is an optional profile across these horizons, not another deployment
engine or delivery horizon. It may begin after `E0` and becomes production-ready
only after its named `H0` and `C0` foundations pass.

```mermaid
flowchart LR
    BX0[Box-only execution and build] --> R0[Universal Runtime]
    R0 --> F0[Cloud foundation]
    F0 --> N0[Node control]
    N0 --> D0[OCI deployment]
    D0 --> E0[Reachable service]
    E0 --> G0[Source delivery]
    G0 --> P0[Developer workflows]
    F0 --> A01[A0.1 asset identity]
    A01 --> A02[A0.2 repository safety]
    G0 --> A03[A0.3 release publication]
    A02 --> A03
    A03 --> A04[A0.4 Agent deployment]
    A04 --> A05[A0.5 Skill and catalog]
    E0 --> C0[Control surfaces]
    E0 --> U01[U0.1 Use contract and host boundary]
    U01 --> U02[U0.2 trusted catalog reads]
    C0 -->|C0.1/C0.2 reads| U02
    U02 --> U03[U0.3 single-host assignments]
    C0 -->|C0.3 grants and audit| U03
    U03 --> U04[U0.4 executable surfaces]
    H03 --> U05[U0.5 multi-host hardening]
    U04 --> U05
    A03 -->|A1.1 identity| A1[Durable Agent execution]
    A04 -->|A1.2 runtime| A1
    A05 -->|A1.3 bindings| A1
    C0 -->|C0.3 grants and audit| A1
    E0 --> S0[Stateful platform]
    E0 --> H01[H0.1 managed replicas and claims]
    H01 --> H02[H0.2 private target projection]
    H02 --> H03[H0.3 multi-node placement and network]
    P0 --> H04[H0.4 production installation and HA]
    C0 --> H04
    A05 --> H04
    A1 --> H04
    S0 --> H04
    H03 --> H04
    H04 --> H05[H0.5 autoscaling and hardening]
    E0 --> MCP01[MCP0.1 contract freeze]
    MCP01 --> MCP02[MCP0.2 Runtime substrate]
    BX0 --> MCP02
    A03 --> MCP03[MCP0.3 Cloud orchestration]
    H02 --> MCP03
    MCP01 --> MCP03
    MCP01 --> MCP04[MCP0.4 Gateway data plane]
    H02 --> MCP04
    MCP02 --> MCP05[MCP0.5 single-node release]
    MCP03 --> MCP05
    MCP04 --> MCP05
    MCP05 --> MCP06[MCP0.6 production scale]
    H03 --> MCP06
    C0 -->|C0.3 grants and audit| MCP06
    E0 --> I00[I0.0 versioned contracts]
    H01 --> I01[I0.1 accelerator substrate]
    I00 --> I01
    I01 --> I02[I0.2 single-node inference]
    H02 --> I02
    C0 --> I02E[I0.2e governed self-service]
    I02 --> I02E
    H03 --> I034[I0.3 and I0.4 multi-node inference]
    I02E --> I034
    H05 --> I05[I0.5 production hardening]
    I034 --> I05
```

Dependency rules:

- `G0`, `C0`, and `S0` may advance independently from the verified `E0`
  baseline.
- `P0` depends on the immutable source and build contracts from `G0`.
- `A0.1` uses the verified Foundation persistence, idempotency, and Outbox
  contracts. `A0.2` adds hosted repository safety. `A0.3` and later reuse the
  source, Artifact, publication, and deployment contracts verified by `G0`.
- `A1.0` has consolidated shared infrastructure from the verified `E0`
  baseline. `A1.1` consumes a published immutable `A0.3` `AssetRelease`,
  `A1.2` consumes `A0.4` Agent deployment, and `A1.3` consumes `A0.5` Skill and
  MCP bindings; approval and governance consume `C0.3` grants and audit.
- `A1` extends Operations and Flow, Fleet node control, Workloads, Runtime,
  Artifacts, the transactional Outbox, and shared sequence streaming. It does
  not add another scheduler, job queue, node channel, or integration bus.
- `U0.1` pins and adapts the frozen Cloud-to-Use host contract and consumes
  only canonical `a3s-use-core` identities, desired state, catalog records,
  plans, confirmations, receipts, and observations. `U0.2` may add read-only
  signed catalog discovery while A3S Use completes its mutation saga. `U0.3`
  requires the shared Plugin Manager and `C0.3` authorization/audit, and begins
  with one TUF registry and one explicit host/workspace. Host-local executable
  surfaces in `U0.4` use only the injected Runtime/Box and private Use bindings;
  a public or replicated service remains an explicit A0/MCP0 Workload, and
  Secrets/Knowledge retain their existing owners.
  Multi-host operations in `U0.5` consume existing H0/Fleet host membership
  and keep one independent assignment per host; they cannot add a plugin
  scheduler, group rollout controller, queue, or capability registry.
- `MCP0.1` may freeze the cross-repository contract from the verified `E0`
  model. `MCP0.2` consumes Box Service networking, health, and recovery;
  `MCP0.3` consumes immutable `A0.3` releases and `H0.2` target projection;
  `MCP0.4` consumes the same closed contract and `H0.2` managed-snapshot
  boundary. Only their joint evidence can close `MCP0.5`.
- `MCP0.6` consumes `H0.3` multi-node behavior and `C0.3` grants and audit.
  Stateless protocol requests do not bypass replica, rollout, identity, or
  authorization ownership.
- `H0.1` through `H0.3` may first be proven by an owning profile, but the full
  `H0` product gate also requires the single-node `P0`, `C0`, `A0`, `A1`, and
  `S0` surfaces it must scale.
- `I0` is an optional product profile, not another deployment engine. It
  consumes Workloads, Fleet, Edge, Identity, Artifacts, Secrets, Operations,
  and the named `H0`/`C0` foundations.

## 5. Product delivery lanes

### 5.0 `BX0`: Box-only execution and build

`BX0` has priority over feature expansion because every provider-backed gate
depends on it. Cloud reuses the existing Box Runtime driver and extends it in
A3S Box; it does not implement another Box lifecycle adapter. The Node Agent
remains the authenticated remote boundary, Runtime remains the provider-neutral
lifecycle contract, and Box remains local to the node.

No migrated slice may retain Docker as a fallback. A slice lands only when its
Box conformance and cleanup evidence passes; the final slice deletes the
retired code and rejects new Docker/Bollard/configuration references in CI.

The generic Execution slice is implemented above this boundary. It uses the
same Flow, Fleet, Runtime, and Box path as other finite Tasks and replaces the
retired Box-local Lambda lifecycle API. Box remains responsible only for local
provider mechanics. The required node-local `box.isolation` field selects the
pinned Box adapter's concrete backend. The shipped profile selects MicroVM;
shared-kernel execution requires an explicit `sandbox` selection.

### 5.1 `G0`: external source delivery

Next outcome:

1. add a revision-bound Linux gate for the pinned Box build provider without
   introducing another executor, cache, journal, image store, or scheduler;
2. prove exact Fleet and Node Agent replay plus Box operation-journal recovery
   across start and cleanup interruption boundaries;
3. prove immediate-parent Box cache reuse, one external Registry publication,
   one locally verified signature, one evidence document, and authoritative Box
   removal with no node residue;
4. expose build logs only after Box publishes its authoritative durable log
   contract; and
5. promote `G0` only after the complete private-source-to-published-Workload
   evidence remains green with operator-owned providers.

`G0` is complete only when an exact source revision produces a verifiable,
signed, digest-addressed OCI graph, survives retry/cancellation/process death,
deploys through the existing Workload path, and leaves no untracked provider
resource or credential.

### 5.2 `P0`: developer workflows

Ordered delivery:

1. A3S ACL build-plan detection and bounded source-layout proposals;
2. explicit web, worker, and scheduled Task/Service profiles;
3. pull-request previews with bounded lifetime and cleanup;
4. monorepo affected-set planning; and
5. closed stateless Compose import, followed by `S0`-backed stateful mappings.

Detection produces a reviewable proposal. Accepted build, route, storage, and
deployment plans become explicit typed Cloud desired state; an external project
format never becomes a second mutable source of truth.

### 5.3 `C0`: control surfaces and team operations

| Sub-gate | State | Outcome |
| --- | --- | --- |
| `C0.1` | Verified | REST/CLI parity, stable errors, authorized search, focused operational Web workspaces, and automation contracts |
| `C0.2` | Verified | Scoped, sessionless management MCP on the legacy initialization-based `2025-06-18` revision and real PostgreSQL parity over the same commands and queries |
| `C0.2m` | Verified | Modern per-request metadata, `server/discover`, protocol revision `2026-07-28`, and clean real PostgreSQL/Box parity over the existing application-command boundary |
| `C0.3` | Planned | Memberships, grants, role-focused console, attribution, notifications, and audit |
| `C0.4` | Planned | Outbound-protocol exec and terminal with bounded sessions and full audit |

No presentation surface owns business rules or bypasses tenant guards,
idempotency, operations, or audit.

The verified `C0.1` slices establish the shared typed transport,
non-persistent environment/flag context, safe output and exit-code contracts,
read-only tenant commands, then add workload, deployment, route, BuildRun,
   signed-evidence, bounded Workload-log queries, and explicit BuildRun-log
   unavailability. The Web console composes those
same queries, operation streams, projection refreshes, and mutations into
responsive Overview, Workloads, Delivery, and Edge workspaces. Authorized
search and validated deep links select the owning workspace without creating a
second client, navigation backend, or presentation-owned business state. The
operational mutation slice adds explicit idempotent stop, rollback, cancel, and
retry commands, while the desired-state slice adds Cloud-admitted A3S ACL for
Workload create/update and SourceRevision deployment. The core-resource slice
adds Organization, Project, and Environment creation plus version-checked node
lifecycle transitions. The diagnostics slice adds tokenless platform and
health inspection with a stable unhealthy exit contract. The Edge slice adds
DomainClaim query/create/verify/revoke, logical Gateway-scope query/create, and
route publication with explicit idempotency and replay projections. The Source
slice adds GitHub connection inspection/bootstrap, immutable revision
list/resolve, and repository-subscription list/create/deactivate. The Secret
slice adds metadata list/get and idempotent create/add-version/revoke-version
without exposing plaintext outside the request body. The Identity slice adds
API-token metadata list/get and idempotent stdin-only create/revoke without
exposing credentials or bypassing digest-only A3S ORM persistence. The node
bootstrap slice adds stdin-only one-time enrollment issuance plus a
checksum-verified Agent installation invocation without adding an SSH path or
bypassing Fleet A3S ORM persistence. The authorized-search slice adds one
organization-scoped API query over registered credential-free projections,
bounded A3S ORM exact/prefix/contains ranking, typed client and CLI parity, and
debounced Web navigation without broad local reads. The contract slice adds a
public raw OpenAPI v1 snapshot, shared `1.2.0` client/response versioning,
route-snapshot synchronization, semantic compatibility enforcement, and a
minimum 180-day replacement-bound deprecation policy. The final conformance
slice runs raw REST, the Web client import, and compiled CLI against real
PostgreSQL, proves replay and authorization consistency, and rejects plaintext
credentials across responses, logs, and persisted data. `C0.2` established raw,
sessionless Streamable HTTP JSON-RPC,
current-token scope-derived tool discovery, organization context derived only
from the authenticated principal, three core queries, two idempotent create
commands, ten operational Node, Operation, Workload, Deployment, Route, and
  BuildRun queries, one bounded cursor-paginated Workload-log query, one explicit
  BuildRun-log availability query, one signed-evidence query, and five
  replay-safe operational commands through the existing
application buses. Workload stop/rollback and
Deployment cancel require `workload:write`; BuildRun cancel/retry require
`build:write`. It rejects batches, foreign origins, hidden-tool invocation,
forged organization input, invalid arguments or cursors, and revoked tokens
without adding business rules or a persistence path to the presentation
surface. This verified slice has no server-side session, but it is not a modern
`2026-07-28` conformance claim. Its dedicated real PostgreSQL gate proves exact
23-tool administrator and 16-tool read-only catalogs, hidden-mutation
zero-write, Project and Workload
replay through one durable record per idempotency identity, indistinguishable
foreign and missing Project errors, operational read and command boundaries,
next-request revocation, expected A3S ORM rows, and credential-free logs,
evidence, and database dumps. `C0.2` is verified. `C0.2m` replaces only the
legacy protocol adapter with `2026-07-28` per-request protocol/client metadata,
matching `MCP-Protocol-Version`, `Mcp-Method`, and applicable `Mcp-Name`
headers, complete-result metadata, and `server/discover`. It removes
`initialize`, ignores legacy session identifiers without creating session
state, and reuses the same application buses,
authentication, scopes, tenant guards, idempotency identities, audit, and A3S
ORM repositories. Focused conformance and the clean real PostgreSQL/A3S Box
gate pass; `C0.2m` is verified.

### 5.4 `A0`: Agent, MCP, and Skill releases

| Sub-gate | State | Outcome |
| --- | --- | --- |
| `A0.1` | Verified | Exact Asset and AssetRelease aggregates, immutable identity rules, tenant-scoped A3S ORM persistence, optimistic transitions, shared idempotency and Outbox, and real PostgreSQL behavior evidence |
| `A0.2` | Verified | Tenant-authorized Git Smart HTTP, tenant/Asset-bound durable bare repositories, A3S ORM-backed PostgreSQL single-writer leases and quotas, same-lease crash recovery, immutable backup/restore, and pinned `.a3s/asset.acl` admission |
| `A0.3` | In progress | One typed external-or-hosted build path now reserves and repairs hosted work through the existing reconciler, builds the pinned Git input through `cloud.build@5`, and atomically finalizes a successful Agent or MCP BuildRun with its OCI AssetRelease, immutable BuildRun/provenance binding, and schema-v2 Outbox fact through A3S ORM migrations 063-064. Ordinary saves cannot bypass finalization, generic Asset writes cannot publish hosted releases, and exact replay repairs the succeeded-run/draft-release gap. Failed hosted attempts retain the same draft and recover through the existing idempotent BuildRun retry, Operation reconciler, and Flow; concurrent retry/finalization replay yields one next attempt, publication, and Outbox fact. Product yanking and deterministic selection, and public management surfaces remain |
| `A0.4` | Planned | Agent deployment, health, logs, update, rollback, and cleanup through the existing Workload, Flow, Fleet, and Runtime path; hosted MCP deployment is owned by `MCP0` |
| `A0.5` | Planned | Immutable Skill bundle binding plus tenant-authorized release/catalog API, client, CLI, and Web surfaces without generic forge features |

`A0.1` is a durable prerequisite, not a user-visible catalog. `A0.2` closes
through one repository path:

1. retain the local bare-repository, immutable identity, atomic
   provisioning, and shared Git-runner foundation;
2. serve tenant-authorized Smart HTTP through the existing authentication and
   audit boundaries;
3. serialize ref writes, persist audit, and enforce quotas through PostgreSQL
   using A3S ORM while one same-lease journal closes process-death windows;
4. create and restore verified repository bundles through the existing
   immutable-object boundary; and
5. admit only the exact pinned commit's `.a3s/asset.acl` parsed by `a3s-acl`.

No step adds another Git runner, database access layer, queue, object store, or
configuration language. `A0.3` cannot close until the exact `G0` source,
Artifact, publication, and evidence contracts it consumes are verified. A
published `A0.3` release is the first identity that `A1.1` may bind.

Agent and MCP remain immutable asset profiles, not separate schedulers.
`A0.3` publishes their release identities. `A0.4` owns Agent deployment, while
hosted MCP deployment and traffic conformance proceed only through `MCP0`.

### 5.5 `A1`: durable Agent execution

`A1` turns a published immutable `A0.3` Agent release into a tenant-scoped
execution. The Cloud API remains the client control boundary, and Gateway
remains a transport data plane; neither a Harness nor a client gains a direct
path around Cloud authorization, idempotency, Operations, or audit.

This is the native replacement for AX's Agent server, actor controller, event
log, Harness lifecycle, and snapshot roles. The common Workloads, Fleet,
Runtime, Box, Edge, and Gateway path supplies the cluster responsibilities
without importing a Kubernetes controller. The stable responsibility map is
owned by the [technical architecture](docs/architecture.md#11-native-agent-platform-replacing-ax-and-kubernetes).

| Sub-gate | State | Outcome |
| --- | --- | --- |
| `A1.0` | Verified | One sequence-cursor/SSE implementation, one infrastructure-level immutable object client with typed domain adapters, and one reusable node-agent durable outbound-batch journal/receipt primitive |
| `A1.1` | Planned | Add `AgentConversation` and `AgentExecution` aggregates plus one durable, monotonically sequenced semantic event stream |
| `A1.2` | Planned | Add a versioned Harness command/event protocol over the existing Fleet node-control channel and node-agent journal |
| `A1.3` | Planned | Pin Agent, Skill, MCP, workspace, and tool bindings to immutable identities and record auditable tool request/result events |
| `A1.4` | Planned | Add grant-checked approval checkpoints and logical pause/resume through the existing Operation and Harness lifecycle |
| `A1.5` | Planned | Add immutable checkpoints, explicit fork lineage, trajectory export, telemetry correlation, and exact Box checkpoint/suspend/resume recovery certification where provider resume is enabled |

The only new durable domain records are `agent_conversations`,
`agent_executions`, `agent_execution_events`, immutable execution-binding child
records, `agent_approval_checkpoints`, and `agent_execution_checkpoints`.
`agent_conversations.last_event_sequence` is the sole event-stream head.
Events keep bounded content inline or reference an immutable object by digest;
there is no separate Agent execution head or content store. All relational
persistence uses migrations and typed A3S ORM repositories.

| Concern | Existing authority | `A1` rule |
| --- | --- | --- |
| API replay | `idempotency_records` | Reuse the caller-scoped record; do not add Agent-specific idempotency tables |
| Long-running coordination | A3S Flow plus Operations | Flow history controls execution and recovery; `agent_execution_events` remains the user-visible semantic history |
| Node delivery | `node_commands`, leases, and the node-agent durable journal | Extend the versioned protocol and extract the existing pending-batch/receipt primitive; do not add another queue or channel |
| Integration facts | Transactional Outbox plus A3S Event | Publish bounded lifecycle IDs, states, and digests only; prompts, tool payloads, and model output remain in execution storage |
| Audit and approval authority | `audit_records` plus `C0.3` grants | Reuse the common audit chain and authorization evaluator; do not create an Agent audit subsystem |
| Scheduling and provider lifecycle | Workloads plus A3S Runtime | Run the selected Agent release and Harness through the common placement, apply, health, stop, and recovery path |
| Published assets | `A0.3` through `A0.5` `AssetRelease` | Bind immutable Agent, MCP, and Skill release IDs; never copy mutable manifests into an execution |
| Streaming and cursors | Existing Workload sequence stream and Operation snapshot polling; BuildRun logs are unavailable pending Box authority | Reuse the shared sequence cursor, reconnect, gap, SSE, and polling transports before adding the Agent stream |
| Immutable objects | Existing filesystem and S3-compatible object backends | Share one low-level content-addressed client while preserving typed domain ports, namespaces, admission limits, and retention policy |
| Optional Redis | No durable Agent authority | Redis may accelerate ephemeral fan-out only after correctness without it; it never owns conversations, queues, locks, cursors, approvals, or checkpoints |

`A1.0` is implemented. One shared sequence component now
owns the versioned cursor, `Last-Event-ID` precedence, bounded SSE record
events, and cursor advancement for Workload logs. A separate
shared polling transport owns interval scheduling, keepalive cadence, and retry
metadata for those sequence streams and the hash-addressed Operation snapshot
stream without inventing an Operation sequence. The duplicate domain-local
stream files and cursor decoders are removed, and architecture tests prevent
either transport from being reimplemented by controllers. One shared immutable
object client now owns namespaced filesystem and S3-compatible conditional
creation, exact replay, bounded reads and streams, digest verification,
idempotent deletion, and health probes. Log chunks and node Artifacts retain
typed domain adapters, admission rules, receipts, and retention policy without
reimplementing those mechanisms. The node-agent
`outbound_batch::DurableOutboundBatch` primitive now owns single-pending-batch
admission, exact restart replay, typed receipt validation, and settlement.
`LogShippingState` embeds it without changing the version-1 JSON shape, so
cursor advancement and pending-batch removal remain one atomic state write.
Focused compatibility, restart, receipt-integrity, and source-architecture
tests prevent a second outbound-batch lifecycle. Together these slices close
`A1.0` without adding another queue, cursor, or node-control channel.

Google AX may be evaluated only as an optional adapter behind the versioned
Harness port after `A1.5` and after its integration contract is stable. Cloud
does not adopt AX's controller, event-log authority, scheduler, native
configuration, or unstable wire protocol.

### 5.6 `S0`: stateful platform

Ordered delivery:

1. fenced local volumes;
2. explicit PostgreSQL resources;
3. backup, restore, retention, and disaster evidence;
4. additional database engines and remote volume providers through
   conformance; and
5. stateful project-import mappings.

A stateful move cannot proceed until the prior writer is fenced. A backup is
not a product capability until restore passes against a clean environment.

### 5.7 `H0`: production scale

| Sub-gate | State | Foundation | Required evidence |
| --- | --- | --- | --- |
| `H0.1` | Verified | Managed-owner references, durable replica identity, effective placement policy, versioned Fleet inventory, generic hard-resource claims, and fencing | Concurrent create/reconcile/replay produces one provider unit for one replica generation and never reuses an unfenced claim |
| `H0.2` | Verified | Logical Gateway scopes, complete target sets, generation-bound private endpoints, exact snapshot acknowledgement, and rollback | Only healthy exact-generation targets become eligible; restart and rejected apply preserve the prior route |
| `H0.3` | Planned | Multi-node replica sets, placement groups, gang claims, drain, anti-affinity, cluster-private networking, and independently placed Gateways | Real-node scale, drain, partition, stale-node return, and partial preparation converge without duplicate units, claims, members, or targets |
| `H0.4` | Planned | ACL-native, Box-hosted production installation/upgrade plus HA API, workers, relay, Gateway, migrations, and dependencies | Clean-Linux install, upgrade, process/node loss, leadership fencing, migration, rollback, and Gateway readiness gates pass without Kubernetes or Docker |
| `H0.5` | Planned | Sole Workloads autoscaling controller, quotas, telemetry bounds, load limits, backup/restore, and operational hardening | Stale, missing, duplicate, and bursty metrics stay safe without another scaling path; failover and restore meet published limits |

The Cloud production profile is ACL-native and Box-hosted. It does not depend
on Kubernetes, Helm, CRDs, Operators, Docker, or a compatibility daemon;
Workloads remains the only workload scheduler.

The current `H0.1` foundation persists inference-neutral managed-owner
references, one effective single-replica placement policy, one stable
replica/member, and one exact deployment-to-Runtime binding for every existing
Workload. Migration 040 backfills legacy Workloads without changing their
Runtime unit identity. Workload list/detail responses expose owner, policy,
replica generation, member, node, and placement generation.

Migration 041 and the independent Resource Claim repository add canonical hard
resource slots, monotonic slot generations, unguessable fence tokens, and the
durable `reserved_in_db -> preparing_on_agent -> prepared_on_agent ->
bound_to_runtime_unit -> releasing -> released` lifecycle with an
operator-visible `orphaned` branch. Orphaning and timeout retain the active
lease. Only exact Agent release, provider NotFound, or trusted compute fencing
can release it. CPU, memory, and ephemeral-storage slots use shared scalar
capacity accounting, while accelerator, host-port, and volume slots remain
exclusive. Migration 043 narrows active-slot uniqueness to those exclusive
kinds. Each PostgreSQL reservation serializes the stable slot, totals active
allocations, rejects over-capacity requests, and advances the slot generation
and fence token.

Migration 044 extends the durable Fleet command queue with
`resource_claim_prepare` and `resource_claim_release`. The node agent journals
the exact Claim generation, digest, current inventory identity, Runtime
unit/generation, sorted slots, and per-slot fencing evidence before
acknowledgement. A resource-bound Runtime apply must match that prepared
binding, and its observation must carry the exact Claim ID and binding digest
before Cloud persists `bound_to_runtime_unit`. A bound Claim cannot release
until the same Agent journal has durable Runtime stopped-or-absent evidence;
the release command advances the Claim generation and digest and returns exact
slot evidence.

The schema-backed claim CRUD and aggregate JOIN use A3S ORM typed builders. The
full Workloads repository and its shared idempotency/outbox writes use typed
builders for every query and mutation, including PostgreSQL advisory and row
locks, `SKIP LOCKED`, and parameterized JSONPath Secret-binding predicates. An
architecture test rejects raw SQL and direct drivers anywhere in Workloads
production persistence. In-memory and PostgreSQL 17 gates prove exact replay,
competing exclusive and shared reservations, over-capacity rejection, orphan
blocking, safe release, and generation/token rotation after fencing.

Migration 042 adds strict, immutable Fleet inventory snapshots, normalized
slots, and one current generation/digest head per enrolled node. The node agent
persists its canonical inventory across restart, advances generation only for
changed slot content, reports detected CPU and state-filesystem capacity plus
Linux memory when available, and omits resources it cannot prove. The
authenticated inventory endpoint accepts exact and historical replay without
regressing the current head, requires exact next-generation content changes,
and rejects identity conflicts. New v2 heartbeats must reference the current
inventory while legacy v1 batches remain readable during migration. The
PostgreSQL adapter uses only typed A3S ORM tables and builders, and the real
PostgreSQL 17 gate verifies concurrent replay, recovery, head monotonicity, and
stale-heartbeat rejection.

Workloads now compiles CPU, memory, and optional ephemeral-storage requirements
from the current inventory, leaving PID limits Runtime-local. PostgreSQL locks
and verifies the exact Fleet inventory head in the same transaction that
reserves capacity. Deployment Flow reserves before node assignment, derives a
deterministic Claim ID from the Deployment ID, recovers a committed
reservation-before-placement gap, and tries another eligible node after a
typed capacity conflict. Normal cancellation, retirement, and stop paths may
cancel only a never-issued `reserved_in_db` claim; later states retain the
Agent/trusted-fence requirement.

New deployment operations use `cloud.deployment@3`; v1 and v2 remain executable
only for persisted Flow history. Create, update, rollback, source handoff, and
Secret rotation share one version source. The v3 workflow reserves, prepares,
binds, retires, and releases through deterministic commands. Reconciliation
adopts an exact bound Claim, retries failed release with a higher generation and
digest, and preserves allocation ownership when stop evidence is rejected or
ambiguous. Unit and PostgreSQL 17 gates cover Agent journal restart after every
boundary, reservation-before-placement recovery, activation-before-retirement
process death, healthy update stop-before-release ordering, and Secret-rotation
replay.

The isolated real-provider gate now closes the `H0.1` process boundary. It
persists prepare in the real Agent journal, pauses a bound apply after Docker
creates one provider unit but before acknowledgement, replaces the provider
process, kills the Agent process, and reconstructs both Runtime and command
journals. Exact replay must reattach the same sole container and carry the
matching Claim ID and binding digest. Release and a capacity-conflicting Claim
must remain rejected until real Runtime stop/removal and the exact
higher-generation Agent release; only then may the competing Claim prepare.
The provider gate requires one stable pass marker and zero provider or Artifact
residue. `H0.1` is a closed exact-SHA acceptance gate; `H0.3` is the next open
production-scale foundation after the verified `H0.2` Gateway projection gate.
The closing evidence is Cloud commit
`5cd7c4eebc21905cb2758856d0e96b31a111116c` in
[Docker provider conformance run 30157496417](https://github.com/A3S-Lab/Cloud/actions/runs/30157496417);
both the real-provider and Cloud-consumer jobs passed.

The verified `H0.2` foundation includes Cloud-owned logical Gateway scopes. Each
scope belongs to one organization, project, and environment and now stores an
ordered desired member set, a membership generation, and explicit `min_ready`
and `max_unavailable` policy. Environment-scoped create/list APIs persist the
resource idempotently and retain the legacy single-`nodeId` request. A
Cloud-owned planner now resolves every desired member through its exact active
or retiring Deployment, replica binding, Runtime command, generation, and fresh
healthy node-local endpoint. It rejects partial, ambiguous, mixed-revision, and
mixed-port target sets and compiles an independent complete snapshot,
certificate, command, and staged Route projection for every member.
Single-member publication retains the established path. Replicated publication
atomically commits the logical Route, every physical projection, rollout,
publication, certificate, ownership record, idempotency result, and outbox fact.
A conflict rolls back the complete bundle, preventing a bootstrap-primary or
partially addressable apply.

Cloud persists each private route target as an exact immutable revision,
deterministic Runtime unit, positive generation, declared port, canonical
node-local origin, and command-bound healthy observation. Revision, unit, and
generation enter the complete ACL digest. A cutover requires a different
revision and strictly newer generation; rejection retains the prior target,
while the exact applied acknowledgement atomically selects the candidate.
PostgreSQL migration 036 splits legacy shared nodes deterministically by
environment/node binding, backfills Route and serialized recovery documents,
and enforces the complete tenancy/node relationship across restart.

The node agent uses Gateway's native apply and exact-status APIs, Gateway's
journal remains the sole applied-state authority, and unchanged snapshots renew
inside a bounded pre-expiry window without replacing their ACL digest or active
certificate before acknowledgement. A real pinned-Gateway fixture also rotates
independently signed certificates and target origins, rejects the superseded
certificate and selector, and recovers only the replacement after restart.
Before mutation, the agent now selects an explicitly advertised
`a3s.gateway.management-protocol.v1` tuple or the closed legacy-v1 baseline.
Unknown and inconsistent descriptors fail closed. Gateway ack v4 and command
ack v2 persist the selected protocol, while the control plane reads legacy
v3/v1 acknowledgements and migration 037 leaves their unavailable evidence
null. Migrations 038 and 039 add backward-compatible scope membership and a
durable per-member `GatewayRollout` aggregate. Every physical member owns an
independent revision, command, digest, expiry, certificate, and result. Meeting
the configured threshold makes a rollout ready, exact success from every
member makes it succeeded, and a fully observed mixed result becomes degraded.
The worker-role rollout reconciler restores the complete active aggregate and
its publications through typed A3S ORM, idempotently redispatches pending Fleet
commands after restart, and projects exact command-deadline expiry as
unavailable. The complete Edge PostgreSQL repository uses typed A3S ORM tables,
queries, expressions, row locks, and table locks for logical scopes,
publications, routes, cutovers, acknowledgements, DomainClaims, certificates,
convergence, and rollouts. A source architecture test rejects raw SQL and direct
database drivers throughout Edge production persistence. Domain, in-memory,
migration, recreated-PostgreSQL 17, and durable Fleet queue tests cover this
foundation, including route cutover and certificate-convergence recovery.

Migration 045 adds the logical-to-physical Route projections and atomic rollout
ownership model. Migration 046 adds read-only Gateway observation commands;
migration 047 persists per-member physical-state recovery; migration 048 adds
deterministic exact rollback; and migration 049 makes expired certificate
convergence explicitly unavailable without disturbing the prior applied state.
A logical Route activates only when the exact applied projections meet its
threshold. A terminal rollout below threshold keeps the prior active Route,
records rejected or unavailable candidate state, observes ambiguous members,
and stages one higher-revision rollback to the exact known physical state.
Rollback reuses only still-valid Ready certificates and remains visibly blocked
when any compensation is rejected or unavailable. Domain revocation and
certificate replacement release physical ownership member by member only after
the matching acknowledgement.

The cross-repository gate builds Gateway commit
`7a146b6d53635861e5db4870fb4603a5c59c87ee`. Real Gateway processes prove
complete snapshot reload, independent certificate and target replacement, two
member-specific journals and trust roots, continued service after one member is
lost, exact native-journal recovery when it returns, independent Cloud cursors,
and Agent process death after native apply but before acknowledgement. Together
with the recreated PostgreSQL 17 gate, this closes `H0.2`. Independently placed
multi-node Gateways remain `H0.3`; production control-plane and Gateway HA remain
`H0.4`.

### 5.8 `I0`: inference profile

| Sub-gate | Outcome | Dependency |
| --- | --- | --- |
| `I0.0` | Versioned accelerator and node contracts with mixed-version safety | Verified `E0` node control |
| `I0.1` | Single-node accelerator inventory, claims, Box device enforcement, and recovery | `I0.0` + `H0.1` + `BX0.3` |
| `I0.2a` | Immutable model catalog/cache, typed Power compiler, and one healthy private Box-hosted Power Workload | `I0.1` + `PW0.1` |
| `I0.2b` | OpenAI Models, Chat Completions, Completions, and Embeddings data plane, scoped keys, grants, per-Gateway limits, Redis-backed globally exact limits, streaming, and fallback | `H0.2` + `I0.2a` |
| `I0.2c` | Durable Gateway usage spool, Cloud ledger, observability, model rollout, and rollback | `I0.2b` |
| `I0.2d` | Credential-isolated external OpenAI-compatible Provider targets | `I0.2b` + `I0.2c` |
| `I0.2e` | Grant-derived model/key self-service, diagnostics, playground, search, and usage showback | `C0.3` + `I0.2d` |
| `I0.3` | Multi-node independent serving replicas and failover | `I0.2e` + `H0.3` |
| `I0.4` | One typed Power distributed serving replica across multiple nodes | `I0.3` + `H0.3` placement-group and private-network gates |
| `I0.5` | Gateway/control-plane HA, autoscaling, quota, disaster recovery, provider breadth, and load hardening | `I0.4` + `H0.4` + `H0.5` |

The first and required provider combination is NVIDIA, A3S Box, and A3S Power.
Cloud does not expose vLLM, Ray, or another Power engine as a separate
first-class backend. Hardware partitions, additional accelerator vendors,
named external Providers, and additional APIs remain unavailable until their
real conformance gates pass.

### 5.9 `MCP0`: hosted modern MCP services

`MCP0` turns an immutable `A0.3` MCP release into a reachable, authorized
modern MCP Service without creating a second workload engine or putting Cloud
on the request path. The protocol baseline is revision `2026-07-28`.

| Sub-gate | Owner | State | Outcome |
| --- | --- | --- | --- |
| `MCP0.1` | Cloud with Runtime and Gateway review | Contract foundation implemented (2026-07-30); review/merge pending | Closed A3S ACL contracts, identity/digest rules, Runtime projection, Gateway snapshot, retry boundary, stable errors, and frozen cross-repository fixtures pass focused tests |
| `MCP0.2` | Runtime and Box | Foundation in progress | Runtime consumes the frozen profile digest and generation-bound typed endpoint fixture; real Linux Box hosting, recovery, logs, and cleanup evidence remain |
| `MCP0.3` | Cloud | Foundation in progress | Closed contract values, canonical immutable Service-profile and separately mutable Edge route-policy ACL admission, migrations 052-057 plus typed A3S ORM persistence, exact verified DomainClaim binding, exact AssetRelease/profile-bound WorkloadRevision identity, automatic opaque Runtime Service-spec binding, hosted credential lifecycle/persistence and an internal one-shot Argon2id issuer, scope-complete healthy-target validation, exact grant/generation authority resolution, deterministic release-bound per-Gateway loopback-safe route planning with explicit receiving-node binding, conflict-safe multi-route complete-projection assembly, bounded exact active-policy enumeration and active release-bound Workload materialization for one tenant-qualified Gateway scope, retained policy/DomainClaim/Workload/credential CAS evidence and canonical ingress bindings, pure ordinary-plus-MCP physical snapshot composition, durable atomic staging, automatic desired-state convergence, restart-safe Fleet command dispatch, and exact acknowledgement/expiry projection now exist. The composer preserves ordinary routes, unions ordinary/MCP certificate authority, emits exact MCP routers and target services, rejects prefix bypasses, and removes stale MCP policy for an empty set. PostgreSQL staging locks the logical membership and complete policy/ordinary-route sets plus every Claim, Workload, credential, and physical-scope version, then atomically writes the pending publication, optional certificate, immutable MCP publication marker, scope advance, and secret-free Outbox fact. The marker now carries a stable logical desired-state digest and MCP route count. A cursor-fair bounded worker discovers active or previously published scopes, defers any node with a pending physical publication, replans and composes complete state, suppresses physical-identity and observation-time churn, stages changed or route-less expiry cleanup, retries due terminal failures, and repairs a non-empty MCP snapshot displaced by a later physical revision. An applied zero-route marker no longer makes ordinary-only changes MCP-owned. A separate bounded reconciler scans durable pending markers, idempotently enqueues the existing Fleet snapshot command, retains pending state across queue failure, fails expired delivery terminally, and projects only an exact node/command/revision/digest acknowledgement; Applied also advances installed scope and certificate readiness atomically. Focused tests cover no-op convergence, cursor fairness, pending deferral, empty cleanup, terminal retry, displacement repair, stable digest identity, queue interruption, restart replay, expiry, and future-clock fail-closed behavior. The compiled integration fixture additionally checks stored desired identity and automatic post-acknowledgement no-churn, alongside stale-policy rejection, final-Outbox rollback, Fleet replay, certificate readiness, and Applied acknowledgement. Node-wide aggregation for physical Gateways shared by multiple active MCP scopes, unified ordinary-publication composition, proactive MCP-only certificate renewal, revoked-credential cleanup, public idempotent credential delivery/rotation, lifecycle surfaces, control-plane audit, an executed real PostgreSQL gate, and joint Runtime/Gateway recovery evidence remain |
| `MCP0.4` | Gateway | Foundation in progress | Closed request parsing/auth, exact healthy-target selection, one-attempt no-replay dispatch, JSON/notification/SSE/subscription forwarding, cancellation, snapshot-swap old/new target isolation, and listener-first graceful drain pass focused tests; managed stale/rejected snapshots, forced drain, exact readiness, telemetry, real-client/server, fault, and release evidence remain |
| `MCP0.5` | Joint release gate | Planned | Prove one Box-hosted Service end to end through real Cloud, Runtime, and Gateway processes at exact revisions |
| `MCP0.6` | Joint production gate | Planned | Prove multi-replica and multi-node rollout, loss, partition, policy expiry, load, recovery, and cleanup after the required `H0` and `C0` foundations |

The ownership boundary is closed:

| Concern | A3S Runtime | A3S Cloud | A3S Gateway |
| --- | --- | --- | --- |
| Unit lifecycle | Apply, inspect, stop, remove, logs, provider recovery, and typed endpoint evidence for one Service replica | Declare and reconcile the desired Workload and every replica | Never create, schedule, or stop a Runtime Unit |
| Product identity | Bind an opaque semantics-profile digest | Own AssetRelease, immutable hosted MCP Service profile, and separately mutable route policy | Validate the profile digest on every target and the route policy in the complete applied snapshot |
| Replica and rollout | Give each replica a distinct Unit ID and generation | Own count, placement, health eligibility, rollout, rollback, drain order, and sole autoscaling decisions | Select only a healthy target from the complete applied set |
| MCP request path | No role | No synchronous role | Validate, authenticate, authorize, route, stream, cancel, and observe |
| Server capabilities | Treat the workload as a black box | Admit and pin the server release and capability contract | Forward `server/discover`; never invent tools, resources, prompts, or server identity |
| Durable state | Runtime receipts and observations only | Desired state, operations, grants, control-plane audit, and later retained request audit/usage | Applied snapshot/journal and bounded request-path telemetry only |

The modern transport contract requires:

- one POST endpoint and one JSON-RPC request or notification per HTTP request;
- protocol version and client metadata on every request, with
  `MCP-Protocol-Version`, `Mcp-Method`, and applicable `Mcp-Name` headers
  validated against the body before policy uses them;
- `server/discover` support from the hosted server;
- an immediate JSON response or request-scoped SSE, including long-lived
  `subscriptions/listen` streams with bounded backpressure and drain;
- Origin validation and request-level authentication;
- service-level authorization in `MCP0.5`: Gateway strips the client credential
  and forwards no ad hoc user/tenant identity header to the hosted server;
- no initialization handshake, `Mcp-Session-Id`, GET stream, DELETE session,
  sticky routing, or `Last-Event-ID` resumption; and
- no automatic replay after upstream dispatch begins. Statelessness removes
  session affinity; it does not make `tools/call` or an unknown method
  idempotent.

All concurrently eligible targets for one logical hosted MCP route must bind
the same semantics-profile digest. Cloud may mix old and new AssetRelease
targets only during an explicit rollout whose public profile digest is
unchanged. A release that changes the server protocol or discovery contract
uses a new immutable profile, a separately proven target set, and an
acknowledged cutover; Gateway must not expose a mixed contract as one logical
service.

The semantics-profile digest covers canonical hosted-server protocol behavior,
not the artifact, AssetRelease, or mutable Gateway route policy. Cloud binds
release identity separately in the Workload/target projection and binds route
policy through the Gateway snapshot revision and digest. An equal profile
digest never makes two releases interchangeable outside an explicit rollout.

`C0.2m` is a separate migration of Cloud's management MCP presentation
surface. It shares the modern wire requirements but is not an `MCP0` hosted
asset, Runtime Workload, Gateway route, or prerequisite for `MCP0.5`.

Delegated caller identity is a later `MCP0.6`/`C0.3` contract. If admitted, it
uses a versioned, audience- and profile-bound, short-lived signed assertion
with rotation, expiry, replay, and mixed-version evidence. Gateway never
forwards the external bearer credential or invents unsigned identity headers.
Durable per-request audit ingestion is likewise `MCP0.6`/`C0.3` work and must
reuse one ordered acknowledged Gateway-to-Cloud event path; `MCP0.5` audits
control-plane changes and retains bounded Gateway access evidence only.

Protocol baseline:

- [MCP 2026-07-28 versioning and compatibility](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [MCP 2026-07-28 Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [MCP server discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)

### 5.10 `U0`: A3S Use plugin assignments

`U0` adds a Cloud management service for A3S Use plugins without adding a
second plugin platform. Cloud owns registry enrollment and tenant desired
assignment; the shared A3S Use Plugin Manager remains the sole package
lifecycle application service.

| Sub-gate | State | Outcome | Dependency |
| --- | --- | --- | --- |
| `U0.1` | Planned | Pin exact Cloud/Use compatibility revisions, consume the canonical package/surface/plan/confirmation/receipt/observation and protocol-level-1 `PluginHostManager` contracts, and add one Node Agent adapter plus versioned Fleet payloads | A3S Use M0 contracts and the frozen managed-host contract; complete shared-manager composition still gates mutation |
| `U0.2` | Planned | Human-enrolled TUF registry references plus bounded signed catalog search/inspect through A3S Use, with REST/client/CLI/Web/Management MCP read parity and no package download | Completed A3S Use M1/M4 contracts and Cloud `C0.1`/`C0.2` |
| `U0.3` | Planned | One exact TUF package assignment to one explicit host/workspace, canonical plan review, `allow` or trusted-user `ask` confirmation, apply, enable/disable, uninstall, observation, and restart recovery for the upstream safe non-executable slice | A3S Use M2 parent-saga completion, Cloud `C0.3`, and Fleet replay; OKF waits for Use M0K-C-B |
| `U0.4` | Planned | Permission-bearing Tool Task, private Tool Service, standard MCP, Secret-reference, UI, and OKF host adapters with no provider fallback or Cloud-local surface lifecycle | A3S Use M5/M6 plus the named Runtime/Box, Workloads/Fleet, Edge/Gateway, Secrets, and Knowledge gates |
| `U0.5` | Planned | Independent multi-host assignment operations, node loss/replacement, mixed versions, supply-chain rotation/revocation, backup/restore, limits, and production operations without a group rollout aggregate | `U0.4`, A3S Use M7, `H0.3` through `H0.5` as applicable |

As of 2026-08-03, A3S Use already publishes the canonical managed-scope fence,
host capabilities, package identity, plan, digest-only apply, enablement,
observation, and `PluginHostManager` protocol-level-1 contracts. This upstream
readiness does not advance `U0.1`: Cloud still has no plugin module or product
capability until it pins an exact compatible Use revision and proves the Cloud
adapter, Fleet payloads, fixtures, and managed-scope fencing together.

The Cloud API has one assignment vocabulary and imports A3S Use's canonical
`PluginDesiredState`; it does not define a parallel lifecycle enum. The sole
assignment command selects an exact verified catalog record, canonical surface
set, workspace scope, target host, policy reference, and desired state of
`enabled`, `installed-disabled`, or `absent`. REST `DELETE`, CLI remove, and UI
disable actions are presentation mappings to that command, not additional
application handlers or workflows. A newer registry release never changes an
assignment automatically. The reconciler maps desired/observed drift to the
canonical Use install, upgrade, enable, disable, or uninstall operation; Cloud
does not expose parallel lifecycle aggregates for those verbs. Retry and
recovery use the existing Operation/Flow controls and resume the same
`cloud.plugin-assignment@1` run; there is no plugin-specific retry mechanism.

`U0.3` allows one workspace assignment for each package/host. A second
workspace cannot drive a conflicting version or surface plan against the same
Use-owned generation. Multi-workspace reuse waits for a canonical A3S Use
multi-scope parent saga and is not implemented as Cloud-side reference counting
or competing per-workspace flows.

The single-authority split is mandatory:

| Concern | Authority | Cloud projection |
| --- | --- | --- |
| Tenant registry enrollment and assignment intent | Plugins context in PostgreSQL through A3S ORM | Full desired aggregate |
| Signed catalog, package identity, permission ceiling, and dependency closure | A3S Use and TUF | Exact verified record/digests needed for selection and review |
| Immutable plan and confirmation semantics | Canonical `a3s-use-core` contracts | Digest plus bounded immutable review projection |
| Install, generation cutover, grants, bindings, capability publication, drain, and cleanup | Shared A3S Use Plugin Manager | Exact receipt, installed/capability generation, and applied observation only |
| Remote orchestration and delivery | Existing Operations/Flow and Fleet/Node Agent journal | One `cloud.plugin-assignment@1` Operation and existing command records |
| Placement, execution, routing, Secrets, and knowledge indexing | Host-local surfaces use the explicitly injected Runtime/Box and private Use bindings; Cloud-managed/public services remain Workloads/Fleet/Edge/Gateway; Secrets and A3S Knowledge retain their boundaries | References and canonical receipts only |
| Audit and management surfaces | Shared Cloud audit plus one Plugins command/query bus | REST, Web, CLI, and Management MCP adapters only |

The Node Agent invokes the shared Plugin Manager through a typed library/host
adapter. It never shells out to `a3s use`, calls the local manager MCP, accepts
a raw executable/provider/endpoint from Cloud, or opens another management
port. The existing Fleet command queue and Node Agent journal carry bounded
plan, apply, enablement, and observation payloads. A3S Use's local operation
journal then owns its nested package saga; Cloud Flow waits for its exact
result rather than reproducing its stages.

`U0` deliberately does not:

- add `plugin` to the closed Cloud `AssetKind` set or split one Use package
  into synthetic Agent, MCP, Skill, Tool, UI, or OKF Assets;
- copy A3S Use catalog schemas, TUF verification, package bytes, receipts,
  Workspace Grants, Runtime Bindings, Route Leases, dependency/reference-count
  state, capability registry, or surface reconciler into PostgreSQL;
- proxy the A3S Use management MCP or define a universal
  `execute(plugin, action, payload)` API;
- allow an agent to enroll/rotate trust roots, approve an `ask` plan, install
  unsigned local content, grant Secret authority, select a provider, or purge
  user data; or
- create a plugin scheduler, deployment engine, Runtime provider, Gateway
  route owner, knowledge index, command queue, event bus, audit store, object
  client, or Redis-backed authority.

A plugin Tool Service or MCP surface is private to its assigned Use workspace.
`U0` never turns it into a public or replicated Cloud service. That product
outcome requires an explicit immutable A0/MCP0 release and the ordinary
Workloads/Fleet/Edge/Gateway lifecycle; no automatic promotion or mirrored
deployment is planned.

`U0.3` closes only after process death at assignment commit, plan persistence,
confirmation commit, apply dispatch, capability cutover, and observation
acknowledgement converges to one desired generation and one exact Use receipt.
The prior capability generation remains active until the shared Plugin Manager
publishes its complete replacement. Plan expiry, trust/policy/provider drift,
node loss, partial cleanup, or unknown schemas remain explicitly blocked or
unavailable; Cloud never infers success from an enabled flag or missing host.

## 6. Near-term execution order

The default portfolio priority is:

1. complete `BX0.1` through `BX0.5`, retain the old provider evidence only as
   historical regression coverage, and re-certify `R0` through `E0`, `G0`,
   `H0.1`, and `H0.2` on exact Box revisions;
2. freeze `MCP0.1` immediately as a contract-only slice while provider work
   continues; it may not claim hosted MCP availability;
3. execute and retain the remaining operator-owned `G0` certification, preserve
   the verified `A0.2` repository gate, and close `A0.3` so hosted MCP can bind
   a published immutable release;
4. after their dependencies pass, advance Runtime `MCP0.2`, Cloud `MCP0.3`,
   and Gateway `MCP0.4` in parallel, then close only through the joint
   single-node `MCP0.5` gate;
5. complete `PW0.1` and make the immutable Box-hosted Power profile the first
   `I0` backend;
6. preserve the verified `A1.0` shared-infrastructure regressions while
   advancing `C0.3`, `C0.2m`, the contract-only `U0.1`, and the first `S0`
   foundation independently when staffed; `U0.1` must change A3S Use rather
   than copy any missing canonical type into Cloud;
7. re-certify the `H0.1` real-provider Claim behavior while beginning
   `I0.0`, then follow the ordered inference slices without bypassing their
   generic platform dependencies;
8. start `P0` only on verified `G0`; start `A1.1` after immutable published
   `A0.3` identities exist, add `A1.2` after `A0.4` Agent deployment, add
   `A1.3` after `A0.5` bindings, gate `A1.4` on `C0.3` grants and audit, and
   close `A1.5` only with exact checkpoint, suspend/resume, fork, and crash
   recovery evidence;
9. add read-only `U0.2` after the pinned A3S Use catalog/manager contracts
   pass, then start single-host `U0.3` only after the shared Manager mutation
   saga and `C0.3` authorization/audit are ready; keep executable and
   multi-host surfaces behind `U0.4` and `U0.5`;
10. re-certify the `H0.2` projection gate while advancing `H0.3`
   multi-node placement and networking;
11. close `MCP0.6` only after its `H0.3` multi-node and `C0.3` grant/audit
    dependencies pass; and
12. close full production packaging, HA, autoscaling, and inference hardening
    through `H0.4`, `H0.5`, and `I0.5`; and
13. claim native AX-plus-Kubernetes replacement only after the cumulative
    `A0.3` through `A0.5`, `A1.1` through `A1.5`, `C0.3`, `H0.3` through
    `H0.5`, and Box checkpoint/suspend/resume gates pass on a clean supported
    Linux installation.

This order expresses dependency and product risk, not equal staffing or a
calendar promise. The next implementation is the smallest vertical slice that
can pass a real exit gate.

## 7. A3S Gateway relationship

Gateway coordination is one part of the Cloud roadmap, not a replacement for
the Cloud product lanes above.

### 7.1 Product boundary

| Product | Position | Owns |
| --- | --- | --- |
| A3S Runtime | Provider-neutral Unit lifecycle | One Task or Service identity, generation, request replay, capability admission, typed endpoint observations, provider recovery, and cleanup; it owns no product profile or request protocol |
| A3S Cloud | Self-hosted control plane | Tenancy, identity, catalogs, A3S Use plugin assignments, Agent conversations and executions, approvals, checkpoints, Workloads, desired replicas, placement, rollout, autoscaling, complete Gateway policy, operations, usage ledger, and management surfaces |
| A3S Gateway | AI traffic and protocol data plane | Transport, TLS, streaming, local enforcement, healthy endpoint selection, modern MCP and OpenAI protocol handling, atomic snapshot application, request-path telemetry, and the planned durable usage spool; it does not own Agent execution state |

Cloud never becomes the per-request proxy or synchronous authorization
dependency. Gateway never becomes a tenant database, scheduler, production
rollout controller, production autoscaling authority, or long-term usage
ledger.

### 7.2 Gateway operating modes

| Concern | Standalone Gateway | Cloud-managed Gateway |
| --- | --- | --- |
| Desired-state authority | Operator-owned local ACL | Cloud PostgreSQL desired state |
| Traffic configuration | Local startup/watch/provider policy | Complete versioned ACL snapshot delivered through the node agent |
| Target lifecycle | External operator or orchestrator | Cloud Workloads and Edge |
| Rollout and autoscaling | Standalone experiments remain explicitly non-production until proven | Cloud is the only authority |
| Durable business state | None | Cloud |

A minimal managed bootstrap ACL may bind process, management listener,
identity, and Cloud-delivery settings. It cannot define or mutate managed
routes, target sets, rollout, or scaling policy.

### 7.3 Managed runtime contract

```text
Cloud commits desired state
  -> Cloud compiles one generic Runtime Service per desired replica
  -> Runtime/Box converges each Unit and publishes exact typed endpoints
  -> Cloud compiles one complete Gateway-scope ACL snapshot
  -> outbound node agent delivers identity, revision, digest, and validity
  -> Gateway natively applies, journals, and reports exact readiness
  -> node agent records the exact ready-applied or rejected result
  -> Cloud advances only after the matching acknowledgement
```

Gateway may temporarily suppress an unhealthy endpoint, open a circuit, or
drain a connection under the applied policy. It may never invent a target,
change desired weights, create a replica, or promote a revision.

The Cloud API, PostgreSQL, and workers stay off the request path. Authorization
and route snapshots are complete, bounded, and expiring; policy that requires
an unavailable or expired security snapshot fails closed. Retry and fallback
are allowed only where the compiled protocol policy permits them and before the
first response byte. For `MCP0`, Gateway may reselect before dispatch but must
not replay a request after upstream dispatch begins.

### 7.4 Coordinated gates

| Gate | Cloud work | Gateway work | Joint result |
| --- | --- | --- | --- |
| `E0` | Edge desired state, managed TLS, complete snapshots, and exact acknowledgement | Native snapshot apply, HTTPS, routing, health, durable recovery, and prior-revision preservation | Verified clean-host A-to-B-to-cloned-A route and recovery evidence remains the regression baseline |
| `H0.2` | Logical Gateway scopes, ordered membership, exact target derivation, atomic Route-and-rollout staging, threshold activation, per-member recovery, certificate convergence, and exact rollback | Explicit managed mode, advertised management-protocol tuple, native exact apply/readiness, same-digest renewal, durable journal, read-only observation, and rejection of local control loops | Verified against Gateway `7a146b6`: two real members converge independently, preserve service through member loss, recover from native journals, reject cross-member trust, and replay apply-before-ack without duplicate mutation; PostgreSQL 17 proves atomic staging, threshold projection, failure retention, recovery, rollback, and typed A3S ORM persistence |
| `MCP0` | Immutable hosted MCP profile, release binding, Runtime Service projection, replica/rollout authority, expiring authorization policy, complete Gateway ACL snapshot, operations, and audit | Modern `2026-07-28` header/body validation, local request authorization, stateless healthy-target selection, request-scoped SSE, cancellation, no post-dispatch replay, drain, and bounded telemetry | A real MCP client reaches a real Box-hosted server through exact Cloud/Runtime/Gateway revisions; discovery, denial, malformed headers, stream cancellation, process/node loss, rollout, recovery, and cleanup gates pass |
| `I0.2b` | Inference routes, keys, grants, typed local/global limits, and dispatch snapshots | Native OpenAI body-aware dispatch, cached enforcement, Redis-backed globally exact counters, streaming, and pre-first-byte fallback | Real SDK, denial, revocation, local and shared-counter enforcement, framing, disconnect, and acknowledgement gates pass |
| `I0.2c` | Usage ingestion, gaps, immutable ledger, rollups, and rollout authority | Durable ordered request/attempt spool, replay, backpressure, and weight execution | Every started request becomes terminal or visibly unknown after crash and replay |
| `I0.2d` | Same-environment credential-isolated Provider egress Workload | Route only to the internal egress target | Client and provider credentials never cross or enter traffic snapshots |
| `C0.3` + `I0.2e` | Grants, authorized search, key lifecycle, role-focused console, diagnostics, playground, and showback | Expose bounded operational state only | Consumer, steward, and operator surfaces cannot reveal an ungranted resource |
| `A1` + `C0` | Agent release binding, conversations, executions, approvals, checkpoints, identity, and management contracts | Remain transport-only if a future native Agent protocol is justified; do not persist conversations, schedule Harness work, grant approvals, or expose a direct client control path | No second asset, execution, identity, audit, or deployment authority appears in Gateway |
| `H0.3` through `I0.5` | Multi-node placement, Gateway HA, sole autoscaler, quotas, recovery, and provider policy | Private upstream identity, drain, exact-revision readiness, complete signals, and failure hardening | Node/Gateway loss, mixed versions, scale, backlog, and restore meet published limits |

No joint gate is complete because one repository passes unit tests alone.
Compatible Cloud and Gateway revisions must pass the real cross-repository
protocol and recovery gate.

## 8. Definition of done

A product gate is complete only when:

- its domain invariants, commands, queries, persistence, provider adapters,
  transport contracts, web, and applicable CLI/MCP surfaces land together;
- every mutation has tenant scope, idempotency, audit, timeout, cancellation,
  retry, cleanup, and documented error semantics;
- real-provider happy path, failure, process-death, replay, corruption, and
  cleanup gates pass from a clean environment;
- the owning installation gate passes on clean supported Linux with A3S Box and
  without AX, Kubernetes, Helm, CRDs, Operators, Docker, or a compatibility
  daemon when the capability is part of the native replacement outcome;
- Secret handling, authorization, revocation, SSRF, path/URL validation, and
  cross-tenant fixtures pass;
- upgrades, mixed versions, rollback, backup/restore, observability, and
  runbooks pass where the gate requires them;
- README, this roadmap, the owning detailed plan, API documentation, examples,
  and current-evidence tables describe the same verified behavior; and
- unsupported or unverified capability fails explicitly instead of degrading
  silently.

See the [development plan](docs/development-plan.md) and
[inference plan](docs/inference-plan.md) for complete per-gate evidence.

## 9. Product non-goals

The current roadmap does not include:

- a second deployment or scheduling path for imports, Agents, MCP, stateful
  resources, or inference;
- a second Agent event log, execution controller, Harness scheduler, job queue,
  node-control channel, or Redis-backed source of truth;
- a direct client-to-Agent, client-to-Harness, or client-to-Gateway execution
  control path;
- protocol sessions or sticky routing for modern `MCP0` requests;
- Cloud on the live request or token-stream path;
- a Cloud-equivalent control plane inside Gateway;
- training, fine-tuning, or notebook lifecycle inside `I0`;
- GPU host creation or SSH credential custody inside Inference;
- AX as a required Agent controller, event log, scheduler, configuration
  authority, or direct client control path;
- Kubernetes, Helm, CRDs, or Operators as an installation dependency or an
  alternative Workloads scheduler;
- plaintext credentials in ACL, desired state, operations, logs, or events;
- a built-in mail server or divergent native desktop feature set; or
- commercial billing inside the Cloud core.

New capabilities enter the roadmap only after they have one owning context,
one dependency path, a closed contract, and real failure, recovery, and cleanup
evidence.
