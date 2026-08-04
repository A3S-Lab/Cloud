# A3S Cloud CLI

`a3s-cloud` is the presentation-only command-line client for the A3S Cloud
management API. It calls the same public REST queries as the web console and
never reads PostgreSQL or contacts a node directly.

## Build and run

From the Cloud repository root:

```bash
bun install --cwd cli --frozen-lockfile
bun run --cwd cli build
./cli/dist/a3s-cloud --help
```

For development, use `just cloud-cli --help` or invoke the TypeScript entry
point with `bun run --cwd cli src/main.ts`.

## Authentication and context

The API token is read only from `A3S_CLOUD_TOKEN`. A `--token` argument is
rejected so process listings and shell history do not receive credentials. The
CLI does not create a context or credential file. This is the caller credential;
`api-tokens create` accepts a distinct new credential only from standard input.

| Variable | Flag | Purpose |
| --- | --- | --- |
| `A3S_CLOUD_TOKEN` | None | API token required by authenticated API commands |
| `A3S_CLOUD_URL` | `--url` | Absolute API URL ending in `/api/v1` |
| `A3S_CLOUD_ORGANIZATION_ID` | `--organization` | Organization UUID |
| `A3S_CLOUD_PROJECT_ID` | `--project` | Project UUID |
| `A3S_CLOUD_ENVIRONMENT_ID` | `--environment` | Environment UUID |
| `A3S_CLOUD_OUTPUT` | `--output` | `table` or `json` |
| `A3S_CLOUD_TIMEOUT_MS` | `--timeout` | Request timeout from 1 through 300000 ms |

Log commands additionally accept an opaque `--cursor`, a `--limit` from 1
through 256, and an optional `--stream=stdout|stderr` filter. These options are
rejected for commands that do not read logs.

`search resources <query>` requires organization context and accepts a
`--limit` from 1 through 50, defaulting to 20. The query must contain 1 through
128 safe characters. Validation happens before transport, and Cloud performs
the tenant-authorized search through its public API; the CLI never loads broad
resource lists and filters them locally.

Replayable mutation commands require `--idempotency-key=<key>`. The key must
contain only visible ASCII letters, digits, `.`, `_`, `~`, `:`, `/`, or `-`,
and must be at most 255 characters. The CLI never generates a key: retry the
exact command with the same key to receive the durable replay result.
`source-connections begin` is the deliberate exception because the existing
API starts a short-lived no-store browser installation flow instead of a
replayable resource mutation.

Node `ready`, `drain`, and `revoke` additionally require
`--expected-version=<current-aggregate-version>`. The positive safe integer is
sent as the existing optimistic-concurrency precondition; Cloud rejects stale
versions instead of applying a blind lifecycle transition.

`nodes bootstrap <name>` requires `--enrollment-token-stdin`, an RFC 3339
`--expires-at`, an HTTPS `--agent-release-url`, its exact lowercase
`--agent-release-sha256`, an absolute `--node-config` ending in `.acl`, and a
caller-owned idempotency key. The CLI reads at most 70 bytes to detect overflow,
accepts exactly the 69 ASCII bytes formed by `a3sn_` plus 64 lowercase
hexadecimal digits, applies fatal UTF-8 decoding, and clears the input buffer.
It calls the existing `node:write` Fleet command and outputs safe token metadata
plus a Bash installation invocation; the credential is absent from arguments,
configuration, output, and errors. Cloud stores only the digest through the
A3S ORM-backed Fleet repository.

Run the printed invocation on the target Linux host after provisioning the
referenced node configuration from
[`config/node.example.acl`](../config/node.example.acl). The invocation
downloads the HTTPS Agent binary, verifies the supplied SHA-256 before
installation, prompts for the credential without echo, and exports it only for
Agent enrollment. Obtain both URL and digest from trusted signed A3S release
metadata; a caller-supplied checksum does not establish its own trust. Retrieve
the same one-time credential from the trusted secret source when the target
prompt appears. Cloud does not receive an SSH credential, and the CLI does not
contact the node.

`gateway-scopes create` accepts one through 100 unique node UUIDs.
`--min-ready` defaults to `1` and cannot exceed the member count;
`--max-unavailable` defaults to `0` and must remain below the member count.
Cloud remains authoritative for tenant ownership, membership, rollout policy,
and idempotent creation.

Source revision resolution and GitHub subscription creation require
`--context-path`, `--dockerfile-path`, and a comma-separated `--platforms`
value containing `linux/amd64`, `linux/arm64`, or both. `--target` optionally
selects a Dockerfile stage. The CLI validates bounded repository-relative
paths, exact HTTPS GitHub repository URLs, branch/tag/commit syntax, and the
closed `a3s.cloud.build-recipe.v1` Dockerfile recipe before transport. Cloud
remains the source policy, provider-resolution, tenancy, idempotency, and A3S
ORM persistence authority.

Secret create and version-add commands require `--value-stdin`. The CLI reads
at most 1 MiB plus one byte for overflow detection, rejects empty input,
rejects invalid UTF-8 with fatal decoding, preserves the accepted bytes
without trimming, and clears the byte buffer after decoding. There is no
plaintext value argument, environment variable, CLI configuration field, or
CLI-managed value file. Secret responses are projected onto metadata and
version-state fields only; tables, JSON, and sanitized mutation errors never
render the submitted value. Pipe the exact bytes from an interactive or
password-manager source, and remember that line-oriented producers may append
a newline that becomes part of the Secret.

API-token creation requires `--token-stdin` and a comma-separated `--scopes`
value. The optional `--expires-at` value must be an RFC 3339 timestamp. The CLI
reads at most 69 bytes to detect overflow, accepts exactly the 68 ASCII bytes
formed by `a3s_` plus 64 lowercase hexadecimal digits, does not trim input, and
clears the byte buffer after fatal UTF-8 decoding. The new credential has no
argument, environment variable, configuration field, output field, or echoed
error. API-token list/get and create/revoke results are projected onto safe
metadata; Cloud stores only the credential digest through its A3S ORM-backed
Identity repository.

Desired-state commands additionally require `--file=<path>` and accept only a
nonempty UTF-8 A3S ACL document of at most 64 KiB. The CLI sends those exact
bytes as `application/vnd.a3s.acl`; Cloud parses them with `a3s-acl`, applies
bounded closed-schema validation, and dispatches the existing application
command. The CLI does not parse ACL, accept JSON/TOML manifests, or place
manifest content in command arguments.

Flags override environment context. Remote API URLs require HTTPS. Plain HTTP
is accepted only for literal `localhost`, `127.0.0.1`, or `::1` endpoints.

## Commands

```text
context show
diagnostics status
organizations list
organizations create <name>
api-tokens list
api-tokens get <api-token-id>
api-tokens create <name> --token-stdin --scopes=<csv> [--expires-at=<timestamp>]
api-tokens revoke <api-token-id>
projects list
projects create <name>
environments list
environments create <name>
assets list
assets get <asset-id>
assets create <name> <agent|mcp|skill>
assets archive <asset-id>
asset-releases list <asset-id>
asset-releases get <asset-id> <release-id>
asset-releases select <asset-id> [version]
asset-releases create <asset-id> <version> <commit-sha>
asset-releases yank <asset-id> <release-id>
asset-releases deploy <asset-id> <release-id> --file=<path>
asset-releases update <workload-id> <asset-id> <release-id> --file=<path>
skill-bindings bind <workload-id> <skill-asset-id> <skill-release-id>
skill-bindings unbind <workload-id> <skill-asset-id>
nodes list
nodes bootstrap <name> --enrollment-token-stdin --expires-at=<timestamp> --agent-release-url=<https-url> --agent-release-sha256=<digest> --node-config=<absolute-acl-path>
nodes ready <node-id> --expected-version=<version>
nodes drain <node-id> --expected-version=<version>
nodes revoke <node-id> --expected-version=<version>
operations list
search resources <query> [--limit=<1..50>]
workloads list
workloads get <workload-id>
workloads logs <workload-id> <revision-id>
workloads create --file=<path>
workloads update <workload-id> --file=<path>
workloads stop <workload-id>
workloads rollback <workload-id> <revision-id>
source-revisions list
source-revisions resolve <repository-url> <branch|tag|commit> <reference> --context-path=<path> --dockerfile-path=<path> --platforms=<csv> [--target=<stage>]
source-revisions deploy <source-revision-id> --file=<path>
source-connections get
source-connections begin
source-subscriptions list
source-subscriptions create <repository-url> <branch> --context-path=<path> --dockerfile-path=<path> --platforms=<csv> [--target=<stage>]
source-subscriptions deactivate <subscription-id>
secrets list
secrets get <secret-id>
secrets create <name> --value-stdin
secrets add-version <secret-id> --value-stdin
secrets revoke-version <secret-id> <version>
deployments get <deployment-id>
deployments cancel <deployment-id>
domain-claims list
domain-claims get <domain-claim-id>
domain-claims create <pattern>
domain-claims verify <domain-claim-id> <proof>
domain-claims revoke <domain-claim-id> <reason>
gateway-scopes list
gateway-scopes create <node-id> [node-id...] [--min-ready=<count>] [--max-unavailable=<count>]
routes list
routes get <route-id>
routes publish <gateway-scope-id> <workload-revision-id> <domain-claim-id> <hostname> <path-prefix> <port-name>
build-runs list
build-runs get <build-run-id>
build-runs evidence <build-run-id>
build-runs logs <build-run-id>
build-runs cancel <build-run-id>
build-runs retry <build-run-id>
```

Asset and release commands require organization context. Asset create/archive
and release create/yank require a caller-owned idempotency key. Release create
accepts only a canonical semantic version and a full 40- or 64-character Git
object ID; Cloud reads the exact hosted commit and derives the admitted
manifest digest. `asset-releases select` chooses the highest stable published
version when no version is supplied. Draft and yanked releases are never
selected, while `asset-releases get` retains exact access to yanked identities
for pinned deployments. Skill releases publish the exact reachable hosted-Git
commit as an immutable content-addressed bundle without a BuildRun.

`asset-releases deploy` creates an ordinary Workload from an exact published
Agent release. `asset-releases update` creates the next revision of an existing
Workload bound to the same Agent Asset and the selected exact release. Both
commands require `--file` and a caller-owned idempotency key. The manifest must
omit `artifact`; Cloud loads the release's successful BuildRun and injects its
exact OCI URI, digest, and media type. Fresh bindings reject archived Assets
and draft or yanked releases. Exact replay, rollback, and Secret-triggered
restart preserve the already pinned identity.

`skill-bindings bind` selects one exact published Skill release and creates the
next immutable revision of an active Agent Workload. Rebinding the same Skill
Asset replaces only that Asset's release; `skill-bindings unbind` creates a new
revision without it. Older revisions remain available for rollback. Cloud
derives read-only Runtime Artifact mount names and targets, and never schedules
a Skill as a standalone service. Both commands require a caller-owned
idempotency key.

`build-runs logs` currently reports the API's explicit `503 Service
Unavailable` result. A successful log page is unavailable until A3S Box
exposes the authoritative durable build-log contract; the CLI does not fall
back to Runtime or Workload logs.

Use [`examples/workload.oci.example.acl`](../examples/workload.oci.example.acl)
for direct OCI create/update requests. Use
[`examples/workload.source.example.acl`](../examples/workload.source.example.acl)
for SourceRevision and Agent release deployment; these manifests must omit
`artifact` because Cloud derives the verified published artifact from the
selected BuildRun.
Every manifest declares `version = 1` and exactly one named `workload` block.
Unknown fields and blocks are rejected. Secret bindings contain only Secret ID
and version references and exactly one `environment`, `file`, or
`registry_credential` target; plaintext secret values are not valid manifest
fields.

`context show` reports only whether a token is configured; it never prints the
token. `diagnostics status` is public: it calls `/platform`, `/health/live`, and
`/health/ready` without requiring or sending a token. A wrapped health report
returned with HTTP `503` is still diagnostic data, not an API failure. The
command prints that report to stdout and exits with `8` when liveness or
readiness is down. A `503` error envelope remains a normal Cloud API error.
Other API commands require the context implied by their REST scope. Use
`--output=json` for automation. Success JSON is the API resource or resource
array, while failure JSON is written to stderr under an `error` object.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | Sanitized unexpected CLI failure |
| `2` | Invalid arguments or missing context |
| `3` | Authentication or authorization failure |
| `4` | Resource not found |
| `5` | Conflict |
| `6` | Other valid Cloud API failure |
| `7` | Network, timeout, cancellation, or invalid response failure |
| `8` | Cloud liveness or readiness is down; diagnostics remain on stdout |

Table cells and error metadata are bounded and control characters are
neutralized. Sensitive error-detail keys are redacted before JSON output.
DomainClaim create/verify/revoke and Gateway-scope create output includes the
server's durable `replayed` value. Route publication includes `replayed` and
`commandReplayed`, so automation can distinguish request replay from Gateway
command replay without inspecting internal state.
Source revision resolution and repository-subscription create/deactivate output
also includes the authoritative `replayed` value. `source-connections begin`
returns a short-lived no-store installation URL; use `--output=json` when the
complete URL must be copied because bounded table cells may abbreviate it.
Secret create, add-version, and revoke-version output includes only the safe
metadata projection, changed version state, and authoritative `replayed`
value. Plaintext is excluded even if an invalid upstream response attempts to
add a value field or echo it in an error.
API-token list/get output contains metadata only. Create/revoke output adds the
authoritative `replayed` value; an unexpected response field or upstream error
cannot make the submitted credential visible in table, JSON, or error output.
Node bootstrap output contains credential-free enrollment-token metadata, the
authoritative `replayed` value, and the checksum-verified Bash installation
invocation. An unexpected response field or upstream error cannot render the
submitted enrollment credential.

This is the verified `C0.1` automation surface. Tenant and operational reads plus
explicitly idempotent operational mutations and ACL-backed Workload
create/update/source deployment are implemented. Core Organization, Project,
and Environment creation and version-checked node lifecycle transitions are
also implemented. Public platform and health diagnostics are implemented with
a stable unhealthy exit contract. DomainClaim, logical Gateway-scope, and route
publication parity is implemented through the same typed client. Source
revision, GitHub connection, and repository-subscription parity is also
implemented without bypassing the public API. Secret metadata and version
lifecycle parity is implemented with standard-input-only material handling.
Identity API-token metadata and lifecycle parity is implemented with
standard-input-only credential creation and digest-only persistence. Node
bootstrap is implemented with standard-input-only credential issuance,
digest-only Fleet persistence, and a checksum-verified installation invocation.
Organization-scoped authorized search parity is implemented through the same
typed client and is also available in the Web console. The
compatibility/deprecation gate passes, and the real cross-surface gate proves
raw REST, the Web client import, and this compiled CLI against one Cloud process
and PostgreSQL database. The first `C0.2` scoped management MCP slice now reuses
the same core application commands and queries; it does not change this CLI's
transport or credential contract.
