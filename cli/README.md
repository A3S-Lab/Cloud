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

Flags override environment context. Remote API URLs require HTTPS. Plain HTTP
is accepted only for literal `localhost`, `127.0.0.1`, or `::1` endpoints.

## Commands

```text
context show
organizations list
projects list
environments list
nodes list
operations list
workloads list
workloads get <workload-id>
workloads logs <workload-id> <revision-id>
deployments get <deployment-id>
routes list
routes get <route-id>
build-runs list
build-runs get <build-run-id>
build-runs evidence <build-run-id>
build-runs logs <build-run-id>
```

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

This is an in-progress `C0.1` surface. Tenant and operational reads are
implemented. Mutations, administrative diagnostics, node bootstrap, authorized
search, and the compatibility/deprecation gate remain planned.
