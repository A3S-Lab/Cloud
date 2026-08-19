# PostgreSQL schema management

## Authority

Cloud owns the ordered business-schema manifest in [`migrations/`](../migrations).
A3S Flow owns the `a3s_flow` manifest and A3S Boot owns the `a3s_boot` queue
manifest. A3S ORM is the sole migration mechanism for all three: it validates
each component manifest, serializes concurrent runners with a PostgreSQL
advisory transaction lock, applies pending migrations transactionally, and
records versions and checksums in that component schema's
`a3s_orm_migrations` ledger.

`a3s-cloud-migrate` is the only production process root that invokes those
owner APIs. It first applies Cloud's manifest, then delegates the Flow and Boot
manifests to their published migration functions. It is a one-shot process,
not a `server.role`; it registers no HTTP route, background worker, event
transport, repository family, or provider client. API, Worker, Relay, and
`all` use only read-only admission constructors and cannot create or alter
schema objects.

The one closed Cloud ACL names two credential references:

- `postgres.migration_url_env` is resolved only by `a3s-cloud-migrate`;
- `postgres.serving_url_env` is resolved only by API, Worker, Relay, or `all`.

The reference names must differ. The former shared `postgres.url_env` field is
not a compatibility alias and fails ACL admission. A process does not resolve
the other capability's credential merely to compare secret values.

This separation prevents a serving replica from becoming an implicit
installer and keeps migration execution in one existing mechanism rather than
copying component SQL or adding a Cloud lock, manifest verifier, or retry loop.

## Run order

With the closed Cloud ACL and credentials available through its named
environment variables, run:

```bash
export A3S_CLOUD_POSTGRES_MIGRATION_URL="postgres://cloud_migrator:replace-me@127.0.0.1:5432/a3s_cloud"
cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl

unset A3S_CLOUD_POSTGRES_MIGRATION_URL
export A3S_CLOUD_POSTGRES_URL="postgres://cloud_serving:replace-me@127.0.0.1:5432/a3s_cloud"
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

The repository development launcher performs the same order and removes the
migration variable before it replaces itself with the serving process. For
local development only, it may assign both distinct references the same URL.
A production installation or upgrade must preserve this sequence:

1. make PostgreSQL reachable and prove backup/restore readiness;
2. stop before migration if the release notes require an incompatible
   contract phase;
3. run the exact target release's `a3s-cloud-migrate` job to successful exit;
4. start target API, Worker, and Relay replicas;
5. retain old replicas only while every applied change is expand-compatible;
6. drain old replicas before any later contract migration removes their
   admitted schema.

Re-running the exact migrator is safe. Concurrent jobs converge through the
A3S ORM advisory lock in each component schema. One process may apply Cloud
versions while another later wins the Flow or Boot component lock; the union
of their evidence contains each pending version once, and a subsequent replay
reports the complete installation current.

## Serving-process admission

A serving build admits a database only when every required Cloud, Flow, and
Boot migration exists in its owner schema with the exact recorded checksum.
Cloud persistence verifies `public`, the Flow event store verifies `a3s_flow`,
and the Boot task queue verifies `a3s_boot`. Each intentionally accepts later
records in the same component ledger so old and new binaries can overlap
during an expand-compatible rolling upgrade.

Admission fails before domain repositories or workers are constructed when:

- a required component ledger does not exist;
- a required version is missing; or
- a required checksum differs from the compiled Cloud manifest.

Readiness repeats the same check. It never applies a pending migration and
never treats the existence of one arbitrary migration row as schema readiness.
An unknown later version is not permission to perform a breaking contract
change; compatibility remains an installer/release obligation.

## Remaining production boundary

The executable boundary, PostgreSQL 17 concurrency/admission gate, and the
[ACL-native Box baseline](../deploy/production/README.md) are implemented. On
a new checked-in PostgreSQL volume the installer creates a migration owner and
a non-DDL serving role, records schema-wide default grants for future
migration-owned objects, transfers ownership to a non-superuser migrator,
disables bootstrap-superuser login, projects only the migration URL to the
terminating unit, projects only the serving URL to API/Worker/Relay, and
encodes PostgreSQL health -> migration success -> serving startup.

`H0.4` remains open for existing or externally managed database grant
reconciliation, password rotation, clean-host evidence, PostgreSQL failover,
backup/restore, contract migration, replicated process placement, and the
independently versioned Gateway installation.

Those installation duties must use the same A3S ORM migration mechanism and
the existing component-scoped ledgers. They must not introduce another grant
runner inside Cloud, migration runner, copied manifest, or mutable
schema-version authority.
