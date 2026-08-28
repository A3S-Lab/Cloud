# 0077: Use one elastic Workload authority with explicit state-safety semantics

Status: Accepted

## Context

A3S Cloud must deploy and scale stateless Functions and MCP services, warm
stateful Agent Harnesses, first-class Durable Cell collaboration spaces,
finite Workflow/Function Tasks, ordinary CPU Services, and distributed GPU
inference. These shapes have different state and traffic safety conditions.

Giving each product a scheduler or autoscaler would duplicate desired replica
state, placement, Resource Claims, Runtime lifecycle, drain, route, and failure
recovery. Treating every shape as an interchangeable stateless replica would
instead lose Agent workspace/checkpoint state, permit split-brain Cell writers,
misrepresent finite Tasks, or expose partial distributed model replicas.

The platform also needs two different kinds of scheduling: placing processes
onto CPU/GPU hardware and selecting an already eligible inference endpoint for
one request. Conflating those decisions would let the request path mutate
infrastructure or let the control plane ingest privacy-sensitive KV state.

## Decision

Workloads and Fleet remain the only tenant resource-placement authority.
Workloads owns desired replicas, scaling policy/decisions, stable replica and
placement-group identities, rollout, drain/retirement, and Runtime
reconciliation. Fleet owns node inventory, maintenance, and Claim delivery.
A3S Runtime owns only generic `Task` and `Service`; A3S Box is its production
provider. Edge produces complete targets and A3S Gateway is the only public
ingress.

All Service profiles use one outer transition:

```text
desired -> Claims -> Runtime apply -> healthy -> target -> serving
        -> admission closed -> product-safe -> target removed
        -> Runtime fenced/stopped/removed -> Claims released -> retired
```

An immutable state-safety profile selects the product proof required at
`product-safe`:

- stateless Services drain bounded requests;
- Agent Services stop new session leases and verify checkpoints before
  retirement or recovery;
- Durable Cell provider shards seal state and fence the prior writer epoch
  before another writer is published;
- provider-replicated state proves quorum-safe membership change;
- distributed inference removes or admits one complete placement group; and
- finite Tasks use accepted-work concurrency and terminal/cancellation
  evidence rather than replica scaling.

Manual scale, autoscale, rollout, rollback, node drain, maintenance, and
failure recovery all enter this transition. None may implement a separate
stop/move path.

Workloads will extend its existing control aggregate with immutable scaling
policy revisions, bounded source-attributed signal windows, idempotent scaling
decisions, drain leases, and correlated evidence. Metrics are evidence; the
accepted Workload control generation remains desired-state truth. Missing or
stale signals never cause scale down.

Scale to zero is opt-in and requires a bounded Cloud-owned activation path,
cold-start objective, overload behavior, and state-safe retirement. Gateway
may publish demand or call the activation command, but it never writes desired
replicas or starts Runtime units.

Durable Cell is a first-class service for human/multi-Agent shared rooms,
blackboards, presence, alarms, and live sessions. Individual Cells remain
provider-owned named state and are not Runtime units. Horizontal Cell scaling
uses provider shards and fenced partition movement compiled into the common
Workload path; it never creates one Service per Cell.

Distributed inference has separate managed Workload role slots such as
`serve`, `prefill`, `decode`, and independently gated `encode`. Each slot may
scale independently through the same Workloads evaluator; each replica inside
a slot may be a gang placement group. A complete Gateway serving cohort binds
all required role endpoints to one compatible model/deployment revision.
Gateway performs request-scoped endpoint selection, while Workloads/Fleet
alone places resources. Power owns model execution and opaque KV/embedding
transfer.

Replica demand and machine count remain distinct aggregates. A future Compute
provider may consume pool-level `CapacityIntent` derived from pending Claims,
but it cannot place workloads, commit Claims, or forge Fleet inventory. Node
termination requires cordon, safe evacuation, settled journals, Claim release,
credential revocation, and provider confirmation.

Cloud's own API, Worker, Relay, Gateway, migrator, and middleware remain in the
operator/A3S OS bootstrap plane so Cloud does not depend on its tenant
scheduler to keep that scheduler alive.

## Consequences

- Stateful and stateless products share mechanisms without pretending their
  safety proofs are identical.
- Durable Cell remains first-class without a `RuntimeUnitClass::Cell`, a Cell
  scheduler, or a per-Cell Cloud table.
- Agent warm-pool scaling cannot evict an execution that lacks verified
  recovery evidence.
- Functions choose Task, stateless Service, or external Connector explicitly.
- Distributed inference can scale phase roles and gang members without an
  `a3s-llm-d` control plane or inference replica registry.
- Gateway request selection and Workloads placement remain separately
  testable, with one owner at each layer.
- `H0.5` and each product gate must provide real process/node/device failure,
  replay, drain, fence, recovery, cleanup, quota, and oscillation evidence
  before elasticity is advertised.

