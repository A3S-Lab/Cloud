# 0042: Expose project-member Application lifecycle and replay through C6

Status: Accepted

## Context

Decision 0038 exposed project-member session admission, invocation admission,
and narrow reads, but deliberately left session close, invocation cancellation,
and complete session replay internal. Decision 0036 already owns those state
transitions and queries, including authorization before replay, Principal-bound
session access, optimistic versions, ambiguous-commit recovery, WorkflowRun
cancellation, contiguous message validation, and current-variable resolution.

Creating presentation-specific close, cancellation, or replay state would
split authority from Applications and Workflow. Returning only the existing
message list would also discard the session head, current variable revision,
next cursor, and `hasMore` evidence needed for deterministic recovery.

## Decision

REST/OpenAPI `1.44.0`, the maintained TypeScript client, CLI, and Management
MCP expose three project-member management operations:

- close one caller-owned session at an exact aggregate version;
- cancel one caller-owned invocation at an exact aggregate version; and
- replay one bounded contiguous channel page together with the session head,
  current Applications-owned variable revision, next sequence, and `hasMore`.

The adapters dispatch the existing `CloseApplicationSession`,
`CancelApplicationInvocation`, and `ReplayApplicationSession` contracts. They
persist no state and introduce no repository or lifecycle abstraction. Close
and cancel require the normal bounded idempotency credential at the transport
boundary, while the target identity plus `expectedVersion` is the C6 replay
fence. Exact retries return the existing `replayed` evidence; changed or stale
versions conflict.

All three operations retain Decision 0038's `application:write` scope and exact
Project/Principal ownership checks. Cancellation delegates to the sole
Workflow cancellation port and returns its optional WorkflowRun evidence.
Replay projects only Applications-owned session, message, and conversation
variable state; it does not expose or reconstruct Workflow or A3S Flow history.

## Consequences

- Project-member operators can finish and recover the admitted management
  lifecycle through every maintained interface without a second authority.
- Full replay preserves the exact session and variable heads alongside bounded
  ordered messages, so callers can advance a cursor without inference.
- Close and cancel remain optimistic, authorization-first, and exactly
  replayable after ambiguous persistence outcomes.
- This slice adds no application credential, authenticated-end-user or
  anonymous admission, blocking response wait, answer stream, Gateway route,
  provider mechanism, feedback, citation, or availability claim.
- `APP0.2` remains in progress; application-scoped public delivery and retained
  production interface recovery evidence still gate product availability.
