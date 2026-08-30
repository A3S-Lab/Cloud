# 0079: Keep workload trust in Identity and execution evidence with existing owners

Status: Accepted

## Context

A3S Cloud needs short-lived cryptographic identity and default-deny private
connectivity for Agent, Workflow, Function, MCP, Durable Cell, inference,
build, Gateway, and Cloud system-service workloads. Existing management
Principals, Fleet node certificates, Runtime Unit IDs, network addresses,
image names, process IDs, and shared cluster credentials each prove a
different fact. Treating any one of them as workload identity would permit a
stale or unrelated process to inherit authority.

Adding a service-mesh control plane, a product-specific Agent identity model,
or a certificate table beside Secrets would duplicate existing ownership.
Identity must own trust policy while Fleet, Workloads, Runtime, Box and Secrets
retain their evidence and lifecycle authority.

## Decision

Identity owns two canonical ACL contracts:

- `cloud.identity.trust-domain.v1` binds an installation-scoped trust domain
  to an exact non-secret provider profile, trust bundle, node-attestation
  profiles, credential formats and bounds, revocation policy, and explicit
  federation bundles; and
- `cloud.identity.workload-policy.v1` binds one tenant Workload revision and
  closed product role to the exact trust domain, node pool, A3S Runtime
  `Task`/`Service` class, Runtime isolation level, semantics profile,
  attestation profile, credential rotation policy, audiences, private service
  names, and peer-policy revision digests.

Both contracts are parsed and generated only with `a3s-acl`, are closed to
unknown fields, have bounded canonical set ordering, and carry SHA-256
identity. Accepted revisions use deterministic IDs derived from owner,
revision number, and contract digest. Repository ports require the expected
previous revision, so a future PostgreSQL adapter can select one head under
concurrency without a distributed lock as correctness proof.

The one replaceable workload-identity provider port initially exposes only
capability and exact observed root/federation trust-bundle inspection. It
carries no key material and cannot issue a credential. A support boolean
cannot substitute for the canonical observed federation-bundle digest set.
Issuance is added only after Fleet and Runtime provide an exact admitted Node,
Claim, Unit, and generation attestation in `WI2`.

`WI1-C2` persists this decision with migration `179`: two immutable,
predecessor-linked revision histories and one current head per aggregate. A
Workload Identity Policy carries the exact `TrustDomainRevisionId` in its
canonical ACL, so its digest and deterministic revision ID cannot drift with a
later trust-head change. Identity serializes protected reads and writes against
the canonical Installation and reuses the sole transaction-local privileged
authorization decision, shared idempotency, Audit and Outbox rails. Tenant
lineage, Workload revision and NodePool ownership remain foreign-key references
to their existing owners; no second authorization, lock, cache or event store is
introduced.

## Consequences

- All Runtime profiles share one identity policy abstraction; there is no
  Agent, Function, Cell, inference, or Cloud-system identity subsystem.
- Runtime remains product neutral: Identity reuses its published Unit and
  isolation types but does not add product fields to Runtime.
- Secrets remains the durable application/provider credential authority;
  ephemeral workload private keys and certificates never enter management
  APIs or policy ACL.
- Fleet node identity cannot substitute for workload identity, and mTLS alone
  cannot substitute for the consuming domain's peer authorization.
- Redis, DNS, proxies, and provider registration databases may carry
  projections but cannot become policy truth.
- `WI1-C1/C2` plus the maintained REST/OpenAPI `1.79.0`, TypeScript client,
  CLI, and Installation-bound Management MCP surface remain a foundation, not
  an availability claim. Main PostgreSQL proof, attestation, issuance,
  discovery, enforcement, provider revocation drills, and exact-provider
  evidence remain required.
