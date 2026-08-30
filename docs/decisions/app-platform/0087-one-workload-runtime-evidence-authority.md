# 0087: One workload Runtime evidence authority

Status: Accepted

Date: 2026-08-30
Owners: Identity, Workloads, Fleet, Runtime, Box
Gate: `H0.4-WI2`

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

Identity will expose one consumer-owned Application port for obtaining the
normalized candidate. Its sole Infrastructure adapter may compose published
Workloads and Fleet owner interfaces plus Runtime's public attestation
contract. Application handlers may not import foreign repositories directly.
The adapter creates no repository, cache, retry loop, lock, queue, event store,
or provider lifecycle.

PostgreSQL compare-and-swap, unique deterministic identity, request-digest
idempotency and immutable rows will own concurrency. Redis and A3S Lane may
reduce read or dispatch pressure but cannot admit evidence. Runtime and Box
remain provider/lifecycle authorities; Identity never parses provider-specific
attestation documents.

## Consequences

- Agent, Workflow, Function, MCP, Durable Cell, inference, build, Gateway and
  Cloud-system services use the same binding contract.
- `WI2-C1` is a component foundation and must not be marketed as workload
  identity availability.
- `WI2-C2` adds the one owner port and anti-corruption adapter; `WI2-C3` adds
  immutable persistence and replay/concurrency gates; `WI2-C4` adds Fleet Node
  hardware evidence and the issuance-ready versioned decision.
- `WI3` cannot issue, rotate or locally deliver credentials until `WI2-C4` and
  its stale/revoked/replayed evidence tests pass.
