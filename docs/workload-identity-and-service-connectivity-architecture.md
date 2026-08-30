# A3S Cloud Workload Identity and Service Connectivity Architecture

## 1. Decision

Every process that communicates inside an A3S Cloud installation must prove
which exact admitted workload generation it represents. Network location,
node identity, image name, process ID, mutable DNS, and possession of a shared
cluster secret are insufficient identity.

A3S Cloud adds one workload-trust contract and one private-service projection:

- Identity owns trust-domain and workload-identity policy;
- Fleet owns node enrollment and attestation;
- Workloads owns logical service identity and desired membership;
- Runtime owns exact Unit/generation identity and observations;
- Box/OCI Runtime provide process/isolation evidence;
- Secrets/PKI bind a short-lived credential to the admitted identity through a
  provider port;
- the consuming bounded context owns peer authorization and egress intent; and
- a connectivity compiler projects that intent to Runtime/Box network and
  identity attachments.

A SPIFFE-compatible provider is preferred where it fits the deployment, but
SPIFFE, SPIRE, a proxy, or a PKI product is a replaceable mechanism. Its
registration database is never Cloud product truth.

## 2. Identity taxonomy

| Identity | Meaning | Authority |
| --- | --- | --- |
| `PrincipalId` | Human or service actor issuing a management/API command | Identity |
| `NodeId` and node generation | Enrolled compute host and its current attested control channel | Fleet |
| `WorkloadId` and `WorkloadRevision` | Logical desired service or task profile | Workloads |
| `DeploymentId` and placement generation | One admitted rollout/placement decision | Workloads/Fleet |
| `RuntimeUnitId` and Unit generation | One physical Task or Service lifecycle | Runtime |
| `WorkloadIdentityId` and policy revision | Cryptographic identity allowed for an exact logical/physical binding | Identity |
| Invocation/Run identity | Agent execution, WorkflowRun/activity, Function invocation, Cell operation, inference request, build/test stage | Owning product |
| Secret version | Authorized materialization of a provider or application credential | Secrets |

These identities are correlated but never substituted. A valid node
certificate does not prove which workload is calling. A workload identity does
not grant management Principal permissions. A Function invocation ID is not a
Runtime Unit ID.

## 3. Trust domains

A trust domain is an administrative and cryptographic boundary, not a tenant
namespace. The default hierarchy is:

```text
installation
  -> region or physically independent failure domain
    -> security environment class, such as production or non-production
```

Organizations and Projects remain authorization scopes inside a trust domain.
Operators may isolate regulated tenants in a separate domain, but ordinary
tenant creation must not create a new root of trust.

`TrustDomain` owns:

- stable identity and canonical name;
- root/bundle provider binding and current revision;
- allowed node-attestation profiles;
- credential formats and maximum lifetimes;
- rotation overlap and revocation policy;
- explicitly enrolled federation relationships; and
- immutable audit lineage.

Federation exchanges verification bundles and closed authorization intent. It
never implies cross-domain access.

## 4. Workload identity policy

`WorkloadIdentityPolicyRevision` is canonical ACL owned by Identity and binds:

- installation, trust domain, Organization, Project and Environment;
- logical Workload/revision and closed product role;
- required Runtime class and semantics-profile digest;
- required node pool, isolation, attestation and confidential-compute claims;
- allowed identity format and lifetime;
- allowed audience/service names;
- peer-policy references;
- issuance, rotation, drain and revocation behavior; and
- policy revision/digest.

The policy contains references, not private key bytes. A revision is immutable.
Changing trust, audience or peer scope creates a new revision and a new
Deployment/Unit generation where enforcement cannot safely update in place.

## 5. Attestation and issuance chain

```text
Fleet enrolls and attests Node generation
  -> Workloads/Fleet admit Deployment and resource Claim
  -> Runtime applies exact Unit generation with policy digest
  -> Box/OCI Runtime creates isolation boundary and reports evidence
  -> identity agent verifies Node + Claim + Unit + process selectors
  -> PKI provider issues short-lived workload credential
  -> Runtime observation binds credential generation and expiry
  -> private-service compiler admits the endpoint
```

Issuance is denied if any identity, generation, digest, Claim, node evidence,
isolation capability, or selector is missing or stale. Private keys are
generated or delivered through the local workload endpoint and are never
returned through Cloud management APIs.

Runtime remains product neutral. Its generic capability reports an opaque
identity-attachment digest and generation-bound evidence; it does not parse
Organization, Agent, Function, or service-policy fields.

## 6. Private service discovery

`PrivateService` is a Workloads-owned projection over admitted logical members,
not a new deployment aggregate. It contains:

- stable service identity and tenant scope;
- protocol and named port;
- exact active WorkloadRevision;
- complete healthy member set of Node, Runtime Unit, generation and private
  endpoint identities;
- workload-identity policy and peer-policy revisions;
- publication generation, expiry and acknowledgement; and
- consistency watermark.

Internal names resolve only to the currently acknowledged complete set. DNS,
client-side discovery, or a local proxy may carry the projection, but none is
authoritative. A member is removed before or atomically with fencing/drain; a
stale endpoint is never retained because its address still responds.

Workflows normally call owner Application ports. Network discovery is for
deployed service-to-service protocols, not a way to bypass domain ownership or
reach another context's database.

## 7. Connectivity classes

| Class | Admission and enforcement |
| --- | --- |
| Same-Unit loopback | Runtime spec and process boundary; never published as cluster identity |
| Same-node private | Exact Unit identities plus Box-local network policy and short-lived mTLS where processes differ |
| Cluster private | PrivateService snapshot, peer policy, workload mTLS, bounded ports and Fleet/Box network enforcement |
| Distributed inference transport | All cluster-private rules plus topology generation, GPU Claim, transport/RDMA capability, and Power role identity |
| External egress | Product-owned destination/credential policy compiled to Box enforcement; DNS and address changes cannot widen the policy silently |
| Public ingress | Edge complete snapshot and A3S Gateway only; internal workload identity still authenticates Gateway-to-origin traffic |

There is no implicit flat cluster network. Default policy is deny. The absence
of a policy, identity provider, current bundle, or enforcement capability is
not interpreted as unrestricted communication.

## 8. Peer authorization

The resource-owning bounded context defines who may call which service role.
Its immutable ACL is compiled into a closed `PeerAuthorizationSnapshot`:

- destination PrivateService and revision;
- allowed source WorkloadIdentity IDs/roles and trust domains;
- protocol, method or operation class where supported;
- audience, port, deadline and connection bounds;
- request identity/trace requirements;
- policy generation, digest, expiry and revocation epoch; and
- enforcement points and acknowledgement requirements.

Identity verifies the caller; the destination enforces the owner policy.
Neither mTLS nor presence in the same Project is authorization by itself.

## 9. Credential rotation and revocation

- Workload credentials are short lived and automatically rotated before
  expiry.
- A bounded overlap permits established connections to drain only when policy
  allows it.
- New credentials bind the same or newer policy and exact live Unit
  generation.
- Node revocation, Claim fencing, Unit stop/remove, policy revocation, tenant
  suspension, or trust-bundle withdrawal prevents renewal immediately and
  removes the member from discovery.
- Emergency revocation advances a durable epoch distributed through the same
  complete-snapshot mechanism. A stale enforcement point becomes unready.
- Long-lived shared cluster tokens are forbidden.

Secrets owns application/provider secret versions. Workload identity keys and
certificates are ephemeral PKI products and are not exposed as tenant Secrets.

## 10. Distributed consistency

Identity admission uses durable idempotency, request-digest equality and
optimistic concurrency. A private publication is valid only when all of these
match:

```text
WorkloadRevision + Deployment generation + Fleet Claim/fence
+ Runtime Unit/generation + node attestation
+ identity policy revision + credential generation/expiry
+ peer policy revision + endpoint health observation
```

The compiler creates a complete candidate snapshot and uses compare-and-swap
to select it after every enforcement point acknowledges the exact digest.
Rejected or timed-out candidates preserve the prior valid snapshot where that
policy remains safe. Unknown revocation or expiry fails closed.

Redis may cache verification material and discovery snapshots with shorter
expiry than their signed inputs. Loss causes refetch/revalidation, never
implicit allow. Distributed locks are not correctness proof; durable owner
versions and fence tokens are.

## 11. Failure behavior

| Failure | Required behavior |
| --- | --- |
| Identity issuer unavailable | Existing unexpired credentials may continue only within policy; new Units/renewals stay unready |
| Bundle/discovery propagation lag | Enforcer keeps the last safe acknowledged snapshot and exposes lag; expiry fails closed |
| Node or identity agent loss | Fleet fences the node; members leave discovery; replacement receives new generations |
| Runtime/Box process loss | Reconciliation may reattach only the same exact provider resource; replacement identity is newly issued |
| Clock skew | Issuance and validation apply bounded skew policy and health alarms; excessive skew makes the node ineligible |
| Network partition | No new cross-partition writer or identity authority is elected without the durable Fleet/Workloads fence |
| Revocation during a stream | New calls stop; existing stream handling follows the explicit drain/terminate policy and is audited |
| Federation endpoint loss | Cached bundles obey expiry; no foreign identity is accepted after safe validity ends |

## 12. Observability and interfaces

Required evidence links trust domain, Node, WorkloadRevision, Deployment,
Claim, Runtime Unit/generation, workload identity, policy revision, peer,
issuance/rotation/revocation, connection and trace. Private keys, bearer
credentials and application payloads are excluded.

System administrators manage trust domains, issuer providers, node-attestation
profiles, federation and emergency revocation. Tenant owners manage only
tenant-scoped workload/peer policies allowed by platform ceilings. REST,
OpenAPI, SDK, CLI and Management MCP call the same Application services. No
Cloud Dashboard or private identity-provider UI is required.

## 13. Delivery gates

| Gate | Outcome | Current state |
| --- | --- | --- |
| `H0.4-WI1` | TrustDomain and WorkloadIdentityPolicy ACL, DDD owner and provider ports | `WI1-C1`, the `WI1-C2` persistence core, and the maintained management surface are implemented locally; main verification is pending. Canonical ACLs and deterministic revisions feed migration `179`'s immutable histories and sole heads, and policies bind the exact TrustDomain revision. PostgreSQL reuses the Installation lock, sole privileged decision issuer, shared idempotency/Audit/Outbox, and exact Workload/NodePool owner FKs; the in-memory privileged path fails closed. REST/OpenAPI `1.79.0`, TypeScript client, CLI, and nine Installation-bound Management MCP tools reuse the same CQRS. The retained two-replica/revocation gate is registered. Real provider evidence remains open, so WI1 is not yet available. |
| `H0.4-WI2` | Node and exact Runtime Unit attestation binding through Fleet/Box | Planned |
| `H0.4-WI3` | Short-lived issuance, local workload endpoint, rotation and Secret separation | Planned |
| `H0.4-WI4` | PrivateService and PeerAuthorization complete snapshots | Planned |
| `H0.4-WI5` | Box network enforcement, egress policy and Gateway-to-origin identity | Planned |
| `H0.4-WI6` | Revocation, expiry, clock, process/node loss, partition and upgrade evidence | Planned |
| `H0.4-WI7` | Optional trust-domain federation, region isolation and exact-provider conformance | Planned |

`C0.5-MT1-C3` establishes the canonical persisted Installation identity and
explicit scope-aware Audit/Outbox evidence, and verified `MT2` supplies the sole
Identity platform-permission decision. `WI1-C2` now builds only on those
authorities: trust-domain state is installation state, never a synthetic
Organization; mutation and reads carry the exact Principal, API token and
request; and no `actor_is_platform_admin` boolean is admitted. Migration `179`
stores immutable predecessor-linked history and one CAS head for each aggregate.
The protected transaction locks the Installation, issues the exact
`WorkloadTrustRead` or `WorkloadTrustManage` decision, validates canonical owner
lineage and current TrustDomain revision, then commits the business fact,
idempotency result, Audit and Outbox together. Redis, Lane, caches and a second
authorization/audit/lock table are not correctness authorities.

`WI1-C1` deliberately exposes only provider capability inspection. Credential
issuance is unavailable until `WI2` can prove the exact Fleet Claim, Node,
Runtime Unit and generation. This prevents an Infrastructure adapter from
minting identity from mutable hostnames, image names, process IDs, or shared
cluster credentials. The decision is recorded in
[ADR 0079](decisions/app-platform/0079-identity-owned-workload-trust-contract.md).

## 14. Non-goals

- A Kubernetes Service, ServiceAccount, NetworkPolicy, sidecar-injection, or
  service-mesh control-plane compatibility layer.
- A second workload scheduler, endpoint registry, certificate worker, Secret
  store, or authorization database.
- Treating mTLS as sufficient authorization.
- Trusting source IP, hostname, process name, image tag, or a shared token as
  workload identity.
- Giving tenant workloads direct access to issuer administration.

## 15. Reference standards

- [SPIFFE Workload API](https://spiffe.io/docs/latest/spiffe-specs/spiffe_workload_api/)
- [SPIFFE federation](https://spiffe.io/docs/latest/spiffe-specs/spiffe_federation/)
