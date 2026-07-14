# A3S Cloud

A3S Cloud is a self-hosted control plane for deploying applications and A3S
assets to operator-owned Linux nodes. It takes the useful product shape of a
platform such as Coolify while keeping A3S execution, workflow, event, gateway,
and asset semantics explicit.

## Status

This directory currently contains architecture and delivery design only. No
runtime, API, database schema, node agent, or user interface is implemented yet.
The documents deliberately separate prerequisites from completed product work.

## Product scope

The first product loop is:

```text
enroll one Linux node
  -> create one environment
  -> deploy one digest-pinned OCI image
  -> wait for a real health check
  -> publish an HTTPS route
  -> stream logs
  -> update or roll back without losing the previous healthy revision
```

A3S asset hosting is an extension of that loop. A3S Cloud supports exactly three
asset kinds:

- `agent`
- `mcp`
- `skill`

No compatibility enum, hidden route, table, UI, or import fallback is planned
for `code`, `workflow`, `knowledge`, `memory`, `model`, or `tool` assets.
Applications, deployments, nodes, routes, databases, and volumes are platform
resources, not asset kinds.

## Design principles

- Start as a modular monolith with a separate outbound-only node agent.
- Make A3S Runtime a general Task and Service runtime; keep Candidate and Judge
  semantics in Bench rather than the Runtime core.
- Keep desired state in PostgreSQL and observed execution state at the node.
- Use A3S Flow as the durable operation history, not as the domain database.
- Publish committed facts through a transactional outbox and A3S Event.
- Treat events as notifications, never as the only reconciliation mechanism.
- Pin every deployment to an immutable source revision and OCI digest.
- Keep asset source, artifact, catalog visibility, and deployment state separate.
- Use only A3S components that own a required product capability.
- Add no AHP dependency.
- Ship vertical slices with crash, replay, idempotency, and real-host tests.

## Documents

- [Domain model](docs/domain-model.md)
- [Technical architecture](docs/architecture.md)
- [Development plan](docs/development-plan.md)

## Planned repository shape

The root A3S repository is not a Rust workspace. A3S Cloud will own an app-local
workspace when implementation begins.

```text
apps/cloud/
├── Cargo.toml                 # app-local workspace only
├── config/
│   ├── cloud.hcl
│   └── node.example.hcl
├── migrations/
├── crates/
│   ├── contracts/            # versioned node and public protocol types
│   ├── control-plane/        # modular monolith: API, worker, reconciler
│   └── node-agent/           # outbound mTLS agent and Runtime adapters
├── web/                      # React control plane
├── deploy/
├── tests/
└── docs/
```

The planned layout is not permission to create a root `Cargo.toml` or turn the
monorepo into a Rust workspace.
