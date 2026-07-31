# Architecture

A3S Workflow separates durable orchestration from node execution. The control
plane decides what should run; an A3S Runtime provider decides where and how a
node runs. There is no in-process node execution path.

```text
React Studio       Coding-agent CLI + Skill
           \         /
            v       v
       A3S Boot API  <--------- runtime evidence
              |                         ^
              v                         |
       A3S Flow engine ---- TaskSpec ----+
              |                         |
              v                         v
         PostgreSQL             A3S Runtime provider
                                  |     |     |
                               CPU   GPU   sandbox pools
```

## Control plane

The API and worker are separate A3S Boot processes. The API validates graphs,
persists definitions, appends A3S Flow events, and enqueues durable work. Any
number of workers can claim PostgreSQL queue rows with `SKIP LOCKED`; expired
leases are recovered when a worker starts.

PostgreSQL is the sole authoritative store for:

- workflow definitions and optimistic versions;
- A3S Flow event streams, pending tasks, and dead letters;
- approval hooks and durable run state;
- A3S Memory items;
- per-node Runtime invocation, placement, generation, digests, and observation.

Redis is not required. A cache may be added later, but it cannot own workflow
truth or recovery state.

## Runtime boundary

Every graph node, including start, router, approval, and output, becomes a
content-addressed A3S Runtime task. The control plane writes a typed invocation
artifact and submits a `RuntimeSpec` containing:

- stable workflow, run, step, node, and attempt identity;
- immutable node-runner artifact URI, media type, and SHA-256 digest;
- provider and optional pool placement;
- CPU, memory, process, timeout, and ephemeral-storage requests;
- isolation and network policy;
- opaque secret references rather than secret values;
- a bounded, content-addressed output artifact contract.

The provider returns a unit ID and generation. The worker observes that exact
generation, verifies the result media type, size, and digest, then appends the
result to the Flow event stream. A stale generation cannot complete a newer
attempt.

Approval nodes cross the Runtime boundary twice: `execute` creates a durable
hook, and `resume` consumes the approved payload in a new Runtime invocation.
Router results select a named source handle; inactive branches are propagated
without running their downstream nodes.

## AI-native nodes

The node runner implements a stable protocol for:

| Kind | Runtime behavior |
| --- | --- |
| Start | Supplies the typed workflow input |
| Template | Produces JSON with typed token substitution |
| LLM | Calls an OpenAI-compatible gateway |
| Agent | Runs a bounded model/tool loop |
| Tool / HTTP | Calls an allow-listed endpoint |
| Router | Selects one explicit graph handle |
| Memory | Stores or searches A3S Memory through the control plane |
| Approval | Suspends and later resumes a durable run |
| Output | Produces the final typed workflow result |

Gateway, memory, and HTTP access are passed to the node as policy and secret
references. They are not ambient capabilities of the Flow worker.

## Independent scaling

The API, Flow workers, and Runtime providers scale independently. Every node
kind can select a provider and pool; stateless node units can be spread across
replicas in pools such as `cpu`, `gpu`, or `sandbox`. Provider implementations
may create as many immutable Runtime units as capacity allows. Scale workers
with `docker compose up --scale worker=3` without moving node execution into
those workers.

Stateful behavior is externalized before scaling:

- run and queue state lives in PostgreSQL;
- Agent memory lives behind A3S Memory and is persisted in PostgreSQL here;
- approvals live as A3S Flow hooks;
- artifacts are addressed by digest;
- secrets remain references resolved by the provider.

The included process provider is deliberately a local-development adapter. It
uses A3S Runtime lifecycle and evidence contracts but does not claim container,
microVM, cgroup, or confidential-computing enforcement. Production providers
must advertise and enforce their real capabilities, and reject unsupported
policies.

## Failure and recovery

- Definitions use optimistic versions to prevent lost edits.
- Flow events are append-only per run.
- PostgreSQL tasks are leased and recoverable after worker failure.
- Runtime unit identity includes the attempt and phase.
- Output is accepted only after digest and size verification.
- Every submitted node retains durable Runtime evidence for audit and UI use.
