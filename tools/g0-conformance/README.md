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

The gate issues one short-lived, repository-scoped credential, resolves the
branch to a full commit, checks out only that commit, removes Git metadata,
replays from the credential-free receipt, and removes the checkout. The
uploaded JSON hashes the private repository identity, commit, and tree rather
than exposing their raw values. It contains no App key, repository URL, branch,
or provider token.

This workflow certifies only the private source-provider boundary. G0 remains
in progress until the separate operator Vault Transit, OCI registry, process
death, replay, and authoritative cleanup gate passes.

## Vault-signed external Registry build

The second manual job runs the real Runtime Task and rootless BuildKit path,
publishes the complete OCI graph to an operator-owned HTTPS Registry, signs the
generated SPDX/SLSA DSSE payload through an operator-owned HTTPS Vault Transit
Ed25519 key, verifies the returned key version and signature locally, replays
the publication, removes the Runtime unit, and scans durable BuildRun/evidence
JSON for exact provider secrets.

Configure these additional repository secrets:

| Secret | Value |
| --- | --- |
| `G0_REGISTRY_URL` | HTTPS Registry origin with an explicit port |
| `G0_REGISTRY_USERNAME` | Registry user with access to the gate prefix |
| `G0_REGISTRY_PASSWORD` | Registry password or access token |
| `G0_VAULT_ADDR` | HTTPS Vault origin trusted by the runner |
| `G0_VAULT_TOKEN` | Bounded token allowed to sign and read the public key |
| `G0_VAULT_TRANSIT_MOUNT` | Transit mount name |
| `G0_VAULT_TRANSIT_KEY` | Existing Ed25519 signing key name |

The uploaded evidence contains the exact Cloud revision, artifact and evidence
digests, public signing-key identity/version, and closed pass checks. It hashes
the Registry authority and build identity and contains neither provider secret.
The job never creates or mutates the Vault key. Before writing that evidence,
the same job sends real `SIGKILL` to child control-plane processes after remote
publication and after evidence persistence, then reconstructs Flow and proves
one publication, one verified evidence document, and authoritative cleanup.

Passing this job still does not close G0 by itself. The private GitHub job and
the complete source-to-published-Workload release evidence must pass for the
same release candidate, and the resulting operator-owned evidence must be
retained.
