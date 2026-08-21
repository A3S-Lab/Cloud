# 0036: Authorize Application delivery before replay and compose through existing owners

Status: Accepted

## Context

Decisions 0031 through 0035 established the release-pinned Application session,
message, variable, invocation, durable execution-authority, and ordinary
WorkflowRun composition contracts. They intentionally exposed no delivery
command boundary. A caller could not yet open or close a session, atomically
admit an invocation and its input, request cancellation, or reconnect through a
bounded cursor without reaching below the Applications application layer.

That boundary must preserve three properties simultaneously. Authorization
must run before any replay or validation result can reveal hidden state. Client
retries and ambiguous database outcomes must adopt the exact committed
identity instead of creating another session, message, or run. Workflow and
Flow must remain the sole authorities for execution and cancellation history.

## Decision

Applications exposes one component CQRS delivery boundary for published
`ProjectMembers` releases. Every command and query first checks the existing
project `ResourceAccessEvaluator`. Existing sessions are then narrowed to the
one `ApplicationEndUser` linked to the acting Principal. A deterministic
Application-and-Principal UUIDv5 identity lets separately opened sessions
reuse that end user without copying Membership, role, grant, or credential
state. Missing authorization, a foreign actor, and a foreign invocation fail
with the same hidden-resource result.

`OpenApplicationSession` accepts a stable caller-supplied session identity and
atomically writes the exact release, deterministic end user, initial variable
snapshot, and session through the existing repository. `CloseApplicationSession`
uses the existing optimistic session version. Both resolve concurrent and
ambiguous commits by reading and validating the exact persisted successor.

`RequestApplicationInvocation` accepts a stable invocation identity, exact
Ontology revision and digest, optional Environment, response mode, bounded
object input, timeout, and expected session version. After authorization it
uses the WorkflowRun timeout policy, commits the invocation, immutable
execution authority, first input message, and advanced session head through the
single Decision 0035 write, then calls the existing identity-only composition
handler. Reusing the identity with changed input or execution authority
conflicts. A composition failure leaves the admitted request repairable by an
exact retry; an ambiguous commit is adopted before composition.

`CancelApplicationInvocation` first persists the Applications cancellation
transition and then calls Workflow's existing deterministic cancellation port.
A request that never acquired a WorkflowRun terminalizes as cancelled when
Workflow confirms there is no run. A bound request remains cancelling until
ordinary Workflow evidence is observed. No Applications-local cancellation
worker, run record, queue, or event history is introduced.

`GetApplicationSession`, `GetApplicationInvocation`, and
`ReplayApplicationSession` expose the same authorized component state. Cursor
replay is bounded to 1 through 500 ordered messages, rejects gaps or records
that drift from the session head, and returns the exact current immutable
conversation-variable revision. It projects no Workflow or Flow history.

Migration `127` adds the ordinary WorkflowRun 30-day maximum to the persisted
invocation timeout constraint. The domain, in-memory repository, PostgreSQL
repository, and composition admission therefore reject the same out-of-policy
authority. The production process registers all commands and queries over the
existing Applications and Workflow adapters.

This boundary is component-only. It adds no REST, OpenAPI, client, CLI, MCP,
browser, embed, SSE, application credential, anonymous delivery, Gateway route,
rate limiter, file, feedback, or annotation surface. Identity-issued
application credentials and externally routed delivery remain later APP0
gates.

## Consequences

- Project-member delivery has one authorization-before-replay command/query
  path and one Principal-linked end-user identity.
- Stable session and invocation identities recover concurrent retries and lost
  commit responses without duplicating semantic writes or WorkflowRuns.
- Invocation admission and cancellation reuse the persisted authority and the
  existing Workflow state machine; Applications still owns only delivery
  correlation and channel-visible state.
- Cursor reconnect fails closed on gaps, foreign records, invalid heads, and
  unauthorized callers.
- `APP0.2-C6` remains component-only and unavailable. Public delivery
  interfaces, additional message/file/citation/feedback/annotation semantics,
  blocking/streaming protocol parity, and retained recovery evidence remain
  required for `APP0.2`.
