# 0085: Issue privileged allows through one atomic Identity authority

Status: Accepted

## Context

Persisted platform roles and tenant-support grants are authorization inputs,
not permission to act. A privileged allow must prove that the Principal,
credential, current policy and binding, and exact optional support grant were
simultaneously valid. Loading those facts in separate transactions, trusting a
boolean administrator claim, or using Redis, Lane, or an application cache as
the decision fence leaves a revocation race between evaluation and use.

The existing privileged-decision value model can preserve exact evidence, and
the shared scoped Audit table can retain the immutable allow fact. Identity
therefore needs one transactional decision authority rather than a second
policy evaluator, lock service, decision table, event stream, or audit store.

## Decision

`IPrivilegedAuthorizationDecisionRepository` is the sole privileged-allow
issuance port. The registered `AuthorizePrivilegedAccess` Application command
validates the closed request shape and delegates to that port;
`PostgresIdentityRepository` is the production implementation and the
in-memory composition fails closed.

One PostgreSQL transaction:

- key-share locks the immutable Installation row so concurrent decisions may
  proceed while Installation-scoped mutations remain fenced;
- share locks and resolves the active Principal, exact API-token version,
  current accepted policy revision, active role binding, and exact optional
  tenant-support grant;
- requires `cloud:read` or `platform:write` from the embedded credential
  snapshot according to the requested closed permission;
- issues one digest-bound `PrivilegedAuthorizationDecision` containing the
  exact credential, policy, binding, grant, scope, action, resource, request,
  and decision time; and
- stores the complete decision as the details of one
  `identity.privileged-access.authorize` record through the shared scoped Audit
  authority before committing.

The corresponding policy, binding, credential, Principal, and grant mutation
paths retain conflicting row locks. An allow and a revocation therefore have a
single database order: either the allow commits its exact pre-revocation
snapshot first, or the revocation wins and the allow fails closed. A successful
allow emits no Outbox event because authorization evidence is not an
asynchronous integration command.

The issuer is also a transaction-local Identity persistence primitive for a
concrete protected mutation. After taking the canonical Installation mutation
lock, each non-bootstrap platform role-policy/binding or tenant-support
proposal/approval/revocation use case calls that same issuer before its write
and commits both outcomes in one transaction. Its repository DTO accepts the
actor Principal and exact credential ID only. Permission, action, scope, and
resource are closed constants or domain-derived values owned by the concrete
use case, never request-body or tool arguments. Tenant-support authentication
evidence is copied from the issued credential snapshot rather than accepted
from a caller, and the business Audit details retain the exact decision
reference. A denied decision rolls back with the attempted write and emits no
partial evidence.

This command is an internal Application authority, not a public generic policy
evaluator. Maintained REST, client, CLI, and Management MCP use cases must
select their closed permission, action, scope, and resource themselves and
must obtain Principal and credential identity from verified request context.
They cannot accept those authority fields from an untrusted request body or
tool argument.

## Consequences

- The [complete main CI
  run](https://github.com/A3S-Lab/Cloud/actions/runs/33226790289) and its
  [PostgreSQL 17 H0
  job](https://github.com/A3S-Lab/Cloud/actions/runs/33226790289/job/99031980422)
  pass all retained gates. H0 races role, API-token, and support-grant
  revocation against concurrent allow issuance; every later allow is denied.
- Every successful allow, and only a successful allow, writes exactly one
  shared Audit fact containing the reference-matching decision; no privileged
  Outbox, Redis lock, Lane lock, cache truth, or dedicated decision table is
  introduced.
- The decision snapshot is replayable evidence, not a durable capability that
  can authorize a later unrelated operation. A concrete use case consumes the
  result only for the exact action, scope, resource, and request it supplied.
- A real PostgreSQL provider gate races a platform binding mutation with
  revocation of its exact API token. Either the decision and protected business
  fact both commit before revocation, or both are absent; after revocation even
  an otherwise replayable request is denied.
- `C0.5-MT2-C3` remains in progress until maintained concrete interfaces use
  this authority. `MT3` must then remove every
  `actor_is_platform_admin`/ambient-role bypass and route cross-surface
  enforcement through the same Identity decision port.
