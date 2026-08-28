# A3S Cloud Distributed API Consistency Architecture

## 1. Decision

Every A3S Cloud API is designed for concurrent requests against multiple
`api`, `worker`, and `relay` replicas. Correctness cannot depend on which
replica receives a request, process memory, request arrival order, an in-process
mutex, a local cache, a single worker, or a successful HTTP response write.

The universal mutation boundary is:

```text
authenticated scoped command
  -> current authorization snapshot
  -> scoped idempotency identity + canonical request digest
  -> aggregate version / shared-invariant serialization
  -> one PostgreSQL transaction
       aggregate state
       + stable idempotency outcome
       + Operation / Flow correlation when asynchronous
       + audit
       + bounded Outbox facts
  -> commit
  -> response from committed outcome
  -> fenced asynchronous convergence
```

PostgreSQL is the only coordination and business-state authority for Cloud API
replicas. A3S Flow coordinates long-running work. A3S Event/NATS transports
committed facts but is never the consistency authority. Redis may implement a
separately certified request-path limiter; it is not idempotency, locking,
desired state, replay, session, or API consistency truth.

This contract applies to every current and future Organization, Project,
source, build, Registry, model, Agent, Workflow, Function, Durable Cell,
Workload, storage, Gateway, usage and administration API. A product context may
strengthen the contract, never bypass it.

## 2. API operation classes

Every public operation declares one class because the correct response and
consistency proof depend on its shape.

| Class | Contract |
| --- | --- |
| Aggregate command | Atomically changes one aggregate and common transaction evidence; returns the committed aggregate version and replay-stable result |
| Long-running command | Atomically accepts intent plus one Operation/Flow correlation; returns a replay-stable Operation identity, normally before external work finishes |
| Cross-context process | The initiating owner commits intent/fact; Flow calls consumer-owned Application ports in steps. It is never a distributed database transaction across repositories/providers |
| Authoritative query | Reads the owner aggregate from the authoritative PostgreSQL path and returns version/ETag plus a bounded result |
| Projection query | Returns an explicit projection watermark/freshness state and never presents an asynchronously built view as current owner truth |
| Cursor stream/SSE | Resumes from a scope-bound monotonic owner cursor, reports retention gaps explicitly, and never uses process memory as history |
| Byte transfer | Uses a durable reservation and exact digest/size/media identity; completion adopts verified immutable provider state after ambiguous network outcomes |
| Live data-plane request | Enters A3S Gateway against one acknowledged complete snapshot. Cloud management replicas are not on the opaque request byte path |

An endpoint that both mutates durable state and calls an external provider
synchronously is invalid unless the provider call is a separately fenced,
non-replayable attempt whose indeterminate outcome is durable. The default is
to commit intent and complete through Flow/worker processing.

## 3. Command envelope and idempotency

All presentation surfaces map into one Application command containing:

- exact Principal/credential and Organization/Project/Environment or explicit
  installation scope;
- stable caller request ID (the REST `Idempotency-Key` projection where used);
- closed command kind and schema version;
- canonical request digest after ACL parsing and normalization;
- target aggregate identity and expected aggregate/policy version when
  mutating existing state;
- bounded deadline/cancellation intent; and
- trace/request correlation that is not itself replay identity.

The durable idempotency key is scoped by authority, actor/credential, action,
tenant scope and request ID. It cannot collide across tenants or let one actor
retrieve another actor's response. The first commit stores the canonical
request digest and bounded response snapshot or exact result reference in the
same transaction as the mutation.

Subsequent delivery has only three valid outcomes:

1. same identity and same digest returns the original status/result without
   re-running the mutation or external side effect;
2. same identity and different digest returns a stable idempotency conflict;
3. a request outside the caller's current authorization returns the same
   non-enumerating denial used for a first request. Authorization is checked
   before revealing replay state.

Process-local deduplication may reduce load but cannot affect correctness. Its
loss, eviction or disagreement must be invisible.

## 4. Concurrency control

### 4.1 Aggregate-local updates

Every mutable aggregate has a monotonic version. Update/delete/transition SQL
includes the exact current identity, tenant scope and expected version. Zero
affected rows is resolved to one of not found, denied or concurrent conflict
without leaking a foreign resource. A successful transition increments the
version exactly once.

REST exposes the version as a response field and/or ETag. A client may project
it through `If-Match`; the maintained client, CLI and Management MCP map the
same value to the same Application command. Last-write-wins is forbidden for
security policy, release heads, routes, credentials, quotas, rollouts,
retention, role bindings and mutable desired state.

### 4.2 Shared invariants

An invariant spanning several aggregates uses one documented PostgreSQL
serialization point:

- a unique or exclusion constraint for identity/overlap invariants;
- a row lock on the stable owning root for bounded counters, allocation or
  head advancement; or
- serializable retry only for a transaction whose invariant cannot be safely
  represented by the first two mechanisms.

Locks are acquired in canonical ancestor/identity order. Hierarchical quota,
for example, locks installation allocation, Organization, Project and
Environment counters in that order. Transactions remain short and never hold
a database lock across Gateway, Runtime, Box, S3, Git, OCI, Use Registry,
model Provider, SMTP or another network call.

PostgreSQL advisory locking is reserved for the A3S ORM migration authority.
Product code does not invent named advisory locks, Redis locks, filesystem
locks or leader-only assumptions as aggregate consistency.

### 4.3 Authorization races

An authorization decision carries the exact Principal, credential, session,
Membership, role-policy and applicable Grant revisions. Security-sensitive
write transactions revalidate that snapshot with the mutation, or acquire an
owner-provided immutable authorization lease whose revocation semantics are
explicit. This closes the gap where one replica authorizes while another
revokes access.

Delayed high-risk actions reauthorize immediately before Secret
materialization, public route activation, Provider dispatch, Runtime exec or
support access. Historical command replay remains inspectable without making a
revoked authority usable again.

## 5. Transaction and publication boundary

A successful mutation commits all applicable evidence atomically:

1. owning aggregate and version;
2. idempotency outcome;
3. Operation request and Flow correlation for asynchronous work;
4. quota/allocation changes;
5. audit record; and
6. deterministic bounded Outbox facts.

No HTTP success is returned for an uncommitted mutation. If the process dies
or the client disconnects after commit but before receiving the response, a
retry returns the stored outcome. If commit is unknown to the client, the
client retries the same request identity or queries the returned/derived
Operation; it does not generate a new command blindly.

Outbox publication is at least once. Relays lease rows from PostgreSQL with a
generation/fence, publish a deterministic event identity and mark handoff only
for that lease. A handoff is not business completion. Each consumer records its
owner-local inbox/projection receipt atomically with its state change, so
redelivery is ACK-only replay. Recovery scans authoritative unfinished intent
and republishes through the same Outbox path.

There is no global event order. Ordering is exact and monotonic within the
owning aggregate or explicitly named stream. Consumers reject or defer gaps,
ignore exact duplicates, and never infer success from a later unrelated event.

## 6. External side effects and indeterminate outcomes

External work uses one of two contracts:

### Replay-safe contract

The request carries a deterministic provider identity/generation and the
provider is proven idempotent or inspectable. Recovery may resend or adopt the
exact observed result. Runtime apply/remove, immutable object publication and
complete Gateway snapshot apply follow this shape only after conformance.

### Non-replayable contract

The owner persists `reserved`, then a lease/fence. It prepares all validation,
authorization, Secret and network state before atomically advancing to
`dispatching`. After `dispatching`, no replica may issue the external attempt
again. A missing response becomes a durable `indeterminate` outcome that needs
provider inspection or explicit human resolution; it is never a blind retry.

Provider call, command, attempt and receipt identities include exact
generation/fence values. A late or stale result cannot advance current desired
state. Cleanup releases Claims, Secrets, ports, object reservations and writer
rights only after matching fenced evidence.

## 7. Query consistency

### 7.1 Authoritative reads

Command follow-up reads of the owning aggregate use the authoritative primary
path and can observe the committed version. Read replicas may be introduced
only with a consistency token/watermark and bounded-lag policy; they never
silently weaken read-after-write or authorization/revocation semantics.

Conditional reads use exact aggregate/revision ETags. `not modified` means the
authoritative version is unchanged, not merely that one API replica's cache is
unchanged.

### 7.2 Projections and search

Every asynchronous projection response exposes the relevant source
watermark, `observed_at`, and explicit `current`, `stale`, `incomplete`, or
`unknown` state. Counts and search suggestions obey the same tenant and grant
filter as individual resources. A projection cannot authorize a mutation.

Where a caller needs “read your accepted command,” the API returns the owner
aggregate/Operation directly or waits against a bounded committed watermark;
it does not poll arbitrary replicas until a projection happens to appear.

### 7.3 Pagination and streams

Offset pagination over mutable collections is forbidden for consistency-
sensitive APIs. Keyset cursors bind:

- tenant and authorization scope digest;
- query/filter/sort digest;
- stable ordered key and tie-break identity;
- snapshot/high-water boundary where required;
- schema version and expiry; and
- integrity protection.

A cursor cannot be replayed for another tenant, actor, query or later-expanded
permission. Concurrent insertions cannot duplicate or skip records inside the
declared snapshot contract. Deletion may create an explicit tombstone/gap; it
does not silently rewind a cursor.

SSE/semantic streams use the owning durable sequence. Reconnect returns exact
later events, an explicit retention gap, or authorization denial. Load
balancer movement between API replicas is behaviorally irrelevant.

## 8. Rate limiting, concurrency admission, and quota

“Limit” is not one mechanism. The owner declares a typed immutable policy;
the enforcement primitive matches the consequence of overshoot:

| Limit type | Purpose | Authority and mechanism | Failure contract |
| --- | --- | --- | --- |
| Burst/rate shaping | Protect public APIs and Providers from request bursts | Gateway enforces a versioned token-bucket/GCRA profile using bounded local state plus an optional certified distributed limiter | Policy declares maximum failover overshoot; security-sensitive endpoints fail closed when the required limiter is unavailable |
| Hard request concurrency | Bound simultaneous inference, Agent, Function, upload or exec work | Generation-bound permits from PostgreSQL or another separately certified strongly consistent lease provider | No permit means no dispatch; expiry/takeover advances a fence and late release is harmless |
| Durable tenant quota/budget | Prevent CPU/GPU/storage/build/token allocation beyond Organization/Project/Environment policy | Owning PostgreSQL reservation/allocation ledger, charged hierarchically in one transaction | Never fail open or reconstruct truth from Redis/metrics |
| Physical capacity | Prevent node/device/port/volume over-allocation | Workloads/Fleet Claims and placement-group transaction | All required Claims commit or all compensate |
| Provider capacity | Avoid overloading one external account/resource | Inference/Connector typed policy plus fenced attempt/concurrency permits | Provider failure releases only the exact permit and cannot mark unrelated resources unhealthy |

Limiter keys bind installation/tenant, credential or Principal, endpoint/action,
model/provider where relevant, and policy revision. Raw API keys, tenant names,
prompts and responses are not keys or metrics. Gateway returns stable limit kind,
policy revision, bounded retry metadata and request correlation without
revealing other tenants' load.

Redis can be a high-throughput distributed **rate-shaping provider** only after
its atomic-per-key operation, topology, timeout, replica failover, clock,
eviction, restart and maximum-overshoot behavior are certified. Ordinary Redis
replication does not make a counter globally exact during failover. Therefore:

- hard concurrency, commercial entitlement and durable quota never rely only
  on Redis;
- a policy that requires zero overshoot uses durable fenced permits or fails
  closed;
- a policy that permits bounded degradation states the local emergency budget,
  duration and reconciliation rule explicitly; and
- limiter telemetry never mutates durable quota, desired replicas or billing
  truth.

Management mutations receive conservative per-credential, per-Principal,
per-Organization and source-address shaping before command dispatch, plus the
durable command/quota checks in the transaction. Login, key issuance, support
access, Secret, exec and recovery endpoints have stricter independent buckets
and fail closed. Limits themselves do not replace authentication, idempotency,
authorization or workload admission.

## 9. Cache consistency

Caching is allowed only when its source, key, staleness and invalidation
contract are explicit:

| Cache | Allowed contents | Consistency rule |
| --- | --- | --- |
| Immutable object/Web/model bytes | Digest-verified immutable bytes | Key includes provider authority, tenant namespace, content digest and representation; corruption evicts/fails closed |
| Aggregate/query response | Bounded non-secret response projection | Key includes tenant, effective authorization/policy digest, query digest and aggregate/projection version; no write-back |
| Model/Use/provider catalog | Immutable revision or signed catalog snapshot | Exact revision/digest and expiry; candidate presence never proves admission or conformance |
| Gateway route/key/policy state | Complete acknowledged snapshot only | Atomically replaced by generation/digest; old snapshot remains on rejection; revocation waits for required acknowledgement |
| Authorization/session hint | Only bounded compiled decision metadata | Very short or snapshot-bound; current revocation/session gates cannot be satisfied from a stale entry |
| Negative result | Non-sensitive stable absence only | Tenant/auth/query/version scoped, short-lived, and never used where it changes non-enumerating authorization behavior |

No API replica cache is desired state, lock state, idempotency truth, quota
truth, Workflow history, Agent transcript, usage ledger or Secret store. An
invalidation event is an optimization; correctness must still follow version,
expiry or authoritative revalidation if the event is lost.

Cache stampede protection may coalesce identical reads in one process, but it
does not serialize mutations. Distributed cache population uses create-if-
absent by immutable digest or a fenced lease; a stale filler cannot replace a
newer revision. Cache keys and telemetry are bounded and cannot leak cross-
tenant cardinality or authorization state.

## 10. Distributed locks and leadership

A generic distributed lock API is deliberately not a platform abstraction.
The invariant selects the smallest safe primitive:

| Invariant | Primitive |
| --- | --- |
| One immutable identity/name | PostgreSQL unique/exclusion constraint |
| One aggregate transition | Expected-version CAS |
| Shared bounded allocation/head | Stable owner row lock in canonical order |
| Long-running worker ownership | Expiring PostgreSQL lease with monotonic generation/fence |
| Node/provider side effect | Command/attempt generation plus receiver journal/fence |
| Complete Gateway publication | Edge snapshot generation/digest CAS plus exact acknowledgement |
| Schema migration | A3S ORM's single PostgreSQL advisory transaction lock |
| Immutable cache/object publication | Content digest and create-if-absent/adoption |

Leases make progress; fences preserve correctness. Every leased write or side
effect includes the acquired generation, and the authoritative receiver/store
rejects stale generations after takeover. Merely checking that a lock is still
held before a write is insufficient because ownership can change between the
check and the side effect.

Redis Redlock, filesystem locks, DNS ownership, load-balancer affinity and
“only one replica is configured” are not accepted correctness boundaries. A
dedicated consensus service may be admitted later only behind the same typed
lease/fence contract and after partition, pause, clock and stale-writer tests;
its introduction cannot create another business-state authority.

## 11. Database and cross-provider distributed transactions

A3S Cloud uses one local PostgreSQL transaction for one owning consistency
boundary. It does **not** use XA/two-phase commit across PostgreSQL, NATS, S3,
Git, OCI Registry, A3S Use, Gateway, Runtime, Box, Redis or model Providers.
Those systems have different failure, retention and compensation semantics;
pretending they share one atomic commit would block availability and still
leave unsafe heuristic outcomes.

Cross-boundary work uses a durable saga:

```text
owner PostgreSQL commit + Operation + Outbox
  -> Flow step invokes a consumer-owned Application port
  -> consumer commits its own state + inbox receipt + Outbox
  -> external attempt uses replay-safe adoption or non-replayable fence
  -> exact observation advances the Operation
  -> failure executes typed compensation or exposes blocked/indeterminate state
```

Compensation is a domain command, not database rollback in disguise. It may
remove an unpublished release, revoke a route, release a Claim or tombstone an
object reference, but it never erases committed audit/history or assumes an
external side effect did not happen. Flow history records coordination;
aggregate state and provider receipts remain the decision evidence.

If Cloud later partitions PostgreSQL by tenant or region:

- one Organization has one versioned home write partition at a time;
- request routing resolves that binding before opening a transaction;
- movement uses a fenced copy/catch-up/cutover protocol and stable epoch;
- no ordinary product command synchronously mutates two home partitions;
- cross-Organization operations are independently committed sagas with an
  explicit partial/blocked outcome; and
- global catalog/search/usage views are projections with watermarks, never a
  distributed serializable transaction.

PostgreSQL HA is one logical database authority. The selected provider must
document commit durability, synchronous-replication policy, failover data-loss
bound, split-brain fencing, read-replica lag and recovery-point behavior.
Cloud refuses writes when the provider cannot establish one writable primary.
A failover never authorizes replay with a new idempotency identity.

Common cross-provider patterns are fixed:

| Boundary | Safe consistency pattern |
| --- | --- |
| PostgreSQL -> NATS | Transactional Outbox, deterministic fact ID, consumer inbox/receipt |
| PostgreSQL -> S3 | Durable reservation, digest-bound create, verify/adopt, then reference; fenced garbage collection |
| PostgreSQL -> Gateway | Complete staged snapshot, atomic apply, exact generation/digest acknowledgement |
| PostgreSQL -> Runtime/Box | Leased command, node journal before side effect, generation-bound receipt and cleanup fence |
| PostgreSQL -> Git/OCI/Use/model source | Exact revision/digest, attempt record, provider inspection/adoption and provenance |
| PostgreSQL -> Redis | Policy snapshot plus ephemeral limiter/cache state; durable decision never waits for Redis commit as half of a transaction |

## 12. Gateway and desired-state consistency

Cloud never patches live Gateway state route by route. Edge compiles one
complete immutable snapshot per physical Gateway scope with generation,
digest, expiry and every active/previously published logical owner. Gateway
stages, validates and atomically activates it, then acknowledges the exact
generation/digest. Rejection or timeout leaves the prior acknowledged snapshot
serving.

Workload health, rollout, credential revocation, static Web, inference, MCP,
Agent, Function and Durable Cell publication all reuse that authority. A
product API cannot push a partial route or infer activation from a successful
dispatch. Multiple workers racing to reconcile converge through the same Edge
CAS and publication-owner marker.

## 13. Mixed-version replicas and schema evolution

Distributed deployment includes old and new replicas during rollout:

- A3S ORM migrations are expand-compatible and run only in the terminating
  migrator process.
- Serving replicas verify their required migration version/checksum subset and
  never migrate on startup.
- New writers do not emit a contract old consumers cannot safely reject or
  ignore until the compatibility gate is open.
- ACL/event/protocol versions are closed and explicit. Unknown required fields,
  command kinds or semantic versions fail closed.
- Contracting schema or removing behavior occurs only after old replicas,
  workers, queued messages, cursors and Gateway/Runtime generations are drained
  beyond the documented window.

## 14. Time, leases and cancellation

Database time is authoritative for PostgreSQL leases, expiry and takeover.
Caller clocks are never trusted for privilege, quota reset or fencing.
Externally observed timestamps are evidence, not lease authority.

Every lease includes owner, generation/fence, acquisition, expiry and bounded
renewal. Takeover advances the fence; a stale holder cannot commit, publish,
acknowledge or clean up. A singleton worker is an optimization only; correctness
survives concurrent workers.

Cancellation is durable intent with an expected version. It races safely with
completion: exactly one legal transition wins, while later cleanup may still
continue. An HTTP disconnect is not cancellation unless the endpoint contract
explicitly persists it.

## 15. Error and retry semantics

Errors are closed and tell a client whether the same request identity may be
retried:

| Outcome | Client behavior |
| --- | --- |
| Validation or policy rejection | Correct intent; do not retry unchanged input |
| Authorization/non-enumerating absence | Re-authenticate or request access; a retry cannot reveal replay state |
| Expected-version or idempotency conflict | Re-read authoritative state; never overwrite blindly |
| Quota/rate/concurrency rejection | Honor bounded retry metadata only when supplied |
| Dependency unavailable before commit | Retry the same request identity within its deadline |
| Accepted Operation still running | Query/stream the same Operation; do not submit another command |
| Provider outcome indeterminate | Inspect the durable attempt/Operation or use the explicit recovery command; never blind retry |

Servers do not ask clients to infer commit from a timeout, connection reset or
generic `500`. Logs and traces retain the request/Operation/aggregate
correlation without becoming the recovery authority.

## 16. Enforcement and delivery gates

This is a cross-cutting `F0`/`H0.4` release condition, not another product
controller or database.

| Gate | Required outcome |
| --- | --- |
| `H0.4-API1` | Freeze the universal command/idempotency/version/Operation/error and query watermark/cursor contracts in closed ACL/OpenAPI types |
| `H0.4-API2` | Architecture ratchets reject process-memory idempotency, API-local mutable authority, unscoped repository calls, external I/O inside transactions and serving-process migration |
| `H0.4-API3` | Run every mutation family concurrently across at least three API replicas; same request converges to one commit/result and conflicting requests preserve aggregate/shared invariants |
| `H0.4-API4` | Inject process death and client disconnect before transaction, during commit, after commit/before response, during Outbox handoff and after consumer commit/before ACK; recovery produces no duplicate side effect |
| `H0.4-API5` | Prove authorization revocation races, tenant isolation, version conflicts, hierarchical quota lock order, lease takeover and stale-fence rejection on PostgreSQL 17 |
| `H0.4-API6` | Prove keyset pagination, projection watermarks, read-after-write, stream reconnect/gaps and optional read-replica lag without cross-tenant or stale-authorization disclosure |
| `H0.4-API7` | Prove mixed old/new API/Worker/Relay/Gateway/Runtime revisions across expand/rollback windows and reject unsupported contracts without partial state |
| `H0.4-API8` | Load/fault test PostgreSQL failover, NATS loss, Redis limiter loss, duplicate/out-of-order delivery and network partitions; desired state and recovery remain PostgreSQL/Flow authoritative |
| `H0.5-API9` | Certify hierarchical rate/concurrency/quota profiles across Gateway replicas, bounded Redis/provider failover overshoot, hard-permit fencing, retry metadata and fail-open/closed policy |
| `H0.4-API10` | Prove tenant/auth/version-qualified caches, lost invalidation, stampede, corruption, eviction, restart and revocation with no stale-authority or cross-tenant result |
| `H0.4-API11` | Prove every unique/CAS/row-lock/lease/fence serialization point under contention, takeover, pause and partition; source ratchets reject generic product distributed locks |
| `H0.4-API12` | Prove Outbox/Inbox/Flow sagas and compensation across PostgreSQL/NATS/S3/Gateway/Runtime/Registry faults, including indeterminate outcomes and PostgreSQL HA failover without XA |

Every new API must name its operation class, aggregate/version authority,
idempotency scope, transaction boundary, consistency level, cursor/stream
semantics, side-effect retry class and fault tests in its design and OpenAPI
review. Missing any item blocks merge and availability.

## 17. Non-goals

- Global serializable transactions for every request.
- Distributed transactions across PostgreSQL, NATS, S3, Gateway, Runtime,
  Registry or model Providers.
- Exactly-once network delivery claims.
- A per-product idempotency table, Outbox, queue, worker lease or retry engine.
- Redis, NATS, an API replica cache or a leader process as business truth.
- Last-write-wins mutation of durable desired state.
- Blind retry after a non-replayable dispatch fence.
- Offset pagination or browser-side filtering for protected mutable resources.
