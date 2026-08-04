# A3S Cloud TypeScript Client

`@a3s/cloud-client` is the single typed REST client shared by the A3S Cloud web
console and CLI. It contains transport and public response types only; business
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

The package currently exposes the Web management calls plus `C0.1` tenant,
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
projection. It is internal and versioned with Cloud until public package
compatibility and deprecation policy are completed.

`listAssets`, `getAsset`, `createAsset`, and `archiveAsset` expose the
organization Asset lifecycle. Release list/get/create/yank methods preserve
draft and yanked management visibility, while `selectAssetRelease` calls the
server-owned deterministic new-binding selector. Omitting its version selects
the highest stable published semantic version; an explicit version may select
a published prerelease. The client never derives manifest digests or chooses a
release locally. Cloud admits the exact hosted Git commit, excludes draft and
yanked releases from new bindings, and keeps exact yanked identities available
to existing pinned deployments.

`bindSkillRelease` and `unbindSkillRelease` use the tenant-scoped Workload
lifecycle and require caller-owned idempotency keys. A bind names one exact
published Skill AssetRelease; an unbind names the Skill Asset already present
on the active Agent revision. The response and Workload projections expose the
immutable bundle digest, size, media type, and derived read-only mount, while
Cloud alone creates the next revision and never schedules a Skill as a separate
Runtime unit.

`searchResources` validates a 1-to-128-character safe query and a result limit
from 1 through 50 before transport, then calls the organization-scoped public
search endpoint. It returns contextual, credential-free projections only.
Authorization, ranking, and resource registration remain Cloud
responsibilities; callers must not emulate search by loading broad resource
lists.

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
are persisted desired state and must not contain secret material.

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
the sole parser and schema authority. Existing Web methods continue to use the
semantically equivalent JSON request contract and share durable idempotency
records with ACL requests.
