# 0032: Persist Application session semantics atomically

Status: Accepted

## Context

Decision 0031 established one Applications-owned session, invocation,
ordered-message, conversation-variable, and Workflow semantic-effect contract.
Its component conformance repository proved the state transitions and replay
rules, but it deliberately retained no production state. A process restart
would therefore lose the delivery correlation even though the referenced
`WorkflowRun` and A3S Flow history remained durable.

Persisting each child independently would allow a message without its session
head, a variable successor without its lineage update, or one Flow-derived
effect to become both an Answer and an assignment. Copying WorkflowRun or Flow
history into the Applications schema would instead create a second execution
authority.

## Decision

Migration `125` and one A3S ORM repository persist `ApplicationEndUser`,
`ApplicationSession`, `ApplicationInvocation`, immutable
`ApplicationMessage`, immutable `ConversationVariableRevision`, and the
cross-kind Workflow-effect claim. The production PostgreSQL adapter factory is
the sole constructor for this repository.

Session creation atomically inserts or adopts the Application-scoped end user,
the exact-release session head, and revision-one variables. Invocation request
atomically inserts the invocation, deterministic input message, and advanced
session head. Each later Answer, final output, or variable revision inserts its
immutable semantic record, claims the exact run/step/attempt/ordinal tuple,
and advances the optimistic session head in the same transaction. Invocation
and close transitions use row locks plus expected aggregate versions. Stable
session, invocation, message, variable, and effect identities make exact
retries return the stored value after reconnect; changed reuse conflicts.

Database constraints pin every session child to its exact organization,
project, Application, release ID, and release digest. Deferred head and
bidirectional effect-claim checks admit only complete transactions. Message
sequence and variable lineage remain contiguous, final output is unique and
terminal for later channel appends, and semantic children are immutable.
Application-level advisory locks serialize creation of shared end-user and
session identities without introducing a lock table.

The schema stores only Applications semantic delivery state and an ordinary
foreign key to the existing `WorkflowRun`. It contains no graph, Flow event,
attempt log, scheduler state, provider payload history, cancellation queue,
Identity authority, or second replay engine. The PostgreSQL 17 integration
gate migrates a clean database, crosses the production repositories, restarts
the adapter, proves exact replay and cross-kind exclusion, and rejects direct
mutation.

## Consequences

- Application session and invocation state now survives process restart with
  the same optimistic and exactly-once semantics as the component contract.
- Partial message, variable, claim, and head writes roll back together, while
  database constraints fail closed if repository invariants drift.
- A3S Flow remains the sole durable execution, attempt, scheduling,
  cancellation, and history authority; Workflow remains the sole graph and
  `WorkflowRun` authority.
- `APP0.2-C2` is component-only. Typed production WorkflowRun composition,
  delivery commands and authorization, public blocking/streaming interfaces,
  remaining records, and retained recovery evidence are still required before
  `APP0.2` or any Application delivery capability is available.
