# 0058: Close expired Connector dispatches with an exact indeterminate resolution

Status: Accepted

## Context

A Connector attempt that has crossed its durable dispatch fence cannot be
reacquired. After its bounded outcome deadline, Cloud correctly observes the
attempt as indeterminate and Flow must not authorize another provider call.
Before this decision, that safe observation had no durable operator conclusion:
the attempt remained unresolved forever even when the provider outcome could no
longer be established.

Treating the attempt as accepted or rejected would fabricate provider evidence.
Retrying or cancelling it would claim provider capabilities that the bounded
HTTP contract does not expose. Mutating the attempt without a separate fact
would also lose who concluded the recovery and why.

## Decision

Connectors owns one immutable `ConnectorExecutionAttemptResolution` for one
exact organization, project, environment, profile, revision, attempt, request
digest, request byte count, dispatch start, and outcome deadline. The only v1
resolution is `indeterminate`. It may commit only after the stored dispatch
deadline and records a bounded control-free operator reason, actor, and time.

Migration `155` atomically pairs that resolution with body-free terminal
`Indeterminate` execution evidence, transitions the exact attempt to
`terminal`, and writes the existing idempotency, audit, and Outbox facts. Both
directions of the resolution/evidence pair are deferred database constraints;
neither fact can commit alone. The generic C6 settlement port rejects
`Indeterminate`, so only this exact recovery authority can create it.

Authorization runs before idempotency replay. REST/OpenAPI `1.66.0` and the
maintained TypeScript client expose bounded unresolved-attempt listing, an
exact safe attempt projection, exact resolution reads, and one idempotent
resolution write. The projections omit the fence token, request and response
bodies, endpoint, credentials, and provider text.

A resolved terminal attempt is still projected to Workflow and other C6
consumers as `Indeterminate`, never as an ordinary completed provider result.
Replay therefore performs no materialization, egress authorization, provider
dispatch, retry, or cancellation.

## Consequences

Operators can close an otherwise permanent recovery item with durable,
auditable evidence while preserving conservative provider semantics. The
unresolved keyset feed shrinks atomically with the terminal transition, and an
exact replay returns the original conclusion.

This adds no provider call, retry permission, cancellation, queue, scheduler,
retry counter, response store, copied Flow history, or second orchestration
authority. It does not claim whether the provider accepted or rejected the
original request, certify an external provider, or make HTTP Request publicly
available as a Workflow product capability.
