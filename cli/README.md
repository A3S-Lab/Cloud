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
CLI does not create a context or credential file.

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
projects list
projects create <name>
environments list
environments create <name>
nodes list
nodes ready <node-id> --expected-version=<version>
nodes drain <node-id> --expected-version=<version>
nodes revoke <node-id> --expected-version=<version>
operations list
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

Use [`examples/workload.oci.example.acl`](../examples/workload.oci.example.acl)
for direct OCI create/update requests. Use
[`examples/workload.source.example.acl`](../examples/workload.source.example.acl)
for SourceRevision deployment; source manifests must omit `artifact` because
Cloud derives the verified published artifact from the selected BuildRun.
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

This is an in-progress `C0.1` surface. Tenant and operational reads plus
explicitly idempotent operational mutations and ACL-backed Workload
create/update/source deployment are implemented. Core Organization, Project,
and Environment creation and version-checked node lifecycle transitions are
also implemented. Public platform and health diagnostics are implemented with
a stable unhealthy exit contract. DomainClaim, logical Gateway-scope, and route
publication parity is implemented through the same typed client. Source
revision, GitHub connection, and repository-subscription parity is also
implemented without bypassing the public API. Secret metadata and version
lifecycle parity is implemented with standard-input-only material handling.
Remaining identity resource parity, node bootstrap, authorized search, and the
compatibility/deprecation gate remain planned.
