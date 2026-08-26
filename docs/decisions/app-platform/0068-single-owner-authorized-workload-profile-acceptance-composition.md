# 0068: Compose one owner-authorized workload-profile acceptance path

Status: Accepted

## Context

`P0.2-C1/C2` define the canonical workload-profile contract, immutable logical
profile and revision identities, the authorization-first acceptance command,
and migration `147`'s atomic revision, idempotency, audit, and Outbox write.
`P0.2-C4` production-composes the accepted-revision compilation read path.
The acceptance command itself still had no production CQRS registration.

`P0.1-C4` already introduced the one Developer Workflows consumer
authorization adapter over Identity Membership and Resource Grant interfaces,
Identity's sole `ResourceAccessEvaluator`, and the exact Projects Environment.
Constructing another adapter, evaluator, or owner lookup for workload profiles
would duplicate policy and permit the two acceptance paths to drift. Moving the
command into Workloads, Executions, Artifacts, an HTTP handler, or a worker
would cross the bounded-context boundary and create a second orchestration
mechanism.

## Decision

`P0.2-C5` production-composes exactly one internal `AcceptWorkloadProfile`
command on the existing CQRS bus. The production root constructs one
`Arc<dyn IDeveloperWorkflowAuthorizationPort>` and shares that exact instance
with both `AcceptBuildPlanHandler` and `AcceptWorkloadProfileHandler`.

The workload-profile handler continues to depend only on the Developer
Workflows-owned authorization and repository interfaces. Authorization runs
before ACL parsing and idempotency replay. The handler then validates the
canonical `a3s.cloud.workload-profile.v1` contract against the exact accepted
BuildPlan, derives the stable logical profile and next immutable revision, and
delegates the only write to the existing `IWorkloadProfileRepository`.
Migration `147` remains the sole production authority for continuous revision
sequence, idempotency, audit, and Outbox atomicity.

Identity and Projects types remain confined to the existing Infrastructure
authorization adapter. Workloads, Executions, and Artifacts remain confined to
the existing compilation anti-corruption adapters. No foreign owner model,
concrete repository, or transport enters the Application handler.

C5 adds no public route, source-layout acquisition, table, migration,
authorization evaluator, repository, event rail, relay, queue, worker,
BuildRun, Workload, WorkloadRevision, Deployment, ExecutionTemplate revision,
Execution, Route, Operation, retry, timer, scheduler, or owner lifecycle write.

## Consequences

- Production has one authorization-first workload-profile acceptance command
  and one exact accepted-revision compilation query.
- BuildPlan and workload-profile acceptance cannot select different Identity
  or Projects policy mechanisms because they share the same production port
  instance.
- Developer Workflows retains profile acceptance authority; downstream owners
  retain template validation and lifecycle authority.
- Architecture and composition tests freeze the single adapter construction,
  shared port use, Application isolation, and exactly-once CQRS registration.
- Public management surfaces and every Workload, Execution, route, Operation,
  and scheduling handoff remain later explicit slices.
