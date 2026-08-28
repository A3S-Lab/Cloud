# A3S Cloud DDD, AOP, and Design Pattern Architecture

## 1. Decision

A3S Cloud uses Domain-Driven Design to assign business meaning and authority,
hexagonal architecture to isolate mechanisms, and explicit aspect pipelines to
apply cross-cutting policy consistently. Design patterns are admitted only
when they remove a real source of variation, concurrency risk or duplication.
Pattern vocabulary never justifies another controller, repository, queue,
cache, gateway, scheduler or configuration language.

The governing rule is:

```text
one business decision -> one owning bounded context
one use case -> one Application command/query boundary
one mechanism -> one shared port and certified adapter family
one cross-cutting concern -> one ordered aspect/decorator implementation
```

Rust has no need for runtime method weaving. A3S Boot guards, pipes,
interceptors and exception filters plus explicit Application/Infrastructure
decorators provide compile-time-visible AOP. Attribute macros may generate
ordinary definitions, but they cannot hide database access, network calls,
authorization, retry or transaction boundaries.

## 2. Layer responsibilities

| Layer | May own | Must not own |
| --- | --- | --- |
| Domain | Aggregates, entities, value objects, invariants, policies/specifications, state transitions, domain services/events | HTTP/DTOs, ORM records, provider clients, tracing spans, retries, caches, clocks with side effects |
| Application | Commands/queries, use-case orchestration, owner ports, authorization requirements, idempotency/concurrency intent, transaction outcome | SQL/provider details, presentation status codes, foreign repositories, mutable global services |
| Infrastructure | Repository and provider adapters, A3S ORM mappings, object/Git/Registry/Runtime/Gateway/Doris/Redis implementations, telemetry decorators | Business policy, aggregate mutation outside owner services, alternate idempotency/retry/authorization rules |
| Presentation | REST/OpenAPI, maintained client/CLI/MCP projections, request bounds, protocol parsing, Boot aspect composition, error serialization | Repositories, business branching, persistence records, provider selection, UI-specific lifecycle |
| Composition root | Select exact adapters, roles and policies; construct one shared mechanism instance and project typed owner ports | Domain decisions, service locator, runtime plugin discovery as authority |

Cross-context collaboration is only a consumer-owned Application port
implemented by the owner or an aggregate-free versioned fact committed through
Outbox. Importing another context's Infrastructure, ORM row or Presentation DTO
is not DDD collaboration.

## 3. Ordered AOP pipeline

Aspects are split by the boundary at which they have enough authority. One
giant HTTP interceptor cannot correctly own a database transaction, audit fact
and provider retry.

### 3.1 Presentation pipeline

The canonical outer-to-inner order is:

1. protocol/version, method, content type and body/header size bounds;
2. request ID, trace context, deadline and cancellation context;
3. anonymous/source-address abuse shaping;
4. credential extraction and authentication;
5. installation or Organization/Project/Environment scope resolution;
6. authenticated per-Principal/credential/tenant rate shaping;
7. action/resource authorization without existence disclosure;
8. closed ACL/DTO parsing, normalization and semantic-independent validation;
9. command/query dispatch; and
10. stable exception/error mapping, response bounds and telemetry settlement.

Authentication precedes tenant-scoped idempotency and resource lookup.
Authorization precedes replay disclosure. Parsing that is needed to identify a
bounded action may occur early, but product semantics remain in the owner
Application/Domain boundary.

### 3.2 Application command decorators

The command bus composes one explicit decorator chain:

```text
deadline/cancellation guard
  -> authorization-snapshot capture
  -> idempotency scope + canonical digest
  -> Unit of Work / transaction
  -> current authorization and expected-version revalidation
  -> owning command handler / aggregate
  -> Operation + quota + audit + Outbox + replay outcome enlistment
  -> commit
```

Every enlisted record is written through its owner port in the same
PostgreSQL transaction. A generic decorator supplies mechanics and correlation;
the use case supplies the action, target, redaction and domain outcome. Audit
cannot be a best-effort “after success” HTTP hook.

Queries use a separate chain: authorization, consistency selection,
scope-bound keyset cursor, repository/read-model call, watermark/ETag and
bounded response. Query caching is an Infrastructure decorator keyed by tenant,
authorization/policy and source version; it cannot wrap authorization itself.

### 3.3 Outbound port decorators

Provider calls may compose explicit decorators for:

- Secret materialization immediately before the call;
- egress authorization and endpoint pinning;
- timeout and cancellation;
- concurrency/bulkhead admission through the shared Lane/Redis profile;
- tracing, metrics and redacted diagnostics;
- circuit-breaker observation; and
- owner-approved retry or the non-replayable dispatch fence.

The order is fixed per port contract and tested. A generic HTTP client must not
retry a POST because a global middleware guessed it was safe. Circuit state is
an operational hint; it cannot revise Provider desired state or authorize a
fallback excluded by policy.

## 4. Pattern catalog and owners

| Pattern | A3S use | Owner/boundary | Misuse rejected |
| --- | --- | --- | --- |
| Aggregate + value object | Protect one transactional business invariant and typed identity | Owning Domain | Anemic ORM rows mutated by controllers |
| Repository | Persist/reconstitute one aggregate or deliberate read model | Owner Domain port, Infrastructure adapter | Generic repository, foreign table access, repository per interface |
| Specification/policy object | Compose pure closed admission/selection rules | Owning Domain | Provider labels or UI flags as policy |
| Factory/compiler | Turn immutable product intent into Runtime/Gateway/provider contracts | Owner Application/Domain service | Mutable provider config as desired state |
| Strategy | Select one certified provider/algorithm behind a stable port | Composition + owner policy | Provider switch statements across handlers |
| Adapter / anti-corruption layer | Translate external or foreign owner language into consumer language | Consumer Infrastructure | Importing foreign aggregates/DTOs |
| Decorator | Add one visible cross-cutting concern around a port/use case | Boot/Application/Infrastructure aspect layer | Business logic hidden in macros/interceptors |
| CQRS | Separate mutation authority from optimized authorized projections | Application boundary | A second write model or projection authorization |
| Domain event | Record meaningful within-domain transition | Owning Domain/transaction | Using transport events to mutate aggregate before commit |
| Outbox + Inbox | Reliably deliver committed aggregate-free facts | Integration Events plus consumer receipt | NATS as business truth or publish-before-commit |
| Saga/process manager | Coordinate long-running cross-owner outcomes and compensation | Operations + A3S Flow | XA across providers or a workflow per product |
| State machine | Make legal lifecycle transitions, retries and terminal outcomes closed | Aggregate/attempt/lease model | Boolean flag combinations or implicit status from logs |
| Optimistic concurrency/CAS | Resolve concurrent aggregate writes | Aggregate version + PostgreSQL predicate | Last-write-wins desired state |
| Lease + fencing token | Permit takeover without stale writers | PostgreSQL/Fleet/provider contract | Generic lock without receiver-side fence |
| Bulkhead/admission lane | Bound concurrency and isolate work classes | A3S Lane + owner policy | Lane queue as accepted business intent |
| Circuit breaker | Avoid repeated calls to a currently failing dependency | Provider adapter observation | Desired-state mutation or cross-tenant global breaker |
| Content-addressed object | Make immutable bytes verifiable/adoptable | Shared object authority + typed owner namespace | Mutable URL/tag as release identity |
| Complete immutable snapshot | Apply coherent live routing/policy atomically | Edge -> Gateway | Incremental product-owned route patches |
| Projection/materialized view | Optimize read/search/analytics with watermark | Search/Security/Inference/observability adapter | Projection as authorization or write truth |
| Null object | Explicitly disabled optional mechanism with safe behavior | Configuration/composition | Silent in-memory production fallback |

“Singleton” means one logical authority, not one process. In HA deployments
the authority is represented by durable constraints, versions and fences; no
correctness claim depends on a static global instance.

## 5. Unified cross-cutting aspects

| Concern | One abstraction | Enforcement location |
| --- | --- | --- |
| Authentication/session | Identity `PrincipalContext` with exact credential/session revisions | Gateway/Boot authentication adapter, Identity decision |
| Tenant/system scope | Explicit installation or tenant `ScopeContext` | Boot resolver + every Application command/query/repository identity |
| Authorization | Identity effective decision port with owner action/resource language | Application boundary; delayed high-risk recheck |
| Validation/canonicalization | Closed A3S ACL schema and owner constructor | Presentation parsing then Domain invariants |
| Idempotency | Scoped request identity + canonical command digest + stable outcome | Shared transaction enlistment, owner response projection |
| Transactions | A3S ORM Unit of Work over one owner consistency boundary | Application command decorator/repository family |
| Audit | Shared audit port with owner action/target/redaction | Enlisted in mutation transaction |
| Integration events | Deterministic Outbox fact + consumer receipt | Same transaction + Relay |
| Rate/concurrency | Typed hierarchical policy; Gateway/Redis/Lane provider; durable quota/permits | Before dispatch plus owner transaction |
| Cache | Versioned tenant/auth-qualified read-through adapter | Infrastructure only |
| Retry/timeout/cancel | Owner policy + Flow or fenced attempt contract | Application/process manager; not generic transport |
| Logs/traces/metrics | Common correlation/redaction/schema and exporter ports | Every boundary, observational only |
| Error model | Closed domain/application/provider outcomes mapped once per protocol | Exception filter/presentation adapter |

No bounded context defines a private version of one of these aspects. It
contributes typed action, target, policy and redaction metadata to the shared
mechanism.

## 6. Dependency inversion and ownership tests

The source dependency rule is executable:

- Domain imports only its stable language and pure shared-kernel types.
- Application imports Domain and inward-owned ports.
- Infrastructure imports the port it implements and external mechanisms.
- Presentation imports Application commands/queries and response contracts.
- Cross-context Infrastructure imports are confined to named anti-corruption
  adapters implementing a consumer-owned port.
- Concrete construction and downcasting occur only in the composition root.

Architecture tests enumerate module imports, public facade exposure, ORM table
mappings, repository constructors, low-level clients and aspect registrations.
The release target is one physical table mapping, object/Redis/Doris/provider
client instance and interceptor/decorator registration authority per concern.

## 7. Aspect correctness tests

Every command family proves the aspect order, not just business success:

- malformed/oversized input cannot reach authentication-sensitive or provider
  code;
- unauthenticated traffic cannot consume a tenant bucket or reveal tenancy;
- unauthorized traffic cannot inspect idempotency replay, cache, count or
  resource timing;
- concurrent authorized duplicates produce one transaction/result;
- revocation between outer authorization and commit fails the current
  revalidation;
- audit/Outbox failure rolls back the aggregate;
- commit followed by response loss replays the exact outcome;
- telemetry/export failure cannot roll back committed business state, but its
  gap/health is visible; and
- a panic/error passes through cancellation, lease/fence and redacted error
  settlement without leaking private input.

Boot route tests assert the canonical aspect stack once at composition. A
module cannot reorder or omit a mandatory aspect by registering a private
controller pipeline.

## 8. Delivery gates

| Gate | Required outcome |
| --- | --- |
| `DDD-AOP1` | Freeze layer/cross-context dependency rules, pattern owners, aspect metadata and canonical ordering |
| `DDD-AOP2` | Provide one Boot/Application decorator set for context/authn/scope/rate/authz/validation/idempotency/UoW/audit/Outbox/telemetry/error mapping |
| `DDD-AOP3` | Migrate duplicate guards, transactions, audit mappings, retry wrappers, caches and provider clients to owner/shared ports; no new debt baseline entries |
| `DDD-AOP4` | Source ratchets reach zero foreign outer-layer imports, duplicate ORM mappings, public Infrastructure/Presentation facades and context-private cross-cutting mechanisms |
| `DDD-AOP5` | Run aspect-order, concurrency, revocation, failure, panic, mixed-version and cross-surface parity tests on real PostgreSQL/providers |

These are architecture release conditions applied to product gates, not a new
runtime service or business bounded context.

## 9. Non-goals

- Runtime reflection/weaving, magic annotations or an aspect service locator.
- A pattern for every class or a generic abstraction before two proven
  consumers share the same invariant.
- Domain entities that depend on repositories, clocks, telemetry or providers.
- Generic CRUD services/repositories that erase aggregate language.
- A universal event, config, resource, status or provider object.
- Cross-context transactions/repositories used to avoid an explicit owner port
  or saga.
- Retry, cache, distributed lock or rate-limit middleware with undeclared
  business semantics.
