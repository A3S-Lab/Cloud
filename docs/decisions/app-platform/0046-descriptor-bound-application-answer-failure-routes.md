# 0046: Route Application Answer failures through descriptor-bound DAG edges

Status: Accepted

## Context

Decision 0040 dispatches descriptor-bound Application Answer effects through
the Applications-owned message port and resumes the Workflow only after that
write has committed. Every owner error previously left the Answer Hook
unresolved, even when Applications had returned a deterministic terminal
rejection and the immutable graph declared an error edge. The admitted
descriptor and executable DAG therefore could not express a bounded Answer
failure path for either a root invocation or a composite child frame.

Adding an Applications-specific branch engine, retry rail, error table, or raw
owner-error copy would duplicate existing authorities and could disclose
private session or message state. Reinterpreting Plan versions 1 through 6 or
WorkflowRun versions 1 through 14 would also break immutable replay behavior.

## Decision

The exact `application.answer` descriptor may declare one required, static,
single object-valued `error` output, owner-classified retry, and failure-branch
fallback. A graph may select that output through one ordinary Workflow DAG
edge in addition to the Answer's normal output edge. Candidate Output steps
remain structurally restricted, and revision semantic validation proves the
exact Applications owner, owning-application execution class, release binding,
descriptor identity, failure contract, and selected handle. Every other Output
descriptor remains ineligible.

The compiler emits `cloud.workflow.plan.v7` only when an exact Application
Answer selects that descriptor-bound edge. Application-composed Plan-v7 runs
use immutable WorkflowRun input/runtime/Flow version 15 and runtime build
`a3s-cloud-workflows@17`; builds `@1` through `@16` remain explicit replay
entries. Plans v1-v6 and WorkflowRun inputs v1-v14 retain their exact bytes and
behavior. Plan v7 can preserve an independently admitted Application-variable
failure route in the same immutable graph.

The existing Answer Hook remains the sole observation path. Applications
`Invalid`, `NotFound`, `Conflict`, and `Forbidden` results are deterministic
terminal rejections. The coordinator resumes the exact root or frame-bound
Hook with only the closed classification and its existing authority-bound
identity. Flow converts it into `cloud.workflow.step-failure.v4`, selects the
descriptor's `error` handle, and lets the ordinary DAG activate the reachable
failure branch. `Unavailable` and `Internal` results remain unresolved
coordination failures; the Hook stays active for the existing idempotent retry
path.

Failure v4 stores only the step identity, closed classification, and one stable
classification-specific message. Raw Applications errors, Answer content,
session details, and copied owner evidence are excluded. Root resume schema
`cloud.workflow.application-answer-failure-resume.v1` and frame resume schema
`cloud.workflow.application-answer-failure-resume.v2` preserve the same exact
effect identity, root invocation authority, frame path, and zero-based Answer
ordinal already frozen by Decisions 0040 and 0043.

Projection reconstructs the same result from immutable WorkflowRun input and
verified Flow history. The source Answer Output remains `failed`, exposes the
exact `error` selected handle and redacted error, and has no successful result.
Its reachable failure sink may complete, allowing the parent WorkflowRun to
complete without converting the rejected Answer write into success. Composite
children retain the root Application effect identity and do not project a
second child lifecycle effect. Exact replay does not repeat a terminal owner
write.

Migration `143` widens only the existing Workflow step-projection selected-
handle constraint to admit failed Output routing evidence. The aggregate still
proves Plan v7, the exact Applications descriptor, failure contract, declared
edge, v4 evidence, and selected handle before persistence. Completed Output
steps with a selected handle remain rejected. No public OpenAPI schema change
is required.

## Consequences

- Deterministic Application Answer write rejections use the same descriptor,
  Plan, DAG, Flow, and projection mechanisms as other admitted step failures.
- Root and repeated composite-frame Answers share one failure model while
  retaining exact root effect identity and frame-local routing.
- Transient or internal owner failures retain Decision 0040's fail-closed,
  idempotent retry behavior instead of becoming business data.
- Persisted Hook, failure, and projection evidence never includes raw owner
  error text or Answer content.
- This adds one structural projection migration but no table, column, queue,
  worker, scheduler, timer, retry counter, provider path, second message store,
  public route, or configuration language.
