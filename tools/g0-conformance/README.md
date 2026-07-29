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
in progress until the typed A3S Box build boundary lands and its separate
operator-owned Registry, Vault Transit, process-death, replay, and cleanup gate
passes. The retired build-provider script is intentionally not retained as a
fallback.
