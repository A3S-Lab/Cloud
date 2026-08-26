# 0066: Compose one closed BuildPlan detection path

Status: Accepted

## Context

`P0.1-C1` defines one bounded canonical `SourceLayoutSnapshot`, the
`IBuildPlanDetector` interface, deterministic BuildPlan proposals, closed
diagnostics, and the built-in Asset ACL and Dockerfile detector adapters. The
Asset ACL detector is authoritative while Dockerfile discovery is heuristic.
Those components were tested but were not selected by the production
application or reachable through its existing CQRS bus.

Constructing detector lists in controllers, source adapters, or callers would
make precedence and supported detector inventory depend on the entry point.
Letting detection read Sources repositories, checkout directories, or provider
credentials would also turn a deterministic projection into another source
resolution authority.

## Decision

`P0.1-C3` production-composes one internal `DetectBuildPlanProposals` query.
Its input is one already canonical `SourceLayoutSnapshot`; its output is the
existing `BuildPlanDetection` value. The Application handler depends only on
the local `BuildPlanDetectionService` and maps invalid source evidence to a
closed input error.

The production composition root constructs one service with exactly one
`AssetAclBuildPlanDetector` and one `DockerfileBuildPlanDetector`. The service
sorts the interface implementations by their closed detector kind, so explicit
Asset ACL intent is evaluated first and can terminate heuristic fallback. The
handler is registered exactly once on the existing CQRS bus.

Concrete detectors remain Developer Workflows Infrastructure adapters. The
query handler imports no Assets or Sources model, detector implementation,
repository, persistence adapter, transport, or provider. It does not enumerate
a checkout directory; producing a trustworthy source-layout snapshot remains a
separate later Sources-facing boundary.

C3 adds no acceptance authority, public route, table, migration, aggregate,
event, Outbox, relay, queue, worker, retry rail, timer, scheduler, BuildRun,
Workload, Execution, Route, Operation, or owner lifecycle write.

## Consequences

- Every production caller observes the same bounded detector inventory,
  precedence, proposal ordering, and closed diagnostics.
- Detection remains a deterministic read over exact content evidence; Sources
  remains the only source revision and provider authority.
- Architecture tests reject concrete detectors, foreign contexts, repositories,
  and delivery mechanisms from the query handler and require one production
  construction and registration path.
- Source-layout acquisition, authorization-first BuildPlan acceptance, public
  interfaces, build execution, and all downstream lifecycle handoffs remain
  explicit later P0 slices.
