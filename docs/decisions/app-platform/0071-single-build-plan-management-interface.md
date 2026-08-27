# 0071: Expose one BuildPlan management interface

Status: Accepted

## Context

`P0.1-C3/C4/C5` production-compose deterministic BuildPlan detection, explicit
acceptance, exact Sources evidence, and trusted source-layout acquisition. The
commands and queries are internal, so operators and maintained automation still
cannot review proposals or retrieve the immutable accepted contract. Adding
REST, Management MCP, client, and CLI independently would risk four projections,
multiple page rules, repository reads in Presentation, and authorization that
drifts from the existing Developer Workflows policy.

Accepted BuildPlans are immutable records, but repository implementations must
still be treated as untrusted outer-layer adapters. A public read path must
detect an out-of-scope row, malformed persisted ACL, excessive page, duplicate,
or non-canonical order before exposing it. It must also authorize before
validating private resource identifiers so denied callers cannot probe tenant
state.

## Decision

`P0.1-C6` adds one `BuildPlanQueryService` in Developer Workflows Application.
It depends only on `IBuildPlanRepository` and
`IDeveloperWorkflowAuthorizationPort`. Both typed read handlers share this
service. The service:

1. authorizes the exact Organization, Project, Environment, Principal, and
   closed `ReadBuildPlan` action before validating or loading private input;
2. enforces the single `1..=200` page bound;
3. requires every repository result to revalidate its canonical ACL and exact
   tenant, Project, Environment, BuildPlan, and SourceRevision scope; and
4. requires list results to be bounded and strictly ordered by the domain's
   canonical `(project_root, BuildPlanId)` comparator.

The repository contract names that ordering, and both in-memory and PostgreSQL
adapters implement it. Presentation never reads the repository or imports a
Projects, Sources, or concrete Developer Workflows adapter. It consumes only
Identity's root-published tenant Guard for coarse route admission; exact policy
remains behind the Application authorization port. The bounded-context root
keeps its `presentation` module private and re-exports only the module, response
DTO, and route contracts required by production composition adapters.

The public interface is a projection over the existing CQRS only:

- REST detects proposals, accepts one canonical `proposalAcl`, lists accepted
  plans for one exact `sourceRevisionId`, and gets one exact accepted plan;
- the versioned OpenAPI contract describes those same closed requests and
  responses;
- the maintained TypeScript client and CLI call those REST operations; and
- four Management MCP tools dispatch the same commands and queries through the
  shared buses and reuse the same response DTOs.

Detection, list, and get require `cloud:read`; acceptance requires
`build:write`. These transport scopes are coarse admission only. The existing
Application authorization port remains the exact membership, Resource Grant,
and Environment authority for every surface. Management MCP binds all four
tools to explicit Project and Environment arguments rather than resolving a
second resource policy.

Requests are closed and accept no caller-authored layout, source bytes,
proposal object, credentials, checkout receipt, or local path. Responses expose
canonical `proposalAcl` and accepted `contractAcl`, their typed digests and
evidence, and immutable acceptance facts. They expose no credential or local
checkout state. A3S ACL remains the sole product configuration language and is
parsed only by the existing domain objects through `a3s-acl`.

The embedded Dockerfile recipe remains the Sources-published `BuildRecipe`;
Developer Workflows does not define another recipe DTO, client type, platform
enumeration, or OpenAPI property contract. One schema builder preserves the
single directional difference: Sources mutation input may omit nullable
`target`, while an accepted-plan response always serializes it. Ordinary API
successes and failures use the shared private/no-store response policy; the
explicitly public OpenAPI document keeps its own cache policy.

C6 adds no table, migration, aggregate, parser, evaluator, checkout, provider,
cache, queue, worker, Relay, scheduler, BuildRun, Workload, Route, or Operation
authority.

## Consequences

- REST, Management MCP, client, and CLI cannot disagree about BuildPlan read
  authorization, page bounds, ordering, canonical ACL, or evidence projection.
- A malformed or drifting repository adapter fails closed in Application before
  Presentation can serialize its state.
- Detection and acceptance retain their existing single authorities; the new
  interface does not turn HTTP or MCP into an orchestration layer.
- Pre-acceptance repository discovery, monorepo fan-out, Compose import, and
  downstream build/deployment handoffs remain later P0 work.
