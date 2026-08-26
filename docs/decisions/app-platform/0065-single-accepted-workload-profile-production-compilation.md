# 0065: Compose one exact accepted workload-profile compilation path

Status: Accepted

## Context

`P0.2-C1/C2` give Developer Workflows a closed workload-profile compiler and
immutable accepted BuildPlan and workload-profile revision authorities.
`P0.2-C3a/C3b/C3c` add the only concrete anti-corruption adapters for
Artifacts build outcomes and the existing Workloads Service and Executions
Task-template contracts. Those pieces were deliberately component-only: the
production application did not select the repositories, assemble the adapter
chain, or expose one exact accepted-revision compilation query.

Composing the same logic inside Workloads, Executions, an HTTP handler, or a
new worker would move Developer Workflows acceptance authority into another
bounded context or create a second orchestration mechanism. Creating owner
lifecycle state during compilation would also conflate a deterministic read
with deployment, execution, scheduling, routing, and Operation writes.

## Decision

`P0.2-C4` production-composes one internal
`CompileAcceptedWorkloadProfile` query on the existing CQRS bus. The query
requires exact Organization, Project, Environment, BuildPlan, logical profile,
profile revision, and successful BuildRun identities. Its Application handler
depends only on the Developer Workflows-owned `IBuildPlanRepository` and
`IWorkloadProfileRepository` interfaces plus the existing
`WorkloadProfileCompilationService`.

The handler loads the exact accepted BuildPlan and profile revision, validates
their persisted identities and immutable relationship, then invokes the sole
C3 chain. `ArtifactsWorkloadBuildOutcomeAdapter` obtains the owner-published
successful BuildRun value and binds it to the local plan;
`WorkloadsServiceProfileAdapter` or
`ExecutionsScheduledTaskProfileAdapter` validates the translated owner
template. The result retains the exact logical profile identity, revision
identity, and revision number so a later owner handoff can preserve causation.
Repository identity substitution, invalid persisted authority, or a compiler
binding change fails closed.

The production composition root constructs this chain exactly once. Its typed
PostgreSQL factory selects one Developer Workflows management repository family
for API/Worker roles. The existing Preview projection family remains separate
and Relay-only, because it has a different consistency and delivery role.
Cross-context types remain confined to the C3 Infrastructure adapters; the C4
Application handler imports no foreign bounded-context model or concrete
persistence implementation.

C4 adds no public route, authorization claim, table, migration, aggregate,
event, Outbox, relay, queue, worker, retry rail, timer, scheduler, Operation,
Workload, WorkloadRevision, Deployment, ExecutionTemplate revision, Execution,
Route, or owner lifecycle write.

## Consequences

- Production has one exact accepted-revision compilation read path, assembled
  from consumer-owned interfaces and the three existing owner ACLs.
- Developer Workflows remains the only BuildPlan/profile acceptance and
  compilation authority; Artifacts, Workloads, and Executions retain their own
  contracts and lifecycle ownership.
- Architecture tests require one construction and CQRS registration path, and
  focused tests exercise the same adapter chain against exact accepted state.
- Authorization-first acceptance/public interfaces and every owner lifecycle
  handoff remain explicit later P0.2 slices; C4 does not make Developer
  Workflows publicly available.
