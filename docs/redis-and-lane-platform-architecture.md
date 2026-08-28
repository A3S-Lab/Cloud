# Redis and A3S Lane Platform Architecture

## 1. Decision

Redis and A3S Lane are shared **acceleration, admission, fairness and
backpressure mechanisms** in A3S Cloud. They are useful precisely because they
stay below product semantics:

- PostgreSQL remains desired state, idempotency, quota reservation, Operation,
  usage and audit truth;
- A3S Flow remains durable workflow, retry, timer, cancellation and saga
  authority;
- transactional Outbox/A3S Event remains committed-fact delivery;
- Workloads/Fleet remains placement, Claim, rollout and node-command authority;
- A3S Runtime/Box remains Unit lifecycle and node-local execution; and
- A3S Gateway remains the live request path.

A3S Lane may prioritize and admit already-authorized work. Redis may share
bounded queue/limiter/cache state across replicas. Losing either must not lose
accepted business intent, create an unauthorized side effect, bypass a hard
quota, or make recovery ambiguous.

The currently inspected A3S Lane `0.5.1` provides local named lanes, priority,
concurrency, timeout, pressure signals, metrics/telemetry, host-owned typed
queues, and an evolving Redis job backend with leases, rate/active limits,
events, retries, DLQ, logs and flow dependencies. Cloud adopts only the
sub-capabilities assigned below. Package presence is not conformance or an
availability claim.

## 2. Abstraction boundary

```text
owning Cloud aggregate / Gateway request policy
  -> durable admission identity + policy revision + deadline
  -> optional Redis shared rate/concurrency/buffer state
  -> A3S Lane local priority/concurrency admission
  -> owner-approved executor / provider attempt
  -> generation-bound result
  -> authoritative PostgreSQL/Gateway/Runtime settlement
```

Lane job/command identity is derived from the owner Operation, Flow step,
request attempt, Execution, Workload command or Runtime Unit generation. It is
never a new product identity. Lane payloads contain opaque IDs, digests,
priority/deadline and bounded non-secret execution hints; they do not copy
Secrets, prompts, responses, source archives, model weights, semantic Agent
events or mutable aggregates.

The owner validates every result and fence. A valid Redis/Lane lease alone
cannot mutate an aggregate, publish a route, release a Claim, debit durable
quota or acknowledge a Flow step.

## 3. Approved uses

| Area | Redis role | A3S Lane role | Durable authority |
| --- | --- | --- | --- |
| Management API protection | Cross-Gateway rate-shaping counters with bounded overshoot policy | Local per-endpoint/priority concurrency and backpressure | Identity policy + PostgreSQL command/idempotency/quota |
| FaaS synchronous traffic | Bounded activation/request buffer and shared active count when scale-to-zero is enabled | Deadline/priority admission to warm Function Service replicas | Function invocation/Operation when durable; Gateway snapshot + Workloads activation |
| FaaS asynchronous invocation | Reconstructible delivery accelerator keyed by exact invocation | Fair worker admission after owner lease | Function/Execution aggregate + Flow/Outbox |
| Workflow node execution | Optional worker pressure/cache hints only | Local fairness between already-ready Flow steps | WorkflowRun + A3S Flow history/timers/retries |
| Agent execution | Optional bounded presence/pressure hints | A3S Code session single-flight turns and tool concurrency; Cloud workers may use local Agent admission | Agents semantic sequence + provider/Runtime evidence |
| Build/model ingest/evaluation Tasks | Optional reconstructible dispatch acceleration | Per-pool priority/concurrency and resource-pressure feedback | BuildRun/Execution/Operation + Fleet Claims |
| Inference requests | Distributed rate/active shaping, bounded affinity or hot-target hints | Request priority, deadline and phase-local admission inside Gateway/Power adapters | Inference policy/usage + Gateway snapshot; Workloads owns capacity |
| Durable Cell requests | Optional connection/presence hints only when provider permits | Provider-local turn/fairness scheduling | Cell provider SQLite/epoch state + Gateway snapshot |
| Background reconciliation | No required role | Local concurrency cap after PostgreSQL lease acquisition | Owning row/lease + Flow/Operation |
| Observability export | Short bounded batch buffer only | Priority/backpressure between critical audit-safe operational signals and best-effort telemetry | Source histories; object/log manifests; external telemetry backend |

### 3.1 Worker integration profile

Cloud Worker first acquires the authoritative PostgreSQL/Flow lease, then
offers a typed item to a local Lane. It does not pre-claim an unbounded backlog.
Lane priority may choose which eligible item runs first but cannot change
tenant authorization, durable priority class, deadline, retry count or
ownership.

The recommended Cloud profile uses host-owned typed queues or the smallest
QueueManager surface with:

- closed lane IDs and maximum concurrency from admitted platform policy;
- host-owned cancellation and terminal settlement;
- Lane retry disabled for product/provider attempts (`RetryPolicy::none()`);
- no Lane DLQ as a substitute for a visible blocked/failed Operation;
- no Lane persistence as the only copy of accepted intent; and
- pressure/latency/active metrics exported as observations only.

Pure, side-effect-free local computations may use bounded Lane retries, but
the result remains discardable and cannot advance business state without
owner validation.

### 3.2 Redis job-backend profile

The Redis backend is admitted only for a queue whose complete pending set can
be reconstructed from PostgreSQL/Gateway desired state. Enqueue occurs after
the durable owner commit and uses a deterministic custom job ID. Completion is
written to the owner transaction before Redis acknowledgement/removal. On
ambiguous acknowledgement, replay either observes the owner receipt or reruns
only under the owner's still-current fence.

Redis job `retry`, `repeat`, `flow`, dependency, delayed, DLQ and log features
are disabled unless a named owner contract proves that the feature is merely
an acceleration of existing Flow/Operation semantics. In particular:

- Lane Flow jobs never represent A3S Workflow or A3S Flow history;
- Lane retry counters never define provider retry policy;
- Lane DLQ never hide a terminal/blocked domain outcome;
- Lane repeat jobs never define schedules; Boot/Automations owns due time; and
- Lane job logs are queue diagnostics, not Runtime logs, Agent events, audit or
  durable usage.

## 4. Redis responsibility and topology

One deployment may expose logically separated Redis namespaces/profiles, but
not one Redis mechanism per product:

| Profile | Data lifetime | Required behavior |
| --- | --- | --- |
| `rate` | Window/permit TTL | Atomic per-key update, bounded time source, eviction forbidden for hard profiles, explicit fail-open/closed and overshoot |
| `admission` | Request/job deadline plus recovery window | Deterministic ID, token-owned lease, stale-token rejection, bounded payload, reconstruction scan |
| `cache` | Revision/expiry bounded | Tenant/auth/policy/version key, no write-back, lost invalidation safe |
| `affinity` | Short request/session/model TTL | Hint only; missing/stale entry falls back to the frozen eligible set and cannot grant access |

Keys begin with deployment authority and profile schema revision, then opaque
tenant/scope and owner digests. Human-readable tenant names, raw credentials,
Secrets, prompts/responses and provider tokens are forbidden. Cluster hash tags
may co-locate one atomic owner key set but cannot merge unrelated tenant keys.

Production requires TLS, ACL users per process/profile, encryption/Secret
rotation, memory and eviction policy, topology discovery, backups only where
the profile requires them, bounded command/script execution, metrics and
tested failover. Redis scripts/functions are versioned artifacts with canonical
digests and mixed-version rules; arbitrary runtime Lua from product code is
forbidden.

## 5. Failure and consistency rules

| Failure | Required outcome |
| --- | --- |
| Redis unavailable before admission | Hard/security limit fails closed; policy-permitted soft shaping may consume only its bounded local emergency budget |
| Redis loses recent limiter state on failover | Measured overshoot remains within the certified policy; durable quota/usage is unchanged and still authoritative |
| Redis loses queue state | Recovery scan republishes current owner intent with the same job IDs; stale/terminal intent is not recreated |
| Worker dies after Lane/Redis claim | Owner lease expires and advances its fence; stale worker result cannot settle; replay follows owner retry semantics |
| Worker settles PostgreSQL but loses Redis ACK | Redelivery observes exact owner receipt and performs ACK-only cleanup |
| Lane process restarts | Local pending work is re-derived from owner leases/intent; no accepted command is lost merely because Lane memory is empty |
| Queue pressure | Admission rejects/delays with typed reason and deadline; pressure is an autoscaling signal, never a desired-replica write |
| Redis partition/split brain | Required atomicity profile rejects unsafe writes; no lock/permit without a valid owner fence can commit business state |

## 6. Fairness and priority

Product contexts may choose a closed semantic priority class, not an arbitrary
integer. Workloads admission maps it to the common Lane policy after enforcing
Organization/Project/Environment quotas. Suggested classes are:

1. safety/cancellation and writer fencing;
2. interactive control and user-visible Agent/Function requests;
3. normal workflow/build/inference work;
4. background reconciliation, ingest and cache prewarm; and
5. maintenance, cleanup and best-effort telemetry export.

Priority cannot preempt an already dispatched non-preemptible side effect.
Cancellation must settle or fence active work before capacity is reused.
Aging prevents starvation within configured deadline/tenant fairness bounds;
it cannot elevate work above a security or hard-cap lane.

## 7. Observability contract

A3S Lane metrics and pressure events enrich the common observability pipeline:

- queue depth, oldest eligible age, enqueue/admit/start/settle rate;
- active count, configured/effective concurrency and rate-limit delay;
- lease renewal/loss, stalled recovery and duplicate suppression;
- completion/failure/timeout/indeterminate outcomes;
- per-lane wait/run latency histograms; and
- Redis round-trip, script error, failover, eviction and reconstruction lag.

Labels use closed lane/profile/outcome/revision values. Organization/Project
are authorized log/usage dimensions, not unbounded Prometheus labels. Job IDs,
payloads, user text and Secrets do not enter metrics. Trace links carry owner
request/Operation/Flow/Execution/Unit correlation but Lane spans remain
operational evidence, not the semantic history.

## 8. Delivery gates

| Gate | Required outcome |
| --- | --- |
| `H0.5-LANE1` | Freeze Cloud's closed Lane profile, priority classes, owner/fence envelope, Redis namespaces and negative feature boundary |
| `H0.5-LANE2` | Pin an exact A3S Lane release; certify local host-owned concurrency, cancellation, drain, pressure and telemetry without Lane retry/DLQ/Flow authority |
| `H0.5-LANE3` | Certify Redis rate/admission/cache/affinity profiles over TLS and ACLs, including atomic script digests, mixed versions, eviction and topology failover |
| `H0.5-LANE4` | Prove PostgreSQL-first enqueue, deterministic reconstruction, owner-settle-before-ACK, stale fence rejection and zero duplicate provider side effects across three workers |
| `H0.5-LANE5` | Run multi-tenant fairness, starvation, overload, deadline, cancellation, Redis loss/partition/restart and bounded-overshoot load gates |
| `H0.5-LANE6` | Verify FaaS, Workflow, Agent, build/ingest and inference consumers reuse the same profile or document why they need only local Lane; source ratchets reject product queues |

The first production adoption should be one bounded worker-admission slice with
no Redis dependency. Redis-backed admission follows only after reconstruction
and failover evidence. A broad “replace all queues with Lane” migration is
explicitly prohibited.

## 9. Non-goals

- Replacing A3S Flow with Lane Flow jobs.
- Replacing PostgreSQL Operations/desired state with Redis jobs.
- Replacing Outbox/NATS committed-fact delivery with Lane events.
- Replacing Fleet command journals or Runtime receipts with Lane leases.
- Treating Redis locks, counters, caches or queue state as hard business truth.
- Copying prompts, responses, Secrets, source, weights or aggregate JSON into
  generic job payloads/logs.
- Giving each product its own Redis cluster, queue vocabulary, retry rail, DLQ
  or priority system.
