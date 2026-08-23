# A3S Cloud TypeScript Client

`@a3s/cloud-client` is the single maintained typed REST client shared by the
A3S Cloud CLI and external integrators. It contains transport and public response types only; business
rules remain in Cloud application commands and queries.

```typescript
import { CloudApi } from '@a3s/cloud-client';

const api = new CloudApi(process.env.A3S_CLOUD_TOKEN!,
  'https://cloud.example.test/api/v1');
const organizations = await api.listOrganizations();

const publicApi = new CloudApi(undefined,
  'https://cloud.example.test/api/v1');
const diagnostics = await publicApi.getDiagnostics();
```

Every request has a finite timeout and expects the standard Cloud success or
error envelope. Invalid JSON, invalid envelopes, network failure, timeout, and
caller cancellation become stable `CloudApiError` values. Tokens are sent only
in authorization headers and never appear in generated stream URLs or error
messages.

`getPlatform`, `getLiveness`, `getReadiness`, and `getDiagnostics` use the
public Cloud endpoints and support a client without a token. No Authorization
header is emitted when the token is absent. Health endpoints deliberately use
HTTP `503` with a standard success envelope when the health report is down; the
client returns that report as diagnostics. A `503` error envelope remains a
`CloudApiError`. Authenticated methods still require server-authorized
credentials.

The package currently exposes the `C0.1` tenant,
operational-resource, evidence, and bounded paged-log queries. Its Workload,
deployment, and route types match the current replica/member and Gateway scope
REST projections. DomainClaim and logical Gateway-scope queries and mutations,
plus route publication, use the same tenant-guarded REST resources. Source
revision, GitHub connection, and repository-subscription methods use the
existing Source controllers. Secret list/get/create/add-version/revoke-version
methods use the existing Secret controllers and expose metadata and version
state only. API-token list/get/create/revoke methods use the existing Identity
controllers and return credential-free metadata. `issueEnrollmentToken` uses
the existing Fleet controller and returns one credential-free enrollment-token
projection. `listNodePools`, `getNodePool`, `createNodePool`,
`addNodePoolMembers`, `requestNodePoolMemberRemoval`,
`scheduleNodePoolMaintenance`, and `cancelNodePoolMaintenance` expose
Fleet-owned membership, generation-fenced removal, and bounded maintenance
policy through REST contract `1.21.0`. Workload ACL creation and
update methods carry an optional immutable `placement { node_pool_id = ... }`
selection through that same contract. The package is internal and
versioned with Cloud until public package compatibility and deprecation policy
are completed.

`getProjectAttribution`, `getProjectAttributionRevision`, and
`updateProjectAttribution` expose REST contract `1.30.0`. The update carries one
positive Project version in `x-a3s-expected-version` and a caller-owned
idempotency key. The client validates only transport bounds for the
business-owner reference, optional cost-attribution code, and at most 32
labels; Cloud Projects remains authoritative for canonicalization, immutable
lineage, Resource Grants, persistence, Outbox, and audit. These fields are
non-monetary showback metadata, not a client billing or usage ledger.

`listAssets`, `getAsset`, `createAsset`, and `archiveAsset` expose the
organization Asset lifecycle. Release list/get/create/yank methods preserve
draft and yanked management visibility, while `selectAssetRelease` calls the
server-owned deterministic new-binding selector. Omitting its version selects
the highest stable published semantic version; an explicit version may select
a published prerelease. The client never derives manifest digests or chooses a
release locally. Cloud admits the exact hosted Git commit, excludes draft and
yanked releases from new bindings, and keeps exact yanked identities available
to existing pinned deployments.

`getMcpServiceProfile` and `bindMcpServiceProfileFromAcl` expose the one
immutable Service-profile binding owned by a published MCP OCI
`AssetRelease`. Binding sends one nonempty A3S ACL document of at most 64 KiB
as `application/vnd.a3s.acl` with a caller-owned idempotency key. Cloud parses
and canonicalizes the document, so semantically equivalent ACL produces the
same profile digest and an identical binding is a replay/no-op. The client
does not create another profile store, route policy, deployment path, or MCP
scheduler.

`listMcpRoutePolicies`, `getMcpRoutePolicy`,
`createMcpRoutePolicyFromAcl`, and `reviseMcpRoutePolicyFromAcl` expose the
separately mutable Edge desired-state policy. Writes send one nonempty UTF-8
A3S ACL document of at most 512 KiB through the shared ACL transport with a
caller-owned idempotency key. Cloud alone canonicalizes the policy, validates
its exact Service profile, release, domain, Workload, Gateway scope, grants,
revision, limits, and expiry, and commits audit/Outbox evidence. The client
does not publish a Gateway snapshot, derive targets, or create a second MCP
policy lifecycle.

`listGatewayRoutePolicySecurityTimeline` exposes the owner/admin-only,
read-only investigation projection added by REST contract `1.55.0`. It pages
the exact typed `edge.mcp-route-policy.created` and `.revised` owner facts in
descending order and reports shared-audit correlation as `verified` or
`missing`. The projection never returns canonical policy ACL, raw Outbox
payload, or audit details, and the client does not infer Gateway denials or
create a second incident/evidence store.

`listOntologies`, `getOntology`, `createOntologyFromAcl`,
`listOntologyRevisions`, `getOntologyRevision`, `diffOntologyRevisions`, and
`reviseOntologyFromAcl` expose the backend `W0.2` lifecycle through REST
contract `1.15.0`. Writes transport at most 1 MiB of closed A3S ACL unchanged.
Revision requires a positive expected aggregate version and may name one
portable migration rule ID; Cloud admits a breaking diff only when that exact
target ACL rule has kind `migration`. The client does not parse Ontology ACL,
infer migration policy, maintain revision state, or create a graph index.

`listWorkflowDefinitions`, `getWorkflowDefinition`,
`createWorkflowDefinitionFromAcl`, `listWorkflowRevisions`,
`getWorkflowRevision`, and `reviseWorkflowDefinitionFromAcl` expose the
`W0.3` planning lifecycle. Publication uses a bounded JSON transport envelope
only to carry the canonical closed Workflow ACL and every exact typed
configuration, data-schema, and policy ACL payload atomically; JSON is not a
second product-configuration format. `listWorkflowGoals`, `getWorkflowGoal`,
`createWorkflowGoalFromAcl`, and `getWorkflowPlanRevision` bind exact Workflow
and Ontology revisions and read the deterministic immutable plan. The client
validates transport bounds and optimistic version shape but does not parse
ACL, compile plans, or retain revision state.

`startWorkflowRun`, `cancelWorkflowRun`, `listWorkflowRuns`,
`getWorkflowRun`, `waitWorkflowRun`, `getWorkflowRunOutput`, and
`getWorkflowRunHistory` expose the minimal `W0.3` run lifecycle added by REST
contract `1.15.0`. Start binds one exact Goal and Plan revision, accepts a
bounded optional deadline, and requires caller-owned idempotency. Cancel is
also replay-safe; list, wait, and history enforce the server's finite bounds
before transport. Cloud remains authoritative for the correlated Operation,
A3S Flow run, WorkflowStepProjection state, immutable replay checks,
cancellation, timeout, output digest, and redacted history.

Composite Plan v2 runs pin WorkflowRun runtime/Flow v3. The server dispatches
each Iteration or Loop ordinal through one authority-bound Flow hook and a
deterministic ordinary child WorkflowRun, links exact child authority, reduces
the digest-bound result, and cancels/awaits children before parent termination.
The client adds no composite scheduler or child lifecycle.

Workflow definition publication may also carry the complete revision semantic
contract set: descriptor bindings, the exact recoverable descriptor registry
snapshot, and the typed-variable contract. REST contract `1.29.0` introduced
those three mandatory children; `1.34.0` added optional
`variableDefaultsAcl`, and `1.35.0` adds optional
`compositeRegionsAcl`. The latter exactly covers admitted Iteration and Loop
descriptors with bounded region policy and one exact child WorkflowRevision
binding. Cloud persists the set atomically, returns canonical ACL and digests,
and compiles Plan v2 with exact per-step descriptor, semantic-set, variable,
and optional `compositeRegionsDigest` pins. The client validates only
transport shape and UTF-8 byte bounds, including the 512 KiB composite ACL
limit; Cloud remains the ACL and compiler authority. A graph that opts into the
exact finite-Execution descriptor error port is returned by REST contract
`1.40.0` as Plan v3 with each step's immutable `failure` contract. REST contract
`1.41.0` adds the mutually exclusive Plan v4 `defaultOutput` port contract and
typed `defaultOutputEvidence` on completed Run v7 projections. The exact value
remains pinned by the existing step policy digest. Run v4 typed errors and Run
v7 defaults still travel
through the ordinary Workflow output/history shape, so the client adds no
failure scheduler or provider lifecycle. The exported
`WorkflowStepFailureOutput`, `WorkflowStepDefaultOutputContract`,
`WorkflowStepDefaultOutputEvidence`, and Execution-detail types describe those
stable values without attempting to interpret arbitrary Workflow output.

`getWorkflowNodeCatalog` exposes the project-authorized read-only discovery
projection added by REST contract `1.31.0`. It returns the exact frozen baseline,
manifest/profile digests, parity flag, and 23 node entries with owner, gate,
dependencies, availability, coarse kind, execution class, semantic profiles,
evidence, and unavailable reason. The client neither merges catalog sources nor
infers descriptor admission or public availability; Cloud remains authoritative
for composition and project access.

`getWorkflowRunVariables` exposes the Flow-derived typed-value inspection added
by REST contract `1.33.0`. Cloud resolves the WorkflowRun's existing project
authorization, exact Plan v2 variable contract, immutable run input, and
correlated A3S Flow history through the same materializer used by execution.
The client returns the versioned, declaration-ordered inspection with observed
Flow sequence, materialized/unavailable state, metadata, inline or opaque values,
and value digests. Secret-reference values are always redacted. A run not yet
created in Flow may expose immutable inputs at sequence zero; Plan v1 returns a
conflict. The client does not reconstruct, cache, or mutate variable state.

`listHumanTasks`, `getHumanTask`, `claimHumanTask`, `releaseHumanTask`, and
`submitHumanTask` expose the protected HumanTask surface in REST contract
`1.24.0`. Lists accept only the closed status set and a limit from 1 through 200
and return summaries without interaction payloads. Detail may return the
request-bound native A3S Form interaction only when the bearer principal is the
current claimant. Claim/release use explicit version and idempotency headers;
submission transports the exact native Form envelope, whose `taskVersion` and
`idempotencyKey` remain the single source of those values. Cloud remains
authoritative for project access, assignment policy, claimant identity, Form
evaluation, Identity authorization evidence, decision persistence, and replay.
The client does not implement a second Form validator or authorization model.

`listFormDrafts`, `getFormDraft`, `createFormDraft`, `reviseFormDraft`,
`listFormReleases`, `getFormRelease`, and `publishFormRelease` expose the native
Form draft and immutable release lifecycle added by REST contract `1.15.0`.
Draft writes carry only a bounded `{name, description?, document}` JSON
transport, and revise/publish require a positive expected aggregate version.
The client validates transport shape, text bounds, canonical document size,
UUIDs, and idempotency keys; Cloud calls the pinned A3S Form owner compiler and
persists through A3S ORM. The client does not parse Form semantics, compile a
Form plan, validate submitted values, retain revision state, or create another
Form configuration format.

`bindSkillRelease` and `unbindSkillRelease` use the tenant-scoped Workload
lifecycle and require caller-owned idempotency keys. A bind names one exact
published Skill AssetRelease; an unbind names the Skill Asset already present
on the active Agent revision. The response and Workload projections expose the
immutable bundle digest, size, media type, and derived read-only mount, while
Cloud alone creates the next revision and never schedules a Skill as a separate
Runtime unit.

`searchResources` validates a 1-to-128-character safe query and a result limit
from 1 through 50 before transport, then calls the organization-scoped public
search endpoint. It returns contextual, credential-free projections, including
organization-scoped `plugin_registry` results whose links target the Cloud-owned
Registry detail surface. It never returns A3S Use catalog rows or TUF metadata.
Authorization, ranking, and resource registration remain Cloud responsibilities;
callers must not emulate search by loading broad resource lists.

`listPluginRegistries` and `getPluginRegistry` expose the Cloud-owned tenant
Registry projection. `searchPluginCatalog`, `searchCachedPluginCatalog`,
`inspectPluginCatalog`, and `inspectCachedPluginCatalog` send canonical A3S Use
JSON through the four explicit read-query endpoints. Those POST requests carry
no `Idempotency-Key`: they are queries, not mutations. The client deliberately
types Use-owned request and result bodies as opaque canonical JSON objects
instead of restating the upstream catalog fields, bounds, cursors, or release
records.

Replayable mutating methods require a caller-owned idempotency key. The client
accepts a portable visible-ASCII subset up to the server's 255-byte limit,
rejects an invalid key before transport, and sends the value only in
`Idempotency-Key`. `beginGithubConnection` is intentionally non-replayable:
it invokes the existing no-store endpoint that returns one short-lived browser
installation URL.

`createOrganization`, `createProject`, and `createEnvironment` use the existing
resource commands. `markNodeReady`, `drainNode`, and `revokeNode` additionally
require a positive safe-integer aggregate version and preserve the server's
optimistic-concurrency contract.

`listExecutions`, `getExecution`, `createExecution`, and `cancelExecution`
expose the finite Runtime Task lifecycle through the tenant-scoped Execution
controllers. Create and cancel require caller-owned idempotency keys. The
client transports the typed digest-pinned template and authoritative projection
without inferring placement or provider state; input and process environment
are persisted desired state and must not contain secret material. Workflow-
owned children additionally expose their immutable Run/Plan/step/attempt and
ExecutionTemplate binding; the client does not create or interpret it.

`listExecutionTemplates`, `getExecutionTemplate`, and
`createExecutionTemplate` expose the immutable project-scoped finite-task
definition lifecycle added by REST contract `1.24.0`. Create transports one
bounded A3S ACL document and caller-owned idempotency key. Cloud alone parses,
canonicalizes, digest-binds, persists, and materializes the template; the
client retains no parser, revision store, Workflow dispatcher, scheduler, or
Runtime provider.

`listApplications`, `getApplication`, `createApplication`,
`publishApplicationRelease`, `listApplicationReleases`, and
`getApplicationRelease` expose the project-scoped `APP0.1` lifecycle added by
REST contract `1.42.0`. Writes carry one bounded canonical Application release
A3S ACL; publication also requires the current positive aggregate version.
Lists default to 50 and accept at most 200 records. Cloud alone authorizes the
project, validates the exact Workflow definition/revision plus contract,
payload-set, semantic-contract-set, input-schema, and output-schema evidence,
and commits immutable lineage, idempotency, audit, and Outbox facts. The client
does not parse release ACL, resolve a mutable Workflow head, or create graph,
Flow, provider, session, Secret, or Gateway state.

`openApplicationSession`, `getApplicationSession`,
`requestApplicationInvocation`, `getApplicationInvocation`, and
`listApplicationMessages` expose the project-member management delivery slice
added by REST contract `1.43.0`. Session and invocation creation require
caller-owned idempotency keys; initial variables and invocation input are
bounded JSON objects, and message reads use an exclusive sequence cursor with
100/500 defaults and limits. Cloud derives stable Principal-bound identities,
admits one exact release and Ontology revision, and creates or adopts the
ordinary Workflow Goal, Plan, and Run through the existing Workflow authority.
REST contract `1.44.0` additionally exposes `closeApplicationSession`,
`cancelApplicationInvocation`, and `replayApplicationSession` over the same
authority. Close and cancel require a positive expected aggregate version and
idempotency key. Full replay returns the session head, bounded contiguous
messages, current variable revision, next sequence, and `hasMore` evidence.
The client does not create an end-user credential, session store, Workflow
runtime, answer stream, cancellation authority, or Gateway route. Blocking and
streaming response-mode values are retained as invocation intent only; this
slice returns asynchronous admission evidence rather than a synchronous or SSE
answer.

`listConnectorProfiles`, `getConnectorProfile`, `createConnectorProfile`,
`reviseConnectorProfile`, `listConnectorRevisions`, and
`getConnectorRevision` expose the environment-scoped immutable Connector
profile lifecycle added by REST contract `1.36.0`. Writes transport one
bounded canonical A3S ACL in a strict JSON envelope; revise also carries one
positive expected aggregate version. Lists default to 50 and accept at most
200 records. Cloud remains authoritative for ACL parsing, exact Secret-version
admission, Resource Grants, optimistic concurrency, idempotency, immutable
digest lineage, Outbox, audit, and persistence. The client never resolves a
Secret or projects endpoint, credential, provider body, attempt/evidence, or
retry state.

`listDurableCellApplications`, `getDurableCellApplication`,
`createDurableCellApplication`, `reviseDurableCellApplication`,
`startDurableCellApplication`, `stopDurableCellApplication`,
`listDurableCellApplicationRevisions`, `getDurableCellApplicationRevision`,
`deployDurableCellApplication`, and `publishDurableCellApplicationRoute`
expose the `CELL0.4-C5` interface added by REST contract `1.38.0`.
Application definitions, Service profiles, provider Workloads, and
plaintext-free storage bindings remain bounded canonical A3S ACL strings; the
client validates transport bounds and delegates parsing to Cloud. Deployment
contract `1.39.0` additionally accepts optional `storageProviderProfileAcl`;
supplying it activates the exact C3b publication gate while omission preserves
the earlier v1 request behavior. The client validates the optional ACL with
the existing S0 profile bound and adds no parser.
Deployment
reuses the existing Workload ACL bound and returns only existing-owner
references and digests. Route publication selects no port locally and sends
the Service profile to the existing Edge-backed server command. The client
adds no ACL parser, OCI/DNS validator, S0 lifecycle, scheduler, or state store.

`listNotificationAlertPolicies`, `getNotificationAlertPolicy`,
`createNotificationAlertPolicy`, and `revokeNotificationAlertPolicy` expose the
personal alert-policy lifecycle added by REST contract `1.47.0`. Create sends
one bounded canonical `cloud.notification.alert-policy.v1` A3S ACL; list/get
reuse bounded personal pagination, and revoke carries one positive expected
aggregate version. Cloud remains authoritative for the exact recipient,
target existence, current Resource Grants, closed typed source registry,
projection, idempotency, Outbox, audit, and persistence. The client has no ACL
parser, event expression evaluator, incident state, projector, or delivery
mechanism. Contract `1.49.0` extends the closed source union with
`edge.gateway-certificate-renewal-status.v1` without adding another client
operation or configuration format. Contract `1.50.0` adds
`workload.deployment-health.v1` through those same four operations and keeps
Workloads as the rollout-health authority. Contract `1.51.0` adds
`edge.gateway-certificate-expiry-status.v1` through the same operations while
Edge remains the certificate-expiry authority. Contract `1.54.0` adds canonical
`cloud.notification.alert-policy.v2` only for
`fleet.node-availability-status.v1` and an exact Node. Every response includes
one discriminated Environment-or-Node `target`; deprecated `projectId` and
`environmentId` remain nullable compatibility projections and are `null` for
Node policies. The client adds no Node lookup, heartbeat interpretation, or
availability state.

`listRecipientContacts`, `getRecipientContact`,
`requestRecipientContactVerification`,
`completeRecipientContactVerification`, and `revokeRecipientContact` expose
the exact-owner Identity self-service lifecycle added by REST contract
`1.52.0`. The client validates bounded canonical-address and proof transport
inputs, positive optimistic versions, and caller-owned idempotency keys. It
returns only opaque contact/Principal IDs, address digest, `***@domain` hint,
closed status, version, timestamps, and replay state; challenge identity,
mailbox, and proof are never response fields or remapped error details. Cloud
remains authoritative for exact active human-Principal ownership, Membership,
verification, persistence, Outbox, and audit.

`listOutboundNotificationSubscriptions`, `getOutboundNotificationSubscription`,
`createOutboundNotificationSubscription`, and
`revokeOutboundNotificationSubscription` expose the recipient-bound lifecycle
added by REST contract `1.37.0`. Create sends one bounded canonical A3S ACL as
`application/vnd.a3s.acl`; revoke carries one positive expected aggregate
version. Contract `1.46.0` widens the response definition schema to v1/v2/v3,
retains required `maximumProviderAttempts`, and adds nullable `suppressBefore`;
v1 remains exactly eight, v2 pins one value from 1 through 8, and v3 also pins
the bounded immutable event-time cutoff. Contract `1.53.0` adds SMTP-only v4
and returns one required discriminated `target`: an exact Connector revision
for signed-webhook/Slack delivery or one opaque `recipient_contact` ID for
SMTP. The four deprecated nullable Connector fields remain populated for
Connector subscriptions and are `null` for SMTP so `1.52` response consumers do
not break. Lists use the shared opaque cursor and 50/200 bounds. Notifications
remains authoritative for recipient identity, Resource Grants, exact target
admission, idempotency, Outbox, audit, and persistence. The client has no ACL
parser and never resolves mailbox, endpoints, Secrets, credentials, provider
bodies, delivery evidence, receipts, or retry state.

`listAgentConversations`, `getAgentConversation`, and
`createAgentConversation` expose the `A1.1` conversation lifecycle.
`listAgentExecutions`, `getAgentExecution`, and `startAgentExecution` bind one
logical execution to an exact published Agent AssetRelease and its immutable
BuildRun/OCI identity. `getAgentExecutionEvents` reads the authoritative
contiguous semantic sequence with a bounded opaque cursor, while
`agentExecutionEventStreamUrl` builds the credential-free shared SSE URL used
by streaming consumers. Conversation creation and execution start require caller-owned
idempotency keys. This contract reserves an Operation identity but does not
claim Harness, Fleet, Workload, or Runtime dispatch; those are `A1.2` work.

`issueEnrollmentToken` validates the fixed `a3sn_` plus 64-lowercase-hex
credential format and RFC 3339 expiry before transport, then calls the existing
tenant-scoped Fleet command with a caller-owned idempotency key. Cloud remains
authoritative for the maximum 24-hour lifetime, one-time consumption, tenant
guard, digest-only A3S ORM persistence, mTLS enrollment, and replay. CLI callers
supply the credential only through the bounded `--enrollment-token-stdin` path;
the client response type contains metadata and `replayed`, never the credential.

`listApiTokens` and `getApiToken` expose tenant-scoped metadata only.
`createApiToken` validates the fixed `a3s_` plus 64-lowercase-hex credential
format, one or more unique bounded scopes, and an optional RFC 3339 expiry
before transport. `createApiToken` and `revokeApiToken` require caller-owned
idempotency keys and return metadata plus the durable `replayed` flag. Cloud
remains authoritative for scope delegation, tenant guards, digest-only storage,
and A3S ORM persistence. CLI callers supply a new credential only through the
bounded `--token-stdin` path; it is never a command argument, configuration
value, result field, or echoed error.

`listMembershipInvitations`, `getMembershipInvitation`,
`createMembershipInvitation`, and `revokeMembershipInvitation` expose the
organization-administrator invitation history and mutations added by REST
contract `1.25.0`. `listMyMembershipInvitations` and
`acceptMembershipInvitation` are exact authenticated-Principal self-service
methods across organizations. Create validates one Principal UUID, ordinary
Membership role, RFC 3339 expiry, and caller-owned idempotency key; accept and
revoke require a positive expected version and idempotency key. Cloud alone
enforces the maximum 30-day lifetime, active exact Principal, administrator
authority, duplicate Membership exclusion, and atomic invitation-to-Membership
transition. The client adds no email/OIDC discovery, role inference, invitation
store, notification queue, or authorization model.

`listAuditRecords` implements the owner/admin-only read projection added by
REST contract `1.26.0` and extended by contract `1.56.0`. It validates exact
Principal, aggregate, request, Project, Environment, and immutable attribution-
profile UUIDs; canonical action names; the closed `legacy_unknown`,
`not_applicable`, `profile_missing`, or `profile_bound` status; inclusive RFC
3339 time bounds; the opaque cursor; and the 1-through-200 page limit before
transport. Each result adds only the request-time Project, optional Environment,
exact profile reference, and closed status to the existing seven typed fields.
It cannot expose shared `details`, profile labels, business-owner or cost-code
text, or create a client-side audit store.

`exportAuditRecords` implements REST contract `1.57.0` over that same query.
It requires explicit inclusive `from` and `to` timestamps, rejects windows
wider than 31 days before transport, and retains the same exact filters,
opaque cursor, and one-through-200 limit. The result is the complete DSSE
envelope plus its Ed25519 public verification key, SHA-256 key ID, and optional
external key version. The client never receives private signing material and
does not treat response-supplied public material as a trust anchor: callers
must compare the key ID or public key with an independently trusted deployment
fingerprint. It does not create a second audit store, retention policy, object
copy, manifest, or SIEM-delivery authority.

`createMembership` is the single Principal-plus-Membership mutation retained in
REST contract `1.29.0`. Callers choose the closed `human` or `service` Principal
kind explicitly; the client does not expose a second service-only creation
method or infer human identity from a credential.

`oidcLoginUrl` builds the public organization-scoped login entry point, while
`beginOidcLink` uses the authenticated `cloud:read` transport and
`credentials: include` to receive callback-only HttpOnly cookies plus the
provider `authorizationUrl`. The browser navigates to that URL. The client
does not read nonce/PKCE cookies, persist an OIDC session, place a Cloud
credential in a URL, or infer Membership authority from provider claims.

`createDomainClaim`, `verifyDomainClaim`, and `revokeDomainClaim` return the
complete DomainClaim projection with its durable `replayed` flag.
`createGatewayScope` accepts an ordered member list plus `minReady` and
`maxUnavailable`, and returns the complete logical scope with the same replay
contract. `publishRoute` returns the Route, managed certificate, request replay
state, and Gateway-command replay state. The client transports these commands;
Cloud application services and A3S ORM-backed repositories remain
authoritative for validation, tenancy, and persistence.

`listSourceRevisions` and `resolveSourceRevision` expose the closed GitHub
repository, branch/tag/commit, and `a3s.cloud.build-recipe.v1` Dockerfile
contracts. `getGithubConnection` and `beginGithubConnection` expose
authoritative provider status and the short-lived installation flow.
`listGithubRepositorySubscriptions`,
`createGithubRepositorySubscription`, and
`deactivateGithubRepositorySubscription` reuse the existing tenant-guarded
subscription commands. Resolution and subscription mutation results include
the API's durable `replayed` state; the client never resolves Git references
or contacts GitHub itself.

`listSecrets` and `getSecret` expose the tenant-scoped metadata and version
projections. `createSecret`, `addSecretVersion`, and `revokeSecretVersion`
reuse the existing application commands and require caller-owned idempotency
keys. Value-bearing calls reject empty values or values larger than 1 MiB in
UTF-8 before transport. Mutation responses contain only metadata, changed
version state, and the durable `replayed` flag; Cloud remains authoritative for
tenancy, encryption, rotation effects, and A3S ORM-backed persistence. Client
callers must obtain plaintext from a secure input source rather than process
arguments, environment variables, or configuration.

`createWorkloadFromAcl`, `updateWorkloadFromAcl`, and
`deploySourceRevisionFromAcl` transport one nonempty A3S ACL document of at
most 64 KiB without rewriting it. They use `application/vnd.a3s.acl`; Cloud is
the sole parser and schema authority. JSON and ACL requests share the same
durable idempotency records.
