# A3S Cloud Observability and Analytics Architecture

## 1. Decision

Logs and observability are first-class platform capabilities. They must answer:

- what product intent was accepted;
- which durable operation and workflow decisions followed;
- where and under which generation work executed;
- which public/internal requests reached it;
- what resource, model and provider behavior occurred;
- whether data is missing, delayed or redacted; and
- which operator or automated policy action changed the outcome.

Observability is not a second business database. PostgreSQL owner aggregates,
Flow histories, Runtime/Box journals, Gateway applied snapshots, immutable
object manifests, and product usage ledgers retain their named authority.
Metrics, indexes, traces and analytical tables are derived evidence with
explicit watermarks and gaps.

## 2. One causal identity chain

Every relevant signal carries the applicable subset of:

```text
installation / region / organization / project / environment
principal / request / idempotency key / operation
pipeline run / workflow run / agent execution / function invocation
cell operation / inference request / build run
workload / revision / deployment / replica / placement / claim
runtime unit / generation / node / gateway snapshot
trace / span / event / sequence / occurred_at / observed_at
schema revision / policy revision / release and artifact digests
```

Identities are typed fields, not parsed from message text or mutable labels.
High-cardinality dimensions are permitted where necessary but bounded and
classified. Prompt, response, Tool output, file content, Secret material and
raw tenant payloads are excluded by default.

## 3. Distinct histories

| History | Authority | Observation relationship |
| --- | --- | --- |
| Aggregate lifecycle | Owning PostgreSQL repository and Domain events | Telemetry links to versions; it never reconstructs commands as truth |
| Long-running management operation | Operations aggregate | Span/log projections explain an operation but cannot settle it |
| Workflow decisions | A3S Flow append-only history | Search projection is diagnostic only |
| Agent semantic events | Agents/Code journal and immutable content references | Trace/log indexes carry bounded metadata and references |
| Function invocation/result | Functions and its result/artifact policy | Runtime logs are execution evidence, not invocation truth |
| Runtime/Box lifecycle | Runtime and provider journals/observations | Cloud accepts exact generation evidence only |
| Gateway applied state and request facts | Gateway journal plus owner usage ingestion | Access logs do not authorize or republish targets |
| Audit | Shared append-only Audit authority | Signed exports/projections preserve redaction and watermark |
| Usage | Product owner fact plus admitted Usage ledger | Metrics and provider invoices are reconciliation inputs |

## 4. Signal pipeline

```text
Cloud / Gateway / Runtime / Box / Flow / Code / Power / providers
  -> in-process OTel SDK or bounded native event adapter
  -> local collector and A3S Observer kernel correlation
  -> redaction, sampling, resource identity and loss accounting
  -> metrics backend for real-time SLO evaluation
  -> immutable object chunks for retained logs/evidence where required
  -> optional Apache Doris projectors for search and OLAP
  -> authorized query APIs, alerts, Incident links and exports
```

Collectors are stateless or checkpointed consumers. A3S Lane may bound
telemetry ingestion priority and pressure after an owner record or durable
object chunk exists. Redis may cache recent query results and cardinality
controls. Neither may be the only copy of acknowledged audit, usage or runtime
evidence.

## 5. Logs

### 5.1 Contract

A log record contains typed severity, producer, schema revision, sequence or
gap marker, causal identities, bounded structured attributes, message class,
redaction class and optional immutable content reference. It has separate
`occurred_at` and `observed_at` values.

Runtime logs preserve per-stream order for one Unit/generation and expose
explicit truncation, rotation and gap evidence. Cross-unit global order is not
claimed. Agent and Tool output may be retained as separately authorized
immutable content; the default log projection contains only metadata.

### 5.2 Retention and access

- retention is driven by immutable data/audit/evidence policy revisions;
- tenant users see only authorized tenant/resource scopes;
- system administrators see platform health without implicit tenant payload
  access;
- approved support access requires a time-bounded support grant or break-glass
  record;
- searches report coverage watermark and unavailable partitions; and
- deletion/hold acts through the owner lifecycle and produces evidence rather
  than issuing uncoordinated deletes to every projection.

## 6. Metrics, traces and profiles

Metrics use closed names, units, temporality and bounded dimensions. Hard quota
and usage accounting never depend on sampled metrics. Required platform
families include:

- API/Gateway request, concurrency, rate-limit, cache and stream behavior;
- PostgreSQL, NATS, Redis, object, Git and Registry health/lag/capacity;
- Operation, Outbox, Flow, Lane and worker queue/lease behavior;
- Workload/Fleet placement, Claims, node pressure, Runtime/Box lifecycle;
- CPU/GPU allocation, inference queue/cache/topology/token/latency behavior;
- Agent Sessions/checkpoints, Function activation, Cell ownership and Web
  delivery; and
- pipeline stage, artifact, rollout, policy and restore behavior.

Distributed traces cross public request, Application handler, repository
transaction, Outbox/Flow, worker, Runtime/Box, Gateway origin and external
provider boundaries. Async links preserve causality when a child is not nested
in the parent's wall-clock span.

Continuous profiles or eBPF evidence are opt-in, capability-gated, bounded and
classified. A3S Observer reports acquisition gaps and overhead.

## 7. SLO and error-budget model

An `ServiceLevelObjectiveRevision` is Operations-owned canonical ACL that
binds:

- service/product and scope;
- indicator query and good/total or threshold semantics;
- target, evaluation windows and minimum data completeness;
- burn-rate alert windows;
- excluded maintenance policy;
- owner/on-call routing reference;
- release/promotion gate usage; and
- immutable revision/digest.

Missing data is not success. An evaluator emits a typed snapshot containing
source watermarks, gaps, calculation, result and revision. Workloads or
Delivery Pipelines may consume a committed decision through an owner command;
the metric backend never mutates desired state directly.

## 8. Incident lifecycle

Notifications delivers alerts. A separate, small Operations `Incident`
aggregate owns operational response:

```text
Detected -> Acknowledged -> Mitigating -> Monitoring -> Resolved -> Reviewed
```

It binds severity, affected scopes/services, exact alert/evidence references,
commander/owners, operator actions, mitigation Operations, status timestamps,
customer-impact classification and post-incident reference. It does not copy
raw logs, traces, aggregate state or provider payloads.

Automated mitigation is an ordinary authorized, idempotent owner command with
a preaccepted policy revision. It cannot be an arbitrary action encoded in an
alert expression.

## 9. Apache Doris role

Apache Doris is an optional shared OLAP projection for high-cardinality,
high-volume operational analytics. It is useful for:

- redacted log search and aggregation;
- trace/span analytics and critical-path studies;
- Gateway/inference request-attempt exploration;
- pipeline/build/deployment reliability trends;
- resource/usage/showback projections; and
- fleet/capacity and incident correlation.

It is not used for:

- aggregate command transactions, idempotency or optimistic concurrency;
- Flow, Runtime, Box or Gateway journals;
- hard quota or concurrency permits;
- authorization truth, Secrets, release selection or placement;
- the sole copy of audit, billing-grade usage or retained logs; or
- distributed locks or cross-provider transactions.

### 9.1 Ingestion

Two paths are allowed:

1. best-effort OpenTelemetry export for ordinary diagnostic telemetry; and
2. durable owner projectors for audit-safe metadata, usage, release evidence
   and immutable log manifests.

A durable projector derives a deterministic Stream Load label from dataset,
source partition/range and content digest. A timeout with unknown publication
outcome is inspected/reconciled using that same identity; it is not retried
under a new label. The projector advances its source watermark only after the
accepted load is visible.

### 9.2 Table design

- partition by bounded event time and retention class;
- distribute by installation/tenant/resource identity appropriate to the
  dataset;
- use inverted indexes only for admitted redacted search fields;
- keep raw/high-cardinality attributes bounded and typed;
- use synchronous single-table materialized views only where same-transaction
  consistency is required by the projection; and
- expose asynchronous multi-table views with an explicit refresh watermark.

### 9.3 Isolation and access

Tenants receive no direct Doris credentials. Authorized Cloud query services
apply tenant/resource policy and return bounded projections. Doris Workload
Groups may isolate query/ingest resource classes, but per-node/frontend limits
are not a platform-wide hard quota; Cloud admission and Lane pressure remain
authoritative. Stronger noisy-neighbor boundaries use separate compute/resource
groups or deployments where the provider supports them.

## 10. Cache and query consistency

Redis may cache authorized queries using:

```text
tenant + principal/grant epoch + query digest + source watermark
+ policy/redaction revision + result schema revision
```

Revocation or policy changes advance the relevant epoch. Cache loss is a miss.
Stale-while-revalidate is allowed only for explicitly diagnostic data and must
return its observation/watermark; security, quota, current deployment and
incident mutation prerequisites require the owner's declared consistency.

Queries use keyset pagination, bounded time windows and deterministic order.
They report partial source coverage rather than silently merging incomparable
watermarks.

## 11. Failure behavior

| Failure | Required behavior |
| --- | --- |
| Collector unavailable | Producers apply bounded buffering/sampling policy and emit loss counters; product execution follows its own safety policy |
| Metrics backend unavailable | SLO state becomes unknown, not healthy; autoscaling follows its declared missing-signal rule |
| Object log store unavailable | Required durable evidence can block terminal acknowledgement; diagnostic-only logs may shed visibly |
| Doris unavailable | Owner paths continue; projection lag grows; analytics queries report unavailability/watermark |
| Redis/Lane unavailable | Ingestion/query acceleration degrades or reconstructs; no authoritative record is lost |
| Clock skew | Preserve occurred/observed clocks, report skew and avoid false total ordering |
| Cardinality attack | Bound dimensions, shed diagnostic detail, retain counters/evidence of shedding, and protect owner paths |
| Redaction failure | Fail closed for classified export; never send raw payload as fallback |

## 12. Interfaces and no-Dashboard rule

REST/OpenAPI, maintained SDKs, CLI and Management MCP expose authorized:

- logs and tail streams with gaps;
- metric/SLO status and source completeness;
- trace and causal-resource lookup;
- pipeline/deployment/runtime diagnostics;
- incidents and response actions;
- usage/capacity summaries; and
- export/retention/projector health.

A3S Cloud provides no management Dashboard. Tenant applications may render
their own operational UI through `WEB0` using the same public APIs.

## 13. Delivery gates

| Gate | Outcome |
| --- | --- |
| `H0.5-OBS1` | Common resource/correlation schema, classification and loss semantics |
| `H0.5-OBS2` | Cloud/Gateway/Runtime/Box/Flow/Code/Power/Observer trace and log propagation |
| `H0.5-OBS3` | Metrics backend, closed signal families, cardinality and missing-data rules |
| `H0.5-OBS4` | SLO revisions, burn-rate evaluation and safe promotion/autoscaling consumption |
| `H0.5-OBS5` | Incident lifecycle, alert correlation, audited mitigation and post-incident evidence |
| `H0.5-OBS6` | Retention, redaction, tenant/system-admin access, exports and immutable log/evidence storage |
| `H0.5-OBS7` | Optional Doris schemas/projectors, deterministic ingestion, watermarks, isolation and rebuild |
| `H0.5-OBS8` | Load, dependency loss, telemetry loss, cardinality attack, disaster recovery and upgrade evidence |

## 14. Non-goals

- A second audit, usage, workflow, runtime-log, deployment or incident signal
  writer hidden inside a telemetry system.
- Direct tenant access to infrastructure databases.
- Prompt, response, Tool output, file content or Secret capture by default.
- Metrics that mutate desired state without a versioned evaluator decision.
- Treating an attractive visualization as availability evidence.
- Shipping an A3S Cloud management Dashboard.

## 15. Provider references

- [OpenTelemetry observability overview for Apache Doris](https://doris.apache.org/docs/dev/observability/overview/)
- [Apache Doris OpenTelemetry integration](https://doris.apache.org/docs/dev/ecosystem/observability/opentelemetry)
- [Apache Doris Stream Load](https://doris.apache.org/docs/dev/data-operate/import/import-way/stream-load-manual/)
- [Apache Doris load transaction behavior](https://doris.apache.org/docs/4.x/key-features/load-transaction/)
- [Apache Doris workload management](https://doris.apache.org/docs/dev/admin-manual/workload-management/resource-isolation-intro/)

