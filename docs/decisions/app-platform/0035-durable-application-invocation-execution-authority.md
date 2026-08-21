# 0035: Persist Application invocation execution authority before composition

Status: Accepted

## Context

Decision 0033 established deterministic composition from an Application
invocation into an ordinary Workflow Goal, Plan, and Run. Its first
implementation accepted the exact Ontology revision, optional Environment,
requesting Principal, and timeout on the later composition command.

That boundary is not sufficient for recovery. Applications commits an
invocation and its input before Workflow records are created. Process death or
concurrent cancellation can therefore require composition or cancellation
after the original request has disappeared. Asking a retrying caller to supply
those fields again permits authority drift and makes internal recovery depend
on transient delivery state. Deterministic run IDs detect some drift, but they
do not retain the authority needed to reconstruct the first request.

## Decision

Applications owns one immutable `ApplicationInvocationWorkflowAuthority`
companion for each invocation. It contains only the exact release identity and
digest, Ontology identity/revision/digest, optional Environment, requesting
Principal, and bounded timeout needed to construct the ordinary WorkflowRun
request. Its owner fields must match the invocation exactly.

`RequestApplicationInvocationWrite` commits that authority in the same
transaction as the invocation, input message, and advanced session head. The
in-memory conformance repository uses one write lock. PostgreSQL migration
`126` adds one table with exact invocation, Ontology, Environment, and Principal
foreign keys, a deferred Ontology content-digest check, and immutable update and
delete rejection. A failed authority check rolls back the entire invocation
request. Exact replay compares the complete stored authority and rejects any
changed reuse.

The internal composition command now carries only Organization, Project,
Application, session, and invocation identities. Its handler loads the stored
authority and constructs every start, adoption, and cancellation request from
that record. A cancelling or cancelled invocation requests idempotent
WorkflowRun cancellation from the same deterministic identity and never starts
a new run. The same path repairs cancellation after process restart and the
existing bind race.

Decision 0036 adds the authorization-before-replay component delivery CQRS over
this record without changing its ownership. This record is not an authorization
cache. It stores no credential, token, Membership, Resource Grant snapshot,
Secret, Workflow graph, Plan, run state, attempt, event, or Flow history.
Public delivery interfaces remain part of the later APP0.2 work, and
Gateway-routed delivery remains `APP0.3`.

This decision supersedes only Decision 0033's use of transient composition
fields. Its deterministic identities, typed request/evidence port, ordinary
Workflow repositories, and Flow ownership remain unchanged.

## Consequences

- Composition and cancellation recovery no longer depend on caller-supplied
  authority after invocation admission.
- Invocation acceptance is atomic across input, correlation, execution
  authority, and the session sequence.
- Missing or drifted authority fails closed before Workflow composition; exact
  retry remains restart-safe.
- Workflow remains the sole Goal, Plan, WorkflowRun, and cancellation
  authority, and A3S Flow remains the sole durable execution-history authority.
- `APP0.2-C5` remains component-only. Decision 0036 adds internal
  session/invocation commands, while public delivery protocols, remaining
  message/file/feedback records, and retained delivery recovery evidence are
  still required before `APP0.2` is available.

Decision 0038 subsequently adds the project-member management admission/read
subset. Application-scoped credentials, cancellation, answer delivery, and the
remaining public delivery/recovery contracts stay open.
