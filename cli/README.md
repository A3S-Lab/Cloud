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
| `A3S_CLOUD_TOKEN` | None | API token required by API commands |
| `A3S_CLOUD_URL` | `--url` | Absolute API URL ending in `/api/v1` |
| `A3S_CLOUD_ORGANIZATION_ID` | `--organization` | Organization UUID |
| `A3S_CLOUD_PROJECT_ID` | `--project` | Project UUID |
| `A3S_CLOUD_ENVIRONMENT_ID` | `--environment` | Environment UUID |
| `A3S_CLOUD_OUTPUT` | `--output` | `table` or `json` |
| `A3S_CLOUD_TIMEOUT_MS` | `--timeout` | Request timeout from 1 through 300000 ms |

Log commands additionally accept an opaque `--cursor`, a `--limit` from 1
through 256, and an optional `--stream=stdout|stderr` filter. These options are
rejected for commands that do not read logs.

Mutation commands require `--idempotency-key=<key>`. The key must contain only
visible ASCII letters, digits, `.`, `_`, `~`, `:`, `/`, or `-`, and must be at
most 255 characters. The CLI never generates a key: retry the exact command
with the same key to receive the durable replay result.

Node `ready`, `drain`, and `revoke` additionally require
`--expected-version=<current-aggregate-version>`. The positive safe integer is
sent as the existing optimistic-concurrency precondition; Cloud rejects stale
versions instead of applying a blind lifecycle transition.

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
source-revisions deploy <source-revision-id> --file=<path>
deployments get <deployment-id>
deployments cancel <deployment-id>
routes list
routes get <route-id>
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
token. API commands require the context implied by their REST scope. Use
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

Table cells and error metadata are bounded and control characters are
neutralized. Sensitive error-detail keys are redacted before JSON output.

This is an in-progress `C0.1` surface. Tenant and operational reads plus
explicitly idempotent operational mutations and ACL-backed Workload
create/update/source deployment are implemented. Core Organization, Project,
and Environment creation and version-checked node lifecycle transitions are
also implemented. Remaining edge, source, Secret, and identity resource
mutations, administrative diagnostics, node bootstrap, authorized search, and
the compatibility/deprecation gate remain planned.
