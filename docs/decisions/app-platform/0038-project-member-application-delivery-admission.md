# 0038: Admit project-member Application invocations through one owned path

Status: Accepted

## Context

Decisions 0031 through 0037 established the Applications-owned session,
message, variable, semantic-effect, invocation-authority, authorized delivery
CQRS, and Workflow-effect records and their composition into ordinary Workflow
Goals, Plans, and Runs. Those capabilities had no maintained delivery
interface. Adding an interface without a closed identity and replay contract
could create presentation-owned end users, duplicate sessions under concurrent
retries, or allow a management token to read another Principal's conversation.

The release contract already distinguishes `project_members`,
`authenticated_end_users`, and `anonymous` audiences. The current Identity
authority authenticates management Principals and authorizes Projects, but it
does not yet issue application-scoped end-user credentials. The first delivery
slice therefore must not imply the later public credential, Gateway, blocking,
or streaming contracts.

## Decision

Applications admits this slice only for a `project_members` release and an
authenticated Principal that is authorized for the exact Project. One stable
Application end-user identity is UUIDv5-derived from Application and Principal.
Session and invocation identities are UUIDv5-derived from their owner plus the
idempotency scope and key. Request content is deliberately excluded from the
identity so reuse of the same key with changed content reaches the existing
record and fails as a conflict instead of creating a second aggregate.

The management admission commands are thin adapters over Decision 0036's
explicit delivery CQRS. Session open maps the idempotency identity to a stable
caller-owned session and delegates the exact release and initial variables.
Invocation admission authorizes any optional Environment, resolves the exact
Ontology revision and digest, supplies the current optimistic session version,
and retries bounded concurrent advances before delegating the immutable input
and execution authority. The existing composer creates or adopts the ordinary
deterministic Workflow Goal, Plan, and Run; Workflow and A3S Flow remain their
sole authorities.

Repository replay compares the semantic client request. It excludes
server-owned timestamps and an input-message sequence allocated from the
session head, while retaining exact release, content, digest, response mode,
and complete Workflow-authority checks. The adapter never owns session,
invocation, WorkflowRun, or Flow state.

Session, invocation, and ordered-message queries require the same Project
authorization and the linked Principal. Another Principal receives the same
not-found result as a missing session. All five Management MCP tools require
`application:write`, including reads, so a broad `cloud:read` token does not
gain conversation access.

REST/OpenAPI `1.43.0`, the maintained TypeScript client, CLI, and Management
MCP are thin adapters over these commands and queries. `blocking` and
`streaming` remain accepted invocation intent from the immutable release, but
this interface returns admission evidence only and never waits for or streams
an answer.

## Consequences

- Project members can idempotently open/read their own exact-release sessions,
  request/read invocations, and page ordered messages through every maintained
  management interface.
- Concurrent retries cannot create a second session, invocation, input
  message, Goal, Plan, or WorkflowRun for one idempotency identity.
- Applications retains session and invocation truth; Workflow retains
  Goal/Plan/Run truth; A3S Flow retains durable execution history.
- This slice adds no application-scoped credential, anonymous or
  authenticated-end-user admission, public close/cancel interface,
  synchronous/SSE answer delivery, provider mechanism, Gateway route,
  feedback, citation, or product-availability claim.
- `APP0.2-C8` is a project-member management interface only. Workflow dispatch
  over Decision 0037, the remaining `APP0.2` delivery and recovery contracts,
  and `APP0.3` through `APP0.6` still gate availability.
