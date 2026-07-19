# A3S Cloud Domain Model

## 1. Domain objective

A3S Cloud manages the path from an immutable source to a healthy, reachable
workload on an operator-owned node. The model must support ordinary OCI
applications and A3S-native Agent, MCP, and Skill assets without pretending
that every platform object is an asset.

The domain uses ordinary transactional aggregates. It does not event-source all
business data. A3S Flow event-sources long-running operations, and A3S Event
distributes committed facts after the corresponding database transaction.

## 2. Ubiquitous language

| Term | Meaning |
| --- | --- |
| Organization | Tenant and billing/security boundary. |
| Project | Product grouping owned by one organization. |
| Environment | Isolated desired-state namespace such as production or staging. |
| Asset | Hosted reusable A3S unit. Its kind is exactly Agent, MCP, or Skill. |
| Asset revision | An immutable Git commit plus its validated manifest digest. |
| Asset release | An immutable, versioned publication of one asset revision and artifact. |
| Source | Origin used to produce a workload revision: hosted asset release, external Git commit, or OCI digest. |
| Artifact | Content-addressed build output or bundle. OCI artifacts use a manifest digest. |
| Workload | Environment-scoped desired long-running service. It is not an Asset. |
| Workload revision | Immutable desired runtime specification derived from one source. |
| Deployment | One attempt to make a workload revision active on a node. |
| Node | Enrolled Linux execution target running the A3S Cloud node agent. |
| Observation | Node-reported fact about the current provider resource and health. |
| Log chunk | One ordered stdout/stderr position for a Runtime unit generation, stored as verified object bytes with authoritative metadata until body retention leaves a durable tombstone and later compaction leaves a durable sequence range. |
| Route | Domain/path mapping from A3S Gateway to one healthy workload revision. |
| Domain claim | Tenant-scoped proof that an exact or one-label wildcard DNS pattern may be routed. |
| Gateway certificate | Public certificate lifecycle bound to one node, claim set, Gateway revision, command, and snapshot digest. |
| Managed database | Stateful platform service with an engine contract, persistent volume, backup policy, and lifecycle. It is not an Asset. |
| Persistent volume | Node/provider-backed durable storage with explicit attachment, retention, and backup state. |
| Backup | Immutable, verified snapshot descriptor stored outside the source volume. |
| Secret | Tenant-owned secret identity with immutable encrypted versions. |
| Operation | Durable A3S Flow run coordinating a deployment, build, backup, restore, rollback, or repair. |

Terms such as resource, package, release, and status must not be used without
their bounded context. An asset release, deployment result, and catalog listing
are different facts.

## 3. Bounded contexts

### 3.1 Identity and access

Owns users, organizations, memberships, roles, API tokens, and tenant context.
It answers who may issue a command. It does not decide runtime placement or
store asset collaborator data in an unvalidated metadata document.

Primary aggregates:

- `Organization`
- `Membership`
- `ApiToken`

### 3.2 Projects

Owns `Project` and `Environment`. An environment belongs to exactly one project
and carries configuration isolation boundaries. Deleting an environment is a
workflow, not a cascade hidden inside one request transaction.

Primary aggregates:

- `Project`
- `Environment`

### 3.3 Asset hosting

Owns hosted assets, repositories, revisions, releases, and asset-scoped access.
It deliberately excludes Issues, pull requests, stars, watches, wikis, generic
code repositories, knowledge bases, models, workflows, and social features.

Primary aggregates:

- `Asset`
- `AssetRelease`

The only legal asset kinds are:

```text
agent | mcp | skill
```

An Agent or MCP release may be deployed after it resolves to a digest-pinned OCI
artifact. A Skill release is a distributable bundle and may be bound to an
Agent workload, but it is never deployed independently.

### 3.4 Artifacts

Owns immutable artifact metadata, provenance, checksums, signatures, and
registry locations. Blob bytes live in an OCI registry or S3-compatible object
store. The database stores descriptors, never an image or repository file tree.

Primary aggregate:

- `Artifact`

### 3.5 Fleet

Owns enrollment, node identity, capabilities, scheduling eligibility, drain,
revocation, last accepted observation, and authenticated bounded log ingestion
and body-retention/compaction metadata. A node agent does not receive direct
database or NATS credentials. Log bodies are immutable object-store payloads
rather than Fleet table values.

Primary aggregate:

- `Node`

### 3.6 Workloads and deployments

Owns desired service state, immutable workload revisions, placement intent,
deployments, active revision selection, update, stop, and rollback.

Primary aggregates:

- `Workload`
- `Deployment`

`Workload` is the single deployment abstraction. Its source may be a generic
application image or an Agent/MCP release. This avoids parallel deployment
engines while preserving the stricter Asset domain. Workloads also owns the
tenant-authorized query that maps one exact revision and assigned deployment to
ordered Fleet log metadata; it does not become the owner of log bodies.

### 3.7 Edge routing

The implemented slice owns hostname/path rules, exact and one-label wildcard
domain claims, managed certificate public state, and the desired A3S Gateway
configuration revision. It resolves a route only from a healthy active workload
revision covered by verified claims, compiles one HTTPS-only snapshot, and does
not mark the route or certificate ready until the Gateway acknowledges that
exact complete snapshot. The node generates and retains the private key; the
control plane sees only a CSR and public certificate material.

Primary domain records:

- `Route`
- `DomainClaim`
- `GatewayCertificate`
- `GatewayScopeState`
- `GatewayPublication`

### 3.8 Secrets

Owns secret identities, encrypted versions, key rotation, materialization
authorization, and access audit. An immutable workload revision binds an exact
Secret version to a typed environment-variable or absolute-file target. Only
canonical references cross persistent application and Runtime boundaries.
Plaintext must not enter desired-state rows, domain events, Flow history,
Runtime state, Fleet commands, logs, or API responses.

Primary aggregate:

- `Secret`

### 3.9 Data services and storage

Owns managed database intent, persistent volume identity, attachment policy,
backup schedules, backup records, and restore operations. Databases and volumes
are platform resources, never Asset kinds. A managed database uses the common
Workload deployment path but adds engine-specific readiness, durability, and
restore invariants in this context.

Primary aggregates:

- `ManagedDatabase`
- `PersistentVolume`
- `Backup`

The first stateless deployment slice does not implement this context. Its
boundary is defined now so stateful behavior is not later hidden in workload
metadata or provider-specific JSON.

### 3.10 Operations and audit

Coordinates long-running work with A3S Flow and maintains query projections for
the UI. It consumes domain ports from other contexts; it does not mutate their
tables directly. Audit records are append-only and separate from event delivery.

## 4. Aggregate invariants

### Organization

- Every tenant-owned aggregate carries `organization_id`.
- Cross-organization references are rejected before persistence.
- The last organization owner cannot be removed.
- API token scopes cannot exceed the issuing member's effective permissions.

### Project and Environment

- Project names are unique within one organization.
- Environment names are unique within one project.
- Environment deletion requires all workloads to reach a terminal stopped or
  explicitly orphaned state.

### Asset

- `kind` is one of `agent`, `mcp`, or `skill`; unknown values fail closed.
- Asset names are unique within one organization namespace.
- The hosted Git repository is addressed by immutable `asset_id`, not its name.
- The default branch is mutable metadata; releases always pin a commit SHA.
- Archiving prevents new releases but never deletes existing releases.
- Asset ACL changes are read from a commit and validated before release.

### Asset release

- A published release is immutable.
- `(asset_id, version)` is unique.
- The release binds `commit_sha`, `manifest_digest`, and `artifact_digest`.
- Agent and MCP releases require an OCI artifact and runtime contract.
- Skill releases require a bundle artifact and cannot contain a workload spec.
- A yanked release remains addressable by existing deployments but is hidden
  from new selection.

### Artifact

- Identity is the content digest, not a mutable tag.
- Tags may be recorded as aliases but never used as deployment identity.
- Provenance records bind source revision, builder identity, and build operation.
- An artifact cannot be reassigned to another organization.

### Node

- Enrollment tokens are one-time, short-lived, and stored only as hashes.
- Node certificates are independently revocable and rotate before expiry.
- Only a ready, non-draining node may receive new work.
- Capabilities are observations; operators cannot claim an unsupported provider.
- A stale heartbeat changes scheduling eligibility but does not invent a failed
  deployment result.

### Workload

- A workload belongs to one environment.
- Desired state is `running` or `stopped`; operation progress is not stored here.
- Every revision is immutable and has a monotonically increasing generation.
- A revision pins a resolved source revision and artifact digest.
- At most one revision is active, but previous healthy revisions remain
  available for rollback until retention removes them.
- Secret and Skill bindings reference immutable versions and are part of the
  immutable revision template.
- Each Secret binding has a unique name and target and selects either an
  environment variable or an absolute file path plus mode. It must reference
  an active version in the workload's organization, project, and environment.

### Deployment

- `deployment_id` is also the idempotent business key for its Flow run.
- Repeating a deploy command with the same idempotency key returns the same
  deployment; a different request under that key is a conflict.
- Provider resource identity is recorded once and cannot change silently.
- Success requires Runtime convergence, a real health result, and gateway
  acknowledgement when a public route is requested.
- Failure never rewrites the previously active healthy deployment.

### Route

- A hostname/path tuple has one owner within a gateway scope.
- Route publication targets an immutable workload revision.
- The target port must be declared by that revision and resolved from current
  healthy Runtime evidence to a node-local HTTP origin.
- Gateway configuration is published as a complete revision with compare-and-
  swap semantics; partial route writes are forbidden.
- A gateway scope has at most one pending complete snapshot.
- Route, publication, Fleet command, and acknowledgement bind the same node,
  command ID, revision, snapshot digest, and original correlation ID.
- Every published route references verified, same-tenant claims that cover its
  canonical hostname and one certificate owned by the target node.
- Only the exact `applied` acknowledgement activates a route; a rejected
  publication cannot produce false activation.

### Domain claim

- A claim belongs to one organization, project, and environment.
- Exact patterns cover only the exact hostname. A wildcard such as
  `*.example.com` covers one label such as `api.example.com`, never the apex or
  a deeper name.
- Only a verified claim can authorize route and certificate publication.
- Verification and rejection are terminal from `pending`; only a verified claim
  can be revoked.

### Gateway certificate

- A certificate binds one node, a sorted nonempty claim set, the complete
  Gateway revision and command, its snapshot digest, and one sorted SAN set.
- Snapshot schema v2 digests the certificate request with the ACL; a legacy
  snapshot cannot carry certificate intent.
- PostgreSQL may store the CSR digest and public certificate chain, but never
  the private key or plaintext key material.
- `ready` requires valid issued material and the exact applied Gateway
  acknowledgement. A rejected reload cannot make a certificate ready.

### Secret

- Secret payloads use authenticated provider encryption with a key identifier;
  production Transit/KMS providers own their internal key hierarchy.
- Updating a secret creates a new version; it never mutates ciphertext in place.
- Deletion is blocked while a live workload revision references the version,
  unless an explicit force workflow records the impact.
- Durable workload, Runtime, Fleet, Flow, event, label, and API state carries
  only the canonical workload-revision, Secret-ID, and version reference.
- Materialization is authorized only for the authenticated node assigned to the
  exact bound revision while it is converging, or while it remains the current
  active revision of a running workload. Tenant scope and active Secret/version
  state are revalidated before decryption.
- Node material responses are short-lived and non-cacheable. Environment
  material exists only at Docker container creation; file material is written
  atomically beneath a Linux tmpfs root, bind-mounted read-only at the requested
  path, and removed when the provider resource is retired.

### Workload log

- A log identity binds the authenticated node, Runtime unit ID, immutable
  generation, provider cursor, strictly increasing sequence, observation time,
  stdout/stderr stream, checksum, and object key.
- Successful Runtime apply outcomes add active node-agent log targets, and
  successful remove outcomes retire the matching generation. The durable
  cursor advances only after the control plane validates an exact batch
  receipt.
- One node may have at most one persisted pending upload batch. Exact replay is
  idempotent; a changed batch, sequence, cursor, or object body is a conflict.
- PostgreSQL stores ordering and integrity metadata only. The log report body is
  stored as an immutable object and verified again before a tenant query returns
  its text.
- Every object adapter enforces create-once semantics. Exact byte replay is
  idempotent, while different bytes at the same derived object key are a
  conflict; reads revalidate the bounded body, report schema, and expected
  checksum.
- Development may use the filesystem adapter. The production security profile
  requires HTTPS S3-compatible storage selected through typed ACL, with
  credential values supplied only through named environment variables.
- Body retention is based on the control plane's durable receipt time. The
  worker deletes the object first and records `retained_at` only after that
  idempotent deletion succeeds; deletion or metadata-commit failures remain
  eligible for retry.
- A retained row remains in sequence order as an explicit `retained` gap.
  Concurrent workers compare-and-set the tombstone, and replay of its persisted
  batch is resolved before object writes so it cannot recreate the retained
  body.
- Tombstones have an independent retention age. A bounded transaction replaces
  eligible per-chunk rows and batch memberships with continuous sequence-range
  markers, coalescing adjacent ranges for the same node, unit, and generation.
  Batch headers and payload digests remain durable, so exact old-batch replay
  still returns its receipt without recreating objects.
- A compacted range is returned as an explicit `compacted` gap with inclusive
  sequence bounds and a compacted-chunk count. Individual cursor, observation,
  and stream values are not retained; compacted ranges therefore remain visible
  under a stream filter. An unseen sequence must advance beyond the maximum live
  or compacted sequence for its node, unit, and generation.
- Organization, workload, and revision ownership are checked before metadata is
  read. An object that is absent or fails verification produces an ordered
  `missing` or `corrupt` gap; storage transport failure is not disguised as a
  gap.
- Bound Secret material is resolved and redacted at the Docker log boundary.
  Failure to authorize or materialize every binding fails the log read closed.

### Managed database, volume, and backup

- A managed database belongs to one environment and references one immutable
  engine/version specification.
- A database upgrade and a restore always create operations; they are never
  implicit effects of editing metadata.
- A persistent volume has one stable identity independent of a workload
  revision and an explicit retain or delete policy.
- The first storage implementation permits at most one read-write attachment.
- A deployment cannot become active until every required volume attachment is
  observed at the same desired generation.
- A backup is successful only after its object digest and restore metadata have
  been verified outside the source volume.
- Backup retention never deletes the last verified recovery point while a
  database policy requires one.

## 5. Source model

Workload authoring accepts three source forms. Deployment always resolves them
to immutable identifiers before Runtime receives work.

```text
WorkloadSource
├── HostedAssetRelease { asset_release_id }  # Agent or MCP only
├── ExternalGit        { repository, commit_sha, build_recipe }
└── OciImage           { repository, digest }
```

Branches, tags, and image tags may be convenient request inputs. A resolver
must turn them into a commit SHA or OCI digest and store the resolved value in a
new workload revision. Reconciliation never resolves a mutable reference again.

## 6. State models

### Asset state

`Asset` uses only `active` and `archived`. Build progress does not belong to the
asset state.

`AssetRelease` uses:

```text
draft -> published -> yanked
```

Publishing is atomic after validation. A build failure leaves the draft and its
operation history intact.

### Node state

```text
pending -> ready -> draining -> revoked
               \-> offline -/
```

`offline` is a projection derived from heartbeat age. It is not written by the
node itself.

### Deployment operation state

```text
queued -> resolving -> applying -> verifying -> publishing -> succeeded
   |          |           |           |             |
   +----------+-----------+-----------+-------------+-> failed
   +------------------------------------------------> cancelled
```

This state is a projection of Flow history. Workload health is a separate
projection: `unknown`, `healthy`, `degraded`, or `unavailable`.

### Route state

```text
pending -> publishing -> active
                     \-> rejected
```

`pending` exists only while constructing the aggregate. Persistence atomically
stores the staged route as `publishing` with its complete Gateway publication.
`active` and `rejected` require an exact terminal Gateway acknowledgement.

### Domain claim state

```text
pending -> verified -> revoked
       \-> rejected
```

### Gateway certificate state

```text
provisioning -> issued -> ready -> revoked
            \-> failed
```

The node may replay the same CSR after interruption. The control plane returns
the same public material for the same CSR digest and rejects a conflicting CSR.

## 7. Data ownership

| Fact | Authoritative owner |
| --- | --- |
| Tenant, project, environment, desired workload | PostgreSQL domain tables |
| Asset repository refs and objects | Git repository store |
| Asset release and artifact descriptors | PostgreSQL domain tables |
| Artifact bytes | OCI registry or S3-compatible object store |
| Operation history | A3S Flow PostgreSQL event store |
| Operation summary | Rebuildable PostgreSQL projection |
| Provider resource and live health | Node agent plus Runtime provider |
| Last accepted observation | PostgreSQL fleet/deployment projection |
| Route desired state, Gateway scope, and publication identity | PostgreSQL Edge tables |
| Domain claims and Gateway certificate public material | PostgreSQL Edge tables |
| Gateway active config | A3S Gateway, keyed by config revision |
| Gateway private key and CSR files | Node-local managed certificate directory |
| Secret identity and encrypted immutable versions | PostgreSQL Secret tables |
| Workload Secret bindings and canonical references | Immutable workload revision and reference-only Runtime/Fleet state |
| Transient Secret material | Authorized control-plane decryption and node-local Docker create boundary; file targets use Linux tmpfs only |
| Durable Runtime log cursor and pending upload | Node-agent secure state, keyed by unit and generation |
| Log chunk ordering, cursor, stream, checksum, object key, retained tombstone, compacted range, and batch replay header | PostgreSQL Fleet telemetry tables |
| Log chunk report bodies | Immutable object storage selected by typed ACL; filesystem adapter for development and HTTPS S3-compatible storage for production |
| Database intent, volume identity, and backup descriptors | PostgreSQL domain tables |
| Provider volume attachment and live database health | Node agent plus Runtime provider |
| Backup bytes | S3-compatible object storage |
| Integration notifications | A3S Event; never the sole source of truth |

## 8. Domain events

Event keys are lowercase and dot-separated. Events are facts in past tense and
carry a versioned envelope:

```text
identity.organization.created
project.environment.created
asset.asset.created
asset.release.published
artifact.artifact.registered
fleet.node.enrolled
fleet.node.observed
workload.revision.created
deployment.deployment.requested
deployment.deployment.succeeded
deployment.deployment.failed
edge.route.publication-staged
edge.domain-claim.created
edge.domain-claim.verified
edge.domain-claim.rejected
edge.domain-claim.revoked
secret.secret.created
secret.version.created
secret.version.revoked
data.database.provisioned
data.backup.completed
```

Each envelope includes `event_id`, `event_key`, `schema_version`,
`organization_id`, `aggregate_id`, `aggregate_version`, `occurred_at`,
`correlation_id`, `causation_id`, and a typed payload. The command transaction
writes the aggregate and outbox row together. A relay publishes the row through
A3S Event and records delivery without changing the domain result.

## 9. Explicit exclusions

The first architecture does not implement:

- asset kinds other than Agent, MCP, and Skill;
- pull requests, Issues, stars, watches, wikis, or social graphs;
- a generic digital-asset metadata bag;
- mutable-tag deployments;
- database writes from node agents;
- direct node access to NATS;
- SSH as the normal control channel;
- event-only reconciliation;
- a second deployment engine for Agent or MCP workloads.
