# G0 Provider Conformance

G0 closes only against operator-owned external providers. Local HTTP fixtures
remain regression evidence and cannot certify the private-provider boundary.

## Private GitHub provider

The manual `G0 external provider conformance` workflow requires a GitHub App
installed on one operator-owned private repository with read-only Contents
permission. Configure these repository secrets before dispatching the workflow
from the default branch:

| Secret | Value |
| --- | --- |
| `G0_GITHUB_APP_CLIENT_ID` | GitHub App client ID |
| `G0_GITHUB_APP_PRIVATE_KEY` | Current App PEM private key |
| `G0_GITHUB_INSTALLATION_ID` | Numeric installation ID |
| `G0_GITHUB_PRIVATE_REPOSITORY` | Exact private HTTPS repository URL |
| `G0_GITHUB_PRIVATE_BRANCH` | Existing branch used by the certification |

The selected revision must contain a root `Containerfile` for `linux/amd64`
that completes with Box build networking disabled. Keep the fixture bounded and
self-contained; a `FROM scratch` file that copies a small repository payload is
the expected certification shape. The workflow does not inject build arguments
or fetch an unpinned base image for the fixture.

The gate issues one short-lived, repository-scoped credential, resolves the
branch to a full commit, checks out only that commit, removes Git metadata,
replays from the credential-free receipt, and removes the checkout. The
uploaded JSON hashes the private repository identity, commit, and tree rather
than exposing their raw values. It contains no App key, repository URL, branch,
or provider token.

## Registry and Vault providers

The same workflow requires these operator-owned provider secrets:

| Secret | Value |
| --- | --- |
| `G0_REGISTRY_URL` | Exact HTTPS Registry origin, without credentials or a repository path |
| `G0_REGISTRY_REPOSITORY_PREFIX` | Lowercase repository prefix reserved for G0 evidence |
| `G0_REGISTRY_USERNAME` | Principal with pull and push access to that prefix |
| `G0_REGISTRY_PASSWORD` | Password or token for that principal |
| `G0_VAULT_ADDR` | Exact HTTPS Vault origin |
| `G0_VAULT_TOKEN` | Token allowed to sign with and read the configured Transit key |
| `G0_VAULT_TRANSIT_MOUNT` | Transit mount name, commonly `transit` |
| `G0_VAULT_BUILD_EVIDENCE_KEY` | Existing Ed25519 Transit key name |

Before provisioning PostgreSQL or compiling the pinned providers, the workflow
runs a lightweight configuration job that requires all thirteen bindings. A
missing binding fails that job while reporting only the missing Actions secret
name; secret values are never printed. The provider steps repeat the relevant
checks before consuming either the private-source or Registry/Vault boundary.
Presence is only a preflight check: the real provider tests remain responsible
for proving credential scope, endpoint behavior, publication, recovery, and
cleanup.

The job passes the exact production source Artifact through the real Linux Box
adapter, process-death replay, immediate-parent cache hydration, and
authoritative removal. It then admits the returned OCI graph, publishes and
re-reads it from the external Registry, signs deterministic SPDX/SLSA evidence
through Vault Transit with local Ed25519 verification, restores the succeeded
BuildRun from PostgreSQL, and creates the idempotent `cloud.deployment@3`
Workload handoff.

Private handoffs are restricted to mode `0700` directories and `0600` files and
are removed in the unconditional cleanup step. Only revision-bound public JSON
and certification markers are uploaded. A successful retained run of this
manual workflow is still required before G0 can be promoted; the separate Box
provider workflow must also retain its Fleet/Flow process-death matrix. The
retired build-provider script is intentionally not retained as a fallback.
