# 0031: Keep one Application session and semantic-effect authority

Status: Accepted

## Context

`APP0.1` established one immutable Application release bound to an exact
Workflow revision. Application delivery also needs end-user correlation,
release-pinned sessions, invocation requests, ordered Answer and final-output
messages, and conversation variables. Workflow already owns graph and semantic
step definitions, while A3S Flow owns run scheduling, attempts, cancellation,
recovery, and durable history. Identity owns Principals, Memberships, roles,
Resource Grants, and credentials.

Copying Flow history into an Applications event log, treating an application
end user as a workspace Principal, or allowing delivery retries to append the
same Answer or assignment twice would create conflicting authorities.

## Decision

Applications owns one `ApplicationEndUser`, `ApplicationSession`,
`ApplicationInvocation`, immutable `ApplicationMessage`, and immutable
`ConversationVariableRevision` model. A session pins one exact
`ApplicationRelease` identity and digest. It owns a monotonic channel-message
sequence, one optimistic conversation-variable head, and one aggregate version.
An invocation owns only request and delivery correlation and may bind one exact
`WorkflowRun`; it never stores a graph, attempt history, scheduler state, or
provider output history.

Every Workflow-derived Applications write carries the exact Workflow run,
portable step ID, positive attempt, and zero-based effect ordinal. That tuple
derives a stable UUIDv5 identity. One tuple may own exactly one Applications
semantic write across Answer, final output, and conversation-variable
assignment. Exact retries replay the immutable value; changed content or reuse
for another semantic kind conflicts. Each invocation admits at most one final
output, after which no new channel message may be appended.

An application end user is scoped to one Application and audience. Any link to
an Identity Principal is explicit and creates no Membership, role, Resource
Grant, session, or credential. Project-member delivery requires a Principal
link, while anonymous delivery cannot imply one.

`APP0.2-C1` supplies the domain and repository contract plus an atomic in-memory
conformance implementation. It adds no PostgreSQL table, production adapter,
public endpoint, credential, file reference, queue, Flow runtime, or product
availability claim. Later slices must persist the same constraints through A3S
ORM and call the existing WorkflowRun authority through a typed port.

## Consequences

- Sessions and conversation variables have one Applications-owned optimistic
  state authority without becoming a second Workflow variable engine.
- Duplicate delivery of an exact Flow-derived effect cannot duplicate a
  channel message or variable assignment, and effect drift fails closed.
- Flow remains the sole execution, attempt, cancellation, replay, and run-
  history authority; Applications retains only exact references and semantic
  delivery projections.
- Identity remains the sole workspace authorization authority, and caller-
  controlled end-user identifiers cannot mint workspace access.
- `APP0.2` and all application delivery capabilities remain unavailable until
  production persistence, WorkflowRun composition, the remaining session
  records, interfaces, and named dependency gates pass.
