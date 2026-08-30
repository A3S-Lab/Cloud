# 0087: One workload Runtime evidence authority

Status: Accepted

Date: 2026-08-30
Owners: Identity, Workloads, Fleet, Runtime, Box
Gate: `H0.4-WI2`
Evidence: [main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33319781762), [same-revision Box provider conformance](https://github.com/A3S-Lab/Cloud/actions/runs/33319781830)

## Context

A workload credential is safe only when one decision binds the accepted
Identity policy to the exact Workloads Claim, current Fleet Node session, and
provider-attested Runtime Unit generation. A hostname, image digest, process
ID, Node certificate, Runtime provider name, or cached placement cannot prove
that conjunction. Persisting a second Claim, Node, Runtime, or attestation
lifecycle inside Identity would create competing authorities.

Runtime `0.4.0` now carries one opaque `identity_attachment_digest` through the
Unit specification and provider evidence and publishes
`RuntimeAttestationBinding`. Box's confidential provider binds that attachment
to its provider attestation. Fleet does not yet publish an immutable Node
hardware-attestation fact, so this evidence is necessary but not sufficient
for credential issuance.

## Decision

Identity owns one versioned, immutable decision projection named
`cloud.identity.workload-runtime-evidence-binding.v1`. `WI2-C1` binds:

- exact accepted WorkloadIdentityPolicy ID, revision and ACL digest;
- exact Installation/Organization/Project/Environment and Workload revision;
- Workloads ResourceClaim ID, generation, Claim digest and prepared binding
  digest;
- NodePool ID and spec digest;
- Fleet Node ID, Agent instance, capability digest and last observation;
- Runtime report, Unit ID/generation, class, isolation, semantics, Spec digest,
  attachment digest and running state;
- provider resource/build, provider-attestation digest, Runtime-attestation
  binding digest and observation/receipt time.

The Identity policy ACL digest is the Runtime identity attachment. The binding
has a canonical SHA-256 digest and deterministic UUIDv5 identity, so the same
evidence replays to the same fact. Admission rejects reordered time, future
facts, non-running observations, lineage drift and evidence older than the
fixed 120-second protocol ceiling. Stored evidence is never a freshness cache:
every later issuance decision must re-read current owner state.

Version 1 requires `node_attestation_binding_digest = null` and its domain
method always returns false for credential-issuance authority. This is an
intentional fail-closed boundary, not an optional security mode. Full `WI2`
requires a new Fleet-owned, immutable Node hardware-attestation fact bound to
the policy's exact attestation profile; only a later versioned Identity
decision may consume it.

`WI2-C2` exposes one consumer-owned Identity Application port for obtaining the
normalized candidate. Its sole Infrastructure adapter composes exactly two
owner interfaces plus Runtime's public attestation contract:

- Workloads publishes `a3s.cloud.bound-runtime-claim.v1` through
  `IBoundRuntimeClaimQueryPort`. The owner query alone interprets current
  ResourceClaim state and exact replica-member/revision lineage, selects the
  same member-binding authority for ordinary and placement-group Deployments,
  and for a placement group verifies its immutable plan and exact role-specific
  member template. Both paths reuse the sole Workloads Runtime compiler. After
  C3a the query accepts no caller-authored execution semantics: it loads the
  exact immutable Deployment admission and publishes no Claim for legacy or
  explicitly unbound Deployments;
- Fleet publishes `a3s.cloud.runtime-node-evidence.v1` through
  `IRuntimeNodeEvidenceQueryPort`. The owner query alone checks current pool
  organization and membership/removal/maintenance, Ready Node state, Agent
  session, capability document/digest and exact Runtime observation; and
- `OwnerWorkloadRuntimeEvidenceAdapter` calls
  `RuntimeConsumerRequirements` and `RuntimeAttestationBinding`, cross-checks
  both owner identities and normalizes the C1 candidate.

Fleet observation records now carry their exact Agent instance and preserve
the first accepted receipt time on replay in both PostgreSQL and in-memory
adapters. Runtime's millisecond protocol time is verified within the stored
Cloud/PostgreSQL microsecond timestamp, then published canonically at protocol
precision. Application handlers do not import foreign repositories directly.
The adapter creates no repository, cache, retry loop, lock, queue, event store,
or provider lifecycle. Fleet uses one concrete repository across pool, Node and
control views. Workloads retains its sole Claim repository and one concrete
Workload/placement-group repository, rather than merging distinct aggregate
repositories for convenience. Each query double-collects its versioned heads
and latest observation. If any Claim, binding, revision/plan, pool,
Node/session, or observation changes during collection, it returns a
concurrency conflict and emits no fact.

PostgreSQL compare-and-swap, unique deterministic identity, request-digest
idempotency and immutable rows will own concurrency. Redis and A3S Lane may
reduce read or dispatch pressure but cannot admit evidence. Runtime and Box
remain provider/lifecycle authorities; Identity never parses provider-specific
attestation documents.

C2's original generic execution input was only an expected-Spec verifier, not a
mutation or authority to relabel an existing Unit. Component-only `WI2-C3a`
now closes that gap. Identity publishes one
`a3s.cloud.workload-runtime-execution-authorization.v1` owner fact containing
only exact owner lineage, Runtime class, isolation, semantics digest, opaque
attachment digest, NodePool and acceptance time. Its internal read fences the
Organization's canonical Installation, then locks current TrustDomain and
policy heads in the existing order; stale trust and lineage drift fail closed.
Workloads consumes that fact through one consumer-owned admission port and one
anti-corruption adapter.

The Workloads Domain names class and isolation only through the versioned Cloud
Published Language; it does not import Runtime execution/provider authority.
Its PostgreSQL repository maps migration `180` through the existing typed A3S
ORM table, selection and insert abstractions, with no parallel raw-SQL path.

Before scheduling, every current ordinary or placement-group Deployment commits
one immutable `a3s.cloud.deployment-runtime-execution-binding.v1` record through
migration `180`. The record is either the exact generic binding or an explicit
no-policy outcome; therefore absence is replayable and cannot later turn into a
different binding after process loss. Ordinary dispatch, placement-group v2,
reconciliation, restart and rollback projection all use that same record and
the sole Workloads Runtime compiler. Rows are accepted only while the exact
Deployment is Resolving and has no node, command, activation or cancellation
state. Scheduling rechecks the binding's NodePool before reservation and again
inside the final Deployment transition transaction; PostgreSQL uses the
canonical Deployment-then-Control lock order. Exact replay is the only later
write. If multiple Flow workers race with different observation times, the
first committed row wins; a loser reloads and validates that durable winner
instead of reinterpreting policy or overwriting it. Historic workflow versions and
legacy Deployments are neither backfilled nor relabelled; obtaining identity
requires a new Deployment even when the revision is unchanged. The stored value
contains no Identity policy ID, revision, credential rule, key, provider state,
cache, queue or parallel lifecycle.

Component-only `WI2-C3b` persists the sole Identity evidence history without
changing C3a Flow semantics. `WorkloadRuntimeEvidenceRecord` wraps the
deterministic V1 binding and exact admission time; its authority predicate is
also permanently false. The internal recorder derives one canonical
idempotency request from admission, Installation, tenant, Workload, Claim and
evaluation time. It resolves an exact historic replay first. On a miss it reads
the sole current Identity policy and the C2 owner candidate, then delegates one
write to `IWorkloadRuntimeEvidenceRepository`.

The existing `PostgresIdentityRepository` is the only implementation.
Migration `181` creates one all-typed
`workload_runtime_evidence_history` table; it has no current/head companion.
The transaction checks idempotency, takes the canonical Installation shared
fence, reuses the single current TrustDomain/Policy read, serializes the
deterministic binding identity, adopts an identical committed fact, and stores
the shared idempotency response. The database trigger repeats the Installation
and exact current Policy/TrustDomain checks, requires running ordered evidence
within 120 seconds, and forbids update/delete. A concurrent policy successor
therefore either commits after evidence or causes that stale evidence write to
conflict. Exact replay may still return the historic non-authorizing fact after
replacement. There is no provider-document parser, owner lifecycle table,
REST surface, workflow version, cache, Redis/Lane lock, queue, Audit/Outbox, or
parallel evidence registry.

## Consequences

- Agent, Workflow, Function, MCP, Durable Cell, inference, build, Gateway and
  Cloud-system services use the same binding contract.
- `WI2-C1` is a component foundation and must not be marketed as workload
  identity availability.
- `WI2-C2` supplies the verified owner-port chain and anti-corruption adapter;
  `WI2-C3a` supplies the component-only Workloads execution-admission handoff,
  `WI2-C3b` supplies the sole typed Identity evidence history and
  replay/concurrency/policy-replacement gates, and `WI2-C4` adds Fleet Node
  hardware evidence plus the issuance-ready versioned decision.
- `WI3` cannot issue, rotate or locally deliver credentials until `WI2-C4` and
  its stale/revoked/replayed evidence tests pass.
