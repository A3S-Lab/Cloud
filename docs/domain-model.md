# A3S Cloud Domain Model

## 1. Domain objective

A3S Cloud manages the path from an immutable source to a healthy, reachable
workload on an operator-owned node. The model must support ordinary OCI
applications and A3S-native Agent, MCP, and Skill assets without pretending
that every platform object is an asset.

Planned I0 adds models and inference deployments as a separate product profile
that compiles into the same Workloads path; it does not broaden the Asset kind
set or create a second deployment engine.

The domain uses ordinary transactional aggregates. It does not event-source all
business data. A3S Flow event-sources long-running operations, and A3S Event
distributes committed facts after the corresponding database transaction.

## 2. Ubiquitous language

| Term | Meaning |
| --- | --- |
| Organization | Tenant and security boundary. Commercial billing remains externally owned. |
| Project | Product grouping owned by one organization. |
| Project attribution profile | Immutable project showback metadata containing a business-owner reference, optional external cost-attribution code, and validated labels; it is not a price or billing account. |
| Environment | Isolated desired-state namespace such as production or staging. |
| Asset | Hosted reusable A3S unit. Its kind is exactly Agent, MCP, or Skill. |
| Asset revision | An immutable Git commit plus its validated manifest digest. |
| Asset release | An immutable, versioned publication of one asset revision and artifact. |
| Source | Origin used to produce a workload revision: hosted asset release, external Git commit, or OCI digest. |
| Source webhook delivery | An authenticated provider-level branch-push fact keyed by provider and delivery ID; first acceptance may atomically derive tenant revisions through exact active subscriptions. |
| Artifact | Content-addressed build output or bundle. OCI artifacts use a manifest digest. |
| Inference model | Tenant-scoped logical model with immutable, resolved model revisions. It is not an Asset. |
| Inference backend | Versioned, typed compiler profile that turns one model-serving revision into a generic Workload execution plan. |
| Inference deployment | Environment-scoped model-serving intent projected into one inference-managed Workload. |
| Inference route | External model name, target and fallback policy projected into an Edge target set. |
| Workload | Environment-scoped desired long-running service. It is not an Asset. |
| Workload revision | Immutable desired runtime specification derived from one source. |
| Deployment | One attempt to make a workload revision active on a node. |
| Node | Enrolled Linux execution target running the A3S Cloud node agent. |
| Observation | Node-reported fact about the current provider resource and health. |
| Log chunk | One ordered stdout/stderr position for a Runtime unit generation, stored as verified object bytes with authoritative metadata until body retention leaves a durable tombstone and later compaction leaves a durable sequence range. |
| Provider log gap | One ordered, bodyless cursor-loss or source-disconnect boundary for a Runtime unit generation. |
| Route | Domain/path mapping from A3S Gateway to one healthy workload revision. |
| Gateway route cutover | Durable candidate route set and exact Gateway publication identity used to replace all routes for one workload update without mutating the active rows before acknowledgement. |
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

Owns `Project`, its current immutable attribution-profile reference, and
`Environment`. An environment belongs to exactly one project and carries
configuration isolation boundaries. Deleting an environment is a workflow,
not a cascade hidden inside one request transaction. Project attribution is
non-monetary metadata for audit and usage showback; this context does not own
pricing, balances, invoices, settlement, tax, or commercial entitlements.

Primary aggregates:

- `Project`
- `Environment`

### 3.3 External sources

Owns tenant-to-provider installation identity, authenticated source-provider
delivery facts, and immutable external application source revisions accepted
after provider resolution. It deliberately does not own hosted A3S assets,
mutable provider refs, durable provider credentials, build execution,
artifacts, or deployments.

Primary aggregates:

- `GithubConnection`
- `GithubConnectionFlow`
- `GithubRepositorySubscription`
- `ExternalSourceRevision`
- `SourceWebhookDelivery`

The initial provider is GitHub. Provider adapters may resolve convenient refs,
but the immutable revision accepts only a canonical repository identity, a full
commit object ID, and an explicit versioned build recipe. The GitHub App
connection flow verifies one installation through OAuth user authority. An
environment-owned repository subscription then binds that connection and
installation to one canonical repository, exact branch, and recipe. The
provider inbox authenticates and deduplicates typed branch-push facts; only a
new delivery may create revisions through matching active subscriptions.
Connection, subscription, inbox, and revision state contain no durable provider
credential. A bounded installation-authority reconciler polls GitHub with an
App JWT and persists only typed lifecycle/account observations plus generic
check health. The same authority boundary is required immediately before any
private-repository credential is issued.

### 3.4 Asset hosting

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

### 3.5 Artifacts

Owns immutable artifact metadata, provenance, checksums, signatures, and
registry locations. Blob bytes live in an OCI registry or S3-compatible object
store. The database stores descriptors, never an image or repository file tree.

The implemented G0 build boundary lives here rather than in Sources or Runtime.
Its typed Build service and `cloud.build@2` Flow bind a build ID, checked-out
content digest, recipe, Runtime Task identity, and validated OCI root descriptor
to exact Artifact receipts. The BuildKit adapter verifies every referenced blob
and requested platform before accepting the result. Registry publication state
is bound to the validated OCI result; provenance, SBOMs, and signatures remain
subsequent boundaries. The node-transfer store
persists command-scoped directory archives by digest so Runtime input/output
bytes can cross the existing mTLS node boundary without pretending that cache
objects are published OCI artifacts.

Primary aggregate:

- `Artifact`

### 3.6 Fleet

Owns enrollment, node identity, capabilities, scheduling eligibility, drain,
revocation, last accepted observation, and authenticated bounded log ingestion
and body-retention/compaction metadata. A node agent does not receive direct
database or NATS credentials. Log bodies are immutable object-store payloads
rather than Fleet table values.

Primary aggregate:

- `Node`

### 3.7 Workloads and deployments

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

### 3.8 Edge routing

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
- `GatewayRouteCutover`
- `GatewayCertificateConvergence`

### 3.9 Secrets

Owns secret identities, encrypted versions, key rotation, materialization
authorization, and access audit. An immutable workload revision binds an exact
Secret version to a typed environment-variable, absolute-file, or artifact
registry-credential target. Only canonical references cross persistent
application and Runtime boundaries. Plaintext must not enter desired-state
rows, domain events, Flow history, Runtime state, Fleet commands, logs, or API
responses.

Primary aggregate:

- `Secret`

### 3.10 Data services and storage

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

### 3.11 Inference platform (planned I0)

Owns model and backend catalogs, immutable model-serving revisions, model-level
routes and access-policy revisions referencing Identity principals, external
provider targets scoped to one environment, model-aware scaling intent, and the
append-only inference usage ledger. It compiles model-serving intent into the
common Workloads path and never schedules a provider process or writes Fleet,
Workloads, Edge, Identity, Secrets, or Operations tables directly.

Primary aggregates:

- `InferenceModel`
- `InferenceBackend`
- `InferenceDeployment`
- `InferenceRoute`
- `ExternalModelProvider`

Primary append-only records:

- `InferenceUsageRecord`
- `InferenceUsageAttempt`

Inference does not own replicas, placement members, accelerator claims, node
cache state, instance endpoints, Gateway acknowledgements, or operation status.
Those facts are composed from their authoritative contexts. The complete
planned boundary is defined in [`inference-plan.md`](inference-plan.md).

### 3.12 Operations and audit

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
- Each accepted project-attribution update creates a new immutable reference.
  It contains a tenant-local business-owner reference, an optional external
  cost-attribution code, and validated bounded labels; changing the current
  reference never rewrites an audit or usage fact that selected an older one.
- Environment deletion requires all workloads to reach a terminal stopped or
  explicitly orphaned state.

### GitHub source connection

- A Cloud organization owns at most one current (`active` or `suspended`)
  GitHub connection. A current numeric GitHub installation ID and an
  `(account_kind, account_id)` identity may each belong to at most one Cloud
  organization. Terminal connections remain history under immutable IDs.
- Installation setup and OAuth are two stages of one expiring flow. Each stage
  has an independent random 32-byte state, PostgreSQL stores only its SHA-256
  digest, and advancing or completing a stage makes it single-use.
- The setup-provided installation ID is untrusted until the OAuth user token
  can see that exact ID through GitHub's user-installations API.
- S256 PKCE binds the OAuth callback. Only the verifier digest is durable; the
  verifier itself exists in a short-lived secure, HTTP-only, same-site cookie.
- Completion stores durable numeric installation, account, and verifying-user
  IDs, account kind, display logins, `active` status, aggregate version, and
  connection/update time plus initial provider-check timestamps in the same
  transaction as
  `source.github-connection.created`.
- OAuth code, client secret, user access/refresh token, PKCE verifier, and
  provider response bytes are transient and never enter the aggregate,
  PostgreSQL, event payload, response, or error.
- A connection remains durable installation/account ownership only; it stores
  no credential. Anonymous source failure may use that same tenant authority to
  issue one short-lived, repository-bound, read-only installation token for
  resolution or checkout only while status is `active`. Repository
  subscriptions are separate environment-owned aggregates.
- Due active/suspended connections are inspected through
  `GET /app/installations/{installation_id}` using a fresh App JWT. Successful
  observations reconcile suspension, login, deletion, and exact numeric account
  identity. Provider uncertainty records a generic bounded retry state without
  granting authority; malformed or identity-confused responses fail closed.
- Last successful check, last attempted check, next check, consecutive failures,
  and a closed error category are durable. Saves compare the expected aggregate
  version and atomically emit `source.github-connection.reconciled` only when
  lifecycle or account-login state changed.
- Private token issuance requires a fresh successful observation for the exact
  organization, connection, and installation and then rechecks `active` state.
  The underlying issuer is never called when provider authority is unavailable
  or terminal, so both authenticated resolution and checkout fail closed.
- Signed installation suspend/unsuspend/delete, installation-target rename, and
  verifying-user App-authorization revocation facts reconcile only current
  connections. Same numeric account identity may update its login; account
  ID/kind mismatch fails closed. Each changed aggregate advances its version
  and emits `source.github-connection.reconciled` atomically with the state.
- `verification_revoked` is immediately terminal. A webhook-produced
  `installation_deleted` or `account_changed` status remains eligible for
  provider confirmation while its last successful check predates the webhook;
  this repairs a delayed fact when GitHub still reports the exact active or
  suspended installation. A provider-confirmed deletion/account drift is
  terminal for that connection ID. A fresh installation/OAuth flow creates a
  new ID and never transfers subscriptions from the historical connection;
  optimistic versions and current-connection uniqueness prevent an old repair
  from changing the replacement.
- GitHub exposes no tokenless API for querying the verifying user's current App
  OAuth grant. User access/refresh tokens remain non-durable, so signed
  `github_app_authorization.revoked` delivery is authoritative for that state.

### GitHub repository subscription

- A subscription belongs to exactly one organization, project, and environment
  and references that organization's verified GitHub connection plus its exact
  installation ID. Both ownership chains are PostgreSQL foreign keys.
- The binding contains one canonical allowlisted GitHub repository, one exact
  safe branch without a `refs/` prefix, and one validated explicit recipe plus
  its canonical digest.
- Active natural identity is organization, project, environment, connection,
  repository, branch, and recipe digest. An active duplicate returns the same
  logical resource; an inactive historical record does not block a new binding.
- State is only `active -> inactive`. Deactivation is explicit, retained,
  versioned, idempotent, and terminal for that aggregate.
- Only active subscriptions can authorize webhook fanout. Installation,
  connection, repository, or branch mismatch creates no tenant revision and
  exposes no tenant state to the provider response. PostgreSQL also requires
  and locks the exact joined connection in `active` state during fanout.
- Subscription API, idempotency state, database rows, and events contain no
  access token, private key, credential reference, or raw webhook body.

### External source revision

- A revision belongs to exactly one organization, project, and environment.
- The repository identity is canonical and provider-qualified. The initial
  provider accepts only exact HTTPS GitHub owner/repository locators permitted
  by the configured allow/deny policy.
- A revision pins a full Git commit object ID. A branch or tag is never stored
  as execution authority and is never resolved again by reconciliation.
- A typed branch, tag, or full commit input is resolved through the source
  provider exactly once for a new idempotent request. Replay returns the
  already accepted revision without contacting the provider, so later ref
  movement cannot alter the pinned commit.
- The versioned build recipe is explicit, path-safe, platform-ordered, and
  bound by a canonical SHA-256 digest.
- The same environment, repository, commit, and recipe digest identify one
  logical revision. HTTP replay and canonical duplicates return that revision.
- A webhook delivery identity is bound to the repository-plus-commit digest.
  Reusing it for another source identity conflicts atomically.
- Credential values and references do not enter the revision, its idempotency
  response, or its domain event.
- Checkout is a separate provider-neutral service over the accepted canonical
  repository and full commit. One checkout ID is immutable, replay revalidates
  its credential-free content digest, unsupported gitlinks and escaping
  symlinks fail closed, and Git metadata is never part of the build context.

### Source webhook delivery

- Provider authentication covers the exact bounded raw request body. GitHub
  uses HMAC-SHA256 with a secret read from its configured environment variable
  for every request; an A3S bearer token is never an alternative proof.
- Only a signed non-deleted branch push becomes a `SourceWebhookDelivery`.
  Supported signed connection-lifecycle events become a separate typed
  lifecycle receipt. Other authenticated events are acknowledged without
  durable state.
- A delivery records provider, bounded delivery ID, canonical repository,
  positive installation ID, safe branch, full nonzero commit ID,
  exact-payload SHA-256 digest, and canonical receipt time. Raw payload and
  secret material are never stored.
- `(provider, delivery_id)` identifies one provider fact. Replaying the exact
  payload returns the first fact; reusing the key with another payload or typed
  identity conflicts atomically.
- The inbox identity remains provider-level. Only first acceptance joins exact
  active subscriptions by authoritative connection ID, installation,
  repository, and branch while requiring the joined connection to remain
  `active`, then creates each matching environment/recipe revision, tenant
  delivery reservation, and `source.revision.accepted` outbox fact in the same
  transaction. Replay never re-runs fanout. It still does not create a build or
  deployment.
- A lifecycle receipt stores provider, bounded delivery ID, event/action,
  installation-or-user subject, exact-payload digest, and canonical receipt
  time. First acceptance locks matching active/suspended connections and
  commits every state/outbox change with the receipt. Exact replay changes
  nothing; reuse with a changed action, subject, or digest conflicts. Raw body
  and credentials remain absent.
- Immediate lifecycle ordering is webhook-receipt and aggregate-version driven.
  Periodic App-JWT installation inspection repairs missed/out-of-order
  installation and account facts; terminal webhook observations that postdate
  the last provider check are revalidated. Every private credential requires a
  fresh successful inspection before authenticated resolution or checkout.
  Verifying-user OAuth revocation remains signed-webhook authoritative because
  user tokens are deliberately non-durable.
- The provider delivery is distinct from an optional
  `ExternalSourceRevision` webhook reservation supplied through the
  authenticated tenant mutation.

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
- Each Secret binding has a unique name and target and selects an environment
  variable, an absolute file path plus mode, or the artifact registry
  credential. It must reference an active version in the workload's
  organization, project, and environment.

### Deployment

- `deployment_id` is also the idempotent business key for its Flow run.
- Repeating a deploy command with the same idempotency key returns the same
  deployment; a different request under that key is a conflict.
- New operations use `cloud.deployment@2`; version 1 is executable only for
  persisted-run compatibility.
- A workload has at most one nonterminal deployment. An update requires an
  active running workload and commits a complete new immutable template.
- Manual rollback requires an older revision of that same active running
  workload and at least one successfully activated deployment for the source.
  Current, newer, failed, unresolved, missing, and cross-workload sources are
  rejected.
- Rollback never reactivates the source revision ID. It clones the source's
  resolved template and template digest into the next generation, pins the
  request to the resolved artifact digest, revalidates its Secret bindings, and
  records the source revision in the new deployment operation.
- Exact rollback replay returns the originally committed deployment before
  consulting mutable workload or Secret state. Reusing the key for another
  source revision is an idempotency conflict.
- An update candidate is scheduled on the previous Runtime node. It cannot
  change the active revision or routes before current-generation health
  succeeds and any required route cutover is exactly acknowledged.
- Cancellation closes at `verifying`. Once health is verified, the deployment
  must converge forward or fail while preserving the prior selection.
- Provider resource identity is recorded once and cannot change silently.
- Success requires Runtime convergence, a real health result, and gateway
  acknowledgement when a public route is requested.
- Failure never rewrites the previously active healthy deployment.
- After candidate activation, `retiring` means the new revision is selected
  while deterministic cleanup of the previous Runtime revision is still
  required. Only durable stopped-or-absent evidence makes it terminal
  `active`.

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

### Gateway route cutover

- One cutover belongs to one deployment and binds the previous and candidate
  immutable revisions, workload node, Gateway revision, deterministic command,
  certificate, snapshot digest, and complete candidate route set.
- Staging validates every current active route for the workload and persists
  the candidate projections separately. The active route rows remain
  byte-identical while the cutover is `pending`.
- An acknowledgement must match the exact node, command, Gateway revision, and
  snapshot digest. A mismatch cannot change either the cutover or live routes.
- `rejected` preserves the previous routes and active workload revision.
  `applied` atomically replaces every affected route target; candidate
  activation requires this durable state.

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
- A ready certificate becomes obsolete only after a newer Gateway revision is
  installed and no active route references it. Provider revocation must
  succeed before the public projection moves to `revoked`; failure remains
  retryable.

### Gateway certificate convergence

- A convergence binds one node/revision/command/digest to the previous
  installed certificate, an optional replacement certificate, and
  aggregate-versioned retained and rejected route sets.
- Reasons are renewal, revoked domain ownership, provider-certificate
  revocation, or projection repair. Every active route must appear exactly once
  in the retained or rejected set at staging.
- Staging never changes active route rows. An exact rejected acknowledgement
  leaves the old routes and certificate authoritative. An exact applied
  acknowledgement atomically binds retained routes to the replacement,
  rejects revoked-claim routes, and advances the installed revision.
- A convergence whose routes are all rejected has no replacement certificate
  or certificate request; its complete snapshot retains only the Gateway
  management endpoint.

### Secret

- Secret payloads use authenticated provider encryption with a key identifier;
  production Transit/KMS providers own their internal key hierarchy.
- Updating a secret creates a new version; it never mutates ciphertext in place.
- A committed rotation event advances every older binding on each active
  revision of a running workload by deriving a new immutable revision. The
  resolved artifact digest and unrelated template fields do not change.
- The derived revision, deployment operation, causal event, and restart record
  commit together after the Secret version commit. A unique event/workload key
  and terminal event checkpoint make worker replay idempotent.
- Deletion is blocked while a live workload revision references the version,
  unless an explicit force workflow records the impact.
- Durable workload, Runtime, Fleet, Flow, event, label, and API state carries
  only the canonical workload-revision, Secret-ID, and version reference.
- Node materialization is authorized only for the authenticated node assigned
  to the exact bound revision while it is converging, or while it remains the
  current active revision of a running workload. The authoritative artifact
  resolver may also materialize an exact registry-credential binding
  transiently after an authentication challenge. Both paths revalidate tenant
  scope and active Secret/version state before decryption; the artifact path
  additionally revalidates project and environment scope.
- Node material responses are short-lived and non-cacheable. Environment
  material exists only at Docker container creation; file material is written
  atomically beneath a Linux tmpfs root, bind-mounted read-only at the requested
  path, and removed when the provider resource is retired. Registry credential
  material exists only while the control plane answers a manifest
  authentication challenge or Docker constructs an authenticated pull for the
  exact artifact registry, and is never projected into the container or
  durable workflow state.

### Workload log

- A log identity binds the authenticated node, Runtime unit ID, immutable
  generation, provider cursor, strictly increasing sequence, observation time,
  stdout/stderr stream, checksum, and object key.
- Successful Runtime apply outcomes add active node-agent log targets, and
  successful remove outcomes retire the matching generation. The durable
  cursor advances only after the control plane validates an exact batch
  receipt.
- One node may have at most one persisted pending upload batch. Exact replay is
  idempotent; chunk and provider-gap counts and memberships are durable, and a
  changed batch, sequence, cursor, reason, or object body is a conflict.
- Runtime distinguishes retryable provider/transport failure from permanent
  `cursor_lost` and `source_disconnected` boundaries. The node accepts a
  boundary only when its unit, generation, and requested cursor match exactly,
  then persists and replays it like a chunk.
- After a gap receipt, the node clears the provider cursor while retaining the
  Cloud sequence watermark. It resumes from the earliest available provider
  record and rebases later chunks monotonically. A continuous disconnect is
  emitted once and is re-armed only after the source succeeds again.
- PostgreSQL stores ordering and integrity metadata only. The log report body is
  stored as an immutable object and verified again before a tenant query returns
  its text.
- Every object adapter enforces create-once semantics. Exact byte replay is
  idempotent, while different bytes at the same derived object key are a
  conflict; reads revalidate the bounded body, report schema, and expected
  checksum.
- An exact object published before a lost control-plane process but without a
  PostgreSQL receipt is adopted by batch retry. Once the receipt exists, replay
  returns that receipt and never overwrites an object that later fails
  verification; the ordered query exposes corruption instead.
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
  chunk, provider gap, or compacted sequence for its node, unit, and generation.
- Provider gaps are returned in the same ordered page as chunks and compaction
  ranges with reason `provider_cursor_lost` or `provider_disconnected`. Their
  stream is unknown, so filters never hide them; the source cursor is nullable.
- Live delivery reads the same tenant-authorized ordered projection in batches
  of at most 16 records. An SSE event is capped at 8 MiB and binds its terminal
  sequence to both `id` and `nextCursor`; reconnect resumes from
  `Last-Event-ID`.
- The web log window is transient, deduplicates sequence replay, and retains at
  most 500 records. It creates no second durable cursor or log-body store.
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

### Inference model, backend, deployment, route, and usage (planned I0)

- A ModelResolutionAttempt carries retry and failure state. Only a successful,
  fully verified attempt creates and seals a ModelRevision that binds the
  immutable manifest digest. An attempt executes in one environment and may
  bind only a Secret version from that environment through a scoped Artifact
  materialization grant. Model bytes remain in an Artifact store.
- A BackendRevision binds a digest-pinned image, a typed compiler profile, and
  declared accelerator, model-format, network, health, and protocol support.
- An InferenceDeployment revision references exact model and backend revisions
  and compiles deterministically to one inference-managed Workload owner
  generation and spec digest.
- Inference does not persist replica, placement-member, device-claim, endpoint,
  node-cache, Gateway-acknowledgement, or operation state.
- An InferenceRoute alias is unique within its environment and references only
  same-tenant local deployments or explicitly registered external providers.
- Every InferenceRoute revision binds one immutable, same-environment Edge
  reference containing DomainClaim, logical Gateway scope, canonical hostname,
  path and binding generation. Claim revocation fails the route closed; scope
  migration requires a new route revision and acknowledged Edge cutover.
- An ExternalModelProvider and its egress Workload bind only a Secret version
  from their own environment.
- Identity owns environment-scoped inference-key verifier hashes, issuance
  generation, expiry and revocation. Inference access-policy revisions reference
  credential IDs and never persist key plaintext or verifier state.
- Route weights are bounded positive integers. Fallback conditions are explicit
  and do not include authorization or invalid-input failures by default.
- A usage record is append-only and deduplicated by stable request/event ID.
  Missing or interrupted usage is represented explicitly and never converted
  to zero.
- A usage record snapshots its project, environment, and the immutable project
  attribution reference effective when the request starts. Later attribution
  updates never rewrite that historical selection.
- Prompts, responses, plaintext provider credentials, and commercial price or
  balance state do not enter the usage ledger; Inference owns no invoice,
  settlement, tax, checkout, or commercial-entitlement authority.

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
new immutable source or workload revision. Reconciliation never resolves a
mutable reference again.

The implemented G0 boundary persists `ExternalSourceRevision` before a build
exists. Its REST boundary enforces exact repository policy, resolves a typed
GitHub branch, tag, or full commit anonymously first and through verified
installation authority only when required, and accepts the resulting immutable
object ID with
`a3s.cloud.build-recipe.v1`. A separate public GitHub endpoint
HMAC-authenticates exact raw requests and durably deduplicates typed branch
pushes in a provider-level inbox. A newly accepted delivery atomically selects
only active subscriptions with the exact authoritative connection,
installation, repository, and branch, then creates one immutable
revision/outbox fact for each matching environment and recipe without
resolving the branch again. Replay does not re-run fanout.
The implemented secure checkout port materializes an accepted commit under
bounded isolated Git configuration, supplies an optional repository-bound
token only through a transient Git HTTP header, removes `.git`, and records an
immutable filesystem digest for credential-free replay. The implemented
Artifact-owned Build service can consume a materialized source and recipe
through rootless BuildKit, then validate and atomically receipt an OCI layout.
The production Build Flow now coordinates this service boundary through an
isolated Runtime Task: it replays the checkout, verifies package-time identity,
admits immutable input bytes, selects a compatible node, applies independent
Runtime and BuildKit network denials, validates the Runtime output, and removes
the Task and checkout before terminal completion. Before cleanup it binds an
immutable `OciPublicationTarget`, pushes blobs and manifests by digest, verifies
the complete remote graph, and records one matching `PublishedOciArtifact`.
Publication replay may adopt only that exact target; cancellation wins the
terminal status but preserves evidence of a push that already completed. It
does not yet record provenance. The published digest can be handed to
Workloads only through an artifact-free command that resolves the exact
tenant-owned successful BuildRun, creates a digest-pinned revision, and reuses
`cloud.deployment@2`. That revision stores an `ExternalBuildReference` binding
the organization, project, environment, source revision, and BuildRun; derived
rollback and Secret-rotation revisions preserve the reference, while ordinary
manual Workload revisions do not invent one. The Artifacts context owns a
deterministic initial `BuildRun` per accepted source revision plus a linear
sequence of deterministic retry attempts. Every retry has a fresh BuildRun and
Operation ID, records its attempt and immediate parent BuildRun, and retains the
exact source revision. Each aggregate binds tenant/environment ownership, the
exact `cloud.build@2` operation, immutable input and Runtime artifact
identities, assigned node and command identities, validated OCI output,
publication target/result, terminal outcome, and cleanup. Concurrent PostgreSQL
reservation, atomic retry creation, exact operation replay, and optimistic
single-transition saves prevent duplicate or forged logical builds across
process loss. Environment list and tenant detail queries expose only public
build and attempt lineage, status, OCI metadata, publication, failure, and
timestamps; node/command identities and internal Artifact URIs remain private.
A `build:write` cancellation request atomically advances the aggregate and
records its idempotency response, while the Build Flow remains responsible for
publication-race adoption and cleanup before terminal state. A separate
idempotent `build:write` retry command accepts only failed or cancelled runs,
atomically creates at most one child BuildRun and new Operation for a parent,
and replays the same child for the same request.
The production worker runs the BuildRun reconciler and a closed Flow router
dispatches only the supported
deployment, workload-stop, and build workflow identities. A separate
implemented GitHub App connection
aggregate verifies and exclusively assigns an installation/account to one Cloud
organization using single-use state, OAuth user authority, and PKCE. The
separate `GithubRepositorySubscription` aggregate provides explicit repository
authority and retained active/inactive lifecycle. Anonymous source resolution
may use only an active connection to issue one repository-scoped read-only
installation token; token and App key material are never durable. Signed
provider lifecycle facts reconcile explicit connection status, retain terminal
history, and prevent old subscriptions from inheriting a fresh connection. A
bounded App-JWT worker also polls the exact installation/account, persists
generic check health with capped retry, repairs missed or delayed lifecycle
facts through optimistic saves, and emits an event only for lifecycle/account
change. The private-credential decorator requires the same fresh authority
check for the exact organization, connection, and installation before either
authenticated resolution or checkout can issue a token.
Local issuer, resolver, and real Git smart-HTTP fixtures cover the private path,
while the
operator-credential external GitHub gate remains unexecuted. GitHub offers no
tokenless current-user App-grant query, so signed authorization-revocation
delivery remains authoritative without persisting OAuth tokens.
BuildRun log queries and resumable streams resolve the aggregate's private node
and deterministic Runtime target internally, then reuse the Fleet-owned durable
log sequence, object, gap, retention, and compaction model. Public projections
bind BuildRun, attempt, parent, and Operation lineage without exposing node or
Runtime unit identity. Provenance/SBOM/signing and cache trust remain later G0
work; authenticated registry publication and BuildRun
status/cancellation/retry/log surfaces are exercised independently.

The implemented node Artifact transfer model binds every request to one
authenticated node, persisted unexpired command, exact Runtime spec digest,
and either one read-only `Artifact` mount or one declared Task output. Download
identity includes the immutable Cloud URI, digest, and media type. Upload
identity additionally includes the exact output size and returns a replayable
`RuntimeOutputArtifact` receipt. The control-plane store and node cache both
rehash bytes; neither accepts a caller- or transport-asserted digest alone.

Node-local blobs use `a3s-node-artifact://sha256/<digest>` and remain internal
until the mTLS upload returns `a3s-cloud-artifact://sha256/<digest>`. Mount and
output receipts bind a blob to the Runtime spec and name. Safe archive
materialization and restart verification preserve a read-only directory view;
spec removal deletes its views and garbage-collects only content with no other
receipt reference. These cache objects carry no tenant authority by
themselves—the persisted command is the transfer authorization source of
truth.

## 6. State models

### Build run state

```text
queued -> preparing -> prepared -> scheduled -> running -> validating
  |          |            |           |           |           |
  +----------+------------+-----------+-----------+-----------+-> cancelling
  +----------+------------+-----------+-----------+-----------+-> cleanup_pending

validating -> cleanup_pending -> succeeded
cancelling -> cleanup_pending -> cancelled
cleanup_pending -> failed | cancelled
```

Failure or cancellation before Runtime dispatch may terminate without a
cleanup command. Once a Runtime Task command exists, terminal state requires a
durable cleanup command identity. Successful completion requires a validated
OCI graph whose artifact exactly matches the collected Runtime output. Exact
transition replay changes neither version nor timestamps. Cleanup first
observes the deterministic Runtime removal receipt, then deletes the checkout;
a build failure is persisted only after this cleanup path completes.

### GitHub source connection state

```text
active <-> suspended
active | suspended -> verification_revoked
active | suspended -> installation_deleted
active | suspended -> account_changed
installation_deleted* | account_changed* -> provider observation
```

Only `active` is authoritative. `active` and `suspended` are current states and
block another connection for the organization, installation, or account.
`verification_revoked` and provider-confirmed deletion/account drift never
transition within the same aggregate; reconnection is a new aggregate after
fresh provider proof. `*` marks a terminal webhook observation whose successful
provider check still predates the webhook. Its provider observation may repair
the same aggregate to the currently reported active/suspended state or confirm
deletion/account drift as terminal. A concurrent replacement connection wins
the uniqueness/CAS boundary and cannot be mutated by that repair.

### GitHub repository subscription state

```text
active -> inactive
```

Creation is valid only beneath the same organization's verified GitHub
connection and an existing organization/project/environment hierarchy. Active
identity is connection, environment, canonical repository, exact branch, and
recipe digest. `inactive` is retained and terminal for that aggregate identity;
a later equivalent binding is a new aggregate. Only `active` participates in
provider fanout.

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

The authoritative `Deployment` aggregate uses:

```text
queued -> resolving -> scheduled -> applying -> verifying -> active
                                                   \-> retiring -> active
```

`retiring` is required when activation supersedes a previous Runtime revision.
Before `verifying`, cancellation may branch through `cancelling` and
`cleanup_pending` to `cancelled`. A pre-activation failure is `failed`; a
failure after activation or after cleanup ownership becomes ambiguous requires
operator-visible `orphaned` state instead of false rollback or success.

### Route state

```text
pending -> publishing -> active
                     \-> rejected
```

`pending` exists only while constructing the aggregate. Persistence atomically
stores the staged route as `publishing` with its complete Gateway publication.
`active` and `rejected` require an exact terminal Gateway acknowledgement.

### Gateway route cutover state

```text
pending -> applied
       \-> rejected
```

Only `applied` changes the live route rows. Both terminal states retain their
publication identity for replay and recovery.

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

### Gateway certificate convergence state

```text
pending -> applied
       \-> rejected
```

The terminal outcome must match the exact node, command, revision, digest, and
acknowledgement time. A rejected convergence does not advance the installed
Gateway revision.

## 7. Data ownership

| Fact | Authoritative owner |
| --- | --- |
| Tenant, project, environment, desired workload | PostgreSQL domain tables |
| Current project attribution reference and immutable business-owner, external cost-attribution code, and label revisions | PostgreSQL Projects tables |
| Expiring GitHub installation/OAuth state digests and PKCE verifier digest | PostgreSQL GitHub connection-flow table; plaintext state and verifier are transient |
| Verified GitHub installation/account ownership, verifying-user identity, explicit status, provider-check health/backoff, and retained history | PostgreSQL GitHub source-connection table; no OAuth credential or raw provider body |
| Provider push delivery identity and exact-payload digest | PostgreSQL source webhook inbox; no raw payload or secret |
| Provider connection-lifecycle event/action, subject, and exact-payload digest | PostgreSQL GitHub lifecycle inbox; no raw payload or credential |
| External source revision, recipe digest, and tenant mutation webhook source-identity reservation | PostgreSQL Sources tables |
| Asset repository refs and objects | Git repository store |
| Asset release and artifact descriptors | PostgreSQL domain tables |
| Artifact bytes | OCI registry or S3-compatible object store |
| Model/backend catalog, environment inference deployment/route/provider intent, and immutable Edge binding reference | PostgreSQL Inference tables |
| Inference-key environment, audience, prefix, verifier hash/algorithm parameters, generation, expiry/revocation and encrypted idempotency receipt | PostgreSQL Identity tables |
| Workload replicas, placement members, accelerator reservations and claims | PostgreSQL Workloads tables |
| Accelerator inventory and node Artifact-cache observations | Node agent plus PostgreSQL Fleet projection |
| Raw accelerator and inference time-series metrics | Configured metrics backend |
| Inference request, attempt and token usage facts, including the request-time project/environment and immutable attribution reference | Durable Gateway spool until contiguous acknowledgement, then append-only PostgreSQL Inference usage ledger |
| Operation history | A3S Flow PostgreSQL event store |
| Operation summary | Rebuildable PostgreSQL projection |
| Provider resource and live health | Node agent plus Runtime provider |
| Last accepted observation | PostgreSQL fleet/deployment projection |
| Route desired state, target-set/rollout generation, Gateway scope, and publication identity | PostgreSQL Edge tables |
| Pending/applied/rejected Gateway route cutover and candidate route projections | PostgreSQL Edge tables |
| Pending/applied/rejected Gateway certificate convergence and versioned route classification | PostgreSQL Edge tables |
| Domain claims and Gateway certificate public material | PostgreSQL Edge tables |
| Gateway active config | A3S Gateway, keyed by config revision |
| Gateway private key and CSR files | Node-local managed certificate directory |
| Secret identity and encrypted immutable versions | PostgreSQL Secret tables |
| Workload Secret bindings and canonical references | Immutable workload revision and reference-only Runtime/Fleet state |
| Artifact ingest attempt, immutable file manifest/digests, storage descriptor and consumed grant ID | PostgreSQL Artifacts tables |
| Secret materialization grant identity, version, environment, attempt/Task/host/digest scope, expiry and revocation | PostgreSQL Secret tables; plaintext is process-create-only and Artifacts consumes the grant by ID |
| Secret-rotation restart causality, derived deployment, and replay checkpoint | PostgreSQL rotation restart/reconciliation tables plus the committed outbox fact |
| Transient Secret material | Authorized control-plane decryption and node-local Docker create boundary; file targets use Linux tmpfs only |
| Durable Runtime log cursor, delivery watermark, last discontinuity, and pending upload | Node-agent secure state, keyed by unit and generation |
| Log chunk ordering, provider-gap boundary, cursor, stream, checksum, object key, retained tombstone, compacted range, and batch replay header | PostgreSQL Fleet telemetry tables |
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
source.github-connection.created
source.github-connection.reconciled
source.github-repository-subscription.created
source.github-repository-subscription.deactivated
source.revision.accepted
asset.asset.created
asset.release.published
artifact.artifact.registered
fleet.node.enrolled
fleet.node.observed
fleet.node-inventory.observed
workload.revision.created
deployment.deployment.requested
deployment.deployment.succeeded
deployment.deployment.failed
inference.model.registered
inference.model-revision.resolved
inference.backend-revision.published
inference.deployment.created
inference.deployment.revised
inference.route.changed
inference.usage.recorded
edge.route.publication-staged
edge.route.cutover-staged
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

Planned I0 also excludes model training/fine-tuning orchestration, unisolated
soft GPU overcommit, price catalogs, monetary credits/balances, checkout,
invoices, settlement, tax and commercial-entitlement authority, and vendor
support based only on unverified capability advertisement.
