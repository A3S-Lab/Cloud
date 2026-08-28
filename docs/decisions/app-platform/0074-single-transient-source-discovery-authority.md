# 0074: Discover GitHub source state through one transient Sources authority

Status: Accepted

## Context

Sources already owns the authoritative GitHub App connection, installation
identity, repository admission policy, repository-scoped credential issuance,
webhook Inbox, subscription, and accepted `SourceRevision`. Before accepting a
repository or reference, however, a user had to type its URL and branch without
being able to inspect the repositories, branches, or tags currently visible to
the connected installation.

GitHub repository and reference lists are current provider observations, not
Cloud desired state. Persisting them would create a competing inventory,
freshness policy, reconciliation lifecycle, cleanup path, and retry owner.
Letting REST, CLI, or Management MCP call GitHub directly would duplicate
connection authority, repository policy, pagination, provider validation, and
credential handling outside Sources.

The existing repository-scoped installation-token issuer is the only provider
credential mechanism. Discovery also needs an installation-wide token to list
accessible repositories, but that wider transient capability must not leak into
Application, DTOs, logs, persistence, or another token service.

## Decision

Sources adds one Application-owned `IGithubSourceDiscoveryProvider` and one
`GithubSourceDiscoveryQueryService` with two read queries:

- list one bounded page of repositories accessible to the authoritative GitHub
  App installation; and
- list one bounded page of branches or tags for an exact canonical,
  policy-admitted GitHub repository.

Before provider I/O, the query service validates public syntax and a `1..=100`
limit, restores and validates the sole `GithubConnection`, verifies exact
Organization ownership and authoritative state, and decodes an opaque cursor.
An explicitly requested repository must also pass the existing
`SourceRepositoryPolicy` before provider I/O; repository-page results pass that
same policy before projection. The cursor is bound by SHA-256 scope to the
Organization, connection, installation, query family, repository and reference
kind when present, and requested page size. It carries only the next provider
page and cannot be replayed across those scopes.

`RevalidatingGithubSourceDiscovery` implements the Application port by first
calling the existing `IGithubConnectionAuthorityService`. The existing
`GithubInstallationTokenIssuer` then issues either:

- a short-lived installation token with read-only contents permission and no
  repository selector for installation repository discovery; or
- the existing short-lived selected-repository token for branch or tag
  discovery.

The adapter keeps the token in a zeroizing value, performs only bounded GETs,
parses bounded pagination metadata, rejects broadened token permissions and
identity/SHA projection violations, silently omits names outside the existing
Sources safe-reference rules, and returns closed credential-free values.
Application revalidates page bounds, canonical repository identity, reference
kind, reference identity, and per-page uniqueness. Policy-denied repositories
are silently omitted from repository pages; a denied explicitly requested
repository fails before provider I/O.

The maintained public mapping is exact:

| Capability | REST | TypeScript client | CLI | Management MCP | Scope |
| --- | --- | --- | --- | --- | --- |
| List installation repositories | `GET /organizations/{organizationId}/source-connections/github/repositories` | `listGithubInstallationRepositories` | `source-repositories list` | `a3s_cloud_github_installation_repositories_list` | `source:write` |
| List repository references | `GET /organizations/{organizationId}/source-connections/github/repository-references` | `listGithubRepositoryReferences` | `source-references list` | `a3s_cloud_github_repository_references_list` | `source:write` |

REST/OpenAPI `1.76.0` declares the exact bounds and typed responses. REST and
Management MCP dispatch the same two CQRS queries and reuse the same response
DTOs. The maintained TypeScript client owns the matching transport validation,
and the CLI delegates URL, reference-kind, cursor, and limit validation to that
client boundary.

Discovery does not accept a `SourceRevision`, create a subscription, or infer a
build recipe. Existing commands remain the only product-state acceptance paths.
This decision adds no aggregate, repository, table, migration, Inbox, Outbox,
Relay, event, queue, worker, scheduler, retry rail, cache, stored provider
inventory, persistent credential, or new product configuration.

## Consequences

- Every public surface observes provider state through one Sources Application
  authority and one interface-sized provider port.
- Connection revocation, installation replacement, repository policy, provider
  identity, pagination, and credential nondisclosure fail closed in one place.
- Repository and reference pages may change between calls because they are
  explicitly transient observations. A cursor detects scope/page-size drift
  and caps continuation at page 10,000; it is not an authentication primitive
  or a durable snapshot.
- Empty policy- or value-filtered pages can still contain a next cursor
  because provider pagination occurs before Sources filtering.
- Production availability still requires retained live-GitHub cross-surface
  evidence; implementation alone does not certify the external provider.
