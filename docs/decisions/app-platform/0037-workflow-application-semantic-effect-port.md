# 0037: Apply Workflow semantic effects through the single Applications authority

Status: Accepted

## Context

Decisions 0031 and 0032 established one Applications-owned session,
message, conversation-variable, invocation-correlation, and exactly-once
Workflow-effect persistence boundary. Decisions 0033 through 0036 made an
authorized delivery request create or adopt one ordinary WorkflowRun. The
reverse semantic boundary remained missing: Workflow had no typed way to read
the Applications-owned variable head, append an Answer or final output,
advance variables optimistically, or report the run's terminal observation.

Calling the repository directly from Workflow would leak Application,
release, session, invocation, and aggregate-version authority into the
executor. Trusting those caller-supplied identities would permit a valid run
identity to be paired with a foreign session. Reconstructing an effect from
the current session head would also make a committed retry fail after a later
message or variable revision advanced that head.

## Decision

Applications exposes one internal `IWorkflowApplicationEffectsPort`. Workflow
addresses it only by Organization and exact `WorkflowRun`, plus the stable
step, positive attempt, ordinal, canonical occurrence time, and semantic
payload for a write. The port resolves Project, Application, exact release,
session, and invocation from the sole persisted WorkflowRun correlation. A
partial unique index and the in-memory conformance map admit at most one such
invocation per Organization and run. Missing or corrupt correlations fail
closed.

The read operation returns the current Applications-owned conversation values
with their immutable revision ID, revision number, and canonical value digest.
A variable write presents that complete compare-and-swap version and a stable
Workflow effect. It never presents the session aggregate version, so ordered
messages may interleave without forging variable authority. The existing
repository atomically advances the session and immutable variable revision.

Answer and final-output writes use the existing monotonic session sequence.
Their message IDs are derived from invocation, semantic kind, and exact
`WorkflowRun + step + attempt + ordinal` effect. Variable revision IDs use the
same effect under the session namespace. Before a write and after any failed or
ambiguous write response, the service reads that deterministic identity and
accepts only exact content, authority, lineage, and occurrence-time replay.
Changed reuse conflicts. The existing cross-kind effect claim and single-final-
output constraints remain the only write authority.

Terminal observation resolves the invocation by WorkflowRun and applies the
existing optimistic invocation transition. An exact status and completion-time
redelivery replays; a different terminal fact conflicts. Workflow and Flow
continue to own execution, attempt history, cancellation, retry, and output
aggregation. This port stores only Applications-visible semantic projection.

The PostgreSQL and in-memory repositories add only exact read methods for
WorkflowRun-correlated invocations and deterministic messages. No migration or
second index is required because migration `125` already owns both identities
and constraints.

This decision is component-only. The current WorkflowRun Flow generation does
not yet invoke the port, so Answer execution, Applications-variable
materialization/assignment, terminal reconciliation, public blocking or
streaming delivery, and retained PostgreSQL command-path evidence remain later
gates.

## Consequences

- Workflow cannot choose or spoof Application, release, session, invocation,
  or session aggregate versions when applying an Applications semantic effect.
- Exact redelivery remains recoverable after later session state advances and
  after a process loses a committed write response.
- Conversation-variable compare-and-swap evidence is explicit and independent
  from interleaved channel-message sequencing.
- Cross-kind reuse, changed payloads, stale variable versions, duplicate final
  output, late frames, and terminal drift fail closed through the existing
  Applications repository invariants.
- `APP0.2-C7` adds no public availability. Workflow runtime integration,
  message variants, files/citations, feedback/annotations, channel protocols,
  and production PostgreSQL recovery evidence remain required.
